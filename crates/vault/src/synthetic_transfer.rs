//! Synthetic-only rust-06 stateful differential workload for `vault-sync --all`.

use crate::backup::{self, Role as BackupRole};
use crate::canon::Canon;
use crate::keyfile;
use crate::store::Store;
use crate::sync::{self, Role};
use crate::transfer::{backup_stamp, Context};
use base64::{engine::general_purpose, Engine as _};
use flate2::{write::GzEncoder, Compression};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const CANARY: &str = "SYNTHETIC-RUST06-CANARY-7Q9";

type CaseResult<T> = Result<T, &'static str>;

fn sha(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn io(_: std::io::Error) -> &'static str {
    "OSError"
}

fn typed(value: Value) -> Value {
    match value {
        Value::Number(number) => json!({"int": number.to_string()}),
        Value::Array(items) => Value::Array(items.into_iter().map(typed).collect()),
        Value::Object(items) => Value::Object(
            items
                .into_iter()
                .map(|(key, value)| (key, typed(value)))
                .collect(),
        ),
        other => other,
    }
}

fn canon_output(value: &Canon) -> Value {
    match value {
        Canon::Null => Value::Null,
        Canon::Bool(value) => json!(value),
        Canon::Int(value) => json!({"int": value.to_string()}),
        Canon::Float(value) => json!(value.to_string()),
        Canon::Str(value) | Canon::Date(value) | Canon::DateTime(value) => json!(value),
        Canon::List(items) => Value::Array(items.iter().map(canon_output).collect()),
        Canon::Map(items) => {
            let mut out = Map::new();
            for (key, value) in items {
                if let Canon::Str(key) = key {
                    out.insert(key.clone(), canon_output(value));
                }
            }
            Value::Object(out)
        }
    }
}

fn store_error(error: &crate::atomic::StoreError) -> &'static str {
    match error {
        crate::atomic::StoreError::Utf8 => "UnicodeDecodeError",
        crate::atomic::StoreError::Vault | crate::atomic::StoreError::NoKey => "VaultError",
        crate::atomic::StoreError::Codec("json" | "jsonl") => "JSONDecodeError",
        crate::atomic::StoreError::Codec("yaml") => "YAMLError",
        _ => "OSError",
    }
}

struct Workspace {
    base: PathBuf,
    root: PathBuf,
    home: PathBuf,
    backups: PathBuf,
    key: PathBuf,
    pair: PathBuf,
    context: Context,
}

impl Workspace {
    fn new(base: &Path, name: &str) -> CaseResult<Self> {
        let base = base.join(name);
        let root = base.join("data");
        let home = base.join("home");
        let backups = base.join("backups");
        let key = base.join("keys/backup.key");
        let pair = base.join("keys/pair.key");
        std::fs::create_dir_all(&root).map_err(io)?;
        std::fs::create_dir_all(&home).map_err(io)?;
        std::env::set_var("HOME", &home);
        std::env::set_var("CAREERBREW_HOME", &root);
        std::env::set_var("CAREERBREW_KEY_FILE", &key);
        std::env::set_var("CAREERBREW_PAIR_KEY_FILE", &pair);
        std::env::set_var("CAREERBREW_BACKUP_DIRS", &backups);
        std::env::set_var("CAREERBREW_EVENTS", "off");
        std::env::set_var("CAREERBREW_ENCRYPT", "on");
        std::env::set_var("CAREERBREW_NOW", "2026-03-01T09:30:00+00:00");
        std::env::set_var("TZ", "UTC");
        std::env::remove_var("CAREERBREW_RUST06_ALLOWED");
        let context = Context::from_env().map_err(|_| "RuntimeError")?;
        Ok(Self {
            base,
            root,
            home,
            backups,
            key,
            pair,
            context,
        })
    }

    fn context(&self) -> CaseResult<&Context> {
        Ok(&self.context)
    }

    fn store(&self) -> CaseResult<Store<'_>> {
        Ok(self.context()?.store())
    }

    fn rel(&self, path: &Path) -> String {
        let relative = keyfile::resolve(path)
            .strip_prefix(keyfile::resolve(&self.base))
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if relative.is_empty() {
            ".".into()
        } else {
            relative
        }
    }

    fn evidence(&self, root: &Path) -> CaseResult<Value> {
        let mut files = Vec::new();
        if root.exists() {
            collect(root, &mut files)?;
        }
        files.sort();
        let store = self.store()?;
        let mut rows = Vec::new();
        for path in files {
            let raw = std::fs::read(&path).map_err(io)?;
            let mut row = Map::new();
            row.insert(
                "path".into(),
                json!(path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/")),
            );
            row.insert(
                "mode".into(),
                json!(format!(
                    "{:03o}",
                    std::fs::metadata(&path).map_err(io)?.permissions().mode() & 0o777
                )),
            );
            row.insert("sealed".into(), json!(raw.starts_with(crate::MAGIC)));
            match store.read_text(&path, None, false) {
                Ok(text) => {
                    row.insert(
                        "text_sha256".into(),
                        text.map_or(Value::Null, |text| json!(sha(text.as_bytes()))),
                    );
                }
                Err(error) => {
                    row.insert("read_error".into(), json!(store_error(&error)));
                }
            }
            rows.push(Value::Object(row));
        }
        Ok(Value::Array(rows))
    }
}

fn collect(folder: &Path, found: &mut Vec<PathBuf>) -> CaseResult<()> {
    for entry in std::fs::read_dir(folder).map_err(io)? {
        let path = entry.map_err(io)?.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(io)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect(&path, found)?;
        } else if metadata.is_file() {
            found.push(path);
        }
    }
    Ok(())
}

