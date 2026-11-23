//! Component-only encrypted tar.gz backup create, verify and restore.
//!
//! The caller supplies the data root, external key path, targets, role and
//! corporate destination policy. No production dispatch selects this module.

use crate::{json_yaml, keyfile, VaultError};
use base64::{engine::general_purpose, Engine as _};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const KEEP: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Personal,
    Corporate,
}

#[derive(Debug)]
pub enum BackupError {
    Io,
    Value,
    Verification,
    HashMismatch,
    NameMismatch,
    ReadArchive,
    InvalidNames,
    DestinationLink,
    Role,
    Key(keyfile::KeyError),
}

impl BackupError {
    pub fn python_kind(&self) -> &'static str {
        match self {
            Self::Io => "OSError",
            Self::Value => "ValueError",
            Self::Verification | Self::HashMismatch | Self::NameMismatch => "RuntimeError",
            Self::ReadArchive => "ReadError",
            Self::InvalidNames | Self::DestinationLink => "RuntimeError",
            Self::Role => "RoleError",
            Self::Key(error) => match error {
                keyfile::KeyError::InsideDataRoot => "ValueError",
                keyfile::KeyError::Missing(_) => "FileNotFoundError",
                keyfile::KeyError::MissingWithSealedData => "VaultError",
                keyfile::KeyError::Empty(_) | keyfile::KeyError::Io(_) => "OSError",
                keyfile::KeyError::UnknownUser => "RuntimeError",
            },
        }
    }

    pub fn message(&self, archive: &Path) -> String {
        let name = archive.file_name().unwrap_or_default().to_string_lossy();
        match self {
            Self::Verification => format!("backup {name} failed verification: wrong key or corrupted file"),
            Self::HashMismatch => format!("backup {name} hash mismatch"),
            Self::NameMismatch => format!("backup {name} content does not match its name"),
            Self::InvalidNames => format!("backup {name} holds file name(s) careerbrew never writes; nothing restored"),
            Self::DestinationLink => "restore destination holds a link on a restored path; remove the link or choose an empty folder; nothing restored".into(),
            Self::Role => "this backup holds job-hunt data; restore it on your personal machine, never on a corporate one".into(),
            Self::Key(error) => error.to_string(),
            Self::Value => "invalid backup key".into(),
            Self::ReadArchive => "backup archive cannot be read".into(),
            Self::Io => "backup file operation failed".into(),
        }
    }
}

impl From<std::io::Error> for BackupError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}

impl From<keyfile::KeyError> for BackupError {
    fn from(error: keyfile::KeyError) -> Self {
        Self::Key(error)
    }
}

fn sha(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn any_sealed(folder: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            return false;
        };
        if metadata.file_type().is_symlink() {
            return false;
        }
        if metadata.is_dir() {
            any_sealed(&path)
        } else {
            metadata.is_file()
                && std::fs::read(path)
                    .ok()
                    .is_some_and(|raw| raw.starts_with(crate::MAGIC))
        }
    })
}

pub fn load_key(root: &Path, path: &Path, create: bool) -> Result<String, BackupError> {
    let has_sealed = || any_sealed(root);
    let facts = keyfile::KeyFacts {
        env_key_file: None,
        home: Path::new("/synthetic-home"),
        data_root: root,
        has_sealed_data: &has_sealed,
    };
    let bytes = keyfile::load_key(create, Some(path), &facts)?;
    String::from_utf8(bytes).map_err(|_| BackupError::Value)
}

fn excluded(relative: &Path) -> bool {
    relative
        .components()
        .any(|part| matches!(part.as_os_str().to_str(), Some("cache" | ".staging")))
}

fn collect_files(root: &Path, folder: &Path, found: &mut Vec<PathBuf>) -> Result<(), BackupError> {
    for entry in std::fs::read_dir(folder)? {
        let path = entry?.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        let relative = path.strip_prefix(root).map_err(|_| BackupError::Io)?;
        if excluded(relative) {
            continue;
        }
        if metadata.is_dir() {
            collect_files(root, &path, found)?;
        } else if metadata.is_file() {
            found.push(path);
        }
    }
    Ok(())
}

