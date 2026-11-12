//! Fixed snapshot export. The directory and file descriptors have snapshot scope.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::Path,
};

pub const CHUNK: u64 = 131072;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Identity {
    device: u64,
    inode: u64,
    mode: u32,
    uid: u32,
    links: u64,
    size: u64,
    mtime_ns: u64,
    ctime_ns: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    identity: Identity,
    sha256: String,
}
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    files: Vec<Entry>,
    issues: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Chunk {
    version: u32,
    name: String,
    offset: u64,
    length: u64,
    identity: Identity,
    sha256: String,
    data: Vec<u8>,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hash_ok(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn trace(name: &str) -> bool {
    name.strip_prefix("trace.")
        .and_then(|s| {
            s.parse::<u32>()
                .ok()
                .map(|pid| pid > 0 && pid.to_string() == s)
        })
        .unwrap_or(false)
}
fn name_ok(name: &str) -> bool {
    matches!(name, "stdout" | "stderr" | "status.json") || trace(name)
}
fn manifest(wire: &str, uid: u32) -> Result<Manifest, String> {
    if wire.len() > 65536 {
        return Err("manifest wire bound exceeded".into());
    }
    let value: Manifest = serde_json::from_str(wire).map_err(|_| "malformed export manifest")?;
    if value.version != 1
        || value.files.len() > 35
        || value.issues.len() > 66
        || value.issues.iter().any(|s| s.len() > 128)
    {
        return Err("invalid export manifest bounds".into());
    }
    let mut names = BTreeSet::new();
    let mut count = 0;
    let mut total = 0u64;
    for file in &value.files {
        let stamp = &file.identity;
        if !name_ok(&file.name)
            || !names.insert(&file.name)
            || !hash_ok(&file.sha256)
            || stamp.uid != uid
            || stamp.links != 1
            || stamp.mode & u32::from(libc::S_IFMT) != u32::from(libc::S_IFREG)
        {
            return Err("unsafe export manifest entry".into());
        }
        if trace(&file.name) {
            count += 1;
            total = total.checked_add(stamp.size).ok_or("trace byte overflow")?;
            if count > 32 || total > 8388608 {
                return Err("trace manifest bound exceeded".into());
            }
        } else if stamp.size > 1048576 {
            return Err("stream manifest bound exceeded".into());
        }
    }
    Ok(value)
}
fn folder_stamp(folder: &Path) -> Result<(u64, u64), String> {
    let value = fs::symlink_metadata(folder).map_err(|e| e.to_string())?;
    if !value.is_dir() || value.uid() != unsafe { libc::geteuid() } || value.mode() & 0o777 != 0o700
    {
        return Err("snapshot directory ownership differs".into());
    }
    Ok((value.dev(), value.ino()))
}
fn create_file(directory: &File, name: &str) -> Result<File, String> {
    let name = CString::new(name).map_err(|_| "invalid destination name")?;
    // The open directory descriptor anchors this exclusive write to its owned inode.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
            0o600,
        )
    };
    if fd < 0 {
        return Err(format!(
            "exclusive snapshot file: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
#[cfg_attr(test, allow(dead_code))]
pub fn export(
    folder: &Path,
    call: impl FnMut(&[String]) -> Result<String, String>,
) -> Result<Value, String> {
    export_with_uid(folder, 65534, call)
}
pub fn export_with_uid(
    folder: &Path,
    uid: u32,
    mut call: impl FnMut(&[String]) -> Result<String, String>,
) -> Result<Value, String> {
    fs::DirBuilder::new()
        .mode(0o700)
        .create(folder)
        .map_err(|e| format!("new snapshot directory required: {e}"))?;
    let stamp = folder_stamp(folder)?;
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(folder)
        .map_err(|e| e.to_string())?;
    let actual = directory.metadata().map_err(|e| e.to_string())?;
    if (actual.dev(), actual.ino()) != stamp {
        return Err("snapshot directory changed".into());
    }
    let first = manifest(&call(&["--export-manifest".into()])?, uid)?;
    let mut issues = first.issues.clone();
    let mut failures = Vec::new();
    let mut files = serde_json::Map::new();
    let mut chunks = 0u32;
    let names = first
        .files
        .iter()
        .map(|f| f.name.as_str())
        .collect::<BTreeSet<_>>();
    if !["stdout", "stderr", "status.json"]
        .iter()
        .all(|name| names.contains(name))
        || !names.iter().any(|name| trace(name))
    {
        issues.push("missing-required-files".into());
    }
    for entry in &first.files {
        let result = (|| -> Result<(), String> {
            if folder_stamp(folder)? != stamp {
                return Err("snapshot directory replaced".into());
            }
            let mut file = create_file(&directory, &entry.name)?;
            let mut hasher = Sha256::new();
            let mut offset = 0;
            while offset < entry.identity.size {
                chunks += 1;
                if chunks > 128 {
                    return Err("export chunk count exceeded".into());
                }
                let length = CHUNK.min(entry.identity.size - offset);
                let args = vec![
                    "--export-chunk".into(),
                    entry.name.clone(),
                    offset.to_string(),
                    length.to_string(),
                    serde_json::to_string(&entry.identity).map_err(|e| e.to_string())?,
                ];
                let wire = call(&args)?;
                if wire.len() > 786432 {
                    return Err("chunk wire bound exceeded".into());
                }
                let part: Chunk =
                    serde_json::from_str(&wire).map_err(|_| "malformed export chunk")?;
                if part.version != 1
                    || part.name != entry.name
                    || part.offset != offset
                    || part.length != length
                    || part.identity != entry.identity
                    || part.data.len() as u64 != length
                    || digest(&part.data) != part.sha256
                {
                    return Err("chunk identity/offset/bytes/hash differs".into());
                }
                file.write_all(&part.data).map_err(|e| e.to_string())?;
                hasher.update(&part.data);
                offset += length;
            }
            let metadata = file.metadata().map_err(|e| e.to_string())?;
            if metadata.len() != entry.identity.size
                || metadata.nlink() != 1
                || !metadata.is_file()
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.mode() & 0o777 != 0o600
                || format!("{:x}", hasher.finalize()) != entry.sha256
                || folder_stamp(folder)? != stamp
            {
                return Err("reconstructed file identity/hash differs".into());
            }
            files.insert(
                entry.name.clone(),
                json!({"bytes":metadata.len(),"regular":true,"sha256":entry.sha256}),
            );
            Ok(())
        })();
        if let Err(error) = result {
            failures.push(format!("{}:{error}", entry.name));
        }
    }
    match call(&["--export-manifest".into()]).and_then(|wire| manifest(&wire, uid)) {
        Ok(final_manifest) if final_manifest == first => {}
        Ok(_) => failures.push("final manifest changed".into()),
        Err(error) => failures.push(format!("final manifest unavailable:{error}")),
    }
    if folder_stamp(folder)? != stamp {
        return Err("snapshot directory replaced".into());
    }
    let completed = failures.is_empty();
    issues.extend(failures);
    Ok(
        json!({"copy_completed":completed,"raw_evidence_complete":completed&&issues.is_empty(),"files":files,"issues":issues,"chunk_reads":chunks}),
    )
}
