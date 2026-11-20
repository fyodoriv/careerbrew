//! Native atomic write (rust-05d): the port of `store._tmp_for`,
//! `store.write_text` and `store.write_text_if_changed`.
//!
//! - Each write goes to a temp sibling `.<name>.<pid>.<8 hex>.tmp`, unique to
//!   the writer, created with `O_EXCL`, then renamed over the target.
//! - A failed write or rename removes the temp file (rust-05b contract).
//! - `write_text_if_changed` leaves the file and its mtime alone when the
//!   stored text already matches; an unreadable file is written again.
//!
//! The caller supplies the key and whether the path is sealed (Python:
//! `vault.enabled() and vault.sealable(path)`), so this module needs no
//! config or event code. No production dispatch uses it yet.

use std::collections::hash_map::RandomState;
use std::fs::OpenOptions;
use std::hash::{BuildHasher, Hasher};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Eight lowercase hex digits, like Python's `uuid4().hex[:8]`.
fn tag() -> String {
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    hasher.write_u128(nanos);
    format!("{:08x}", hasher.finish() as u32)
}

/// `store._tmp_for`: a temp sibling name unique to this writer.
pub fn tmp_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), tag()))
}

/// Write `bytes` to `path` through a unique temp file and a rename.
pub fn write_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let (tmp, mut file) = loop {
        let tmp = tmp_for(path);
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => break (tmp, file),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let result = file
        .write_all(bytes)
        .and_then(|()| file.flush())
        .and_then(|()| {
            drop(file);
            std::fs::rename(&tmp, path)
        });
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Why a store write failed.
#[derive(Debug)]
pub enum StoreError {
    /// A sealed write had no key.
    NoKey,
    /// The codec refused.
    Vault,
    /// A file system error.
    Io(std::io::Error),
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

/// `vault.encode`: seal with the key, or keep the text as is.
fn encode(key: Option<&str>, seal: bool, text: &str) -> Result<Vec<u8>, StoreError> {
    if !seal {
        return Ok(text.as_bytes().to_vec());
    }
    crate::seal(key.ok_or(StoreError::NoKey)?, text.as_bytes()).map_err(|_| StoreError::Vault)
}

/// `store.write_text`.
pub fn write_text(
    path: &Path,
    key: Option<&str>,
    seal: bool,
    text: &str,
) -> Result<(), StoreError> {
    let bytes = encode(key, seal, text)?;
    write_bytes(path, &bytes)?;
    Ok(())
}

/// `store.write_text_if_changed`: true when it wrote.
pub fn write_text_if_changed(
    path: &Path,
    key: Option<&str>,
    seal: bool,
    text: &str,
) -> Result<bool, StoreError> {
    match std::fs::read(path) {
        Ok(stored) => {
            if let Ok(plain) = crate::open(key, &stored) {
                if plain == text.as_bytes() {
                    return Ok(false);
                }
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    write_text(path, key, seal, text)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cbv-atomic-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn leftovers(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            })
            .count()
    }

    #[test]
    fn temp_names_are_hidden_siblings_and_unique() {
        let path = Path::new("/synthetic-home/data/x.json");
        let a = tmp_for(path);
        let b = tmp_for(path);
        assert_ne!(a, b);
        assert_eq!(a.parent(), path.parent());
        let name = a.file_name().unwrap().to_string_lossy().into_owned();
        let parts: Vec<&str> = name.split('.').collect();
        assert_eq!(parts[..3], ["", "x", "json"]);
        assert_eq!(parts[3], std::process::id().to_string());
        assert!(parts[4].len() == 8 && parts[4].bytes().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(parts[5], "tmp");
    }

    #[test]
    fn sealed_write_round_trips_and_leaves_no_temp() {
        let dir = scratch("seal");
        let path = dir.join("sub/a.yaml");
        write_text(&path, Some(KEY), true, "synthetic: 1\n").unwrap();
        let stored = std::fs::read(&path).unwrap();
        assert!(stored.starts_with(crate::MAGIC));
        assert_eq!(crate::open(Some(KEY), &stored).unwrap(), b"synthetic: 1\n");
        assert_eq!(leftovers(&dir.join("sub")), 0);
        assert!(matches!(
            write_text(&path, None, true, "x"),
            Err(StoreError::NoKey)
        ));
    }

    #[test]
    fn write_if_changed_skips_equal_text_and_keeps_the_mtime() {
        let dir = scratch("changed");
        let path = dir.join("b.md");
        assert!(write_text_if_changed(&path, Some(KEY), true, "one").unwrap());
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(!write_text_if_changed(&path, Some(KEY), true, "one").unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before
        );
        assert!(write_text_if_changed(&path, Some(KEY), true, "two").unwrap());
        assert_eq!(
            crate::open(Some(KEY), &std::fs::read(&path).unwrap()).unwrap(),
            b"two"
        );
    }

    #[test]
    fn an_unreadable_file_is_written_again() {
        let dir = scratch("unreadable");
        let path = dir.join("c.json");
        std::fs::write(&path, b"CBV1\n!").unwrap();
        assert!(write_text_if_changed(&path, Some(KEY), true, "{}").unwrap());
        assert_eq!(
            crate::open(Some(KEY), &std::fs::read(&path).unwrap()).unwrap(),
            b"{}"
        );
    }

    #[test]
    fn a_failed_rename_removes_the_temp_file() {
        let dir = scratch("fail");
        let target = dir.join("d");
        std::fs::create_dir_all(target.join("child")).unwrap();
        assert!(write_bytes(&target, b"x").is_err());
        assert_eq!(leftovers(&dir), 0);
    }

    #[test]
    fn plaintext_paths_stay_plain() {
        let dir = scratch("plain");
        let path = dir.join("e.txt");
        write_text(&path, None, false, "plain").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"plain");
    }
}