/// Create the uncompressed tar bytes. Symlinks stay out; hard links become
/// independent regular files, matching Python's dereference=True.
pub fn archive(root: &Path) -> Result<Vec<u8>, BackupError> {
    std::fs::create_dir_all(root)?;
    let _ = std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700));
    let mut files = Vec::new();
    if root.exists() {
        collect_files(root, root, &mut files)?;
    }
    files.sort_by_key(|path| {
        path.strip_prefix(root)
            .map(|relative| relative.as_os_str().as_bytes().to_vec())
            .unwrap_or_default()
    });
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut tar = tar::Builder::new(encoder);
    for path in files {
        let relative = path.strip_prefix(root).map_err(|_| BackupError::Io)?;
        tar.append_path_with_name(&path, relative)?;
    }
    let encoder = tar.into_inner()?;
    Ok(encoder.finish()?)
}

fn cipher(key: &str) -> Result<fernet::Fernet, BackupError> {
    fernet::Fernet::new(key).ok_or(BackupError::Value)
}

/// Strict URL-safe base64 is a separate archive boundary. Unlike vault reads,
/// extra padding and every trailing byte are refused before Fernet decrypts.
pub fn decrypt_archive(raw: &[u8], key: &str) -> Result<Vec<u8>, BackupError> {
    if raw.is_empty() || !raw.len().is_multiple_of(4) {
        return Err(BackupError::Verification);
    }
    general_purpose::URL_SAFE
        .decode(raw)
        .map_err(|_| BackupError::Verification)?;
    let token = std::str::from_utf8(raw).map_err(|_| BackupError::Verification)?;
    crate::decrypt_like_python(&cipher(key)?, token).map_err(|VaultError| BackupError::Verification)
}

fn read_archive(path: &Path, key_path: &Path, root: &Path) -> Result<Vec<u8>, BackupError> {
    let raw = std::fs::read(path)?;
    let key = load_key(root, key_path, false)?;
    decrypt_archive(&raw, &key)
}

pub fn verify(
    path: &Path,
    key_path: &Path,
    root: &Path,
    expect_sha: Option<&str>,
) -> Result<usize, BackupError> {
    let raw = read_archive(path, key_path, root)?;
    let digest = sha(&raw);
    if expect_sha.is_some_and(|expected| expected != digest) {
        return Err(BackupError::HashMismatch);
    }
    let name_matches = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.rsplit('-').next())
        .is_some_and(|suffix| suffix.starts_with(&digest[..12]));
    if !name_matches {
        return Err(BackupError::NameMismatch);
    }
    let decoder = GzDecoder::new(raw.as_slice());
    let mut tar = tar::Archive::new(decoder);
    let mut count = 0;
    for entry in tar.entries().map_err(|_| BackupError::ReadArchive)? {
        entry.map_err(|_| BackupError::ReadArchive)?;
        count += 1;
    }
    Ok(count)
}

pub fn create(
    root: &Path,
    key_path: &Path,
    targets: &[PathBuf],
    stamp: &str,
) -> Result<Vec<PathBuf>, BackupError> {
    create_raw(&archive(root)?, root, key_path, targets, stamp)
}

/// Write and verify already assembled tar.gz bytes. The synthetic differential
/// workload uses this to supply malformed member names and link entries.
pub fn create_raw(
    raw: &[u8],
    root: &Path,
    key_path: &Path,
    targets: &[PathBuf],
    stamp: &str,
) -> Result<Vec<PathBuf>, BackupError> {
    let digest = sha(raw);
    let key = load_key(root, key_path, true)?;
    let token = cipher(&key)?.encrypt(raw);
    let mut written = Vec::new();
    for target in targets {
        std::fs::create_dir_all(target)?;
        let path = target.join(format!("careerbrew-{stamp}-{}.cbk", &digest[..12]));
        std::fs::write(&path, token.as_bytes())?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        verify(&path, key_path, root, Some(&digest))?;
        written.push(path);
        let mut backups = std::fs::read_dir(target)?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("careerbrew-") && name.ends_with(".cbk"))
            })
            .collect::<Vec<_>>();
        backups.sort_by_key(|path| path.file_name().unwrap_or_default().to_owned());
        let remove = backups.len().saturating_sub(KEEP);
        for old in backups.into_iter().take(remove) {
            std::fs::remove_file(old)?;
        }
    }
    Ok(written)
}

