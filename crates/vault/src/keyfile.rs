//! Native key provider (rust-05c): the port of `src/careerbrew/keyfile.py`.
//!
//! Callers supply the facts Python reads from its process (the
//! `CAREERBREW_KEY_FILE` value, the home folder, the data root, whether sealed
//! data exists), so this module never depends on later backup, config or
//! event code. No production dispatch uses it yet.

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// Why a key could not be read or made. Messages match Python's fix lines.
#[derive(Debug)]
pub enum KeyError {
    /// The key file is inside the data folder (Python: `ValueError`).
    InsideDataRoot,
    /// The key is missing and `create` is false (Python: `FileNotFoundError`).
    Missing(PathBuf),
    /// The key is missing but sealed data exists (Python: `VaultError`).
    MissingWithSealedData,
    /// Another writer left the key empty (Python: `OSError`).
    Empty(PathBuf),
    /// Any other file system error.
    Io(std::io::Error),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::InsideDataRoot => write!(f, "backup key must be outside the data folder"),
            KeyError::Missing(p) => write!(
                f,
                "backup key missing at {} (fix: restore it from your password manager)",
                p.display()
            ),
            KeyError::MissingWithSealedData => write!(
                f,
                "backup key missing but encrypted data exists \
                 (fix: restore the key from your password manager)"
            ),
            KeyError::Empty(p) => write!(
                f,
                "backup key at {} is empty (fix: restore it from your password manager)",
                p.display()
            ),
            KeyError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<std::io::Error> for KeyError {
    fn from(e: std::io::Error) -> Self {
        KeyError::Io(e)
    }
}

/// Facts the Python provider reads from the process; supplied by the caller.
pub struct KeyFacts<'a> {
    /// The value of `CAREERBREW_KEY_FILE`, if set.
    pub env_key_file: Option<&'a str>,
    /// The home folder (`Path.home()`).
    pub home: &'a Path,
    /// The data root (`paths.data_root(create=False)`).
    pub data_root: &'a Path,
    /// `vault._has_sealed_data()`; asked only when a key must be created.
    pub has_sealed_data: &'a dyn Fn() -> bool,
}

/// `keyfile.key_path`: the env path with `~` expanded, else the default.
pub fn key_path(env_key_file: Option<&str>, home: &Path) -> PathBuf {
    match env_key_file {
        Some(v) if !v.is_empty() => expand_user(v, home),
        _ => home.join(".config").join("careerbrew").join("backup.key"),
    }
}

fn expand_user(value: &str, home: &Path) -> PathBuf {
    if value == "~" {
        return home.to_path_buf();
    }
    match value.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(value),
    }
}

