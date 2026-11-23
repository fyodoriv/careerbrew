//! Environment adapters for rust-06's seven component CLI handlers.
//!
//! These adapters are compiled only into the synthetic `vault-sync` candidate.
//! Production routes and the shipped native CLI remain unchanged.

use crate::atomic::StoreError;
use crate::backup::{self, BackupError};
use crate::bulk::{self, BulkError};
use crate::canon::Canon;
use crate::events::{self, Clock};
use crate::keyfile;
use crate::store::Store;
use crate::sync::{self, SyncError};
use chrono::{DateTime, SecondsFormat, Utc};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct TransferError {
    pub class: &'static str,
    pub message: String,
}

impl TransferError {
    pub fn new(class: &'static str, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
        }
    }

    fn runtime(message: impl Into<String>) -> Self {
        Self::new("RuntimeError", message)
    }
}

impl From<std::io::Error> for TransferError {
    fn from(_: std::io::Error) -> Self {
        Self::new("OSError", "file operation failed")
    }
}

impl From<StoreError> for TransferError {
    fn from(_: StoreError) -> Self {
        Self::new("OSError", "storage operation failed")
    }
}

impl From<BulkError> for TransferError {
    fn from(error: BulkError) -> Self {
        Self::new(error.python_kind(), "vault status failed")
    }
}

impl From<BackupError> for TransferError {
    fn from(error: BackupError) -> Self {
        let class = error.python_kind();
        Self::new(class, error.message(Path::new("archive.cbk")))
    }
}

impl From<SyncError> for TransferError {
    fn from(error: SyncError) -> Self {
        Self::new(error.class, error.message)
    }
}

#[derive(Clone, Debug)]
pub struct Context {
    pub root: PathBuf,
    pub home: PathBuf,
    pub key_file: PathBuf,
    pub pair_file: PathBuf,
    keys: ContextKeys,
}

#[derive(Clone, Debug)]
struct ContextKeys {
    root: PathBuf,
    key_file: PathBuf,
}

impl crate::store::KeyProvider for ContextKeys {
    fn load_key(&self, create: bool) -> Result<String, StoreError> {
        crate::synthetic::key_for_data(&self.root, &self.key_file, create)
    }
}

fn env_path(name: &str, home: &Path, default: &Path) -> PathBuf {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| expand_user(&path, home))
        .unwrap_or_else(|| default.to_path_buf())
}

fn expand_user(path: &Path, home: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        return home.to_path_buf();
    }
    text.strip_prefix("~/")
        .map(|rest| home.join(rest))
        .unwrap_or_else(|| path.to_path_buf())
}

