//! Synthetic-only stateful rust-05 workload used by `vault-store --all`.
//! It executes the native providers and emits only typed values and digests.

use crate::atomic::StoreError;
use crate::canon::Canon;
use crate::cli::{self, CliError, Editor, Outcome, Terminal};
use crate::events::{self, Clock};
use crate::helpers;
use crate::json_yaml;
use crate::keyfile::{self, KeyFacts};
use crate::receipts::{ReceiptError, Receipts, Runtime};
use crate::secrets::{self, Adapter as SecretAdapter, SecretError};
use crate::settings;
use crate::store::Store;
use careerbrew_policy::vectors::Out;
use num_bigint::BigInt;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const CANARY: &str = "SYNTHETIC-RUST05-CANARY-7Q9";
pub const SECRET: &str = "SYNTHETIC-RUST05-SECRET-NOT-AN-AUTHENTICATION-VALUE";

pub fn sha(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn python_float_hex(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-inf"
        } else {
            "inf"
        }
        .into();
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if value == 0.0 {
        return format!("{sign}0x0.0p+0");
    }
    let bits = value.abs().to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1_u64 << 52) - 1);
    if exponent == 0 {
        format!("{sign}0x0.{fraction:013x}p-1022")
    } else {
        format!("{sign}0x1.{fraction:013x}p{:+}", exponent - 1023)
    }
}

fn typed(value: &Canon) -> Value {
    match value {
        Canon::Null => Value::Null,
        Canon::Bool(value) => json!(value),
        Canon::Int(value) => json!({"int": value.to_string()}),
        Canon::Float(value) => json!({"float": python_float_hex(*value)}),
        Canon::Str(value) => json!(value),
        Canon::Date(value) => json!({"date": value}),
        Canon::DateTime(value) => json!({"datetime": value}),
        Canon::List(items) => Value::Array(items.iter().map(typed).collect()),
        Canon::Map(items) => {
            let mut map = Map::new();
            for (key, value) in items {
                if let Canon::Str(key) = key {
                    map.insert(key.clone(), typed(value));
                }
            }
            Value::Object(map)
        }
    }
}

fn out_typed(value: &Out) -> Value {
    match value {
        Out::Null => Value::Null,
        Out::Bool(value) => json!(value),
        Out::Int(value) => json!({"int": value.to_string()}),
        Out::Num(value) => json!({"number": value}),
        Out::Str(value) => json!(value),
        Out::List(items) => Value::Array(items.iter().map(out_typed).collect()),
        Out::Dict(items) => {
            let mut map = Map::new();
            for (key, value) in items {
                map.insert(key.clone(), out_typed(value));
            }
            Value::Object(map)
        }
    }
}

fn call(cases: &mut Vec<Value>, id: &str, result: Result<Value, &'static str>) {
    let result = match result {
        Ok(value) => value,
        Err(error) => json!({"error": error}),
    };
    cases.push(json!({"id": id, "result": result}));
}

fn store_error(error: &StoreError) -> &'static str {
    match error {
        StoreError::Utf8 => "UnicodeDecodeError",
        StoreError::Vault => "VaultError",
        StoreError::Codec("json" | "jsonl") => "JSONDecodeError",
        StoreError::Codec("yaml") => "YAMLError",
        StoreError::NoKey => "VaultError",
        _ => "OSError",
    }
}

fn receipt_error(error: &ReceiptError) -> &'static str {
    match error {
        ReceiptError::Policy(error) => match error.class {
            "ValueError" => "ValueError",
            "KeyError" => "KeyError",
            "ApprovalRecordError" => "ApprovalRecordError",
            _ => "RuntimeError",
        },
        ReceiptError::Store(error) => store_error(error),
    }
}

fn identity(path: &Path) -> Result<Value, StoreError> {
    let metadata = std::fs::metadata(path)?;
    Ok(json!({
        "sha256": sha(&std::fs::read(path)?),
        "inode": metadata.ino(),
        "mtime_ns": metadata.mtime().saturating_mul(1_000_000_000).saturating_add(metadata.mtime_nsec()),
        "mode": metadata.permissions().mode() & 0o777,
    }))
}

fn any_sealed(folder: &Path) -> bool {
    std::fs::read_dir(folder)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let path = entry.path();
            if path.is_symlink()
                || path.file_name().is_some_and(|name| {
                    name.to_string_lossy().ends_with(".lock")
                        || name.to_string_lossy().ends_with(".tmp")
                })
            {
                return false;
            }
            if path.is_dir() {
                any_sealed(&path)
            } else {
                std::fs::read(path)
                    .ok()
                    .is_some_and(|raw| raw.starts_with(crate::MAGIC))
            }
        })
}

fn key(root: &Path, key_file: &Path, create: bool) -> Result<String, StoreError> {
    let has_sealed = || any_sealed(root);
    let home = root.parent().unwrap_or(root);
    let facts = KeyFacts {
        env_key_file: None,
        home,
        data_root: root,
        has_sealed_data: &has_sealed,
    };
    let bytes = keyfile::load_key(create, Some(key_file), &facts).map_err(|_| StoreError::Vault)?;
    String::from_utf8(bytes).map_err(|_| StoreError::Utf8)
}

pub fn key_for_data(root: &Path, key_file: &Path, create: bool) -> Result<String, StoreError> {
    key(root, key_file, create)
}

struct FakeTerminal {
    tty: bool,
    secret: String,
    restored: bool,
}

impl Terminal for FakeTerminal {
    fn stdin_is_terminal(&self) -> bool {
        self.tty
    }
    fn read_hidden(&mut self, _: &str) -> Result<String, CliError> {
        Ok(self.secret.clone())
    }
    fn restore_echo(&mut self) {
        self.restored = true;
    }
}