pub fn latest(targets: &[PathBuf]) -> Result<Option<PathBuf>, BackupError> {
    let mut found = Vec::new();
    for target in targets {
        if !target.exists() {
            continue;
        }
        found.extend(
            std::fs::read_dir(target)?
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name.starts_with("careerbrew-") && name.ends_with(".cbk")
                        })
                }),
        );
    }
    found.sort_by_key(|path| path.file_name().unwrap_or_default().to_owned());
    Ok(found.pop())
}

#[derive(Debug)]
struct Member {
    name: String,
    data: Vec<u8>,
    mode: u32,
}

fn members(raw: &[u8]) -> Result<Vec<Member>, BackupError> {
    let decoder = GzDecoder::new(raw);
    let mut tar = tar::Archive::new(decoder);
    let mut result = Vec::new();
    for entry in tar.entries().map_err(|_| BackupError::ReadArchive)? {
        let mut entry = entry.map_err(|_| BackupError::ReadArchive)?;
        let name = entry
            .path()
            .map_err(|_| BackupError::ReadArchive)?
            .to_str()
            .ok_or(BackupError::InvalidNames)?
            .to_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        result.push(Member {
            name,
            data,
            mode: entry.header().mode().unwrap_or(0o600) & 0o777,
        });
    }
    Ok(result)
}

fn normalized(name: &str) -> String {
    let absolute = name.starts_with('/');
    let mut parts = Vec::new();
    for part in name.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push(part);
                }
            }
            _ => parts.push(part),
        }
    }
    let body = parts.join("/");
    if absolute {
        format!("/{body}")
    } else if body.is_empty() {
        ".".into()
    } else {
        body
    }
}

fn plain_name(member: &Member) -> bool {
    !member.name.starts_with('/')
        && !member.name.contains('\\')
        && !member.name.split('/').any(|part| part == "..")
        && member.name == normalized(&member.name)
}

fn personal_output(name: &str) -> bool {
    name.strip_prefix("out/").is_some_and(|rest| {
        (rest.starts_with("interview-") && rest.ends_with(".md"))
            || rest == "strategy.md"
            || rest == "progress.html"
            || rest.starts_with("reports/")
    })
}

fn profile_has_job_hunt(text: &str) -> bool {
    let Ok(value) = json_yaml::yaml_read(text) else {
        return true;
    };
    match value {
        crate::canon::Canon::Null => text.lines().any(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#')
        }),
        crate::canon::Canon::Map(items) => items.iter().any(|(key, _)| {
            matches!(key, crate::canon::Canon::Str(key) if matches!(key.trim(), "target_salary" | "remote_preference" | "want_to_learn"))
        }),
        _ => true,
    }
}

fn goal_rows<'a>(
    value: &'a crate::canon::Canon,
    key: &str,
) -> Option<Vec<&'a crate::canon::Canon>> {
    let crate::canon::Canon::Map(items) = value else {
        return None;
    };
    match items
        .iter()
        .find(|(name, _)| matches!(name, crate::canon::Canon::Str(name) if name == key))
        .map(|(_, value)| value)
    {
        None | Some(crate::canon::Canon::Null) => Some(Vec::new()),
        Some(crate::canon::Canon::List(rows)) => Some(rows.iter().collect()),
        _ => None,
    }
}

fn goals_have_job_hunt(text: &str) -> bool {
    let Ok(value) = json_yaml::yaml_read(text) else {
        return true;
    };
    match &value {
        crate::canon::Canon::Null => false,
        crate::canon::Canon::Map(items) if items.is_empty() => false,
        crate::canon::Canon::Map(_) => {
            let Some(queue) = goal_rows(&value, "queue") else {
                return true;
            };
            let Some(history) = goal_rows(&value, "history") else {
                return true;
            };
            let upcoming = match &value {
                crate::canon::Canon::Map(items) => items
                    .iter()
                    .find(|(name, _)| matches!(name, crate::canon::Canon::Str(name) if name == "upcoming"))
                    .map(|(_, value)| value),
                _ => None,
            };
            queue
                .into_iter()
                .chain(history)
                .chain(upcoming.filter(|value| !matches!(value, crate::canon::Canon::Null)))
                .any(|goal| match goal {
                crate::canon::Canon::Map(items) => items.iter().any(|(name, value)| {
                    matches!(name, crate::canon::Canon::Str(name) if name == "kind")
                        && matches!(value, crate::canon::Canon::Str(kind) if matches!(kind.as_str(), "interview" | "job-search"))
                }),
                _ => true,
            })
        }
        _ => true,
    }
}

