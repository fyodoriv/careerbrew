//! Synthetic rust-05i whole-vault scenarios. Reports contain paths, modes,
//! semantic content digests and typed errors only.

use crate::bulk::{self, BulkError, RewritePoint};
use crate::store::Store;
use crate::synthetic::{identity, key_for_data, sha};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const PLAIN: &str = "synthetic bulk record\n";
type Identities = BTreeMap<String, Value>;

fn relative(root: &Path, path: &Path) -> Result<String, String> {
    path.strip_prefix(root)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .map_err(|_| "bulk-path".into())
}

fn collect_paths(root: &Path, folder: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(folder).map_err(|_| "bulk-list")? {
        let path = entry.map_err(|_| "bulk-list")?.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| "bulk-stat")?;
        if metadata.file_type().is_symlink() || metadata.is_file() {
            paths.push(path);
        } else if metadata.is_dir() {
            collect_paths(root, &path, paths)?;
        }
    }
    paths.sort_by_key(|path| relative(root, path).unwrap_or_default());
    Ok(())
}

fn paths(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    collect_paths(root, root, &mut paths)?;
    Ok(paths)
}

fn tree_identities(root: &Path) -> Result<Identities, String> {
    let mut result = BTreeMap::new();
    for path in paths(root)? {
        if std::fs::symlink_metadata(&path)
            .map_err(|_| "bulk-stat")?
            .file_type()
            .is_symlink()
        {
            continue;
        }
        result.insert(
            relative(root, &path)?,
            identity(&path).map_err(|_| "bulk-identity")?,
        );
    }
    Ok(result)
}

fn snapshot(root: &Path, key_file: &Path, store: &Store) -> Result<Value, String> {
    let mut rows = Vec::new();
    for path in paths(root)? {
        let rel = relative(root, &path)?;
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| "bulk-stat")?;
        if metadata.file_type().is_symlink() {
            rows.push(json!({"path": rel, "symlink": true}));
            continue;
        }
        let raw = std::fs::read(&path).map_err(|_| "bulk-read")?;
        let sealed = raw.starts_with(crate::MAGIC);
        let mut row = json!({
            "path": rel,
            "mode": format!("{:03o}", metadata.permissions().mode() & 0o777),
            "sealed": sealed,
        });
        if sealed && !key_file.exists() {
            row["read_error"] = json!("FileNotFoundError");
        } else {
            match bulk::open_bytes(store, &raw, false) {
                Ok(plain) => {
                    if !sealed && bulk::is_fernet_token(&path).map_err(|_| "bulk-read")? {
                        row["bundle"] = json!("fernet");
                    } else {
                        row["sha256"] = json!(sha(&plain));
                    }
                }
                Err(error) => row["read_error"] = json!(error.python_kind()),
            }
        }
        rows.push(row);
    }
    Ok(Value::Array(rows))
}

fn typed_count(value: usize) -> Value {
    json!({"int": value.to_string()})
}

fn typed_status(value: bulk::Status) -> Value {
    json!({
        "enabled": value.enabled,
        "files": typed_count(value.files),
        "sealed": typed_count(value.sealed),
        "plaintext": typed_count(value.files - value.sealed),
    })
}

#[allow(clippy::too_many_arguments)]
fn record(
    cases: &mut Vec<Value>,
    id: &str,
    root: &Path,
    key_file: &Path,
    store: &Store,
    operation: impl FnOnce() -> Result<Value, BulkError>,
    before: Option<&Identities>,
) -> Result<(), String> {
    let result = match operation() {
        Ok(value) => value,
        Err(error) => json!({"error": error.python_kind()}),
    };
    let key_present = key_file.exists();
    let key_mode = if key_present {
        Some(format!(
            "{:03o}",
            std::fs::metadata(key_file)
                .map_err(|_| "bulk-key")?
                .permissions()
                .mode()
                & 0o777
        ))
    } else {
        None
    };
    let mut row = json!({
        "id": id,
        "result": result,
        "state": snapshot(root, key_file, store)?,
        "key": {"present": key_present, "mode": key_mode},
    });
    if let Some(before) = before {
        row["mutation_free"] = json!(*before == tree_identities(root)?);
    }
    cases.push(row);
    Ok(())
}