struct FakeKeychain;
impl SecretAdapter for FakeKeychain {
    fn allowed_backends(&self) -> Option<Vec<String>> {
        Some(vec!["env".into(), "keychain".into()])
    }
    fn env(&self, name: &str) -> Option<String> {
        (name == "SYNTHETIC_SECRET").then(|| SECRET.to_owned())
    }
    fn keychain_read(&self, _: &str, _: &str) -> Option<String> {
        None
    }
    fn op_read(&self, _: &str) -> Option<String> {
        None
    }
    fn keychain_write(&self, _: &str, _: &str, _: &str) -> Result<(), SecretError> {
        Ok(())
    }
    fn user(&self) -> String {
        "synthetic".into()
    }
    fn missing_event(&self, _: &str) {}
}

struct FakeEditor;
impl Editor for FakeEditor {
    fn run(&mut self, argv: &[String]) -> Result<(), CliError> {
        std::fs::write(
            argv.last().ok_or(CliError("editor-command"))?,
            "at: 2026-03-01 10:00:00+00:00\ncount: 9\n",
        )
        .map_err(|_| CliError("edit-write"))
    }
}

fn cli_case(cases: &mut Vec<Value>, id: &str, outcome: Outcome) {
    let output = outcome.stdout;
    assert!(!output.contains(CANARY) && !output.contains(SECRET));
    cases.push(json!({"id": id, "exit": outcome.code, "stdout_sha256": sha(output.as_bytes())}));
}

pub struct EventRuntime<'a, 'root> {
    store: &'a Store<'root>,
    ids: RefCell<std::vec::IntoIter<String>>,
}

impl<'a, 'root> EventRuntime<'a, 'root> {
    pub fn new(store: &'a Store<'root>, ids: Vec<String>) -> Self {
        Self {
            store,
            ids: RefCell::new(ids.into_iter()),
        }
    }
}

impl Runtime for EventRuntime<'_, '_> {
    fn now(&self) -> String {
        "2026-03-01T09:30:00+00:00".into()
    }
    fn new_id(&self) -> String {
        self.ids
            .borrow_mut()
            .next()
            .unwrap_or_else(|| "00000000".to_owned())
    }
    fn emit(&self, event: &str, status: &str) -> Result<(), ReceiptError> {
        let row = Canon::Map(vec![
            (Canon::Str("event".into()), Canon::Str(event.into())),
            (Canon::Str("status".into()), Canon::Str(status.into())),
            (
                Canon::Str("ts".into()),
                Canon::Str("2026-03-01T09:30:00Z".into()),
            ),
            (Canon::Str("v".into()), Canon::Str("0.0.0".into())),
        ]);
        self.store
            .append_jsonl(&self.store.root.join("logs/events.jsonl"), &[row])?;
        Ok(())
    }
}

pub struct FixedEventClock;
impl Clock for FixedEventClock {
    fn stamp(&self) -> String {
        "2026-03-01T09:30:00Z".into()
    }
    fn version(&self) -> String {
        "0.0.0".into()
    }
}

struct DisabledEventClock;
impl Clock for DisabledEventClock {
    fn stamp(&self) -> String {
        panic!("disabled events reached the stamp adapter")
    }
    fn version(&self) -> String {
        panic!("disabled events reached the version adapter")
    }
}

fn file_evidence(store: &Store, root: &Path) -> Result<Value, StoreError> {
    let mut rows = Vec::new();
    walk(store, root, root, &mut rows)?;
    rows.sort_by(|a: &Value, b: &Value| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(Value::Array(rows))
}

fn walk(
    store: &Store,
    root: &Path,
    folder: &Path,
    rows: &mut Vec<Value>,
) -> Result<(), StoreError> {
    let mut entries = std::fs::read_dir(folder)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            walk(store, root, &path, rows)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| StoreError::Codec("path"))?
            .to_string_lossy()
            .into_owned();
        if relative.ends_with(".lock") || relative.ends_with(".tmp") {
            continue;
        }
        let raw = std::fs::read(&path)?;
        let mut row = json!({
            "path": relative,
            "mode": format!("{:03o}", metadata.permissions().mode() & 0o777),
            "sealed": raw.starts_with(crate::MAGIC),
        });
        match store.read_text(&path, None, false) {
            Ok(text) => {
                let text = text.unwrap_or_default();
                row["text_sha256"] = json!(sha(text.as_bytes()));
                row["utf8_bytes"] = json!(text.len());
            }
            Err(StoreError::Utf8) => {
                row["raw_sha256"] = json!(sha(&raw));
                row["text"] = json!("non-utf8");
            }
            Err(error) => return Err(error),
        }
        rows.push(row);
    }
    Ok(())
}

fn plaintext(root: &Path, name: &str, text: &str) -> Result<PathBuf, StoreError> {
    let path = root.join("logs").join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, text)?;
    Ok(path)
}

fn outline_value(store: &Store, path: &Path, strict: bool) -> Result<Value, &'static str> {
    helpers::outline(store, path, strict)
        .map(|outline| {
            json!({
                "kind": outline.kind,
                "sealed": outline.sealed,
                "count": {"int": outline.count.to_string()},
                "keys": outline.keys,
            })
        })
        .map_err(|error| match error {
            helpers::OutlineError::Store(error) => store_error(&error),
            helpers::OutlineError::Value(_) => "ValueError",
        })
}

fn cached_value(value: &(Canon, bool)) -> Result<Value, &'static str> {
    let mut text = json_yaml::json_line(&value.0).map_err(|_| "cache-json")?;
    text.pop();
    Ok(json!({"hit": value.1, "sha256": sha(text.as_bytes())}))
}

