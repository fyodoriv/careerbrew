//! Synthetic-only stateful rust-05 workload used by `vault-store --all`.
//! It executes the native providers and emits only typed values and digests.

use crate::atomic::StoreError;
use crate::canon::Canon;
use crate::cli::{self, CliError, Editor, Outcome, Terminal};
use crate::events::{self, Clock};
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

struct FixedEventClock;
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