fn scenario(
    base: &Path,
    name: &str,
    encryption: bool,
    run: impl for<'a> FnOnce(&Path, &Path, &Store<'a>) -> Result<(), String>,
) -> Result<(), String> {
    let root = base.join(name).join("data");
    std::fs::create_dir_all(&root).map_err(|_| "bulk-home")?;
    let key_file = base.join(name).join("keys/backup.key");
    let keys = |create| key_for_data(&root, &key_file, create);
    let store = Store::new(&root, encryption, &keys);
    run(&root, &key_file, &store)
}

fn plain_file(path: &Path, text: &str) -> Result<PathBuf, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "bulk-write")?;
    }
    std::fs::write(path, text).map_err(|_| "bulk-write")?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o640))
        .map_err(|_| "bulk-mode")?;
    Ok(path.to_path_buf())
}

fn sealed_file(path: &Path, raw: &[u8], store: &Store) -> Result<PathBuf, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "bulk-write")?;
    }
    let sealed = bulk::seal_bytes(store, raw).map_err(|_| "bulk-seal")?;
    std::fs::write(path, sealed).map_err(|_| "bulk-write")?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o640))
        .map_err(|_| "bulk-mode")?;
    Ok(path.to_path_buf())
}

fn listed_paths(store: &Store, root: &Path) -> Result<Vec<String>, BulkError> {
    let mut result = bulk::files(store)?
        .iter()
        .map(|path| {
            path.strip_prefix(root)
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .map_err(|_| BulkError::Io)
        })
        .collect::<Result<Vec<_>, _>>()?;
    result.sort();
    Ok(result)
}

fn operation(
    name: &str,
    store: &Store,
    targets: &[PathBuf],
    fault: impl FnMut(&Path, RewritePoint) -> Result<(), BulkError>,
) -> Result<usize, BulkError> {
    match name {
        "seal-files" => bulk::seal_files_with(store, targets, fault),
        "encrypt-all" => bulk::encrypt_all_with_targets(store, targets, fault),
        _ => bulk::decrypt_all_with_targets(store, targets, fault),
    }
}