fn outline_and_cache_cases(
    cases: &mut Vec<Value>,
    store: &Store,
    root: &Path,
) -> Result<(), String> {
    let projects = root.join("work/projects.yaml");
    store
        .write_text(
            &projects,
            &format!(
                "projects:\n  synthetic-project:\n    created_at: '2026-10-01T09:00:00Z'\n    stale_after_days: 7\n    sources:\n    - kind: doc\n      ref: r1\n  synthetic-project-two:\n    created_at: '2026-10-02T09:00:00Z'\n    owner: me\npeople:\n  {CANARY}: 3\n"
            ),
        )
        .map_err(|_| "projects")?;
    call(
        cases,
        "outline-projects",
        outline_value(store, &projects, true),
    );

    let state = root.join("tutor/state.json");
    let by_id = (0..40)
        .map(|index| format!("\"t{index:03}\": {index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let state_text = format!(
        "{{\"cards\": {{\"What is a heap? (explain it to a junior)\": {{\"due\": \"2026-10-23\", \"reps\": 2}}, \"{CANARY}\": {{\"due\": \"2026-10-24\", \"reps\": 1, \"lapses\": 0}}}}, \"by_id\": {{{by_id}}}, \"rows\": [[1, \"a\"], [2, null]]}}"
    );
    store.write_text(&state, &state_text).map_err(|_| "state")?;
    call(cases, "outline-json", outline_value(store, &state, false));

    let evidence = root.join("work/evidence.jsonl");
    store
        .write_text(
            &evidence,
            &format!("{{\"title\": \"{CANARY}\", \"tags\": [\"delivery\"]}}\n{{\"title\": \"second\", \"at\": 3}}\n"),
        )
        .map_err(|_| "evidence")?;
    call(
        cases,
        "outline-jsonl",
        outline_value(store, &evidence, true),
    );

    let day = root.join("day/log/2026-10-20.md");
    store
        .write_text(&day, &format!("# Day\n\nShipped {CANARY}\n"))
        .map_err(|_| "day")?;
    call(cases, "outline-text", outline_value(store, &day, true));

    let bad_json = root.join("work/bad-outline.json");
    store
        .write_text(&bad_json, "{\"x\":\n")
        .map_err(|_| "bad-json")?;
    call(
        cases,
        "outline-malformed-json",
        outline_value(store, &bad_json, true),
    );
    let bad_jsonl = root.join("work/bad-outline.jsonl");
    store
        .write_text(&bad_jsonl, "{\"x\": 1}\nnot-json\n")
        .map_err(|_| "bad-jsonl")?;
    call(
        cases,
        "outline-malformed-jsonl",
        outline_value(store, &bad_jsonl, true),
    );
    let bad_yaml = root.join("work/bad-outline.yaml");
    store
        .write_text(
            &bad_yaml,
            &format!("rules:\n  - match: {CANARY}\n    bad: [unclosed\n"),
        )
        .map_err(|_| "bad-yaml")?;
    call(
        cases,
        "outline-malformed-yaml",
        outline_value(store, &bad_yaml, true),
    );

    let key_48 = "k".repeat(48);
    for (id, key) in [
        ("outline-key-48", key_48.clone()),
        ("outline-key-48-newline", format!("{key_48}\n")),
        ("outline-key-48-two-newlines", format!("{key_48}\n\n")),
        ("outline-key-49", format!("{key_48}k")),
        ("outline-key-49-newline", format!("{key_48}k\n")),
    ] {
        let path = root.join(format!("work/{id}.json"));
        let value = Canon::Map(vec![(Canon::Str(key), int(1))]);
        let text = json_yaml::json_compact_python(&value).map_err(|_| "outline-key")?;
        store.write_text(&path, &text).map_err(|_| "outline-key")?;
        call(
            cases,
            id,
            outline_value(store, &path, false).map(|value| value["keys"].clone()),
        );
    }

    let digest_parts = vec![
        Canon::Str("synthetic".into()),
        map(&[
            ("b", int(2)),
            ("a", Canon::Str("café".into())),
            ("nan", Canon::Float(f64::NAN)),
            ("infinity", Canon::Float(f64::INFINITY)),
        ]),
        Canon::List(vec![
            int(1),
            Canon::Null,
            Canon::Bool(true),
            Canon::Float(1e-7),
            Canon::Float(-0.0),
        ]),
        Canon::Date("2026-03-01".into()),
        Canon::DateTime("2026-03-01T09:30:00+00:00".into()),
    ];
    call(
        cases,
        "digest-values",
        helpers::digest(&digest_parts)
            .map(|value| json!(value))
            .map_err(|_| "digest"),
    );
    call(
        cases,
        "digest-empty",
        helpers::digest(&[])
            .map(|value| json!(value))
            .map_err(|_| "digest"),
    );

    let key_parts = vec![
        Canon::Str("rust05h".into()),
        map(&[("b", int(2)), ("a", Canon::Str("café".into()))]),
        int(3),
    ];
    let cache_file = helpers::cache_path(root, &key_parts).map_err(|_| "cache-path")?;
    let missing = helpers::cached(
        store,
        &key_parts,
        None,
        || 100.0,
        || {
            Ok(map(&[
                ("title", Canon::Str(CANARY.into())),
                ("count", int(1)),
            ]))
        },
    )
    .map_err(|_| "cached-miss")?;
    call(cases, "cached-miss", cached_value(&missing));
    let cache_before = identity(&cache_file).map_err(|_| "identity")?;
    let hit = helpers::cached(
        store,
        &key_parts,
        None,
        || 100.0,
        || Err(StoreError::Codec("unexpected-cache")),
    )
    .map_err(|_| "cached-hit")?;
    call(cases, "cached-hit", cached_value(&hit));
    cases.push(json!({"id": "cached-hit-identity", "result": cache_before == identity(&cache_file).map_err(|_| "identity")?}));
    let fresh = helpers::cached(
        store,
        &key_parts,
        Some(10.0),
        || 105.0,
        || Err(StoreError::Codec("unexpected-cache")),
    )
    .map_err(|_| "cached-fresh")?;
    call(cases, "cached-fresh", cached_value(&fresh));
    let expired = helpers::cached(
        store,
        &key_parts,
        Some(10.0),
        || 111.5,
        || {
            Ok(map(&[
                ("title", Canon::Str(CANARY.into())),
                ("count", int(2)),
            ]))
        },
    )
    .map_err(|_| "cached-expired")?;
    call(cases, "cached-expired", cached_value(&expired));
    Ok(())
}

fn key_path_cases(cases: &mut Vec<Value>, root: &Path) -> Result<(), String> {
    let home = root.parent().ok_or("home")?.join("home");
    let (user, user_home) = keyfile::current_user().ok_or("current-user")?;
    let missing = "~careerbrew-rust05h-no-such-user-7q9/backup.key";
    let default = home.join(".config/careerbrew/backup.key");
    for (id, value, expected) in [
        ("key-path-home", "~", home.clone()),
        (
            "key-path-home-file",
            "~/backup.key",
            home.join("backup.key"),
        ),
        (
            "key-path-home-repeated-slash",
            "~//backup.key",
            home.join("backup.key"),
        ),
        (
            "key-path-home-repeated-slashes",
            "~////nested//backup.key",
            home.join("nested/backup.key"),
        ),
        (
            "key-path-home-traversal-lexical",
            "~//../backup.key",
            home.join("../backup.key"),
        ),
        ("key-path-empty-default", "", default),
        (
            "key-path-current-user",
            &format!("~{user}"),
            user_home.clone(),
        ),
        (
            "key-path-current-user-file",
            &format!("~{user}/backup.key"),
            user_home.join("backup.key"),
        ),
        (
            "key-path-current-user-repeated-slash",
            &format!("~{user}//backup.key"),
            user_home.join("backup.key"),
        ),
        (
            "key-path-current-user-repeated-slashes",
            &format!("~{user}///nested//backup.key"),
            user_home.join("nested/backup.key"),
        ),
        (
            "key-path-current-user-traversal-lexical",
            &format!("~{user}//../backup.key"),
            user_home.join("../backup.key"),
        ),
    ] {
        let actual = keyfile::key_path(Some(value), &home).ok();
        cases.push(json!({"id": id, "result": actual == Some(expected)}));
    }
    let missing_result = match keyfile::key_path(Some(missing), &home) {
        Err(keyfile::KeyError::UnknownUser) => json!({"error": "RuntimeError"}),
        Ok(_) => json!(false),
        Err(_) => json!({"error": "OSError"}),
    };
    cases.push(json!({"id": "key-path-missing-user", "result": missing_result}));
    Ok(())
}

type Snapshot = BTreeMap<String, (Vec<u8>, i64, i64)>;

fn snapshot_walk(base: &Path, folder: &Path, files: &mut Snapshot) -> Result<(), String> {
    let mut entries = std::fs::read_dir(folder)
        .map_err(|_| "snapshot")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "snapshot")?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let link = std::fs::symlink_metadata(&path).map_err(|_| "snapshot")?;
        if link.is_dir() && !link.file_type().is_symlink() {
            snapshot_walk(base, &path, files)?;
            continue;
        }
        let metadata = std::fs::metadata(&path).map_err(|_| "snapshot")?;
        if metadata.is_file() {
            let relative = path.strip_prefix(base).map_err(|_| "snapshot")?;
            files.insert(
                relative.to_string_lossy().replace('\\', "/"),
                (
                    std::fs::read(&path).map_err(|_| "snapshot")?,
                    metadata.mtime(),
                    metadata.mtime_nsec(),
                ),
            );
        }
    }
    Ok(())
}

