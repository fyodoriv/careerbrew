//! Verify the direct source inventory against the same Git index.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

const INVENTORY: &str = "docs/plans/2026-11-01-rust-code-inventory.csv";
const HEADER: &str = "path,disposition,task,reason";
const EMBEDDED: &str = "docs/plans/2026-11-04-rust-embedded-anchors.csv";
const EMBEDDED_HEADER: &str = "path,task,anchor";
const NATIVE_CONFIG: &[&str] = &[
    ".cargo/config.toml",
    "Cargo.lock",
    "Cargo.toml",
    "rust-toolchain.toml",
    "xtask/Cargo.toml",
];
const SOURCE_EXTENSIONS: &[&str] = &[".py", ".sh", ".js", ".mjs", ".cjs", ".ts", ".tsx", ".rs"];

fn git(args: &[&str]) -> Result<String, &'static str> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|_| "cannot run Git")?;
    if !output.status.success() {
        return Err("cannot read the Git index");
    }
    String::from_utf8(output.stdout).map_err(|_| "Git index output is not UTF-8")
}

pub fn check() -> Result<usize, &'static str> {
    let tracked = git(&["ls-files", "--full-name", "--stage", "-z", "--", ":/"])?;
    let manifest = git(&["show", &format!(":{INVENTORY}")])?;
    verify(&tracked, &manifest)
}

pub fn check_embedded() -> Result<usize, &'static str> {
    let manifest = git(&["show", &format!(":{EMBEDDED}")])?;
    verify_embedded(&manifest, |path| git(&["show", &format!(":{path}")]))
}

fn verify_embedded(
    inventory: &str,
    mut read: impl FnMut(&str) -> Result<String, &'static str>,
) -> Result<usize, &'static str> {
    let mut lines = inventory.lines();
    if lines.next() != Some(EMBEDDED_HEADER) {
        return Err("invalid embedded inventory header");
    }
    let mut seen = BTreeSet::new();
    let mut sources = BTreeMap::new();
    for line in lines {
        let parsed = fields(line).ok_or("invalid embedded inventory row")?;
        let [path, task, anchor] = parsed.as_slice() else {
            return Err("invalid embedded inventory row");
        };
        if path.is_empty() || anchor.is_empty() || !migration_task(task) {
            return Err("invalid embedded inventory assignment");
        }
        if !seen.insert((path.clone(), anchor.clone())) {
            return Err("duplicate embedded anchor");
        }
        if !sources.contains_key(path) {
            sources.insert(path.clone(), read(path)?);
        }
        if sources[path]
            .lines()
            .filter(|line| line.trim() == anchor)
            .count()
            != 1
        {
            return Err("missing or ambiguous embedded anchor");
        }
    }
    if seen.is_empty() {
        return Err("embedded inventory is empty");
    }
    Ok(seen.len())
}

fn migration_task(task: &str) -> bool {
    task.strip_prefix("rust-").is_some_and(|number| {
        number.len() == 2 && number.bytes().all(|digit| digit.is_ascii_digit())
    })
}

fn fields(line: &str) -> Option<Vec<String>> {
    let mut rest = line.chars().peekable();
    let mut fields = Vec::new();
    loop {
        let mut field = String::new();
        if rest.next_if_eq(&'"').is_some() {
            loop {
                match rest.next()? {
                    '"' if rest.next_if_eq(&'"').is_none() => break,
                    ch => field.push(ch),
                }
            }
        } else {
            while let Some(ch) = rest.next_if(|ch| *ch != ',') {
                if ch == '"' {
                    return None;
                }
                field.push(ch);
            }
        }
        fields.push(field);
        match rest.next() {
            Some(',') => {}
            None => return Some(fields),
            _ => return None,
        }
    }
}

fn verify(index: &str, inventory: &str) -> Result<usize, &'static str> {
    let mut tracked = BTreeMap::new();
    for entry in index.strip_suffix('\0').unwrap_or(index).split('\0') {
        let (metadata, path) = entry.split_once('\t').ok_or("invalid Git index entry")?;
        let mut parts = metadata.split(' ');
        let mode = parts.next().ok_or("invalid Git index entry")?;
        if parts.next().is_none()
            || parts.next() != Some("0")
            || parts.next().is_some()
            || tracked.insert(path, mode).is_some()
        {
            return Err("unmerged or invalid Git index entry");
        }
    }
    let mut lines = inventory.lines();
    if lines.next() != Some(HEADER) {
        return Err("invalid inventory header");
    }
    let mut assigned = 0;
    for line in lines {
        let parsed = fields(line).ok_or("invalid inventory row")?;
        let [path, disposition, task, reason] = parsed.as_slice() else {
            return Err("invalid inventory row");
        };
        let valid_assignment = match disposition.as_str() {
            "keep-ui" => task.is_empty(),
            "keep-native" => task == "rust-00",
            "port" => migration_task(task),
            _ => false,
        };
        if reason.trim().is_empty() || !valid_assignment {
            return Err("invalid inventory assignment");
        }
        if tracked.remove(path.as_str()).is_none() {
            return Err("duplicate or untracked inventory assignment");
        }
        assigned += 1;
    }
    if tracked.iter().any(|(path, mode)| {
        *mode == "100755"
            || NATIVE_CONFIG.contains(path)
            || SOURCE_EXTENSIONS.iter().any(|ext| path.ends_with(*ext))
    }) {
        return Err("unassigned tracked source path");
    }
    Ok(assigned)
}

#[cfg(test)]
mod tests {
    use super::verify;

    const INDEX: &str = concat!("100644 oid 0\tx.rs\0", "100755 oid 0\tbin/check\0");
    const HEADER: &str = "path,disposition,task,reason\n";
    const SOURCE: &str = "x.rs,keep-native,rust-00,\"Already native, keep \"\"checked\"\"\"\n";
    const CHECK: &str = "bin/check,port,rust-27,Port gate\n";

    #[test]
    fn valid_inventory_keeps_both_assignments() {
        assert_eq!(verify(INDEX, &format!("{HEADER}{SOURCE}{CHECK}")), Ok(2));
    }

    #[test]
    fn missing_executable_is_refused() {
        assert_eq!(
            verify(INDEX, &format!("{HEADER}{SOURCE}")),
            Err("unassigned tracked source path")
        );
    }

    #[test]
    fn duplicate_or_untracked_row_is_refused() {
        for extra in [SOURCE, "untracked.rs,keep-native,rust-00,Outside index\n"] {
            assert_eq!(
                verify(INDEX, &format!("{HEADER}{SOURCE}{CHECK}{extra}")),
                Err("duplicate or untracked inventory assignment")
            );
        }
    }

    #[test]
    fn malformed_quote_is_refused() {
        assert_eq!(
            verify(
                INDEX,
                &format!("{HEADER}{SOURCE}bin/check,port,rust-27,\"open")
            ),
            Err("invalid inventory row")
        );
    }
}

#[cfg(test)]
mod embedded_tests {
    use super::*;

    #[test]
    fn embedded_rejects_changed_and_duplicate_anchors() {
        let header = "path,task,anchor\n";
        let row = "script.sh,rust-27,python -c example\n";
        let source = |_: &str| Ok("  python -c example\n".to_owned());
        assert_eq!(verify_embedded(&format!("{header}{row}"), source), Ok(1));
        assert!(verify_embedded(&format!("{header}{row}{row}"), source).is_err());
        assert!(verify_embedded(&format!("{header}{row}"), |_| Ok(
            "python -c changed\n".to_owned()
        ))
        .is_err());
    }
}
