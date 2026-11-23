//! Component-only storage readers and writers, composed over the existing
//! codec, key, atomic-write and advisory-lock providers.
//!
//! `Store` borrows an invocation-scoped key provider. No state survives a
//! call, no production route selects it, and readers never take a write lock.

use crate::atomic::{self, StoreError};
use crate::canon::Canon;
use crate::json_yaml::{self, json_line};
use crate::lock;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path};

pub trait KeyProvider {
    fn load_key(&self, create: bool) -> Result<String, StoreError>;
}

impl<F> KeyProvider for F
where
    F: Fn(bool) -> Result<String, StoreError>,
{
    fn load_key(&self, create: bool) -> Result<String, StoreError> {
        self(create)
    }
}

pub struct Store<'a> {
    pub(crate) root: PathBuf,
    encryption: bool,
    keys: &'a dyn KeyProvider,
}

impl<'a> Store<'a> {
    pub fn new(root: &Path, encryption: bool, keys: &'a dyn KeyProvider) -> Self {
        Self {
            root: root.to_path_buf(),
            encryption,
            keys,
        }
    }

    pub(crate) fn replay_checkout(&self, relative: &Path) -> bool {
        let parts: Vec<_> = relative.components().collect();
        if parts.len() < 5 || parts[0].as_os_str() != "learn" || parts[2].as_os_str() != "replay" {
            return false;
        }
        let number = parts[3].as_os_str().to_string_lossy();
        if number.is_empty()
            || !number.is_ascii()
            || !number.bytes().all(|b| b.is_ascii_digit())
            || number.starts_with('0')
        {
            return false;
        }
        let mut checkout = self.root.to_path_buf();
        for part in &parts[..4] {
            checkout.push(part.as_os_str());
        }
        let git = checkout.join(".git");
        let head = git.join("HEAD");
        !git.is_symlink() && git.is_dir() && !head.is_symlink() && head.is_file()
    }

    pub fn encryption_enabled(&self) -> bool {
        self.encryption && !self.root.join(".plaintext").exists()
    }