fn snapshot(root: &Path) -> Result<Snapshot, String> {
    let mut files = BTreeMap::new();
    snapshot_walk(root, root, &mut files)?;
    Ok(files)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Show {
    exit: u8,
    stdout: String,
}

fn show_value(show: &Show) -> Value {
    json!({"exit": show.exit, "stdout": show.stdout})
}

fn show(
    store: &Store,
    root: &Path,
    outside: &Path,
    name: &str,
    keys: bool,
) -> Result<Show, String> {
    let outcome = cli::cmd_data_show(store, name, keys, false, Some("off"), &DisabledEventClock)
        .map_err(|_| "data-show")?;
    let stdout = outcome
        .stdout
        .replace(&root.to_string_lossy().into_owned(), "<data>")
        .replace(&outside.to_string_lossy().into_owned(), "<outside>");
    if stdout.contains(CANARY)
        || stdout.contains(SECRET)
        || stdout.contains("synthetic-outline-value-366")
    {
        return Err("data-show-privacy".into());
    }
    Ok(Show {
        exit: outcome.code,
        stdout,
    })
}

fn show_result(reference: Option<&Show>, alias: &Show, passed: bool) -> Value {
    json!({
        "reference": reference.map(show_value),
        "alias": show_value(alias),
        "passed": passed,
    })
}

fn data_show_cases(cases: &mut Vec<Value>, store: &Store, root: &Path) -> Result<(), String> {
    store
        .write_yaml(
            &root.join("profile.yaml"),
            &map(&[("current_level", int(3))]),
        )
        .map_err(|_| "profile")?;
    store
        .write_yaml(
            &root.join("cache/evidence/index.yaml"),
            &map(&[
                ("count", int(7)),
                ("note", Canon::Str("synthetic-outline-value-366".into())),
            ]),
        )
        .map_err(|_| "cache-evidence")?;
    store
        .write_yaml(
            &root.join("private/nested.yaml"),
            &map(&[("nested_only", int(11))]),
        )
        .map_err(|_| "nested")?;
    store
        .write_yaml(
            &root.join("privateish/literal.yaml"),
            &map(&[("literal_only", int(13))]),
        )
        .map_err(|_| "literal")?;
    let base = root.parent().ok_or("home")?;
    let outside = base.join("outside.yaml");
    std::fs::write(&outside, "outside: synthetic-outline-value-366\n").map_err(|_| "outside")?;
    std::os::unix::fs::symlink(&outside, root.join("escape.yaml")).map_err(|_| "symlink")?;
    let before = snapshot(base)?;

    let reference = show(store, root, &outside, "profile.yaml", false)?;
    let alias = show(store, root, &outside, "private/profile.yaml", false)?;
    let expected = Show {
        exit: 0,
        stdout: "current_level: 3\n".into(),
    };
    cases.push(json!({
        "id": "data-show-P01-personal",
        "result": show_result(Some(&reference), &alias, reference == expected && alias == reference),
    }));

    let reference = show(store, root, &outside, "cache/evidence/index.yaml", true)?;
    let alias = show(
        store,
        root,
        &outside,
        "private/cache/evidence/index.yaml",
        true,
    )?;
    let reference_lines = reference.stdout.lines().skip(1).collect::<Vec<_>>();
    cases.push(json!({
        "id": "data-show-P02-cache-keys",
        "result": show_result(
            Some(&reference),
            &alias,
            reference.exit == 0 && reference_lines == ["count: int", "note: str"] && alias == reference,
        ),
    }));

    let reference = show(store, root, &outside, "cache/evidence/index.yaml", false)?;
    let alias = show(
        store,
        root,
        &outside,
        "private/cache/evidence/index.yaml",
        false,
    )?;
    cases.push(json!({
        "id": "data-show-P03-cache-refusal",
        "result": show_result(
            Some(&reference),
            &alias,
            reference.exit == 3 && reference.stdout.contains("--keys") && alias == reference,
        ),
    }));

    let alias = show(store, root, &outside, "private/private/nested.yaml", true)?;
    let lines = alias.stdout.lines().collect::<Vec<_>>();
    cases.push(json!({
        "id": "data-show-P04-exact-once",
        "result": show_result(
            None,
            &alias,
            alias.exit == 0
                && lines.first().is_some_and(|line| line.starts_with("private/nested.yaml: yaml, encrypted on disk"))
                && lines.get(1..) == Some(["nested_only: int"].as_slice())
                && !alias.stdout.contains("11"),
        ),
    }));

    let absolute_inside = root.join("profile.yaml").to_string_lossy().into_owned();
    let absolute_outside = outside.to_string_lossy().into_owned();
    let controls = vec![
        (
            "C01-personal",
            show(store, root, &outside, "profile.yaml", false)?,
        ),
        (
            "C02-cache-keys",
            show(store, root, &outside, "cache/evidence/index.yaml", true)?,
        ),
        (
            "C03-cache-refusal",
            show(store, root, &outside, "cache/evidence/index.yaml", false)?,
        ),
        (
            "C04-missing",
            show(store, root, &outside, "private/missing.yaml", false)?,
        ),
        (
            "C05-directory",
            show(store, root, &outside, "private/cache", false)?,
        ),
        (
            "C06-traversal",
            show(store, root, &outside, "private/../outside.yaml", false)?,
        ),
        (
            "C07-symlink",
            show(store, root, &outside, "private/escape.yaml", false)?,
        ),
        (
            "C08-absolute-inside",
            show(store, root, &outside, &absolute_inside, false)?,
        ),
        (
            "C09-absolute-outside",
            show(store, root, &outside, &absolute_outside, false)?,
        ),
        (
            "C12-root-directory",
            show(store, root, &outside, "private/", false)?,
        ),
    ];
    for (id, actual) in controls {
        let passed = match id {
            "C01-personal" | "C08-absolute-inside" => actual == expected,
            "C02-cache-keys" => {
                actual.exit == 0
                    && actual.stdout.lines().skip(1).collect::<Vec<_>>()
                        == ["count: int", "note: str"]
            }
            "C03-cache-refusal" => actual.exit == 3 && actual.stdout.contains("--keys"),
            _ => actual.exit == 1 && actual.stdout.contains("no such data file"),
        };
        cases.push(json!({
            "id": format!("data-show-{id}"),
            "result": {"actual": show_value(&actual), "passed": passed},
        }));
    }

    let keys = show(store, root, &outside, "privateish/literal.yaml", true)?;
    let hidden = show(store, root, &outside, "privateish/literal.yaml", false)?;
    let passed = keys.exit == 0
        && keys.stdout.lines().skip(1).collect::<Vec<_>>() == ["literal_only: int"]
        && !keys.stdout.contains("13")
        && hidden.exit == 3
        && hidden.stdout.contains("--keys")
        && !hidden.stdout.contains("literal_only");
    cases.push(json!({
        "id": "data-show-C11-lookalike",
        "result": {"keys": show_value(&keys), "hidden": show_value(&hidden), "passed": passed},
    }));
    cases.push(json!({"id": "data-show-read-only", "result": before == snapshot(base)?}));
    Ok(())
}

pub fn run_all(root: &Path, key_file: &Path) -> Result<Value, String> {
    std::fs::create_dir_all(root).map_err(|_| "home")?;
    let keys = |create: bool| key(root, key_file, create);
    let store = Store::new(root, true, &keys);
    let mut cases = Vec::new();

    call(
        &mut cases,
        "json-missing-default",
        store
            .read_json(
                &root.join("missing.json"),
                map(&[("count", int(0)), ("ok", Canon::Bool(true))]),
                true,
            )
            .map(|v| typed(&v))
            .map_err(|e| store_error(&e)),
    );
    call(
        &mut cases,
        "yaml-missing-default",
        store
            .read_yaml(
                &root.join("missing.yaml"),
                Canon::List(vec![int(1), Canon::Str("yes".into())]),
                true,
            )
            .map(|v| typed(&v))
            .map_err(|e| store_error(&e)),
    );
    call(
        &mut cases,
        "jsonl-missing-empty",
        store
            .read_jsonl(&root.join("missing.jsonl"), true)
            .map(|v| Value::Array(v.iter().map(typed).collect()))
            .map_err(|e| store_error(&e)),
    );

    let text_path = root.join("notes/synthetic.txt");
    let original = format!(" café / 東京\n{CANARY}\n");
    call(
        &mut cases,
        "text-seal",
        store
            .write_text(&text_path, &original)
            .map(|_| Value::Null)
            .map_err(|e| store_error(&e)),
    );
    call(
        &mut cases,
        "text-read-digest",
        store
            .read_text(&text_path, None, true)
            .map(|v| json!({"sha256": sha(v.unwrap_or_default().as_bytes())}))
            .map_err(|e| store_error(&e)),
    );
    let before = identity(&text_path).map_err(|_| "identity")?;
    call(
        &mut cases,
        "text-noop",
        store
            .write_text_if_changed(&text_path, &original)
            .map(|v| json!(v))
            .map_err(|e| store_error(&e)),
    );
    cases.push(json!({"id": "text-noop-identity", "result": before == identity(&text_path).map_err(|_| "identity")?}));

    let json_path = root.join("record.json");
    let json_value = map(&[
        (
            "big",
            Canon::Int(
                "12089258196146291747061791208925819614629174706179"
                    .parse::<BigInt>()
                    .map_err(|_| "int")?,
            ),
        ),
        ("date", Canon::Date("2026-03-01".into())),
        ("at", Canon::DateTime("2026-03-01T09:30:00+00:00".into())),
        (
            "nested",
            map(&[("yes", Canon::Str("yes".into())), ("nothing", Canon::Null)]),
        ),
    ]);
    call(
        &mut cases,
        "json-write",
        store
            .write_json(&json_path, &json_value)
            .map(|_| Value::Null)
            .map_err(|e| store_error(&e)),
    );
    let json_read = store
        .read_json(&json_path, Canon::Null, true)
        .map_err(|_| "json-read")?;
    call(&mut cases, "json-read", Ok(typed(&json_read)));
    let json_before = identity(&json_path).map_err(|_| "identity")?;
    call(
        &mut cases,
        "json-noop",
        store
            .write_json_if_changed(&json_path, &json_read)
            .map(|v| json!(v))
            .map_err(|e| store_error(&e)),
    );
    cases.push(json!({"id": "json-noop-identity", "result": json_before == identity(&json_path).map_err(|_| "identity")?}));

    let yaml_path = root.join("record.yaml");
    let yaml_value = map(&[
        ("at", Canon::DateTime("2026-03-01T09:30:00+00:00".into())),
        ("date", Canon::Date("2026-03-01".into())),
        ("literal", Canon::Str("yes".into())),
        ("count", int(8)),
    ]);
    call(
        &mut cases,
        "yaml-write",
        store
            .write_yaml(&yaml_path, &yaml_value)
            .map(|_| Value::Null)
            .map_err(|e| store_error(&e)),
    );
    call(
        &mut cases,
        "yaml-read",
        store
            .read_yaml(&yaml_path, Canon::Null, true)
            .map(|v| typed(&v))
            .map_err(|e| store_error(&e)),
    );
    let bad = plaintext(root, "bad.jsonl", "{not-json}\n").map_err(|_| "write")?;
    call(
        &mut cases,
        "jsonl-malformed",
        store
            .read_jsonl(&bad, true)
            .map(|v| Value::Array(v.iter().map(typed).collect()))
            .map_err(|e| store_error(&e)),
    );
    let bad_utf8 = root.join("logs/bad-utf8.txt");
    std::fs::write(&bad_utf8, [0xff]).map_err(|_| "write")?;
    call(
        &mut cases,
        "utf8-noop-refusal",
        store
            .write_text_if_changed(&bad_utf8, "replacement")
            .map(|v| json!(v))
            .map_err(|e| store_error(&e)),
    );

    let rows_path = root.join("rows.jsonl");
    call(
        &mut cases,
        "jsonl-write",
        store
            .write_jsonl(&rows_path, &[row("a", 1), row("b", 2)])
            .map(|_| Value::Null)
            .map_err(|e| store_error(&e)),
    );
    call(
        &mut cases,
        "jsonl-append",
        store
            .append_jsonl(&rows_path, &[row("c", 3)])
            .map(|v| json!({"int": v.to_string()}))
            .map_err(|e| store_error(&e)),
    );
    call(
        &mut cases,
        "jsonl-upsert-last-copy",
        store
            .upsert_jsonl(
                &rows_path,
                &[row("b", 20), row("d", 4), row("d", 5)],
                &["v"],
            )
            .map(|(a, r)| json!({"tuple": [{"int": a.to_string()}, {"int": r.to_string()}]}))
            .map_err(|e| store_error(&e)),
    );
    call(
        &mut cases,
        "jsonl-read",
        store
            .read_jsonl(&rows_path, true)
            .map(|v| Value::Array(v.iter().map(typed).collect()))
            .map_err(|e| store_error(&e)),
    );
    let upsert_before = identity(&rows_path).map_err(|_| "identity")?;
    call(
        &mut cases,
        "jsonl-upsert-noop",
        store
            .upsert_jsonl(&rows_path, &[row("b", 20)], &["v"])
            .map(|(a, r)| json!({"tuple": [{"int": a.to_string()}, {"int": r.to_string()}]}))
            .map_err(|e| store_error(&e)),
    );
    cases.push(json!({"id": "jsonl-noop-identity", "result": upsert_before == identity(&rows_path).map_err(|_| "identity")?}));

    let runtime = EventRuntime::new(&store, vec!["a1b2c3d4".into(), "e5f60718".into()]);
    let receipts = Receipts::new(&store, &runtime);
    let requested = receipts
        .request("  Synthetic   approval  ", " synthetic detail ", "agent")
        .map_err(|_| "receipt-request")?;
    call(&mut cases, "receipt-request", Ok(out_typed(&requested)));
    call(
        &mut cases,
        "receipt-pending",
        receipts
            .pending()
            .map(|v| Value::Array(v.iter().map(out_typed).collect()))
            .map_err(|e| receipt_error(&e)),
    );
    let id = match &requested {
        Out::Dict(items) => items
            .iter()
            .find(|(k, _)| k == "id")
            .and_then(|(_, v)| match v {
                Out::Str(id) => Some(id.clone()),
                _ => None,
            }),
        _ => None,
    }
    .ok_or("receipt-id")?;
    call(
        &mut cases,
        "receipt-decide",
        receipts
            .decide(&id, "approved", " synthetic note ")
            .map(|v| out_typed(&v))
            .map_err(|e| receipt_error(&e)),
    );
    call(
        &mut cases,
        "receipt-get",
        receipts
            .get(&id)
            .map(|v| out_typed(&v))
            .map_err(|e| receipt_error(&e)),
    );
    call(
        &mut cases,
        "receipt-guidance",
        receipts
            .guidance(10)
            .map(|v| Value::Array(v.iter().map(out_typed).collect()))
            .map_err(|e| receipt_error(&e)),
    );
    call(
        &mut cases,
        "receipt-already-decided",
        receipts
            .decide(&id, "rejected", "")
            .map(|v| out_typed(&v))
            .map_err(|e| receipt_error(&e)),
    );
    call(
        &mut cases,
        "receipt-unknown",
        receipts
            .get("ffffffff")
            .map(|v| out_typed(&v))
            .map_err(|e| receipt_error(&e)),
    );

    let mut terminal = FakeTerminal {
        tty: true,
        secret: String::new(),
        restored: false,
    };
    let mut editor = FakeEditor;
    cli_case(
        &mut cases,
        "data-edit-missing",
        cli::cmd_data_edit(
            &store,
            "absent.yaml",
            "synthetic-editor",
            &terminal,
            &mut editor,
        )
        .map_err(|_| "data-edit")?,
    );
    terminal.tty = false;
    cli_case(
        &mut cases,
        "data-edit-nonterminal",
        cli::cmd_data_edit(
            &store,
            "record.yaml",
            "synthetic-editor",
            &terminal,
            &mut editor,
        )
        .map_err(|_| "data-edit")?,
    );
    let edit_before = identity(&yaml_path).map_err(|_| "identity")?;
    terminal.tty = true;
    cli_case(
        &mut cases,
        "data-edit-success",
        cli::cmd_data_edit(
            &store,
            "record.yaml",
            "synthetic-editor",
            &terminal,
            &mut editor,
        )
        .map_err(|_| "data-edit")?,
    );
    let edit_after = identity(&yaml_path).map_err(|_| "identity")?;
    cases.push(json!({"id": "data-edit-rewrites", "result": edit_before["inode"] != edit_after["inode"] && !root.join(".edit/record.yaml").exists()}));

    let keychain = FakeKeychain;
    terminal.tty = false;
    cli_case(
        &mut cases,
        "secret-set-nonterminal",
        cli::cmd_secret_set("synthetic.service", &mut terminal, &keychain)
            .map_err(|_| "secret-set")?,
    );
    terminal.tty = true;
    cli_case(
        &mut cases,
        "secret-set-empty",
        cli::cmd_secret_set("synthetic.service", &mut terminal, &keychain)
            .map_err(|_| "secret-set")?,
    );
    terminal.secret = SECRET.into();
    cli_case(
        &mut cases,
        "secret-set-success",
        cli::cmd_secret_set("synthetic.service", &mut terminal, &keychain)
            .map_err(|_| "secret-set")?,
    );
    assert!(terminal.restored);

    call(
        &mut cases,
        "secret-backend-keychain",
        secrets::backend_of("keychain:synthetic")
            .map(|v| json!(v))
            .map_err(|_| "SecretError"),
    );
    call(
        &mut cases,
        "secret-backend-op",
        secrets::backend_of("op://synthetic/item/field")
            .map(|v| json!(v))
            .map_err(|_| "SecretError"),
    );
    call(
        &mut cases,
        "secret-backend-env",
        secrets::backend_of("env:SYNTHETIC_SECRET")
            .map(|v| json!(v))
            .map_err(|_| "SecretError"),
    );
    call(
        &mut cases,
        "secret-backend-literal",
        secrets::backend_of(SECRET)
            .map(|v| json!(v))
            .map_err(|_| "SecretError"),
    );
    call(
        &mut cases,
        "secret-resolve-env",
        secrets::resolve(&keychain, "env:SYNTHETIC_SECRET")
            .map(|v| json!(v == SECRET))
            .map_err(|_| "MissingSecretError"),
    );

    let events = store
        .read_jsonl(&root.join("logs/events.jsonl"), true)
        .map_err(|_| "events")?;
    let event_rows: Vec<Value> = events
        .iter()
        .map(|row| {
            let mut map = Map::new();
            if let Canon::Map(items) = row {
                for (key, value) in items {
                    if matches!(key, Canon::Str(name) if name == "event" || name == "status") {
                        map.insert(
                            match key {
                                Canon::Str(name) => name.clone(),
                                _ => unreachable!(),
                            },
                            typed(value),
                        );
                    }
                }
            }
            Value::Object(map)
        })
        .collect();
    cases.push(json!({"id": "receipt-events", "result": event_rows}));

    let settings_file = settings::settings_path(root);
    cases.push(json!({
        "id": "settings-path",
        "result": settings_file.strip_prefix(root).map_err(|_| "settings-path")?.to_string_lossy(),
    }));
    call(
        &mut cases,
        "settings-missing",
        settings::read_settings(&store)
            .map(|value| typed(&value))
            .map_err(|error| store_error(&error)),
    );
    std::fs::write(&settings_file, "").map_err(|_| "settings-empty")?;
    call(
        &mut cases,
        "settings-empty",
        settings::read_settings(&store)
            .map(|value| typed(&value))
            .map_err(|error| store_error(&error)),
    );
    let settings_value = map(&[
        ("questions_per_day", int(3)),
        ("work_hours", Canon::Float(7.5)),
        ("notify_events", Canon::Bool(true)),
        (
            "nested",
            map(&[("yes", Canon::Str("yes".into())), ("none", Canon::Null)]),
        ),
    ]);
    call(
        &mut cases,
        "settings-write",
        settings::write_settings(&store, &settings_value)
            .map(|_| Value::Null)
            .map_err(|error| store_error(&error)),
    );
    call(
        &mut cases,
        "settings-read",
        settings::read_settings(&store)
            .map(|value| typed(&value))
            .map_err(|error| store_error(&error)),
    );
    let settings_before = identity(&settings_file).map_err(|_| "identity")?;
    call(
        &mut cases,
        "settings-rewrite",
        settings::write_settings(&store, &settings_value)
            .map(|_| Value::Null)
            .map_err(|error| store_error(&error)),
    );
    cases.push(json!({"id": "settings-write-rewrites", "result": settings_before["inode"] != identity(&settings_file).map_err(|_| "identity")?["inode"]}));

    let clean_fields = map(&[
        ("event", Canon::Str("config.set".into())),
        ("status", Canon::Str("a".repeat(48))),
        ("command", Canon::Str("a".repeat(49))),
        ("route", Canon::Str("ok\n".into())),
        ("action", Canon::Str("ok\n\n".into())),
        ("error", Canon::Str("ValueError".into())),
        ("source_kind", Canon::Str("HTTP".into())),
        (
            "count",
            Canon::Int(
                "1208925819614629174706179"
                    .parse::<BigInt>()
                    .map_err(|_| "int")?,
            ),
        ),
        ("ratio", Canon::Float(1.5)),
        ("enabled", Canon::Bool(true)),
        ("note", Canon::Str(CANARY.into())),
        ("nothing", Canon::Null),
        ("items", Canon::List(vec![Canon::Str("dropped".into())])),
        ("nested", map(&[("dropped", Canon::Bool(true))])),
    ]);
    call(
        &mut cases,
        "event-clean",
        events::clean(&clean_fields)
            .map(|value| typed(&value))
            .map_err(|_| "event-clean"),
    );
    call(
        &mut cases,
        "event-record",
        events::record(
            "config.set",
            &map(&[
                ("status", Canon::Str("ok".into())),
                ("note", Canon::Str(CANARY.into())),
            ]),
            "2026-03-01T09:30:00Z",
            "0.0.0",
        )
        .map(|value| typed(&value))
        .map_err(|_| "event-record"),
    );
    call(
        &mut cases,
        "event-write-row",
        events::write_row(root, &map(&[("b", int(1)), ("a", Canon::Str("x".into()))]))
            .map(|_| Value::Null)
            .map_err(|_| "event-write-row"),
    );
    let log = root.join("logs/events.jsonl");
    let last_line = std::fs::read_to_string(&log)
        .map_err(|_| "event-line")?
        .lines()
        .last()
        .ok_or("event-line")?
        .to_owned();
    call(&mut cases, "event-write-row-line", Ok(json!(last_line)));
    call(
        &mut cases,
        "event-emit-config",
        events::emit(
            root,
            Some("on"),
            "config.set",
            &map(&[
                ("status", Canon::Str("ok".into())),
                ("note", Canon::Str(CANARY.into())),
                ("count", int(2)),
            ]),
            &FixedEventClock,
        )
        .map(|_| Value::Null)
        .map_err(|_| "event-emit"),
    );
    call(
        &mut cases,
        "event-emit-error",
        events::emit(
            root,
            Some("on"),
            "cli.error",
            &map(&[
                ("command", Canon::Str("config".into())),
                ("error", Canon::Str("ValueError".into())),
            ]),
            &FixedEventClock,
        )
        .map(|_| Value::Null)
        .map_err(|_| "event-emit"),
    );
    let all_lines = std::fs::read_to_string(&log)
        .map_err(|_| "event-lines")?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let lines = all_lines[all_lines.len().saturating_sub(3)..]
        .iter()
        .map(|line| json!(line))
        .collect::<Vec<_>>();
    call(&mut cases, "event-lines", Ok(Value::Array(lines)));
    let float_row = map(&[
        ("nan", Canon::Float(f64::NAN)),
        ("infinity", Canon::Float(f64::INFINITY)),
        ("negative_infinity", Canon::Float(f64::NEG_INFINITY)),
        ("negative_zero", Canon::Float(-0.0)),
        ("exponent", Canon::Float(1e-7)),
        ("low_fixed", Canon::Float(1e-4)),
        ("low_scientific", Canon::Float(1e-5)),
        ("high_fixed", Canon::Float(1e15)),
        ("high_scientific", Canon::Float(1e16)),
    ]);
    call(
        &mut cases,
        "event-write-float-row",
        events::write_row(root, &float_row)
            .map(|_| Value::Null)
            .map_err(|_| "event-write-float-row"),
    );
    let float_line = std::fs::read_to_string(&log)
        .map_err(|_| "event-float-line")?
        .lines()
        .last()
        .ok_or("event-float-line")?
        .to_owned();
    call(&mut cases, "event-write-float-line", Ok(json!(float_line)));

    let off_before = identity(&log).map_err(|_| "identity")?;
    call(
        &mut cases,
        "event-off",
        events::emit(
            root,
            Some("off"),
            "config.set",
            &map(&[("status", Canon::Str("blocked".into()))]),
            &DisabledEventClock,
        )
        .map(|_| Value::Null)
        .map_err(|_| "event-off"),
    );
    cases.push(json!({"id": "event-off-identity", "result": off_before == identity(&log).map_err(|_| "identity")?}));

    outline_and_cache_cases(&mut cases, &store, root)?;
    key_path_cases(&mut cases, root)?;
    data_show_cases(&mut cases, &store, root)?;

    cases.push(json!({"id": "files", "result": file_evidence(&store, root).map_err(|_| "files")?}));
    Ok(json!({"schema": "careerbrew-rust05-component-v1", "cases": cases}))
}

fn int(value: i64) -> Canon {
    Canon::Int(BigInt::from(value))
}

fn map(items: &[(&str, Canon)]) -> Canon {
    Canon::Map(
        items
            .iter()
            .map(|(key, value)| (Canon::Str((*key).into()), value.clone()))
            .collect(),
    )
}

fn row(id: &str, value: i64) -> Canon {
    map(&[("id", Canon::Str(id.into())), ("v", int(value))])
}
