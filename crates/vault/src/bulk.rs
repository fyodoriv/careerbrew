//! Component-only whole-vault enumeration, status and bulk encryption.
//!
//! The invocation-scoped `Store` supplies the root and key provider. No
//! production route selects this module.

use crate::atomic::{self, StoreError};
use crate::store::Store;
use base64::{engine::general_purpose, Engine as _};
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const MARKER: &str = ".plaintext";
pub const MARKER_TEXT: &str =
    "encryption at rest is off for this data root; `careerbrew data encrypt` turns it on\n";

#[derive(Debug)]
pub enum BulkError {
    Io,
    Vault,
    Value,
    Bundle,
}

impl BulkError {
    pub fn python_kind(&self) -> &'static str {
        match self {
            Self::Io => "OSError",
            Self::Vault | Self::Bundle => "VaultError",
            Self::Value => "ValueError",
        }
    }
}

impl From<std::io::Error> for BulkError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}

impl From<StoreError> for BulkError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NoKey | StoreError::Vault => Self::Vault,
            _ => Self::Io,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub enabled: bool,
    pub files: usize,
    pub sealed: usize,
}

fn ensure_root(store: &Store) -> Result<(), BulkError> {
    std::fs::create_dir_all(&store.root)?;
    // Python makes a best-effort 0700 adjustment to its synthetic data root.
    let _ = std::fs::set_permissions(&store.root, std::fs::Permissions::from_mode(0o700));
    Ok(())
}

pub fn enabled(store: &Store) -> bool {
    store.encryption_enabled()
}

fn walk_files(
    store: &Store,
    resolved_root: &Path,
    folder: &Path,
    include_replay: bool,
    result: &mut Vec<PathBuf>,
) -> Result<(), BulkError> {
    for entry in std::fs::read_dir(folder)? {
        let path = entry?.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            walk_files(store, resolved_root, &path, include_replay, result)?;
            continue;
        }
        if !metadata.is_file()
            || path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.ends_with(".lock") || name.ends_with(".tmp")
            })
        {
            continue;
        }
        let resolved = path.canonicalize()?;
        let Ok(relative) = resolved.strip_prefix(resolved_root) else {
            continue;
        };
        if store.sealable(&path) || (include_replay && store.replay_checkout(relative)) {
            result.push(path);
        }
    }
    Ok(())
}

pub(crate) fn files_with(store: &Store, include_replay: bool) -> Result<Vec<PathBuf>, BulkError> {
    ensure_root(store)?;
    let resolved_root = store.root.canonicalize()?;
    let mut result = Vec::new();
    walk_files(
        store,
        &resolved_root,
        &store.root,
        include_replay,
        &mut result,
    )?;
    Ok(result)
}

pub fn files(store: &Store) -> Result<Vec<PathBuf>, BulkError> {
    files_with(store, false)
}

pub fn status(store: &Store) -> Result<Status, BulkError> {
    let files = files(store)?;
    let mut sealed = 0;
    for path in &files {
        if std::fs::read(path)?.starts_with(crate::MAGIC) {
            sealed += 1;
        }
    }
    Ok(Status {
        enabled: enabled(store),
        files: files.len(),
        sealed,
    })
}

fn key(store: &Store, create: bool) -> Result<String, BulkError> {
    let key = store.key(create).map_err(BulkError::from)?;
    if fernet::Fernet::new(&key).is_none() {
        return Err(BulkError::Value);
    }
    Ok(key)
}

pub(crate) fn seal_bytes(store: &Store, raw: &[u8]) -> Result<Vec<u8>, BulkError> {
    let key = key(store, true)?;
    crate::seal(&key, raw).map_err(|_| BulkError::Value)
}

pub(crate) fn open_bytes(
    store: &Store,
    raw: &[u8],
    create_key: bool,
) -> Result<Vec<u8>, BulkError> {
    if !raw.starts_with(crate::MAGIC) {
        return Ok(raw.to_vec());
    }
    let key = key(store, create_key)?;
    crate::open(Some(&key), raw).map_err(|_| BulkError::Vault)
}