fn write_yaml(w: &Workspace, path: &Path, value: Canon) -> CaseResult<()> {
    w.store()?.write_yaml(path, &value).map_err(|_| "OSError")
}

fn write_text(w: &Workspace, path: &Path, text: &str) -> CaseResult<()> {
    w.store()?.write_text(path, text).map_err(|_| "OSError")
}

fn set_corporate(w: &Workspace, label: &str) -> CaseResult<()> {
    write_yaml(
        w,
        &w.root.join("machine.yaml"),
        Canon::Map(vec![
            (Canon::Str("role".into()), Canon::Str("corporate".into())),
            (Canon::Str("label".into()), Canon::Str(label.into())),
        ]),
    )
}

fn create(w: &Workspace) -> CaseResult<Vec<PathBuf>> {
    let context = w.context()?;
    backup::create(
        &w.root,
        &w.key,
        &context.targets().map_err(|error| error.class)?,
        &backup_stamp(),
    )
    .map_err(|error| error.python_kind())
}

fn restore(w: &Workspace, archive: &Path, dest: &Path) -> CaseResult<usize> {
    let context = w.context()?;
    let role = context.role().map_err(|error| error.class)?;
    let count = backup::restore(
        archive,
        dest,
        &w.key,
        &w.root,
        match role {
            Role::Personal => BackupRole::Personal,
            Role::Corporate => BackupRole::Corporate,
        },
    )
    .map_err(|error| error.python_kind())?;
    if role == Role::Corporate && !keyfile::resolve(dest).join("machine.yaml").exists() {
        let store = w.store()?;
        let info = store
            .read_yaml(&w.root.join("machine.yaml"), Canon::Map(Vec::new()), true)
            .map_err(|_| "OSError")?;
        store
            .write_yaml(&keyfile::resolve(dest).join("machine.yaml"), &info)
            .map_err(|_| "OSError")?;
    }
    Ok(count)
}

fn archive_roundtrip(base: &Path) -> CaseResult<Value> {
    let w = Workspace::new(base, "archive-roundtrip")?;
    write_text(
        &w,
        &w.root.join("profile.yaml"),
        &format!("name: {CANARY}\n"),
    )?;
    write_text(&w, &w.root.join("work/note.md"), "synthetic work\n")?;
    write_text(&w, &w.root.join("cache/ignored.json"), "{}")?;
    write_text(&w, &w.root.join(".staging/ignored.txt"), "ignored")?;
    std::fs::hard_link(w.root.join("work/note.md"), w.root.join("work/twin.md")).map_err(io)?;
    std::os::unix::fs::symlink(w.root.join("profile.yaml"), w.root.join("link.md")).map_err(io)?;
    write_yaml(
        &w,
        &w.root.join("machine.yaml"),
        Canon::Map(vec![
            (Canon::Str("role".into()), Canon::Str("personal".into())),
            (Canon::Str("label".into()), Canon::Str("source".into())),
        ]),
    )?;
    let written = create(&w)?;
    let archive = &written[0];
    let verified =
        backup::verify(archive, &w.key, &w.root, None).map_err(|error| error.python_kind())?;
    let dest = w.base.join("restored");
    write_text(&w, &dest.join("work/note.md"), "old conflict\n")?;
    let restored = restore(&w, archive, &dest)?;
    Ok(json!({
        "archives": written.len(),
        "verified": verified,
        "restored": restored,
        "archive_mode": format!("{:03o}", std::fs::metadata(archive).map_err(io)?.permissions().mode() & 0o777),
        "state": w.evidence(&dest)?,
        "source_link_kept_out": !dest.join("link.md").exists(),
        "cache_kept_out": !dest.join("cache").exists(),
        "staging_kept_out": !dest.join(".staging").exists(),
        "machine_kept_out": !dest.join("machine.yaml").exists(),
        "hardlink_is_file": dest.join("work/twin.md").is_file() && !dest.join("work/twin.md").is_symlink(),
    }))
}

fn archive_rejections(base: &Path) -> CaseResult<Value> {
    let w = Workspace::new(base, "archive-rejections")?;
    write_text(&w, &w.root.join("note.txt"), "synthetic")?;
    let archive = &create(&w)?[0];
    let raw = std::fs::read(archive).map_err(io)?;
    let mut tampered = raw.clone();
    tampered[50] ^= 1;
    let variants = vec![
        ("trailing-junk", [raw.clone(), b"x".to_vec()].concat()),
        ("trailing-padding", [raw.clone(), b"=".to_vec()].concat()),
        ("truncated", raw[..raw.len() - 1].to_vec()),
        ("invalid-base64", [b"!".as_slice(), &raw[1..]].concat()),
        ("empty", Vec::new()),
        ("tampered", tampered),
    ];
    let mut results = Map::new();
    for (name, bytes) in variants {
        let path = w.base.join("variants").join(archive.file_name().unwrap());
        std::fs::create_dir_all(path.parent().unwrap()).map_err(io)?;
        std::fs::write(&path, bytes).map_err(io)?;
        let result = match backup::verify(&path, &w.key, &w.root, None) {
            Ok(_) => "accepted",
            Err(error) => error.python_kind(),
        };
        results.insert(name.into(), json!(result));
    }
    let wrong_name = w
        .base
        .join("variants/careerbrew-20260301T093000000000Z-000000000000.cbk");
    std::fs::write(&wrong_name, &raw).map_err(io)?;
    for (name, result) in [
        (
            "wrong-name",
            backup::verify(&wrong_name, &w.key, &w.root, None),
        ),
        (
            "expected-sha",
            backup::verify(archive, &w.key, &w.root, Some(&"0".repeat(64))),
        ),
    ] {
        results.insert(
            name.into(),
            json!(match result {
                Ok(_) => "accepted",
                Err(error) => error.python_kind(),
            }),
        );
    }
    Ok(Value::Object(results))
}