impl Context {
    pub fn from_env() -> Result<Self, TransferError> {
        let root = std::env::var_os("CAREERBREW_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| {
                TransferError::runtime("CAREERBREW_HOME is required for the synthetic component")
            })?;
        let home = std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/synthetic-home"));
        let key_file = env_path(
            "CAREERBREW_KEY_FILE",
            &home,
            &home.join(".config/careerbrew/backup.key"),
        );
        let pair_file = env_path(
            "CAREERBREW_PAIR_KEY_FILE",
            &home,
            &home.join(".config/careerbrew/pair.key"),
        );
        let keys = ContextKeys {
            root: root.clone(),
            key_file: key_file.clone(),
        };
        Ok(Self {
            root,
            home,
            key_file,
            pair_file,
            keys,
        })
    }

    pub fn store(&self) -> Store<'_> {
        Store::new(&self.root, encryption_env(), &self.keys)
    }

    pub fn role(&self) -> Result<sync::Role, TransferError> {
        let store = self.store();
        let info = store.read_yaml(
            &self.root.join("machine.yaml"),
            Canon::Map(Vec::new()),
            true,
        )?;
        Ok(match map_get(&info, "role") {
            Some(Canon::Str(role)) if role == "corporate" => sync::Role::Corporate,
            _ => sync::Role::Personal,
        })
    }

    pub fn backup_role(&self) -> Result<backup::Role, TransferError> {
        Ok(match self.role()? {
            sync::Role::Personal => backup::Role::Personal,
            sync::Role::Corporate => backup::Role::Corporate,
        })
    }

    pub fn cloud_synced(&self, path: &Path) -> String {
        let home = keyfile::resolve(&self.home);
        let path = keyfile::resolve(path);
        let Ok(relative) = path.strip_prefix(home) else {
            return String::new();
        };
        let parts = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        if parts.starts_with(&["Library".into(), "Mobile Documents".into()]) {
            return "iCloud Drive".into();
        }
        if parts.starts_with(&["Library".into(), "CloudStorage".into()]) && parts.len() > 2 {
            return parts[2].split('-').next().unwrap_or_default().into();
        }
        if let Some(first) = parts.first() {
            if matches!(first.as_str(), "Dropbox" | "Google Drive" | "Box")
                || first.starts_with("OneDrive")
            {
                return first.clone();
            }
            if matches!(first.as_str(), "Desktop" | "Documents") {
                let synced = self
                    .home
                    .join("Library/Mobile Documents/com~apple~CloudDocs")
                    .join(first)
                    .is_dir();
                if synced {
                    return "iCloud Desktop & Documents".into();
                }
            }
        }
        String::new()
    }

    pub fn configured_targets(&self) -> Result<Vec<PathBuf>, TransferError> {
        let store = self.store();
        let settings = store.read_yaml(
            &self.root.join("settings.yaml"),
            Canon::Map(Vec::new()),
            true,
        )?;
        let mut configured = match map_get(&settings, "backup_targets") {
            Some(Canon::List(items)) => items
                .iter()
                .filter_map(|item| match item {
                    Canon::Str(value) => Some(value.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => vec![self
                .home
                .join("Backups/careerbrew")
                .to_string_lossy()
                .into_owned()],
        };
        if let Some(value) =
            std::env::var_os("CAREERBREW_BACKUP_DIRS").filter(|value| !value.is_empty())
        {
            configured = std::env::split_paths(&value)
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
        }
        let mut out = configured
            .iter()
            .map(|path| expand_user(Path::new(path), &self.home))
            .collect::<Vec<_>>();
        if matches!(map_get(&settings, "backup_icloud"), Some(Canon::Bool(true))) {
            let icloud = env_path(
                "CAREERBREW_ICLOUD_DIR",
                &self.home,
                &self
                    .home
                    .join("Library/Mobile Documents/com~apple~CloudDocs/careerbrew"),
            );
            if !out.contains(&icloud) {
                out.push(icloud);
            }
        }
        Ok(out)
    }

    pub fn targets(&self) -> Result<Vec<PathBuf>, TransferError> {
        let corporate = self.role()? == sync::Role::Corporate;
        let allowed = std::env::var_os("CAREERBREW_RUST06_ALLOWED").map(|value| {
            std::env::split_paths(&value)
                .map(|path| expand_user(&path, &self.home))
                .collect::<Vec<_>>()
        });
        let mut out = Vec::new();
        for path in self.configured_targets()? {
            let cloud = if corporate {
                self.cloud_synced(&path)
            } else {
                String::new()
            };
            if !cloud.is_empty() {
                return Err(TransferError::runtime(format!(
                    "backup target is in a cloud-synced folder ({cloud}); corporate data stays on this machine"
                )));
            }
            if allowed
                .as_ref()
                .is_some_and(|allowed| !allowed.contains(&path))
            {
                continue;
            }
            keyfile::require_outside_data_root(&path, &self.root).map_err(|_| {
                TransferError::new(
                    "ValueError",
                    "backup target must be outside the data folder",
                )
            })?;
            out.push(path);
        }
        if out.is_empty() {
            return Err(TransferError::runtime(
                "no backup target allowed by plugin policy (fix: add a target the policy lists with `careerbrew data edit settings.yaml`)",
            ));
        }
        Ok(out)
    }

    pub fn ladders(&self) -> Result<BTreeMap<String, Vec<String>>, TransferError> {
        let mut files = Vec::new();
        for folder in [
            std::env::current_dir()?.join("knowledge/ladders"),
            self.root.join("ladders"),
        ] {
            if folder.is_dir() {
                files.extend(
                    std::fs::read_dir(folder)?
                        .flatten()
                        .map(|entry| entry.path())
                        .filter(|path| {
                            path.extension()
                                .is_some_and(|extension| extension == "yaml")
                        }),
                );
            }
        }
        files.sort();
        let mut out = BTreeMap::new();
        for path in files {
            let text = std::fs::read_to_string(path)?;
            let value = crate::json_yaml::yaml_read(&text)
                .map_err(|_| TransferError::new("LadderError", "invalid ladder"))?;
            let Some(Canon::Str(id)) = map_get(&value, "id") else {
                return Err(TransferError::new("LadderError", "invalid ladder"));
            };
            let Some(Canon::List(levels)) = map_get(&value, "levels") else {
                return Err(TransferError::new("LadderError", "invalid ladder"));
            };
            let levels = levels
                .iter()
                .map(|level| match level {
                    Canon::Str(level) => Ok(level.clone()),
                    _ => Err(TransferError::new("LadderError", "invalid ladder level")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            out.insert(id.clone(), levels);
        }
        Ok(out)
    }

    fn emit(&self, event: &str, fields: Canon) -> Result<(), TransferError> {
        events::emit(
            &self.root,
            std::env::var("CAREERBREW_EVENTS").ok().as_deref(),
            event,
            &fields,
            &EventClock,
        )?;
        Ok(())
    }

    pub fn cmd_data_status(&self) -> Result<String, TransferError> {
        std::fs::create_dir_all(&self.root)?;
        let store = self.store();
        let status = bulk::status(&store)?;
        let plaintext = status.files - status.sealed;
        let mut out = format!(
            "encryption at rest: {} · {} of {} file(s) encrypted · key: {}\n",
            if status.enabled { "on" } else { "off" },
            status.sealed,
            status.files,
            self.key_file.display(),
        );
        if status.enabled && plaintext > 0 {
            out.push_str(&format!(
                "{plaintext} file(s) still plaintext: run `careerbrew data encrypt`\n"
            ));
        }
        Ok(out)
    }

    pub fn cmd_backup_create(&self) -> Result<String, TransferError> {
        std::fs::create_dir_all(&self.root)?;
        let paths = backup::create(
            &self.root,
            &self.key_file,
            &self.targets()?,
            &backup_stamp(),
        )?;
        let mut out = String::new();
        for path in &paths {
            out.push_str(&format!(
                "backup written and verified: {}\n",
                path.display()
            ));
        }
        self.emit(
            "backup.create",
            Canon::Map(vec![
                (Canon::Str("status".into()), Canon::Str("ok".into())),
                (Canon::Str("targets".into()), Canon::Int(paths.len().into())),
            ]),
        )?;
        Ok(out)
    }

    pub fn cmd_backup_verify(&self, file: Option<&Path>) -> Result<String, TransferError> {
        let path = match file {
            Some(path) => path.to_path_buf(),
            None => backup::latest(&self.targets()?)?.ok_or_else(|| {
                TransferError::runtime("no backup found (run `careerbrew backup create`)")
            })?,
        };
        let count = backup::verify(&path, &self.key_file, &self.root, None)?;
        Ok(format!(
            "ok: {} decrypts, hash matches, {count} file(s)\n",
            path.file_name().unwrap_or_default().to_string_lossy()
        ))
    }

    pub fn cmd_backup_restore(&self, file: &Path, dest: &Path) -> Result<String, TransferError> {
        let role = self.role()?;
        if role == sync::Role::Corporate {
            let cloud = self.cloud_synced(dest);
            if !cloud.is_empty() {
                return Err(TransferError::runtime(format!(
                    "restore destination is in a cloud-synced folder ({cloud}); choose a local folder on a corporate machine"
                )));
            }
        }
        let count = backup::restore(file, dest, &self.key_file, &self.root, self.backup_role()?)?;
        if role == sync::Role::Corporate && !keyfile::resolve(dest).join("machine.yaml").exists() {
            let store = self.store();
            let info = store.read_yaml(
                &self.root.join("machine.yaml"),
                Canon::Map(Vec::new()),
                true,
            )?;
            store.write_yaml(&keyfile::resolve(dest).join("machine.yaml"), &info)?;
        }
        self.emit(
            "backup.restore",
            Canon::Map(vec![
                (Canon::Str("status".into()), Canon::Str("ok".into())),
                (Canon::Str("files".into()), Canon::Int(count.into())),
            ]),
        )?;
        Ok(format!(
            "restored {count} file(s) to {}\nthe machine role is not part of a backup: a folder restored on a corporate machine is marked corporate; check with `CAREERBREW_HOME=<folder> careerbrew machine show`\n",
            dest.display()
        ))
    }

    pub fn cmd_pair_join(&self, key: &str) -> Result<String, TransferError> {
        sync::pair_join(&self.pair_file, &self.root, key)?;
        Ok("paired\n".into())
    }

    pub fn cmd_sync_export(&self, file: &Path) -> Result<String, TransferError> {
        let store = self.store();
        let result = sync::export(
            file,
            &self.root,
            &self.pair_file,
            self.role()?,
            &sync_stamp(),
            &store,
        )?;
        self.emit(
            "sync.export",
            Canon::Map(vec![
                (Canon::Str("status".into()), Canon::Str("ok".into())),
                (
                    Canon::Str("source_kind".into()),
                    Canon::Str("personal".into()),
                ),
            ]),
        )?;
        Ok(format!(
            "exported from {} machine: level={} items={} -> {}\n",
            result["from"].as_str().unwrap_or_default(),
            result["level"].as_str().unwrap_or_default(),
            result["items"].as_u64().unwrap_or_default(),
            file.display()
        ))
    }

    pub fn cmd_sync_import(&self, file: &Path) -> Result<String, TransferError> {
        let store = self.store();
        let result = sync::import_bundle(
            file,
            &self.root,
            &self.pair_file,
            self.role()?,
            &store,
            &self.ladders()?,
        )?;
        let added = result["added"].as_u64().unwrap_or_default();
        let ignored = result["ignored"].as_u64().unwrap_or_default();
        self.emit(
            "sync.import",
            Canon::Map(vec![
                (Canon::Str("status".into()), Canon::Str("ok".into())),
                (Canon::Str("items".into()), Canon::Int(added.into())),
            ]),
        )?;
        let mut out = format!(
            "imported bundle from {} machine: {added} new record(s)\n",
            result["from"].as_str().unwrap_or_default()
        );
        if ignored > 0 {
            out.push_str(&format!(
                "ignored {ignored} setting(s): only a known ladder and one of its levels are accepted on a corporate machine\n"
            ));
        }
        Ok(out)
    }
}

fn encryption_env() -> bool {
    std::env::var("CAREERBREW_ENCRYPT").as_deref() != Ok("off")
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

struct EventClock;
impl Clock for EventClock {
    fn stamp(&self) -> String {
        sync_stamp()
    }

    fn version(&self) -> String {
        std::env::var("CAREERBREW_VERSION").unwrap_or_else(|_| "0.1.0".into())
    }
}

fn pinned_now() -> Option<DateTime<Utc>> {
    std::env::var("CAREERBREW_NOW")
        .ok()
        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn sync_stamp() -> String {
    pinned_now()
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub fn backup_stamp() -> String {
    let now = Utc::now();
    format!(
        "{}{:06}Z",
        now.format("%Y%m%dT%H%M%S"),
        now.timestamp_subsec_micros()
    )
}