fn temporary(path: &Path) -> PathBuf {
    let temporary = atomic::tmp_for(path);
    let name = temporary
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    temporary.with_file_name(format!(
        "{}.vault.tmp",
        name.strip_suffix(".tmp").unwrap_or(&name)
    ))
}

fn rewrite_with(
    path: &Path,
    raw: &[u8],
    fault: &mut impl FnMut(&Path) -> Result<(), BulkError>,
) -> Result<(), BulkError> {
    // The synthetic fault enters before a temporary name or bytes exist, like
    // Python's replaced `_rewrite` adapter.
    fault(path)?;
    let (tmp, mut file) = loop {
        let tmp = temporary(path);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
        {
            Ok(file) => break (tmp, file),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };
    let result = (|| -> std::io::Result<()> {
        file.write_all(raw)?;
        file.flush()?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(BulkError::from)
}

pub(crate) fn is_fernet_token(path: &Path) -> Result<bool, BulkError> {
    let raw = std::fs::read(path)?;
    if raw.first() != Some(&b'g') {
        return Ok(false);
    }
    let Ok(decoded) = general_purpose::URL_SAFE.decode(raw) else {
        return Ok(false);
    };
    Ok(decoded.len() >= 73 && decoded.first() == Some(&0x80))
}

fn rewrite_targets(
    store: &Store,
    targets: &[PathBuf],
    decrypt: bool,
    mut fault: impl FnMut(&Path) -> Result<(), BulkError>,
) -> Result<usize, BulkError> {
    let mut count = 0;
    for path in targets {
        let raw = std::fs::read(path)?;
        if raw.starts_with(crate::MAGIC) != decrypt {
            continue;
        }
        let changed = if decrypt {
            open_bytes(store, &raw, true)?
        } else {
            seal_bytes(store, &raw)?
        };
        rewrite_with(path, &changed, &mut fault)?;
        count += 1;
    }
    Ok(count)
}

pub fn seal_files(store: &Store, targets: &[PathBuf]) -> Result<usize, BulkError> {
    seal_files_with(store, targets, |_| Ok(()))
}

pub(crate) fn seal_files_with(
    store: &Store,
    targets: &[PathBuf],
    mut fault: impl FnMut(&Path) -> Result<(), BulkError>,
) -> Result<usize, BulkError> {
    ensure_root(store)?;
    if !enabled(store) {
        return Ok(0);
    }
    let sealable: Vec<PathBuf> = targets
        .iter()
        .filter(|path| path.is_file() && store.sealable(path))
        .cloned()
        .collect();
    rewrite_targets(store, &sealable, false, |path| fault(path))
}

pub fn encrypt_all(store: &Store) -> Result<usize, BulkError> {
    let targets = files(store)?;
    encrypt_all_with_targets(store, &targets, |_| Ok(()))
}

pub(crate) fn encrypt_all_with_targets(
    store: &Store,
    targets: &[PathBuf],
    fault: impl FnMut(&Path) -> Result<(), BulkError>,
) -> Result<usize, BulkError> {
    for path in targets {
        let bundle_suffix = path
            .extension()
            .map(|suffix| suffix.to_string_lossy().to_lowercase())
            .is_some_and(|suffix| matches!(suffix.as_str(), "cbk" | "cbsync"));
        if bundle_suffix || is_fernet_token(path)? {
            return Err(BulkError::Bundle);
        }
    }
    let marker = store.root.join(MARKER);
    match std::fs::remove_file(marker) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    rewrite_targets(store, targets, false, fault)
}

pub fn decrypt_all(store: &Store) -> Result<usize, BulkError> {
    let targets = files_with(store, true)?;
    decrypt_all_with_targets(store, &targets, |_| Ok(()))
}

pub(crate) fn decrypt_all_with_targets(
    store: &Store,
    targets: &[PathBuf],
    fault: impl FnMut(&Path) -> Result<(), BulkError>,
) -> Result<usize, BulkError> {
    let count = rewrite_targets(store, targets, true, fault)?;
    std::fs::write(store.root.join(MARKER), MARKER_TEXT)?;
    Ok(count)
}