fn raw_archive(members: &[(&str, bool, &[u8])]) -> CaseResult<Vec<u8>> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut tar = tar::Builder::new(encoder);
    for (name, link, data) in members {
        let mut header = tar::Header::new_gnu();
        header.set_size(if *link { 0 } else { data.len() as u64 });
        header.set_mode(0o600);
        if *link {
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_link_name(".").map_err(io)?;
            write_raw_name(&mut header, name)?;
            header.set_cksum();
            tar.append(&header, std::io::empty()).map_err(io)?;
        } else {
            write_raw_name(&mut header, name)?;
            header.set_cksum();
            tar.append(&header, *data).map_err(io)?;
        }
    }
    tar.into_inner().map_err(io)?.finish().map_err(io)
}

fn write_raw_name(header: &mut tar::Header, name: &str) -> CaseResult<()> {
    let bytes = name.as_bytes();
    if bytes.len() > 100 {
        return Err("ValueError");
    }
    let raw = header.as_mut_bytes();
    raw[..100].fill(0);
    raw[..bytes.len()].copy_from_slice(bytes);
    Ok(())
}

fn create_raw(w: &Workspace, raw: &[u8]) -> CaseResult<PathBuf> {
    let context = w.context()?;
    let targets = context.targets().map_err(|error| error.class)?;
    Ok(
        backup::create_raw(raw, &w.root, &w.key, &targets, &backup_stamp())
            .map_err(|error| error.python_kind())?[0]
            .clone(),
    )
}

fn archive_paths(base: &Path) -> CaseResult<Value> {
    let w = Workspace::new(base, "archive-paths")?;
    let mut results = Map::new();
    for (index, name) in [
        "./jobs/tracker.yaml",
        "./machine.yaml",
        "work/../machine.yaml",
        "/profile.yaml",
        "work\\note.md",
    ]
    .into_iter()
    .enumerate()
    {
        let raw = raw_archive(&[(name, false, b"x\n")])?;
        let archive = create_raw(&w, &raw)?;
        let dest = w.base.join(format!("odd-{index}"));
        let result = restore(&w, &archive, &dest);
        results.insert(
            format!("odd-{index}"),
            json!({"error": result.err().unwrap_or("accepted"), "created": dest.exists()}),
        );
    }
    let raw = raw_archive(&[("alias", true, b""), ("alias/profile.yaml", false, b"x\n")])?;
    let archive = create_raw(&w, &raw)?;
    let dest = w.base.join("link-member");
    let result = restore(&w, &archive, &dest);
    results.insert(
        "link-member".into(),
        json!({"error": result.err().unwrap_or("accepted"), "created": dest.exists()}),
    );
    write_text(&w, &w.root.join("work/note.md"), "synthetic")?;
    let archive = create(&w)?[0].clone();
    let trapped = w.base.join("trapped");
    std::fs::create_dir(&trapped).map_err(io)?;
    std::os::unix::fs::symlink(".", trapped.join("work")).map_err(io)?;
    let result = restore(&w, &archive, &trapped);
    let entries = std::fs::read_dir(&trapped)
        .map_err(io)?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    results.insert(
        "destination-link".into(),
        json!({"error": result.err().unwrap_or("accepted"), "entries": entries}),
    );
    Ok(Value::Object(results))
}

fn archive_corporate(base: &Path) -> CaseResult<Value> {
    let scenarios = [
        ("work", "out/standup.html", "<p>synthetic</p>\n"),
        ("personal-dir", "Jobs/tracker.yaml", "jobs: []\n"),
        (
            "personal-output",
            "out/reports/run-1.html",
            "<p>synthetic</p>\n",
        ),
        (
            "profile-field",
            "profile.yaml",
            "name: Example\ntarget_salary: 1\n",
        ),
        ("profile-malformed", "profile.yaml", "name: [unclosed\n"),
        (
            "goals",
            "goals.yaml",
            "queue:\n- {kind: interview, title: Synthetic}\n",
        ),
    ];
    let mut results = Map::new();
    for (name, relative, text) in scenarios {
        let source = Workspace::new(base, &format!("corporate-source-{name}"))?;
        write_text(&source, &source.root.join(relative), text)?;
        let archive = create(&source)?[0].clone();
        let corporate = Workspace::new(base, &format!("corporate-dest-{name}"))?;
        std::fs::create_dir_all(corporate.key.parent().unwrap()).map_err(io)?;
        std::fs::write(&corporate.key, std::fs::read(&source.key).map_err(io)?).map_err(io)?;
        std::fs::set_permissions(&corporate.key, std::fs::Permissions::from_mode(0o600))
            .map_err(io)?;
        set_corporate(&corporate, "work")?;
        let dest = corporate.base.join("restore");
        let before = corporate.evidence(&corporate.root)?;
        match restore(&corporate, &archive, &dest) {
            Err(error) => {
                results.insert(
                    name.into(),
                    json!({"error": error, "unchanged": before == corporate.evidence(&corporate.root)?, "created": dest.exists()}),
                );
            }
            Ok(count) => {
                let role = corporate
                    .store()?
                    .read_yaml(&dest.join("machine.yaml"), Canon::Map(Vec::new()), true)
                    .ok()
                    .and_then(|value| match map_get(&value, "role") {
                        Some(Canon::Str(role)) => Some(role.clone()),
                        _ => None,
                    });
                results.insert(
                    name.into(),
                    json!({"restored": count, "role": role, "state": corporate.evidence(&dest)?}),
                );
            }
        }
    }
    Ok(Value::Object(results))
}