    pub(crate) fn sealable(&self, path: &Path) -> bool {
        let resolved_root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        let relative = match path.canonicalize() {
            Ok(resolved) => resolved
                .strip_prefix(&resolved_root)
                .ok()
                .map(Path::to_path_buf),
            Err(_) => path.strip_prefix(&self.root).ok().map(Path::to_path_buf),
        };
        let Some(relative) = relative else {
            return false;
        };
        if relative.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return false;
        }
        let Some(first) = relative.components().next() else {
            return false;
        };
        !matches!(
            first.as_os_str().to_str(),
            Some("logs" | "out" | ".edit" | "plugins")
        ) && relative
            .file_name()
            .is_some_and(|name| name != ".plaintext")
            && !self.replay_checkout(&relative)
    }

    pub(crate) fn key(&self, create: bool) -> Result<String, StoreError> {
        self.keys.load_key(create)
    }

    pub fn read_bytes(&self, path: &Path, create_key: bool) -> Result<Option<Vec<u8>>, StoreError> {
        let raw = match std::fs::read(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if !raw.starts_with(crate::MAGIC) {
            return Ok(Some(raw));
        }
        let key = self.key(create_key)?;
        Ok(Some(
            crate::open(Some(&key), &raw).map_err(|_| StoreError::Vault)?,
        ))
    }

    pub fn read_text(
        &self,
        path: &Path,
        default: Option<&str>,
        create_key: bool,
    ) -> Result<Option<String>, StoreError> {
        self.read_bytes(path, create_key)?
            .map(|bytes| String::from_utf8(bytes).map_err(|_| StoreError::Utf8))
            .transpose()
            .map(|value| value.or_else(|| default.map(str::to_owned)))
    }

    pub fn read_text_nofollow(&self, path: &Path, create_key: bool) -> Result<String, StoreError> {
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        if !file.metadata()?.is_file() {
            return Err(StoreError::Codec("not-regular"));
        }
        let mut raw = Vec::new();
        file.read_to_end(&mut raw)?;
        if raw.starts_with(crate::MAGIC) {
            let key = self.key(create_key)?;
            raw = crate::open(Some(&key), &raw).map_err(|_| StoreError::Vault)?;
        }
        String::from_utf8(raw).map_err(|_| StoreError::Utf8)
    }

    pub fn write_text(&self, path: &Path, text: &str) -> Result<(), StoreError> {
        std::fs::create_dir_all(&self.root)?;
        let _ = std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700));
        let seal = self.encryption_enabled() && self.sealable(path);
        let key = if seal { Some(self.key(true)?) } else { None };
        atomic::write_text(path, key.as_deref(), seal, text)
    }

    pub fn write_text_if_changed(&self, path: &Path, text: &str) -> Result<bool, StoreError> {
        match self.read_text(path, None, true) {
            Ok(Some(old)) if old == text => return Ok(false),
            Ok(_) => {}
            Err(StoreError::Vault) => {}
            Err(error) => return Err(error),
        }
        self.write_text(path, text)?;
        Ok(true)
    }

    pub fn read_json(
        &self,
        path: &Path,
        default: Canon,
        create_key: bool,
    ) -> Result<Canon, StoreError> {
        match self.read_text(path, None, create_key)? {
            None => Ok(default),
            Some(text) => json_yaml::json_read(&text).map_err(|_| StoreError::Codec("json")),
        }
    }

    pub fn write_json(&self, path: &Path, value: &Canon) -> Result<(), StoreError> {
        self.write_text(
            path,
            &json_yaml::json_write(value).map_err(|_| StoreError::Codec("json"))?,
        )
    }

    pub fn write_json_if_changed(&self, path: &Path, value: &Canon) -> Result<bool, StoreError> {
        self.write_text_if_changed(
            path,
            &json_yaml::json_write(value).map_err(|_| StoreError::Codec("json"))?,
        )
    }

    pub fn read_yaml(
        &self,
        path: &Path,
        default: Canon,
        create_key: bool,
    ) -> Result<Canon, StoreError> {
        match self.read_text(path, None, create_key)? {
            None => Ok(default),
            Some(text) => {
                let value = json_yaml::yaml_read(&text).map_err(|_| StoreError::Codec("yaml"))?;
                if value == Canon::Null {
                    Ok(default)
                } else {
                    Ok(value)
                }
            }
        }
    }

    pub fn write_yaml(&self, path: &Path, value: &Canon) -> Result<(), StoreError> {
        self.write_text(
            path,
            &json_yaml::yaml_write(value).map_err(|_| StoreError::Codec("yaml"))?,
        )
    }

    pub fn read_jsonl(&self, path: &Path, create_key: bool) -> Result<Vec<Canon>, StoreError> {
        let text = self
            .read_text(path, Some(""), create_key)?
            .unwrap_or_default();
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| json_yaml::json_read(line).map_err(|_| StoreError::Codec("jsonl")))
            .collect()
    }

    pub fn write_jsonl(&self, path: &Path, rows: &[Canon]) -> Result<(), StoreError> {
        self.write_text(path, &jsonl_text(rows)?)
    }

    pub fn append_jsonl(&self, path: &Path, rows: &[Canon]) -> Result<usize, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let new = jsonl_text(rows)?;
        let seal = self.encryption_enabled() && self.sealable(path);
        lock::with_lock(path, || -> Result<(), StoreError> {
            if !seal {
                let mut file = OpenOptions::new().append(true).create(true).open(path)?;
                file.write_all(new.as_bytes())?;
                return Ok(());
            }
            let old = self.read_text(path, Some(""), true)?.unwrap_or_default();
            self.write_text(path, &(old + &new))
        })??;
        Ok(rows.len())
    }

    pub fn upsert_jsonl(
        &self,
        path: &Path,
        rows: &[Canon],
        fields: &[&str],
    ) -> Result<(usize, usize), StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let seal = self.encryption_enabled() && self.sealable(path);
        lock::with_lock(path, || -> Result<(usize, usize), StoreError> {
            let old = self.read_jsonl(path, true)?;
            let mut out: Vec<(String, Canon)> = Vec::new();
            let mut replaced = Vec::<String>::new();
            for row in old {
                let id = row_id(&row)?;
                if out.iter().any(|(known, _)| *known == id) && !replaced.contains(&id) {
                    replaced.push(id.clone());
                }
                if let Some(slot) = out.iter_mut().find(|(known, _)| *known == id) {
                    slot.1 = row;
                } else {
                    out.push((id, row));
                }
            }
            let mut batch: Vec<(String, Canon)> = Vec::new();
            for row in rows {
                let id = row_id(row)?;
                if let Some(slot) = batch.iter_mut().find(|(known, _)| *known == id) {
                    slot.1 = row.clone();
                } else {
                    batch.push((id, row.clone()));
                }
            }
            let mut added = Vec::new();
            for (id, row) in &batch {
                if let Some(slot) = out.iter_mut().find(|(known, _)| known == id) {
                    if fields.iter().any(|field| {
                        // Python dict.get: a missing field and an explicit
                        // JSON null both produce None.
                        map_get(&slot.1, field).unwrap_or(&Canon::Null)
                            != map_get(row, field).unwrap_or(&Canon::Null)
                    }) {
                        slot.1 = row.clone();
                        if !replaced.contains(id) {
                            replaced.push(id.clone());
                        }
                    }
                } else {
                    added.push(row.clone());
                }
            }
            if !replaced.is_empty() || (!added.is_empty() && seal) {
                let all: Vec<Canon> = out
                    .into_iter()
                    .map(|(_, row)| row)
                    .chain(added.iter().cloned())
                    .collect();
                self.write_jsonl(path, &all)?;
            } else if !added.is_empty() {
                let mut file = OpenOptions::new().append(true).create(true).open(path)?;
                file.write_all(jsonl_text(&added)?.as_bytes())?;
            }
            Ok((added.len(), replaced.len()))
        })?
    }
}

