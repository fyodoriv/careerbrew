//! Component-only pairing and encrypted personal→corporate settings sync.
//!
//! Only `ladder` and `target_level` can cross. Corporate export and corporate
//! bundles on a personal machine are refused before any payload write.

use crate::{canon::Canon, keyfile, store::Store};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub const SETTINGS_KEYS: &[&str] = &["ladder", "target_level"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Personal,
    Corporate,
}

#[derive(Debug)]
pub struct SyncError {
    pub class: &'static str,
    pub message: String,
}

impl SyncError {
    pub fn sync(message: impl Into<String>) -> Self {
        Self {
            class: "SyncError",
            message: message.into(),
        }
    }

    fn value(message: impl Into<String>) -> Self {
        Self {
            class: "ValueError",
            message: message.into(),
        }
    }

    fn io() -> Self {
        Self {
            class: "OSError",
            message: "sync file operation failed".into(),
        }
    }
}

impl From<std::io::Error> for SyncError {
    fn from(_: std::io::Error) -> Self {
        Self::io()
    }
}

fn outside(path: &Path, root: &Path) -> Result<(), SyncError> {
    keyfile::require_outside_data_root(path, root)
        .map_err(|_| SyncError::value("pairing key or sync bundle must be outside the data folder"))
}

fn write_key(path: &Path, root: &Path, key: &str) -> Result<(), SyncError> {
    outside(path, root)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, key)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

pub fn pair_init(path: &Path, root: &Path) -> Result<String, SyncError> {
    let key = fernet::Fernet::generate_key();
    write_key(path, root, &key)?;
    Ok(key)
}

pub fn pair_join(path: &Path, root: &Path, key: &str) -> Result<(), SyncError> {
    if fernet::Fernet::new(key).is_none() {
        return Err(SyncError::sync(
            "not a valid pairing key (fix: copy it exactly from `pair init`)",
        ));
    }
    write_key(path, root, key)
}

fn cipher(path: &Path, root: &Path) -> Result<fernet::Fernet, SyncError> {
    outside(path, root)?;
    if !path.exists() {
        return Err(SyncError::sync(
            "this machine is not paired (fix: `careerbrew pair init` on one machine, `pair join <key>` on the other)",
        ));
    }
    let key = std::fs::read_to_string(path)?;
    fernet::Fernet::new(key.trim()).ok_or_else(|| SyncError::value("not a valid pairing key"))
}

fn canon_json(value: &Canon) -> Result<Value, SyncError> {
    Ok(match value {
        Canon::Null => Value::Null,
        Canon::Bool(value) => json!(value),
        Canon::Int(value) => Value::Number(
            value
                .to_string()
                .parse()
                .map_err(|_| SyncError::value("settings integer is not JSON"))?,
        ),
        Canon::Float(value) => json!(value),
        Canon::Str(value) => json!(value),
        Canon::Date(_) | Canon::DateTime(_) => {
            return Err(SyncError::value("settings date is not JSON"));
        }
        Canon::List(items) => Value::Array(
            items
                .iter()
                .map(canon_json)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Canon::Map(items) => {
            let mut map = Map::new();
            for (key, value) in items {
                if let Canon::Str(key) = key {
                    map.insert(key.clone(), canon_json(value)?);
                }
            }
            Value::Object(map)
        }
    })
}

fn personal_payload(store: &Store) -> Result<Value, SyncError> {
    let settings = store
        .read_yaml(
            &store.root.join("settings.yaml"),
            Canon::Map(Vec::new()),
            true,
        )
        .map_err(|_| SyncError::io())?;
    let Canon::Map(items) = settings else {
        return Err(SyncError::value("settings must be a mapping"));
    };
    let mut selected = Map::new();
    for (key, value) in items {
        let Canon::Str(key) = key else {
            continue;
        };
        if SETTINGS_KEYS.contains(&key.as_str()) {
            selected.insert(key, canon_json(&value)?);
        }
    }
    Ok(json!({"settings": Value::Object(selected)}))
}

pub fn export(
    destination: &Path,
    root: &Path,
    pair_key: &Path,
    role: Role,
    at: &str,
    store: &Store,
) -> Result<Value, SyncError> {
    if role == Role::Corporate {
        return Err(SyncError::sync(
            "corporate data stays on the corporate machine: sync export is personal → corporate only",
        ));
    }
    outside(destination, root)?;
    let payload = personal_payload(store)?;
    let bundle = json!({"from": "personal", "at": at, "payload": payload});
    let token = cipher(pair_key, root)?.encrypt(bundle.to_string().as_bytes());
    std::fs::write(destination, token.as_bytes())?;
    std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o600))?;
    let items = payload["settings"].as_object().map_or(0, Map::len);
    Ok(json!({"from": "personal", "level": "settings", "items": items}))
}