fn archive_key_guards(base: &Path) -> CaseResult<Value> {
    let mut results = Map::new();
    let w = Workspace::new(base, "archive-missing-key")?;
    std::fs::create_dir_all(w.root.join("work")).map_err(io)?;
    let key = backup::load_key(&w.root, &w.key, true).map_err(|error| error.python_kind())?;
    let sealed = crate::seal(&key, b"synthetic").map_err(|_| "ValueError")?;
    std::fs::write(w.root.join("work/sealed.txt"), &sealed).map_err(io)?;
    std::fs::remove_file(&w.key).map_err(io)?;
    let outcome = create(&w);
    let archives = std::fs::read_dir(&w.backups)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "cbk")
                })
                .count()
        })
        .unwrap_or(0);
    results.insert(
        "missing-key".into(),
        json!({
            "error": outcome.err().unwrap_or("accepted"),
            "key_created": w.key.exists(),
            "unchanged": std::fs::read(w.root.join("work/sealed.txt")).map_err(io)? == sealed,
            "archives": archives,
        }),
    );
    let inside = Workspace::new(base, "archive-key-inside")?;
    std::env::set_var("CAREERBREW_KEY_FILE", inside.root.join("keys/backup.key"));
    let outcome = backup::create(
        &inside.root,
        &inside.root.join("keys/backup.key"),
        &inside.context()?.targets().map_err(|error| error.class)?,
        &backup_stamp(),
    )
    .map(|_| Vec::<PathBuf>::new());
    results.insert(
        "inside-key".into(),
        json!({"error": outcome.err().map(|error| error.python_kind()).unwrap_or("accepted"), "created": inside.root.join("keys/backup.key").exists()}),
    );
    let pair = Workspace::new(base, "pair-key-inside")?;
    std::env::set_var("CAREERBREW_PAIR_KEY_FILE", pair.root.join("pair.key"));
    let outcome = sync::pair_init(&pair.root.join("pair.key"), &pair.root);
    results.insert(
        "pair-inside".into(),
        json!({"error": outcome.err().map(|error| error.class).unwrap_or("accepted"), "created": pair.root.join("pair.key").exists()}),
    );
    let valid = Workspace::new(base, "pair-valid")?;
    let key = sync::pair_init(&valid.pair, &valid.root).map_err(|error| error.class)?;
    let cipher = fernet::Fernet::new(&key).ok_or("ValueError")?;
    let token = cipher.encrypt(b"synthetic");
    let roundtrip = crate::decrypt_like_python(&cipher, &token)
        .ok()
        .is_some_and(|value| value == b"synthetic");
    results.insert(
        "pair-roundtrip".into(),
        json!({"key_length": key.len(), "mode": format!("{:03o}", std::fs::metadata(&valid.pair).map_err(io)?.permissions().mode() & 0o777), "roundtrip": roundtrip}),
    );
    let outcome = sync::pair_join(&valid.pair, &valid.root, "not-a-key");
    results.insert(
        "pair-invalid".into(),
        json!(outcome.err().map(|error| error.class).unwrap_or("accepted")),
    );
    Ok(Value::Object(results))
}

fn archive_targets(base: &Path) -> CaseResult<Value> {
    let w = Workspace::new(base, "archive-targets")?;
    write_yaml(
        &w,
        &w.root.join("settings.yaml"),
        Canon::Map(vec![(
            Canon::Str("backup_icloud".into()),
            Canon::Bool(true),
        )]),
    )?;
    std::env::set_var("CAREERBREW_ICLOUD_DIR", w.base.join("icloud"));
    let written = create(&w)?;
    let mut targets = written
        .iter()
        .map(|path| w.rel(path.parent().unwrap()))
        .collect::<Vec<_>>();
    targets.sort();
    let verified = written
        .iter()
        .map(|path| {
            backup::verify(path, &w.key, &w.root, None)
                .map(|count| json!(count))
                .map_err(|error| error.python_kind())
        })
        .collect::<CaseResult<Vec<_>>>()?;
    let allowed = w.base.join("approved");
    std::env::set_var(
        "CAREERBREW_BACKUP_DIRS",
        std::env::join_paths([&w.backups, &allowed]).map_err(|_| "ValueError")?,
    );
    std::env::set_var("CAREERBREW_RUST06_ALLOWED", &allowed);
    let context = w.context()?;
    let policy = context
        .targets()
        .map_err(|error| error.class)?
        .iter()
        .map(|path| json!(w.rel(path.parent().unwrap_or(path))))
        .collect::<Vec<_>>();
    std::env::set_var("CAREERBREW_RUST06_ALLOWED", "");
    let denied = create(&w);
    let corporate = Workspace::new(base, "archive-cloud")?;
    set_corporate(&corporate, "corporate")?;
    std::env::set_var("CAREERBREW_BACKUP_DIRS", corporate.home.join("Dropbox/cb"));
    let cloud = create(&corporate);
    let mut out = json!({
        "icloud": {"targets": targets, "verified": verified},
        "policy": policy,
        "policy-denied": denied.err().unwrap_or("accepted"),
        "cloud-create": {"error": cloud.err().unwrap_or("accepted"), "created": corporate.home.join("Dropbox/cb").exists()},
    });
    std::env::set_var("CAREERBREW_BACKUP_DIRS", &corporate.backups);
    let archive = create(&corporate)?[0].clone();
    for service in ["pCloud Drive", "Sync"] {
        let target = corporate.home.join(service).join("cb");
        std::env::set_var("CAREERBREW_BACKUP_DIRS", &target);
        let created = create(&corporate);
        out.as_object_mut().unwrap().insert(
            format!("cloud-create-{service}"),
            json!({"error": created.err().unwrap_or("accepted"), "created": target.exists()}),
        );
        let dest = corporate.home.join(service).join("restored");
        let restored = corporate
            .context()?
            .cmd_backup_restore(&archive, &dest)
            .map(|_| "accepted")
            .map_err(|error| error.class);
        out.as_object_mut().unwrap().insert(
            format!("cloud-restore-{service}"),
            json!({"error": restored.err().unwrap_or("accepted"), "created": dest.exists()}),
        );
    }
    Ok(out)
}