use std::path::PathBuf;

fn row_id(row: &Canon) -> Result<String, StoreError> {
    match map_get(row, "id") {
        Some(Canon::Str(id)) => Ok(id.clone()),
        _ => Err(StoreError::Codec("jsonl-id")),
    }
}

fn map_get<'a>(row: &'a Canon, key: &str) -> Option<&'a Canon> {
    match row {
        Canon::Map(items) => items
            .iter()
            .find(|(name, _)| matches!(name, Canon::Str(text) if text == key))
            .map(|(_, value)| value),
        _ => None,
    }
}

fn jsonl_text(rows: &[Canon]) -> Result<String, StoreError> {
    let mut out = String::new();
    for row in rows {
        out.push_str(&json_line(row).map_err(|_| StoreError::Codec("jsonl"))?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canon::Canon;
    use num_bigint::BigInt;

    const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "cb-store-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn row(id: &str, value: i64) -> Canon {
        Canon::Map(vec![
            (Canon::Str("id".into()), Canon::Str(id.into())),
            (Canon::Str("v".into()), Canon::Int(BigInt::from(value))),
        ])
    }

    #[test]
    fn upsert_is_one_transaction_with_python_order_and_counts() {
        let root = scratch("upsert");
        let keys = |_: bool| Ok(KEY.to_owned());
        let store = Store::new(&root, true, &keys);
        let path = root.join("rows.jsonl");
        store
            .write_jsonl(&path, &[row("a", 1), row("a", 2)])
            .unwrap();
        assert_eq!(
            store
                .upsert_jsonl(&path, &[row("a", 2), row("b", 3), row("b", 4)], &["v"])
                .unwrap(),
            (1, 1)
        );
        assert_eq!(
            store.read_jsonl(&path, true).unwrap(),
            vec![row("a", 2), row("b", 4)]
        );
        let before = std::fs::read(&path).unwrap();
        assert_eq!(
            store.upsert_jsonl(&path, &[row("a", 2)], &["v"]).unwrap(),
            (0, 0)
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn upsert_missing_and_null_are_python_noops_but_a_real_change_rewrites() {
        use std::os::unix::fs::MetadataExt;
        let root = scratch("missing-null");
        let keys = |_: bool| Ok(KEY.to_owned());
        let store = Store::new(&root, true, &keys);
        let path = root.join("rows.jsonl");
        let missing = Canon::Map(vec![(Canon::Str("id".into()), Canon::Str("a".into()))]);
        let explicit_null = Canon::Map(vec![
            (Canon::Str("id".into()), Canon::Str("a".into())),
            (Canon::Str("v".into()), Canon::Null),
        ]);
        store.write_jsonl(&path, &[missing]).unwrap();
        let before = std::fs::read(&path).unwrap();
        let metadata = std::fs::metadata(&path).unwrap();
        let identity = (metadata.ino(), metadata.mtime(), metadata.mtime_nsec());
        assert_eq!(
            store.upsert_jsonl(&path, &[explicit_null], &["v"]).unwrap(),
            (0, 0)
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(
            (metadata.ino(), metadata.mtime(), metadata.mtime_nsec()),
            identity
        );
        assert_eq!(
            store.upsert_jsonl(&path, &[row("a", 1)], &["v"]).unwrap(),
            (0, 1)
        );
        assert_eq!(store.read_jsonl(&path, true).unwrap(), vec![row("a", 1)]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_utf8_is_not_a_rewrite_permission() {
        let root = scratch("utf8");
        std::fs::create_dir_all(root.join("logs")).unwrap();
        let path = root.join("logs/bad.txt");
        std::fs::write(&path, [0xff]).unwrap();
        let keys = |_: bool| Ok(KEY.to_owned());
        let store = Store::new(&root, true, &keys);
        assert!(matches!(
            store.write_text_if_changed(&path, "new"),
            Err(StoreError::Utf8)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), [0xff]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_reads_are_lazy_and_nofollow_refuses_a_symlink() {
        let root = scratch("reads");
        std::fs::create_dir_all(&root).unwrap();
        let calls = std::cell::Cell::new(0);
        let keys = |_: bool| {
            calls.set(calls.get() + 1);
            Ok(KEY.to_owned())
        };
        let store = Store::new(&root, true, &keys);
        assert_eq!(
            store
                .read_json(&root.join("missing.json"), int_for_test(7), true)
                .unwrap(),
            int_for_test(7)
        );
        assert_eq!(calls.get(), 0);
        let target = root.join("target.txt");
        std::fs::write(&target, "synthetic").unwrap();
        let link = root.join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(store.read_text_nofollow(&link, true).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"synthetic");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replay_checkout_and_sealed_missing_key_keep_their_rules() {
        let root = scratch("replay");
        let path = root.join("learn/project/replay/1/note.txt");
        std::fs::create_dir_all(path.parent().unwrap().join(".git")).unwrap();
        std::fs::write(
            path.parent().unwrap().join(".git/HEAD"),
            "ref: refs/heads/synthetic\n",
        )
        .unwrap();
        let keys = |_: bool| Ok(KEY.to_owned());
        let store = Store::new(&root, true, &keys);
        store.write_text(&path, "plain").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"plain");

        let sealed = root.join("sealed.txt");
        store.write_text(&sealed, "secret").unwrap();
        let no_keys = |_: bool| Err(StoreError::NoKey);
        let unreadable = Store::new(&root, true, &no_keys);
        assert!(matches!(
            unreadable.read_text(&sealed, None, false),
            Err(StoreError::NoKey)
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn int_for_test(value: i64) -> Canon {
        Canon::Int(BigInt::from(value))
    }
}