fn text_setting(value: &Value) -> Option<&str> {
    value.as_str().filter(|value| value.chars().count() <= 64)
}

pub fn import_bundle(
    source: &Path,
    root: &Path,
    pair_key: &Path,
    role: Role,
    store: &Store,
    ladders: &BTreeMap<String, Vec<String>>,
) -> Result<Value, SyncError> {
    let raw = std::fs::read(source)?;
    let cipher = cipher(pair_key, root)?;
    let token =
        std::str::from_utf8(&raw).map_err(|_| SyncError::sync("bundle does not decrypt"))?;
    let decrypted = crate::decrypt_like_python(&cipher, token).map_err(|_| {
        SyncError::sync("bundle does not decrypt (wrong pairing key, or the file changed)")
    })?;
    let bundle: Value = serde_json::from_slice(&decrypted)
        .map_err(|_| SyncError::value("bundle payload is not JSON"))?;
    let from = bundle
        .get("from")
        .and_then(Value::as_str)
        .ok_or_else(|| SyncError::value("bundle has no source role"))?;
    let here = match role {
        Role::Personal => "personal",
        Role::Corporate => "corporate",
    };
    if from == here {
        return Err(SyncError::sync(format!(
            "bundle comes from another {here} machine; sync runs between a corporate and a personal machine"
        )));
    }
    if role == Role::Personal {
        return Err(SyncError::sync(
            "refusing a corporate bundle: corporate data stays on the corporate machine",
        ));
    }
    let payload = bundle
        .get("payload")
        .and_then(Value::as_object)
        .ok_or_else(|| SyncError::value("bundle payload must be a mapping"))?;
    if payload.keys().any(|key| key != "settings") {
        return Err(SyncError::sync(
            "refusing personal data on a corporate machine",
        ));
    }
    let empty = Map::new();
    let incoming = payload
        .get("settings")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let settings_path = store.root.join("settings.yaml");
    let settings = store
        .read_yaml(&settings_path, Canon::Map(Vec::new()), true)
        .map_err(|_| SyncError::io())?;
    let Canon::Map(mut items) = settings else {
        return Err(SyncError::value("settings must be a mapping"));
    };
    let current = |name: &str| {
        items
            .iter()
            .find(|(key, _)| matches!(key, Canon::Str(key) if key == name))
            .and_then(|(_, value)| match value {
                Canon::Str(value) => Some(value.clone()),
                _ => None,
            })
    };
    let ladder = incoming.get("ladder").and_then(|value| text_setting(value));
    let target = incoming
        .get("target_level")
        .and_then(|value| text_setting(value));
    let mut applied = Map::new();
    if ladder.is_some_and(|value| ladders.contains_key(value)) {
        applied.insert("ladder".into(), json!(ladder.unwrap()));
    }
    let ladder_id = applied
        .get("ladder")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| current("ladder"))
        .unwrap_or_else(|| "generic-swe".into());
    if target.is_some_and(|value| {
        ladders
            .get(&ladder_id)
            .is_some_and(|levels| levels.iter().any(|level| level == value))
    }) {
        applied.insert("target_level".into(), json!(target.unwrap()));
    }
    for key in SETTINGS_KEYS {
        if let Some(value) = applied.get(*key) {
            crate::canon::insert(
                &mut items,
                Canon::Str((*key).into()),
                Canon::Str(value.as_str().unwrap_or_default().into()),
            );
        }
    }
    store
        .write_yaml(&settings_path, &Canon::Map(items))
        .map_err(|_| SyncError::io())?;
    let added = applied.len();
    let ignored = incoming.len() - added;
    Ok(json!({"from": from, "added": added, "ignored": ignored}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_allowlist_is_exact() {
        assert_eq!(SETTINGS_KEYS, &["ladder", "target_level"]);
        assert!(!SETTINGS_KEYS.contains(&"backup_targets"));
        assert!(text_setting(&json!("x".repeat(65))).is_none());
        assert!(text_setting(&json!({"not": "text"})).is_none());
    }
}