fn sync_roundtrip(base: &Path) -> CaseResult<Value> {
    let personal = Workspace::new(base, "sync-personal")?;
    write_yaml(
        &personal,
        &personal.root.join("settings.yaml"),
        Canon::Map(vec![
            (
                Canon::Str("ladder".into()),
                Canon::Str("generic-swe".into()),
            ),
            (
                Canon::Str("target_level".into()),
                Canon::Str("staff".into()),
            ),
            (
                Canon::Str("backup_targets".into()),
                Canon::List(vec![Canon::Str("/synthetic/refused".into())]),
            ),
        ]),
    )?;
    let pair_key = sync::pair_init(&personal.pair, &personal.root).map_err(|error| error.class)?;
    let bundle = personal.base.join("bundle.cbsync");
    let context = personal.context()?;
    let exported = sync::export(
        &bundle,
        &personal.root,
        &personal.pair,
        Role::Personal,
        "2026-03-01T09:30:00+00:00",
        &personal.store()?,
    )
    .map_err(|error| error.class)?;
    let corporate = Workspace::new(base, "sync-corporate")?;
    set_corporate(&corporate, "corporate")?;
    sync::pair_join(&corporate.pair, &corporate.root, &pair_key).map_err(|error| error.class)?;
    let imported = sync::import_bundle(
        &bundle,
        &corporate.root,
        &corporate.pair,
        Role::Corporate,
        &corporate.store()?,
        &context.ladders().map_err(|error| error.class)?,
    )
    .map_err(|error| error.class)?;
    let settings = corporate
        .store()?
        .read_yaml(
            &corporate.root.join("settings.yaml"),
            Canon::Map(Vec::new()),
            true,
        )
        .map_err(|_| "OSError")?;
    Ok(json!({
        "exported": typed(exported),
        "imported": typed(imported),
        "bundle_mode": format!("{:03o}", std::fs::metadata(&bundle).map_err(io)?.permissions().mode() & 0o777),
        "settings": canon_output(&settings),
        "state": corporate.evidence(&corporate.root)?,
    }))
}

fn encrypted_bundle(path: &Path, key: &str, value: Value) -> CaseResult<()> {
    let cipher = fernet::Fernet::new(key).ok_or("ValueError")?;
    std::fs::write(path, cipher.encrypt(value.to_string().as_bytes())).map_err(io)
}

fn sync_rejections(base: &Path) -> CaseResult<Value> {
    let initial = Workspace::new(base, "sync-rejections")?;
    sync::pair_init(&initial.pair, &initial.root).map_err(|error| error.class)?;
    let mut results = Map::new();
    let corporate = Workspace::new(base, "sync-export-corporate")?;
    set_corporate(&corporate, "corporate")?;
    sync::pair_init(&corporate.pair, &corporate.root).map_err(|error| error.class)?;
    let refused = corporate.base.join("refused.cbsync");
    let outcome = sync::export(
        &refused,
        &corporate.root,
        &corporate.pair,
        Role::Corporate,
        "2026-03-01T09:30:00+00:00",
        &corporate.store()?,
    );
    results.insert(
        "corporate-export".into(),
        json!({"error": outcome.err().map(|error| error.class).unwrap_or("accepted"), "created": refused.exists()}),
    );
    let personal = Workspace::new(base, "sync-personal-refusal")?;
    let key = std::fs::read_to_string(&corporate.pair)
        .map_err(io)?
        .trim()
        .to_owned();
    sync::pair_join(&personal.pair, &personal.root, &key).map_err(|error| error.class)?;
    let bundle = personal.base.join("corporate.cbsync");
    encrypted_bundle(
        &bundle,
        &key,
        json!({"from": "corporate", "at": "x", "payload": {"evidence": []}}),
    )?;
    let outcome = sync::import_bundle(
        &bundle,
        &personal.root,
        &personal.pair,
        Role::Personal,
        &personal.store()?,
        &personal.context()?.ladders().map_err(|error| error.class)?,
    );
    results.insert(
        "corporate-import".into(),
        json!(outcome.err().map(|error| error.class).unwrap_or("accepted")),
    );
    let same = personal.base.join("same.cbsync");
    encrypted_bundle(
        &same,
        &key,
        json!({"from": "personal", "at": "x", "payload": {"settings": {}}}),
    )?;
    let outcome = sync::import_bundle(
        &same,
        &personal.root,
        &personal.pair,
        Role::Personal,
        &personal.store()?,
        &personal.context()?.ladders().map_err(|error| error.class)?,
    );
    results.insert(
        "same-role".into(),
        json!(outcome.err().map(|error| error.class).unwrap_or("accepted")),
    );
    let wrong = personal.base.join("wrong.cbsync");
    let wrong_key = general_purpose::URL_SAFE.encode((0_u8..32).collect::<Vec<_>>());
    encrypted_bundle(&wrong, &wrong_key, json!({}))?;
    let outcome = sync::import_bundle(
        &wrong,
        &personal.root,
        &personal.pair,
        Role::Personal,
        &personal.store()?,
        &personal.context()?.ladders().map_err(|error| error.class)?,
    );
    results.insert(
        "wrong-key".into(),
        json!(outcome.err().map(|error| error.class).unwrap_or("accepted")),
    );
    let corp = Workspace::new(base, "sync-forbidden")?;
    set_corporate(&corp, "corporate")?;
    sync::pair_join(&corp.pair, &corp.root, &key).map_err(|error| error.class)?;
    let forbidden = corp.base.join("forbidden.cbsync");
    encrypted_bundle(
        &forbidden,
        &key,
        json!({"from": "personal", "at": "x", "payload": {"jobs": []}}),
    )?;
    let before = corp.evidence(&corp.root)?;
    let outcome = sync::import_bundle(
        &forbidden,
        &corp.root,
        &corp.pair,
        Role::Corporate,
        &corp.store()?,
        &corp.context()?.ladders().map_err(|error| error.class)?,
    );
    results.insert(
        "forbidden-payload".into(),
        json!({"error": outcome.err().map(|error| error.class).unwrap_or("accepted"), "unchanged": before == corp.evidence(&corp.root)?}),
    );
    Ok(Value::Object(results))
}