fn open_member(data: &[u8], key: &str) -> Result<String, BackupError> {
    let raw = if data.starts_with(crate::MAGIC) {
        crate::open(Some(key), data).map_err(|_| BackupError::Verification)?
    } else {
        data.to_vec()
    };
    String::from_utf8(raw).map_err(|_| BackupError::Value)
}

fn job_hunt_member(member: &Member, key: &str) -> bool {
    let name = member.name.to_lowercase();
    if name
        .split('/')
        .next()
        .is_some_and(|first| matches!(first, "jobs" | "tutor" | "prep" | "linkedin"))
        || personal_output(&name)
    {
        return true;
    }
    if !matches!(name.as_str(), "profile.yaml" | "goals.yaml") {
        return false;
    }
    let Ok(text) = open_member(&member.data, key) else {
        return true;
    };
    if name == "goals.yaml" {
        goals_have_job_hunt(&text)
    } else {
        profile_has_job_hunt(&text)
    }
}

/// Validate every member and every existing destination link before the first
/// write. `machine.yaml` never crosses machines.
pub fn restore(
    path: &Path,
    destination: &Path,
    key_path: &Path,
    root: &Path,
    role: Role,
) -> Result<usize, BackupError> {
    verify(path, key_path, root, None)?;
    let raw = read_archive(path, key_path, root)?;
    let all = members(&raw)?;
    if all.iter().any(|member| !plain_name(member)) {
        return Err(BackupError::InvalidNames);
    }
    // Non-regular entries carry no bytes in the synthetic archives. Reject
    // them during a second typed pass, including links and directories.
    let decoder = GzDecoder::new(raw.as_slice());
    let mut tar = tar::Archive::new(decoder);
    for entry in tar.entries().map_err(|_| BackupError::ReadArchive)? {
        let entry = entry.map_err(|_| BackupError::ReadArchive)?;
        if !entry.header().entry_type().is_file() {
            return Err(BackupError::InvalidNames);
        }
    }
    let selected = all
        .iter()
        .filter(|member| member.name.to_lowercase() != "machine.yaml")
        .collect::<Vec<_>>();
    let key = load_key(root, key_path, false)?;
    if role == Role::Corporate && selected.iter().any(|member| job_hunt_member(member, &key)) {
        return Err(BackupError::Role);
    }
    let destination = keyfile::resolve(destination);
    for member in &selected {
        let parts = member.name.split('/').collect::<Vec<_>>();
        for index in 1..=parts.len() {
            let path = parts[..index]
                .iter()
                .fold(destination.clone(), |path, part| path.join(part));
            if std::fs::symlink_metadata(&path)
                .ok()
                .is_some_and(|metadata| metadata.file_type().is_symlink())
            {
                return Err(BackupError::DestinationLink);
            }
        }
    }
    std::fs::create_dir_all(&destination)?;
    let count = selected.len();
    for member in selected {
        let path = destination.join(&member.name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options
            .write(true)
            .create(true)
            .truncate(true)
            .mode(member.mode);
        let mut file = options.open(path)?;
        file.write_all(&member.data)?;
        std::fs::set_permissions(
            destination.join(&member.name),
            std::fs::Permissions::from_mode(member.mode),
        )?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_base64_rejects_trailing_data_and_padding() {
        let key = general_purpose::URL_SAFE.encode((0_u8..32).collect::<Vec<_>>());
        let token = cipher(&key).unwrap().encrypt(b"synthetic");
        assert!(decrypt_archive(token.as_bytes(), &key).is_ok());
        assert!(decrypt_archive(format!("{token}x").as_bytes(), &key).is_err());
        assert!(decrypt_archive(format!("{token}=").as_bytes(), &key).is_err());
        assert!(decrypt_archive(&token.as_bytes()[..token.len() - 1], &key).is_err());
    }

    #[test]
    fn plain_names_keep_two_dot_prefixes_but_reject_traversal() {
        let member = |name: &str| Member {
            name: name.into(),
            data: Vec::new(),
            mode: 0o600,
        };
        assert!(plain_name(&member("..notes.md")));
        for name in [
            "./jobs/x",
            "work/../machine.yaml",
            "/profile.yaml",
            "work\\x",
        ] {
            assert!(!plain_name(&member(name)), "{name}");
        }
    }
}