pub(crate) fn run(cases: &mut Vec<Value>, data_root: &Path) -> Result<(), String> {
    let base = data_root.parent().ok_or("bulk-home")?.join("bulk");

    scenario(&base, "enumeration", true, |root, key_file, store| {
        let plain = plain_file(&root.join("work/plain.txt"), PLAIN)?;
        let sealed = sealed_file(&root.join("work/sealed.txt"), PLAIN.as_bytes(), store)?;
        let near = plain_file(&root.join("learn/synthetic/replay/07/near.txt"), PLAIN)?;
        let checkout = root.join("learn/synthetic/replay/7");
        plain_file(&checkout.join(".git/HEAD"), "ref: refs/heads/main\n")?;
        plain_file(&checkout.join("code.py"), "value = 1\n")?;
        for folder in ["logs", "out", ".edit", "plugins"] {
            plain_file(&root.join(folder).join("plain.txt"), PLAIN)?;
        }
        plain_file(&root.join("work/ignored.lock"), PLAIN)?;
        plain_file(&root.join("work/ignored.tmp"), PLAIN)?;
        std::os::unix::fs::symlink(&plain, root.join("work/link.txt")).map_err(|_| "bulk-link")?;
        let mut expected = [&near, &plain, &sealed]
            .map(|path| relative(root, path))
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        expected.sort();
        record(
            cases,
            "bulk-enumeration-status",
            root,
            key_file,
            store,
            || {
                Ok(json!({
                    "paths": listed_paths(store, root)?,
                    "status": typed_status(bulk::status(store)?),
                }))
            },
            None,
        )?;
        let off_keys = |create| key_for_data(root, key_file, create);
        let off = Store::new(root, false, &off_keys);
        record(
            cases,
            "bulk-status-environment-off",
            root,
            key_file,
            &off,
            || bulk::status(&off).map(typed_status),
            None,
        )?;
        plain_file(&root.join(bulk::MARKER), "synthetic existing marker\n")?;
        let plain_before = identity(&plain).map_err(|_| "bulk-identity")?;
        record(
            cases,
            "bulk-marker-disables-seal-files",
            root,
            key_file,
            store,
            || {
                let count = bulk::seal_files(store, std::slice::from_ref(&plain))?;
                Ok(json!({
                    "count": typed_count(count),
                    "unchanged": identity(&plain).map_err(|_| BulkError::Io)? == plain_before,
                    "status": typed_status(bulk::status(store)?),
                }))
            },
            None,
        )?;
        if listed_paths(store, root).map_err(|_| "bulk-list")? != expected {
            return Err("bulk-enumeration".into());
        }
        Ok(())
    })?;

    scenario(&base, "seal-files", true, |root, key_file, store| {
        let first = plain_file(&root.join("work/first.txt"), PLAIN)?;
        let sealed = sealed_file(&root.join("work/already.txt"), PLAIN.as_bytes(), store)?;
        let log = plain_file(&root.join("logs/plain.txt"), PLAIN)?;
        let missing = root.join("work/missing.txt");
        let outside = plain_file(&base.join("seal-files/outside.txt"), PLAIN)?;
        let link = root.join("work/link.txt");
        std::os::unix::fs::symlink(&first, &link).map_err(|_| "bulk-link")?;
        let targets = vec![
            first.clone(),
            sealed.clone(),
            log,
            missing,
            root.join("work"),
            outside,
            link,
        ];
        let sealed_before = identity(&sealed).map_err(|_| "bulk-identity")?;
        record(
            cases,
            "bulk-seal-files",
            root,
            key_file,
            store,
            || {
                let count = bulk::seal_files(store, &targets)?;
                Ok(json!({
                    "count": typed_count(count),
                    "sealed_noop": identity(&sealed).map_err(|_| BulkError::Io)? == sealed_before,
                }))
            },
            None,
        )
    })?;

    scenario(&base, "encrypt-all", true, |root, key_file, store| {
        plain_file(&root.join("work/first.txt"), PLAIN)?;
        plain_file(&root.join("work/second.txt"), PLAIN)?;
        let sealed = sealed_file(&root.join("work/already.txt"), PLAIN.as_bytes(), store)?;
        let log = plain_file(&root.join("logs/plain.txt"), PLAIN)?;
        let replay = plain_file(&root.join("learn/synthetic/replay/7/code.py"), PLAIN)?;
        plain_file(
            &root.join("learn/synthetic/replay/7/.git/HEAD"),
            "ref: refs/heads/main\n",
        )?;
        plain_file(&root.join(bulk::MARKER), "synthetic existing marker\n")?;
        let sealed_before = identity(&sealed).map_err(|_| "bulk-identity")?;
        record(
            cases,
            "bulk-encrypt-all",
            root,
            key_file,
            store,
            || {
                let count = bulk::encrypt_all(store)?;
                Ok(json!({
                    "count": typed_count(count),
                    "sealed_noop": identity(&sealed).map_err(|_| BulkError::Io)? == sealed_before,
                }))
            },
            None,
        )?;
        let identities = tree_identities(root)?;
        record(
            cases,
            "bulk-encrypt-all-noop",
            root,
            key_file,
            store,
            || {
                let count = bulk::encrypt_all(store)?;
                let unchanged = identities == tree_identities(root).map_err(|_| BulkError::Io)?;
                Ok(json!({"count": typed_count(count), "unchanged": unchanged}))
            },
            None,
        )?;
        if std::fs::read(log)
            .map_err(|_| "bulk-read")?
            .starts_with(crate::MAGIC)
            || std::fs::read(replay)
                .map_err(|_| "bulk-read")?
                .starts_with(crate::MAGIC)
        {
            return Err("bulk-plain-directory".into());
        }
        Ok(())
    })?;

    for kind in ["backup-suffix", "sync-suffix", "raw-fernet"] {
        scenario(
            &base,
            &format!("bundle-{kind}"),
            true,
            |root, key_file, store| {
                plain_file(&root.join("record.txt"), PLAIN)?;
                plain_file(&root.join(bulk::MARKER), "synthetic existing marker\n")?;
                if kind == "raw-fernet" {
                    let token =
                        bulk::seal_bytes(store, b"synthetic bundle").map_err(|_| "bulk-seal")?;
                    std::fs::write(
                        root.join("bundle-without-extension"),
                        &token[crate::MAGIC.len()..],
                    )
                    .map_err(|_| "bulk-write")?;
                } else {
                    let name = if kind == "backup-suffix" {
                        "archive.CBK"
                    } else {
                        "archive.CbSyNc"
                    };
                    std::fs::write(root.join(name), "synthetic named bundle\n")
                        .map_err(|_| "bulk-write")?;
                }
                let before = tree_identities(root)?;
                record(
                    cases,
                    &format!("bulk-encrypt-refuses-{kind}"),
                    root,
                    key_file,
                    store,
                    || bulk::encrypt_all(store).map(typed_count),
                    Some(&before),
                )
            },
        )?;
    }

    scenario(
        &base,
        "encrypt-sealed-missing-key",
        true,
        |root, key_file, store| {
            sealed_file(&root.join("old.txt"), PLAIN.as_bytes(), store)?;
            plain_file(&root.join("new.txt"), PLAIN)?;
            plain_file(&root.join(bulk::MARKER), "synthetic existing marker\n")?;
            std::fs::remove_file(key_file).map_err(|_| "bulk-key")?;
            record(
                cases,
                "bulk-encrypt-sealed-missing-key",
                root,
                key_file,
                store,
                || bulk::encrypt_all(store).map(typed_count),
                None,
            )
        },
    )?;

    scenario(
        &base,
        "encrypt-invalid-key",
        true,
        |root, key_file, store| {
            plain_file(&root.join("record.txt"), PLAIN)?;
            plain_file(&root.join(bulk::MARKER), "synthetic existing marker\n")?;
            std::fs::create_dir_all(key_file.parent().ok_or("bulk-key")?)
                .map_err(|_| "bulk-key")?;
            std::fs::write(key_file, b"not-a-valid-fernet-key").map_err(|_| "bulk-key")?;
            record(
                cases,
                "bulk-encrypt-invalid-key",
                root,
                key_file,
                store,
                || bulk::encrypt_all(store).map(typed_count),
                None,
            )
        },
    )?;

    scenario(&base, "decrypt-all", true, |root, key_file, store| {
        sealed_file(&root.join("work/sealed.txt"), PLAIN.as_bytes(), store)?;
        plain_file(&root.join("work/plain.txt"), PLAIN)?;
        sealed_file(
            &root.join("learn/synthetic/replay/7/code.py"),
            b"value = 1\n",
            store,
        )?;
        sealed_file(
            &root.join("learn/synthetic/replay/7/.git/HEAD"),
            b"ref: refs/heads/main\n",
            store,
        )?;
        plain_file(&root.join("logs/plain.txt"), PLAIN)?;
        record(
            cases,
            "bulk-decrypt-all",
            root,
            key_file,
            store,
            || bulk::decrypt_all(store).map(typed_count),
            None,
        )?;
        let identities = tree_identities(root)?;
        record(
            cases,
            "bulk-decrypt-all-noop",
            root,
            key_file,
            store,
            || {
                let count = bulk::decrypt_all(store)?;
                let after = tree_identities(root).map_err(|_| BulkError::Io)?;
                let data_unchanged = identities
                    .iter()
                    .filter(|(path, _)| path.as_str() != bulk::MARKER)
                    .all(|(path, value)| after.get(path) == Some(value));
                Ok(json!({"count": typed_count(count), "data_unchanged": data_unchanged}))
            },
            None,
        )
    })?;

    for kind in ["missing-key", "invalid-key", "wrong-key"] {
        scenario(
            &base,
            &format!("decrypt-{kind}"),
            true,
            |root, key_file, store| {
                sealed_file(&root.join("record.txt"), PLAIN.as_bytes(), store)?;
                match kind {
                    "missing-key" => std::fs::remove_file(key_file).map_err(|_| "bulk-key")?,
                    "invalid-key" => std::fs::write(key_file, b"not-a-valid-fernet-key")
                        .map_err(|_| "bulk-key")?,
                    _ => std::fs::write(key_file, b"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                        .map_err(|_| "bulk-key")?,
                }
                let before = tree_identities(root)?;
                record(
                    cases,
                    &format!("bulk-decrypt-{kind}"),
                    root,
                    key_file,
                    store,
                    || bulk::decrypt_all(store).map(typed_count),
                    Some(&before),
                )
            },
        )?;
    }

    for operation_name in ["seal-files", "encrypt-all", "decrypt-all"] {
        scenario(
            &base,
            &format!("failure-{operation_name}"),
            true,
            |root, key_file, store| {
                let targets = ["a", "b", "c"]
                    .map(|name| root.join(format!("work/{name}.txt")))
                    .to_vec();
                for target in &targets {
                    if operation_name == "decrypt-all" {
                        sealed_file(target, PLAIN.as_bytes(), store)?;
                    } else {
                        plain_file(target, PLAIN)?;
                    }
                }
                if operation_name == "encrypt-all" {
                    plain_file(&root.join(bulk::MARKER), "synthetic existing marker\n")?;
                }
                record(
                    cases,
                    &format!("bulk-{operation_name}-failure-atomicity"),
                    root,
                    key_file,
                    store,
                    || {
                        let fault = |path: &Path, point: RewritePoint| {
                            if path == targets[1] && point == RewritePoint::Temp {
                                Err(BulkError::Io)
                            } else {
                                Ok(())
                            }
                        };
                        let count = operation(operation_name, store, &targets, fault)?;
                        Ok(typed_count(count))
                    },
                    None,
                )
            },
        )?;
    }
    Ok(())
}

pub(crate) fn run_faults(cases: &mut Vec<Value>, data_root: &Path) -> Result<(), String> {
    let base = data_root.parent().ok_or("bulk-home")?.join("bulk-faults");
    let points = [
        ("write", RewritePoint::Write),
        ("flush", RewritePoint::Flush),
        ("chmod", RewritePoint::Chmod),
        ("rename", RewritePoint::Rename),
    ];
    for operation_name in ["seal-files", "encrypt-all", "decrypt-all"] {
        for (point_name, point) in points {
            scenario(
                &base,
                &format!("{operation_name}-{point_name}"),
                true,
                |root, key_file, store| {
                    let targets = ["a", "b", "c"]
                        .map(|name| root.join(format!("work/{name}.txt")))
                        .to_vec();
                    for target in &targets {
                        if operation_name == "decrypt-all" {
                            sealed_file(target, PLAIN.as_bytes(), store)?;
                        } else {
                            plain_file(target, PLAIN)?;
                        }
                    }
                    if operation_name == "encrypt-all" {
                        plain_file(&root.join(bulk::MARKER), "synthetic existing marker\n")?;
                    }
                    let failed_before = identity(&targets[1]).map_err(|_| "bulk-identity")?;
                    let later_before = identity(&targets[2]).map_err(|_| "bulk-identity")?;
                    record(
                        cases,
                        &format!("bulk-{operation_name}-{point_name}-fault"),
                        root,
                        key_file,
                        store,
                        || {
                            let fault = |path: &Path, at: RewritePoint| {
                                if path == targets[1] && at == point {
                                    Err(BulkError::Io)
                                } else {
                                    Ok(())
                                }
                            };
                            operation(operation_name, store, &targets, fault).map(typed_count)
                        },
                        None,
                    )?;
                    let row = cases.last_mut().ok_or("bulk-fault-record")?;
                    row["failed_unchanged"] =
                        json!(identity(&targets[1]).map_err(|_| "bulk-identity")? == failed_before);
                    row["later_unchanged"] =
                        json!(identity(&targets[2]).map_err(|_| "bulk-identity")? == later_before);
                    record(
                        cases,
                        &format!("bulk-{operation_name}-{point_name}-retry"),
                        root,
                        key_file,
                        store,
                        || {
                            operation(operation_name, store, &targets, |_, _| Ok(()))
                                .map(typed_count)
                        },
                        None,
                    )
                },
            )?;
        }
    }

    scenario(
        &base,
        "concurrent-mutation",
        true,
        |root, key_file, store| {
            let target = plain_file(&root.join("work/target.txt"), PLAIN)?;
            let mut mutated = false;
            record(
                cases,
                "bulk-seal-files-concurrent-mutation",
                root,
                key_file,
                store,
                || {
                    let count = operation(
                        "seal-files",
                        store,
                        std::slice::from_ref(&target),
                        |path, point| {
                            if path == target && point == RewritePoint::Rename {
                                std::fs::write(path, "synthetic concurrent mutation\n")?;
                                mutated = true;
                            }
                            Ok(())
                        },
                    )?;
                    Ok(json!({"count": typed_count(count), "mutated": mutated}))
                },
                None,
            )
        },
    )?;
    Ok(())
}