/// `Path.resolve()` (non-strict): canonicalize the longest existing
/// ancestor, then apply the remaining parts lexically. A relative path is
/// joined to the current folder first, as Python does.
fn resolve(path: &Path) -> PathBuf {
    let path = match std::env::current_dir() {
        Ok(cwd) if path.is_relative() => cwd.join(path),
        _ => path.to_path_buf(),
    };
    let path = path.as_path();
    let mut rest: Vec<OsString> = Vec::new();
    let mut base = path.to_path_buf();
    loop {
        if let Ok(mut out) = base.canonicalize() {
            for part in rest.iter().rev() {
                match Path::new(part).components().next() {
                    Some(Component::ParentDir) => {
                        out.pop();
                    }
                    Some(Component::CurDir) | None => {}
                    Some(_) => out.push(part),
                }
            }
            return out;
        }
        let last = base.components().next_back();
        match (last, base.parent()) {
            (Some(c), Some(parent)) => {
                rest.push(c.as_os_str().to_owned());
                base = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// `paths.require_outside_data_root`.
pub fn require_outside_data_root(path: &Path, data_root: &Path) -> Result<(), KeyError> {
    if resolve(path).starts_with(resolve(data_root)) {
        return Err(KeyError::InsideDataRoot);
    }
    Ok(())
}

/// Python `bytes.strip()`: ASCII whitespace at both ends.
fn strip(bytes: &[u8]) -> Vec<u8> {
    let ws = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let start = bytes.iter().position(|b| !ws(b)).unwrap_or(bytes.len());
    let end = bytes.iter().rposition(|b| !ws(b)).map_or(start, |i| i + 1);
    bytes[start..end].to_vec()
}

/// `keyfile.load_key`: read the key at `path` (default `key_path`), or with
/// `create` make a new one at mode 0600 with `O_CREAT | O_EXCL`. A run that
/// loses the creation race reads the winner's key.
pub fn load_key(create: bool, path: Option<&Path>, facts: &KeyFacts) -> Result<Vec<u8>, KeyError> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => key_path(facts.env_key_file, facts.home),
    };
    require_outside_data_root(&path, facts.data_root)?;
    if path.exists() {
        return Ok(strip(&std::fs::read(&path)?));
    }
    if !create {
        return Err(KeyError::Missing(path));
    }
    if (facts.has_sealed_data)() {
        return Err(KeyError::MissingWithSealedData);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let key = fernet::Fernet::generate_key().into_bytes();
    let opened = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path);
    let mut file = match opened {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => return read_existing_key(&path),
        Err(e) => return Err(e.into()),
    };
    file.write_all(&key)?;
    drop(file);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok(key)
}

/// Read a key another process just created; wait briefly while it finishes.
fn read_existing_key(path: &Path) -> Result<Vec<u8>, KeyError> {
    for _ in 0..50 {
        let key = strip(&std::fs::read(path)?);
        if !key.is_empty() {
            return Ok(key);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err(KeyError::Empty(path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cbv-keyfile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn facts<'a>(home: &'a Path, root: &'a Path, sealed: &'a dyn Fn() -> bool) -> KeyFacts<'a> {
        KeyFacts {
            env_key_file: None,
            home,
            data_root: root,
            has_sealed_data: sealed,
        }
    }

    #[test]
    fn key_path_defaults_and_expands_the_env_value() {
        let home = Path::new("/synthetic-home");
        let default = home.join(".config/careerbrew/backup.key");
        assert_eq!(key_path(None, home), default);
        assert_eq!(key_path(Some(""), home), default);
        assert_eq!(key_path(Some("~/k.key"), home), home.join("k.key"));
        assert_eq!(
            key_path(Some("/abs/k.key"), home),
            PathBuf::from("/abs/k.key")
        );
    }

    #[test]
    fn creates_a_0600_fernet_key_once_and_reads_it_back() {
        let dir = scratch("create");
        let root = dir.join("data");
        let no = || false;
        let f = facts(&dir, &root, &no);
        let key = load_key(true, None, &f).unwrap();
        let path = dir.join(".config/careerbrew/backup.key");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert!(fernet::Fernet::new(std::str::from_utf8(&key).unwrap()).is_some());
        assert_eq!(load_key(true, None, &f).unwrap(), key);
        std::fs::write(&path, [b"\n ".as_slice(), &key, b"\n"].concat()).unwrap();
        assert_eq!(load_key(false, None, &f).unwrap(), key);
    }

    #[test]
    fn missing_key_refuses_without_create_or_with_sealed_data() {
        let dir = scratch("missing");
        let root = dir.join("data");
        let yes = || true;
        let f = facts(&dir, &root, &yes);
        assert!(matches!(
            load_key(false, None, &f),
            Err(KeyError::Missing(_))
        ));
        assert!(matches!(
            load_key(true, None, &f),
            Err(KeyError::MissingWithSealedData)
        ));
        assert!(!dir.join(".config/careerbrew/backup.key").exists());
    }

    #[test]
    fn a_key_inside_the_data_root_is_refused() {
        let dir = scratch("inside");
        let root = dir.join("data");
        std::fs::create_dir_all(&root).unwrap();
        let no = || false;
        let f = facts(&dir, &root, &no);
        let inside = root.join("sub/../backup.key");
        assert!(matches!(
            load_key(true, Some(&inside), &f),
            Err(KeyError::InsideDataRoot)
        ));
        assert!(!root.join("backup.key").exists());
    }

    #[test]
    fn an_empty_existing_key_from_a_racing_writer_is_an_error() {
        let dir = scratch("empty");
        let path = dir.join("k.key");
        std::fs::write(&path, b"  \n").unwrap();
        assert!(matches!(read_existing_key(&path), Err(KeyError::Empty(_))));
    }

    #[test]
    fn a_relative_path_resolves_against_the_current_folder() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let out = resolve(Path::new("cbv-missing-part/k.key"));
        assert_eq!(out, cwd.join("cbv-missing-part/k.key"));
        assert!(out.is_absolute());
    }

    #[test]
    fn losing_the_creation_race_reads_the_winners_key() {
        let dir = scratch("race");
        let root = dir.join("data");
        let path = dir.join(".config/careerbrew/backup.key");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let winner = fernet::Fernet::generate_key().into_bytes();
        std::fs::write(&path, [winner.as_slice(), b"\n"].concat()).unwrap();
        assert_eq!(read_existing_key(&path).unwrap(), winner);
        let no = || false;
        let f = facts(&dir, &root, &no);
        assert_eq!(load_key(true, None, &f).unwrap(), winner);
    }
}
