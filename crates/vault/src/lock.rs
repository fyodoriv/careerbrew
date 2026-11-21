//! Advisory file lock shared with Python's `store.append_jsonl` and
//! `store.upsert_jsonl` (rust-05e, component only).
//!
//! Python locks the sibling `.{name}.lock` with `fcntl.flock(LOCK_EX)`.
//! `File::lock` uses `flock` on unix, so a Rust holder and a Python holder
//! exclude each other. The lock is released when the file handle drops.

use crate::atomic::{self, StoreError};
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

/// The sibling lock file Python uses: `.{name}.lock`.
pub fn lock_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.lock"))
}

/// Runs `f` while holding the exclusive lock for `path`.
pub fn with_lock<T>(path: &Path, f: impl FnOnce() -> T) -> std::io::Result<T> {
    // Append mode, created if missing, default mode: like Python's `open("a")`.
    let lock: File = OpenOptions::new()
        .append(true)
        .create(true)
        .open(lock_path(path))?;
    lock.lock()?;
    let out = f();
    drop(lock);
    Ok(out)
}

/// `store.append_jsonl` for rows already encoded as JSON lines.
///
/// A sealed file is one ciphertext: read, add, rewrite atomically, all under
/// the lock. A plain file just gets the text appended under the lock.
pub fn append_jsonl_text(
    path: &Path,
    key: Option<&str>,
    seal: bool,
    rows_text: &str,
) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    with_lock(path, || -> Result<(), StoreError> {
        if !seal {
            let mut fh = OpenOptions::new().append(true).create(true).open(path)?;
            fh.write_all(rows_text.as_bytes())?;
            return Ok(());
        }
        let old = match std::fs::read(path) {
            Ok(stored) => crate::open(key, &stored).map_err(|_| StoreError::Vault)?,
            Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        let mut text = String::from_utf8(old).map_err(|_| StoreError::Vault)?;
        text.push_str(rows_text);
        atomic::write_text(path, key, seal, &text)
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cb-lock-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lock_path_is_the_python_sibling() {
        let p = Path::new("a/b/rows.jsonl");
        assert_eq!(lock_path(p), Path::new("a/b/.rows.jsonl.lock"));
    }

    #[test]
    fn a_second_holder_waits_for_the_first() {
        let dir = scratch("wait");
        let path = dir.join("rows.jsonl");
        with_lock(&path, || {
            let other = OpenOptions::new()
                .append(true)
                .open(lock_path(&path))
                .unwrap();
            assert!(other.try_lock().is_err());
        })
        .unwrap();
        let other = OpenOptions::new()
            .append(true)
            .open(lock_path(&path))
            .unwrap();
        assert!(other.try_lock().is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sealed_appends_from_threads_keep_every_row() {
        let dir = scratch("sealed");
        let path = dir.join("rows.jsonl");
        let handles: Vec<_> = (0..8)
            .map(|t| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for i in 0..10 {
                        let row = format!("{{\"id\": \"t{t}-{i}\"}}\n");
                        append_jsonl_text(&path, Some(KEY), true, &row).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let plain = crate::open(Some(KEY), &std::fs::read(&path).unwrap()).unwrap();
        let text = String::from_utf8(plain).unwrap();
        assert_eq!(text.lines().count(), 80);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn plain_append_adds_text() {
        let dir = scratch("plain");
        let path = dir.join("sub").join("rows.jsonl");
        append_jsonl_text(&path, None, false, "{\"id\": 1}\n").unwrap();
        append_jsonl_text(&path, None, false, "{\"id\": 2}\n").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "{\"id\": 1}\n{\"id\": 2}\n");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
