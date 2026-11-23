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

const RESOURCE_KINDS: &[&str] = &[
    "ladders",
    "job_sources",
    "rules",
    "prompts",
    "skills",
    "evidence_sources",
    "knowledge",
    "rubrics",
    "commands",
];

fn plugin_error() -> TransferError {
    TransferError::new("PluginError", "installed plugin manifest is invalid")
}

fn valid_plugin_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (2..=40).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn reject_plugin_symlinks(root: &Path) -> Result<(), TransferError> {
    for entry in std::fs::read_dir(root).map_err(|_| plugin_error())? {
        let path = entry.map_err(|_| plugin_error())?.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| plugin_error())?;
        if metadata.file_type().is_symlink() {
            return Err(plugin_error());
        }
        if metadata.is_dir() {
            reject_plugin_symlinks(&path)?;
        }
    }
    Ok(())
}

fn all_files(folder: &Path, out: &mut Vec<PathBuf>) -> Result<(), TransferError> {
    for entry in std::fs::read_dir(folder)? {
        let path = entry?.path();
        if path.is_dir() {
            all_files(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn plugin_resource_files(root: &Path, item: &str) -> Result<Vec<PathBuf>, TransferError> {
    let relative = Path::new(item);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        || !keyfile::resolve(&root.join(item)).starts_with(keyfile::resolve(root))
    {
        return Err(plugin_error());
    }
    let pattern = root
        .join(item)
        .to_str()
        .ok_or_else(plugin_error)?
        .to_owned();
    let mut matches = glob::glob(&pattern)
        .map_err(|_| plugin_error())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| plugin_error())?;
    if matches.is_empty() {
        return Err(plugin_error());
    }
    matches.sort();
    let mut files = Vec::new();
    for path in matches {
        if path.is_dir() {
            all_files(&path, &mut files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn ladder_error() -> TransferError {
    TransferError::new("LadderError", "invalid ladder")
}

fn truthy(value: &Canon) -> bool {
    match value {
        Canon::Null => false,
        Canon::Bool(value) => *value,
        Canon::Int(value) => *value != num_bigint::BigInt::from(0),
        Canon::Float(value) => *value != 0.0,
        Canon::Str(value) | Canon::Date(value) | Canon::DateTime(value) => !value.is_empty(),
        Canon::List(items) => !items.is_empty(),
        Canon::Map(items) => !items.is_empty(),
    }
}

fn map_has(map: &Canon, key: &Canon) -> bool {
    let Canon::Map(items) = map else {
        return false;
    };
    items.iter().any(|(name, _)| name == key)
}

fn validate_ladder(value: &Canon) -> Result<(String, Vec<String>), TransferError> {
    let Canon::Map(_) = value else {
        return Err(ladder_error());
    };
    let Some(Canon::Str(id)) = map_get(value, "id").filter(|value| truthy(value)) else {
        return Err(ladder_error());
    };
    let Some(Canon::List(levels)) = map_get(value, "levels").filter(|value| truthy(value)) else {
        return Err(ladder_error());
    };
    let Some(Canon::List(competencies)) =
        map_get(value, "competencies").filter(|value| truthy(value))
    else {
        return Err(ladder_error());
    };
    for competency in competencies {
        let Canon::Map(_) = competency else {
            return Err(ladder_error());
        };
        for key in ["id", "name", "weight", "signals", "expect"] {
            if map_get(competency, key).is_none() {
                return Err(ladder_error());
            }
        }
        let expect = map_get(competency, "expect").unwrap();
        if levels.iter().any(|level| !map_has(expect, level)) {
            return Err(ladder_error());
        }
        for key in ["descriptions", "examples"] {
            if map_get(competency, key).is_some_and(|value| !matches!(value, Canon::Map(_))) {
                return Err(ladder_error());
            }
        }
    }
    let levels = levels
        .iter()
        .filter_map(|level| match level {
            Canon::Str(level) => Some(level.clone()),
            _ => None,
        })
        .collect();
    Ok((id.clone(), levels))
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
            if matches!(
                first.as_str(),
                "Dropbox" | "Google Drive" | "OneDrive" | "Box" | "pCloud Drive" | "Sync"
            ) || first.starts_with("OneDrive")
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

    fn plugin_ladders(&self) -> Result<Vec<(String, PathBuf)>, TransferError> {
        let store = self.store();
        let lock = store.read_json(
            &self.root.join("plugins.lock.json"),
            Canon::Map(vec![(Canon::Str("plugins".into()), Canon::Map(Vec::new()))]),
            true,
        )?;
        let Some(Canon::Map(plugins)) = map_get(&lock, "plugins") else {
            return Err(plugin_error());
        };
        let mut plugins = plugins
            .iter()
            .map(|(name, entry)| match name {
                Canon::Str(name) => Ok((name.clone(), entry)),
                _ => Err(plugin_error()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        plugins.sort_by(|a, b| a.0.cmp(&b.0));
        let mut resources = Vec::new();
        for (name, entry) in plugins {
            if !valid_plugin_name(&name) {
                return Err(plugin_error());
            }
            let root = self.root.join("plugins").join(&name);
            reject_plugin_symlinks(&root)?;
            let manifest = store.read_yaml(
                &root.join("careerbrew-plugin.yaml"),
                Canon::Map(Vec::new()),
                true,
            )?;
            if map_get(&manifest, "name") != Some(&Canon::Str(name.clone()))
                || map_get(&manifest, "version").is_none()
            {
                return Err(plugin_error());
            }
            let origin = match map_get(entry, "origin") {
                Some(Canon::Str(origin)) => origin.clone(),
                _ => return Err(plugin_error()),
            };
            let provides = match map_get(&manifest, "provides") {
                None | Some(Canon::Null) => Vec::new(),
                Some(Canon::Map(items)) => items.clone(),
                _ => return Err(plugin_error()),
            };
            for (kind, value) in &provides {
                let Canon::Str(kind) = kind else {
                    return Err(plugin_error());
                };
                if !RESOURCE_KINDS.contains(&kind.as_str()) {
                    return Err(plugin_error());
                }
                let values: Vec<&Canon> = match value {
                    Canon::List(items) => items.iter().collect(),
                    item => vec![item],
                };
                for item in values {
                    let Canon::Str(item) = item else {
                        return Err(plugin_error());
                    };
                    let files = plugin_resource_files(&root, item)?;
                    if kind == "ladders" {
                        resources.extend(files.into_iter().map(|path| (origin.clone(), path)));
                    }
                }
            }
        }
        Ok(resources)
    }

    pub fn ladders(&self) -> Result<BTreeMap<String, sync::Ladder>, TransferError> {
        let mut sources = Vec::new();
        let base = std::env::current_dir()?.join("knowledge/ladders");
        if base.is_dir() {
            let mut files = std::fs::read_dir(base)?
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file())
                .collect::<Vec<_>>();
            files.sort();
            sources.extend(files.into_iter().map(|path| ("base".to_owned(), path)));
        }
        sources.extend(self.plugin_ladders()?);
        let user = self.root.join("ladders");
        if user.is_dir() {
            let mut files = std::fs::read_dir(user)?
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_file()
                        && path
                            .extension()
                            .is_some_and(|extension| extension == "yaml")
                })
                .collect::<Vec<_>>();
            files.sort();
            sources.extend(files.into_iter().map(|path| ("user".to_owned(), path)));
        }
        let store = self.store();
        let mut out = BTreeMap::new();
        for (origin, path) in sources {
            let value = store.read_yaml(&path, Canon::Map(Vec::new()), true)?;
            let (id, levels) = validate_ladder(&value)?;
            out.insert(
                id,
                sync::Ladder {
                    levels,
                    origin,
                    source: path,
                },
            );
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