fn sync_allowlist(base: &Path) -> CaseResult<Value> {
    let w = Workspace::new(base, "sync-allowlist")?;
    set_corporate(&w, "corporate")?;
    let key = sync::pair_init(&w.pair, &w.root).map_err(|error| error.class)?;
    let inputs = [
        json!({"ladder": "no-such-ladder", "target_level": "DROP"}),
        json!({"ladder": "generic-swe", "target_level": "staff"}),
        json!({"target_level": "wizard"}),
        json!({"ladder": {"not": "text"}, "target_level": "staff", "extra": 1}),
        json!({"ladder": "x".repeat(65), "target_level": "staff"}),
    ];
    let mut results = Vec::new();
    for (index, settings) in inputs.into_iter().enumerate() {
        let path = w.base.join(format!("case-{index}.cbsync"));
        encrypted_bundle(
            &path,
            &key,
            json!({"from": "personal", "at": "x", "payload": {"settings": settings}}),
        )?;
        let outcome = sync::import_bundle(
            &path,
            &w.root,
            &w.pair,
            Role::Corporate,
            &w.store()?,
            &w.context()?.ladders().map_err(|error| error.class)?,
        )
        .map_err(|error| error.class)?;
        let current = w
            .store()?
            .read_yaml(&w.root.join("settings.yaml"), Canon::Map(Vec::new()), true)
            .map_err(|_| "OSError")?;
        results.push(json!({"result": typed(outcome), "settings": canon_output(&current)}));
    }
    Ok(Value::Array(results))
}

fn import_with_loaded_ladders(w: &Workspace, bundle: &Path) -> CaseResult<Value> {
    let ladders = w.context()?.ladders().map_err(|error| error.class)?;
    sync::import_bundle(
        bundle,
        &w.root,
        &w.pair,
        Role::Corporate,
        &w.store()?,
        &ladders,
    )
    .map_err(|error| error.class)
}

fn sync_plugin_ladder(base: &Path) -> CaseResult<Value> {
    let personal = Workspace::new(base, "sync-plugin-personal")?;
    write_yaml(
        &personal,
        &personal.root.join("settings.yaml"),
        Canon::Map(vec![
            (
                Canon::Str("ladder".into()),
                Canon::Str("synthetic-eng".into()),
            ),
            (Canon::Str("target_level".into()), Canon::Str("e5".into())),
        ]),
    )?;
    let pair_key = sync::pair_init(&personal.pair, &personal.root).map_err(|error| error.class)?;
    let bundle = personal.base.join("plugin.cbsync");
    sync::export(
        &bundle,
        &personal.root,
        &personal.pair,
        Role::Personal,
        "2026-03-01T09:30:00+00:00",
        &personal.store()?,
    )
    .map_err(|error| error.class)?;
    let corporate = Workspace::new(base, "sync-plugin-corporate")?;
    set_corporate(&corporate, "corporate")?;
    sync::pair_join(&corporate.pair, &corporate.root, &pair_key).map_err(|error| error.class)?;
    let plugin = corporate.root.join("plugins/synthetic");
    let manifest = crate::json_yaml::yaml_read(
        "name: synthetic\nversion: 1.0.0\nprovides:\n  ladders: ladders/*.yaml\n",
    )
    .map_err(|_| "YAMLError")?;
    write_yaml(
        &corporate,
        &plugin.join("careerbrew-plugin.yaml"),
        manifest.clone(),
    )?;
    let ladder = crate::json_yaml::yaml_read(
        "id: synthetic-eng\nname: Synthetic engineering\nlevels: [e4, e5]\ncompetencies:\n  - {id: delivery, name: Delivery, weight: 1, signals: {}, expect: {e4: 1, e5: 2}}\n",
    )
    .map_err(|_| "YAMLError")?;
    write_yaml(&corporate, &plugin.join("ladders/custom.yaml"), ladder)?;
    let lock = crate::json_yaml::json_read(
        "{\"plugins\":{\"synthetic\":{\"origin\":\"plugin:synthetic\"}}}",
    )
    .map_err(|_| "JSONDecodeError")?;
    corporate
        .store()?
        .write_json(&corporate.root.join("plugins.lock.json"), &lock)
        .map_err(|_| "OSError")?;
    let imported = import_with_loaded_ladders(&corporate, &bundle)?;
    let settings = corporate
        .store()?
        .read_yaml(
            &corporate.root.join("settings.yaml"),
            Canon::Map(Vec::new()),
            true,
        )
        .map_err(|_| "OSError")?;
    let ladders = corporate
        .context()?
        .ladders()
        .map_err(|error| error.class)?;
    let loaded = ladders.get("synthetic-eng");
    let origin = loaded.map_or(Value::Null, |ladder| json!(ladder.origin.clone()));
    let source = loaded.map_or(Value::Null, |ladder| {
        json!(ladder
            .source
            .strip_prefix(&corporate.root)
            .unwrap_or(&ladder.source)
            .to_string_lossy()
            .replace('\\', "/"))
    });
    let mut out = json!({
        "imported": typed(imported),
        "settings": canon_output(&settings),
        "origin": origin,
        "source": source,
    });
    let escape_manifest = crate::json_yaml::yaml_read(
        "name: synthetic\nversion: 1.0.0\nprovides:\n  ladders: ../outside.yaml\n",
    )
    .map_err(|_| "YAMLError")?;
    write_yaml(
        &corporate,
        &plugin.join("careerbrew-plugin.yaml"),
        escape_manifest,
    )?;
    let escape_before = corporate.evidence(&corporate.root)?;
    let escaped = import_with_loaded_ladders(&corporate, &bundle);
    out.as_object_mut().unwrap().insert(
        "escape".into(),
        json!({"error": escaped.err().unwrap_or("accepted"), "unchanged": escape_before == corporate.evidence(&corporate.root)?}),
    );
    write_yaml(&corporate, &plugin.join("careerbrew-plugin.yaml"), manifest)?;
    std::os::unix::fs::symlink(
        plugin.join("ladders/custom.yaml"),
        plugin.join("ladders/link.yaml"),
    )
    .map_err(io)?;
    let symlink_before = corporate.evidence(&corporate.root)?;
    let linked = import_with_loaded_ladders(&corporate, &bundle);
    out.as_object_mut().unwrap().insert(
        "symlink".into(),
        json!({"error": linked.err().unwrap_or("accepted"), "unchanged": symlink_before == corporate.evidence(&corporate.root)?}),
    );
    std::fs::remove_file(plugin.join("ladders/link.yaml")).map_err(io)?;
    let malformed = crate::json_yaml::yaml_read(
        "id: synthetic-eng\nname: Synthetic engineering\nlevels: [e4, e5]\ncompetencies:\n  - {id: delivery, name: Delivery, weight: 1, signals: {}, expect: {e4: 1}, descriptions: []}\n",
    )
    .map_err(|_| "YAMLError")?;
    write_yaml(&corporate, &plugin.join("ladders/custom.yaml"), malformed)?;
    let malformed_before = corporate.evidence(&corporate.root)?;
    let invalid = import_with_loaded_ladders(&corporate, &bundle);
    out.as_object_mut().unwrap().insert(
        "malformed".into(),
        json!({"error": invalid.err().unwrap_or("accepted"), "unchanged": malformed_before == corporate.evidence(&corporate.root)?}),
    );
    Ok(out)
}

