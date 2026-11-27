//! The `/api/changes` version: `server.files_version(CHANGE_DIRS)`.
//!
//! A hash of file names, inodes, sizes and mtimes (never content) under the
//! data root's `work/` and `day/` folders, with the same skip rules.

use sha2::{Digest, Sha256};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Folders whose writes change what Today, Week and Dashboard show.
pub const CHANGE_DIRS: &[&str] = &["work", "day"];

/// Locks, the server's own state and the watcher's bookkeeping: no view input.
pub const CHANGE_SKIP: &[&str] = &[
    "web.json",
    "web-token.json",
    "web.lock",
    "inbox.lock",
    "watch.lock",
    "watch.json",
    "cursors.json",
    "telemetry.json",
    "telemetry.lock",
    "telemetry-ask.lock",
    "feed-cycle.json",
    "loops.json",
];

/// The PM draft the Day log view showed: only a save reads it back.
pub const CHANGE_SKIP_PREFIXES: &[&str] = &["pm-shown-"];

fn pm_sent_lock(name: &str) -> bool {
    // \.pm-sent-[0-9]{4}-[0-9]{2}-[0-9]{2}\.lock
    let Some(rest) = name
        .strip_prefix(".pm-sent-")
        .and_then(|r| r.strip_suffix(".lock"))
    else {
        return false;
    };
    let b = rest.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn row(path: &Path, name: &str, prefix: &str, rows: &mut Vec<String>) {
    if CHANGE_SKIP.contains(&name)
        || CHANGE_SKIP_PREFIXES.iter().any(|p| name.starts_with(p))
        || name.ends_with(".tmp")
    {
        return;
    }
    // `DirEntry.stat()` follows a symlink; a file removed while we scan is gone.
    let Ok(st) = std::fs::metadata(path) else {
        return;
    };
    if prefix == "day/" && st.size() == 0 && (name == ".pm-edits.jsonl.lock" || pm_sent_lock(name))
    {
        return;
    }
    if prefix == "work/feed/" && name == ".lock" && st.size() == 0 {
        return;
    }
    let mtime_ns = i128::from(st.mtime()) * 1_000_000_000 + i128::from(st.mtime_nsec());
    rows.push(format!(
        "{prefix}{name}:{}:{}:{mtime_ns}\n",
        st.ino(),
        st.size()
    ));
}

fn scan(base: &Path, prefix: &str, rows: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            scan(&path, &format!("{prefix}{name}/"), rows);
        } else if std::fs::metadata(&path).is_ok_and(|m| m.is_file()) {
            row(&path, &name, prefix, rows);
        }
    }
}

/// `files_version(dirs)` for the given top-level folders of `root`.
pub fn files_version(root: &Path, dirs: &[&str]) -> String {
    let mut rows = Vec::new();
    for dir in dirs {
        let base = root.join(dir);
        if base.is_dir() {
            scan(&base, &format!("{dir}/"), &mut rows);
        }
    }
    rows.sort();
    let digest = Sha256::digest(rows.concat().as_bytes());
    format!("{digest:x}")[..16].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_rules_follow_the_python_version() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("day")).unwrap();
        std::fs::create_dir_all(root.join("work/feed")).unwrap();
        std::fs::write(root.join("work/a.json"), "x").unwrap();
        let before = files_version(root, CHANGE_DIRS);
        for name in [
            "day/web.json",
            "day/pm-shown-2026-11-20.json",
            "day/x.tmp",
            "day/.pm-sent-2026-11-20.lock",
            "day/.pm-edits.jsonl.lock",
            "work/feed/.lock",
        ] {
            std::fs::write(root.join(name), "").unwrap();
        }
        assert_eq!(files_version(root, CHANGE_DIRS), before);
        std::fs::write(root.join("day/.pm-sent-2026-11-2x.lock"), "").unwrap();
        assert_ne!(files_version(root, CHANGE_DIRS), before);
        assert!(pm_sent_lock(".pm-sent-2026-11-20.lock"));
        assert!(!pm_sent_lock(".pm-sent-2026-1-200.lock"));
    }
}