fn sync_user_ladder(base: &Path) -> CaseResult<Value> {
    let personal = Workspace::new(base, "sync-user-personal")?;
    write_yaml(
        &personal,
        &personal.root.join("settings.yaml"),
        Canon::Map(vec![
            (Canon::Str("ladder".into()), Canon::Str("user-eng".into())),
            (Canon::Str("target_level".into()), Canon::Str("u2".into())),
        ]),
    )?;
    let pair_key = sync::pair_init(&personal.pair, &personal.root).map_err(|error| error.class)?;
    let bundle = personal.base.join("user.cbsync");
    sync::export(
        &bundle,
        &personal.root,
        &personal.pair,
        Role::Personal,
        "2026-03-01T09:30:00+00:00",
        &personal.store()?,
    )
    .map_err(|error| error.class)?;
    let corporate = Workspace::new(base, "sync-user-corporate")?;
    set_corporate(&corporate, "corporate")?;
    sync::pair_join(&corporate.pair, &corporate.root, &pair_key).map_err(|error| error.class)?;
    let path = corporate.root.join("ladders/user.yaml");
    let ladder = crate::json_yaml::yaml_read(
        "id: user-eng\nname: User engineering\nlevels: [u1, u2]\ncompetencies:\n  - {id: delivery, name: Delivery, weight: 1, signals: {}, expect: {u1: 1, u2: 2}, descriptions: {u1: synthetic}, examples: {u2: synthetic}}\n",
    )
    .map_err(|_| "YAMLError")?;
    write_yaml(&corporate, &path, ladder)?;
    let imported = import_with_loaded_ladders(&corporate, &bundle)?;
    let settings = corporate
        .store()?
        .read_yaml(
            &corporate.root.join("settings.yaml"),
            Canon::Map(Vec::new()),
            true,
        )
        .map_err(|_| "OSError")?;
    let ladders = corporate
        .context()?
        .ladders()
        .map_err(|error| error.class)?;
    let loaded = ladders.get("user-eng").ok_or("LadderError")?;
    Ok(json!({
        "imported": typed(imported),
        "settings": canon_output(&settings),
        "origin": loaded.origin.clone(),
        "source": loaded.source.strip_prefix(&corporate.root).unwrap_or(&loaded.source).to_string_lossy().replace('\\', "/"),
        "encrypted": std::fs::read(&path).map_err(io)?.starts_with(crate::MAGIC),
    }))
}

fn normalize_archive_names(text: &str) -> String {
    let mut out = text.to_owned();
    let mut offset = 0;
    while let Some(found) = out[offset..].find("careerbrew-") {
        let start = offset + found;
        let Some(end) = out[start..].find(".cbk").map(|end| start + end + 4) else {
            break;
        };
        out.replace_range(start..end, "careerbrew-<archive>.cbk");
        offset = start + "careerbrew-<archive>.cbk".len();
    }
    out
}

fn cli_handlers(base: &Path) -> CaseResult<Value> {
    let w = Workspace::new(base, "cli-handlers")?;
    write_text(&w, &w.root.join("profile.yaml"), "name: Example\n")?;
    let mut outputs = Map::new();
    let mut invoke = |name: &str, result: CaseResult<String>| -> CaseResult<()> {
        let text = normalize_archive_names(
            &result?.replace(&w.base.to_string_lossy().into_owned(), "<work>"),
        );
        if text.contains(CANARY) || (text.contains("pairing key") && !text.contains("paired")) {
            return Err("RuntimeError");
        }
        outputs.insert(name.into(), json!({"exit": 0, "stdout": text}));
        Ok(())
    };
    invoke(
        "data-status",
        w.context()?.cmd_data_status().map_err(|error| error.class),
    )?;
    invoke(
        "backup-create",
        w.context()?
            .cmd_backup_create()
            .map_err(|error| error.class),
    )?;
    let archive = backup::latest(&w.context()?.targets().map_err(|error| error.class)?)
        .map_err(|error| error.python_kind())?
        .ok_or("RuntimeError")?;
    invoke(
        "backup-verify",
        w.context()?
            .cmd_backup_verify(Some(&archive))
            .map_err(|error| error.class),
    )?;
    invoke(
        "backup-restore",
        w.context()?
            .cmd_backup_restore(&archive, &w.base.join("restored"))
            .map_err(|error| error.class),
    )?;
    let key = sync::pair_init(&w.pair, &w.root).map_err(|error| error.class)?;
    invoke(
        "pair-join",
        w.context()?
            .cmd_pair_join(&key)
            .map_err(|error| error.class),
    )?;
    let bundle = w.base.join("handler.cbsync");
    write_yaml(
        &w,
        &w.root.join("settings.yaml"),
        Canon::Map(vec![
            (
                Canon::Str("ladder".into()),
                Canon::Str("generic-swe".into()),
            ),
            (
                Canon::Str("target_level".into()),
                Canon::Str("staff".into()),
            ),
        ]),
    )?;
    invoke(
        "sync-export",
        w.context()?
            .cmd_sync_export(&bundle)
            .map_err(|error| error.class),
    )?;
    set_corporate(&w, "corporate")?;
    invoke(
        "sync-import",
        w.context()?
            .cmd_sync_import(&bundle)
            .map_err(|error| error.class),
    )?;
    Ok(Value::Object(outputs))
}

fn map_get<'a>(value: &'a Canon, name: &str) -> Option<&'a Canon> {
    let Canon::Map(items) = value else {
        return None;
    };
    items
        .iter()
        .find(|(key, _)| matches!(key, Canon::Str(key) if key == name))
        .map(|(_, value)| value)
}

fn python_json_ascii(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii() {
            out.push(character);
        } else {
            for unit in character.encode_utf16(&mut [0_u16; 2]) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

type CaseFn = fn(&Path) -> CaseResult<Value>;

fn plant_write(base: &Path, selected: &str) -> Result<(), String> {
    let Ok(plant) = std::env::var("CAREERBREW_RUST06_PLANT_WRITE") else {
        return Ok(());
    };
    if plant == "duplicate" && selected == "archive-rejections" {
        let path = base.join("archive-roundtrip/data/profile.yaml");
        let bytes = std::fs::read(&path).map_err(|_| "plant".to_owned())?;
        return std::fs::write(path, bytes).map_err(|_| "plant".into());
    }
    if plant == "duplicate" || selected != "archive-roundtrip" {
        return Ok(());
    }
    match plant.as_str() {
        "extra" => {
            std::fs::write(base.join("planted-extra"), b"synthetic").map_err(|_| "plant".into())
        }
        "missing" => std::fs::remove_file(base.join("archive-roundtrip/data/profile.yaml"))
            .map_err(|_| "plant".into()),
        _ => Err("plant".into()),
    }
}

pub fn run_selected(selected: Option<&str>) -> Result<String, String> {
    let root = std::env::var_os("CAREERBREW_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "home".to_owned())?;
    let base = root.join("rust06-workload");
    std::fs::create_dir_all(&base).map_err(|_| "home".to_owned())?;
    let functions: &[(&str, CaseFn)] = &[
        ("archive-roundtrip", archive_roundtrip),
        ("archive-rejections", archive_rejections),
        ("archive-paths", archive_paths),
        ("archive-corporate", archive_corporate),
        ("archive-key-guards", archive_key_guards),
        ("archive-targets", archive_targets),
        ("sync-roundtrip", sync_roundtrip),
        ("sync-rejections", sync_rejections),
        ("sync-allowlist", sync_allowlist),
        ("sync-plugin-ladder", sync_plugin_ladder),
        ("sync-user-ladder", sync_user_ladder),
        ("cli-handlers", cli_handlers),
    ];
    let mut cases = Vec::new();
    for (name, function) in functions {
        if selected.is_some_and(|selected| selected != *name) {
            continue;
        }
        let result = match function(&base) {
            Ok(value) => typed(value),
            Err(error) => json!({"error": error}),
        };
        plant_write(&base, name)?;
        cases.push(json!({"id": name, "result": result}));
    }
    let text = python_json_ascii(
        &serde_json::to_string(&json!({"cases": cases})).map_err(|_| "json".to_owned())?,
    ) + "\n";
    if text.contains(CANARY) {
        return Err("privacy".into());
    }
    Ok(text)
}

pub fn run_all() -> Result<String, String> {
    run_selected(None)
}

pub fn run_case(case: &str) -> Result<String, String> {
    const IDS: &[&str] = &[
        "archive-roundtrip",
        "archive-rejections",
        "archive-paths",
        "archive-corporate",
        "archive-key-guards",
        "archive-targets",
        "sync-roundtrip",
        "sync-rejections",
        "sync-allowlist",
        "sync-plugin-ladder",
        "sync-user-ladder",
        "cli-handlers",
    ];
    if !IDS.contains(&case) {
        return Err("case".into());
    }
    run_selected(Some(case))
}
