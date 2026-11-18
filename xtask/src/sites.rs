//! Discover embedded code and generated entry points in the staged Git index,
//! and check each one against its reviewed owner assignment.
//!
//! Embedded code: an interpreter call with inline code (`python3 -c`,
//! `node -e`, `bash -c`, a `<<'PY'` heredoc, an inline `<script>`) on one
//! line of an executed file. Entry points: pyproject console scripts,
//! package.json `bin` and `scripts`, Agentfile commands and hooks, git hooks
//! and lefthook `run` lines. A site's key is its trimmed line or its name.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

const SITES: &str = "docs/plans/2026-11-17-rust-embedded-sites.csv";
const HEADER: &str = "kind,path,key,task";

/// Paths whose text is executed by a shell, an interpreter or an agent.
const EXECUTED_PREFIXES: &[&str] = &[
    "scripts/",
    "hooks/",
    "commands/",
    "skills/",
    "src/",
    "tests/e2e/",
    ".github/",
    "evals/",
    "web/src/",
    "web/e2e/",
    "web/visual/",
];

/// Inline-code markers. A line holding any of them is one embedded site.
const MARKERS: &[&str] = &[
    "python -c",
    "python3 -c",
    "python3 -I -c",
    "python - <<",
    "python3 - <<",
    "node -e",
    "node --input-type",
    "bash -c",
    "sh -c '",
    "sh -c \"",
    "<<'PY'",
    "<<PY",
    "<< 'PY'",
    "<<'JS'",
    "<<'NODE'",
    "\"python3\", \"-c\"",
    "\"python\", \"-c\"",
    "\"node\", \"-e\"",
    "\"bash\", \"-c\"",
    "sys.executable, \"-c\"",
    "<script>",
    "<script ",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Site {
    pub kind: &'static str,
    pub path: String,
    pub key: String,
}

fn site(kind: &'static str, path: &str, key: impl Into<String>) -> Site {
    Site {
        kind,
        path: path.to_owned(),
        key: key.into(),
    }
}

pub fn discover(files: &[(String, String)]) -> BTreeSet<Site> {
    let mut found = BTreeSet::new();
    let agent_dirs = agent_command_dirs(files);
    for (path, text) in files {
        let path = path.as_str();
        if EXECUTED_PREFIXES.iter().any(|p| path.starts_with(p)) {
            for line in text.lines() {
                if MARKERS.iter().any(|m| line.contains(m)) {
                    found.insert(site("embedded", path, line.trim()));
                }
            }
        }
        if path == "pyproject.toml" {
            pyproject_scripts(path, text, &mut found);
        }
        if path.ends_with("package.json") && !path.contains("node_modules/") {
            package_entries(path, text, &mut found);
        }
        if path == "lefthook.yml" {
            for line in text.lines().map(str::trim) {
                if line.starts_with("run:") {
                    found.insert(site("lefthook-run", path, line));
                }
            }
        }
        if path == "Agentfile.yaml" {
            for line in text.lines().map(str::trim) {
                let line = line.strip_prefix("- ").unwrap_or(line);
                if line.starts_with("command:") {
                    found.insert(site("agent-hook", path, line));
                }
            }
        }
        if agent_dirs.iter().any(|dir| path.starts_with(dir.as_str())) {
            found.insert(site("agent-command", path, path));
        }
        if (path.starts_with("hooks/") && !path.contains('.'))
            || path.starts_with("hooks/lefthook/")
        {
            found.insert(site("git-hook", path, path));
        }
    }
    found
}

fn agent_command_dirs(files: &[(String, String)]) -> Vec<String> {
    let Some((_, text)) = files.iter().find(|(path, _)| path == "Agentfile.yaml") else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if !line.starts_with(' ') && !line.starts_with('-') {
            inside = line.trim_end() == "commands:";
            continue;
        }
        if inside {
            if let Some(entry) = line.trim().strip_prefix("- ") {
                let dir = entry.trim().trim_start_matches("./").trim_end_matches('/');
                if !dir.is_empty() {
                    dirs.push(format!("{dir}/"));
                }
            }
        }
    }
    dirs
}

fn pyproject_scripts(path: &str, text: &str, found: &mut BTreeSet<Site>) {
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[project.scripts]";
        } else if inside {
            if let Some((name, _)) = line.split_once('=') {
                found.insert(site("pyproject-script", path, name.trim()));
            }
        }
    }
}

fn package_entries(path: &str, text: &str, found: &mut BTreeSet<Site>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    for (field, kind) in [("scripts", "npm-script"), ("bin", "npm-bin")] {
        match value.get(field) {
            Some(serde_json::Value::Object(map)) => {
                for name in map.keys() {
                    found.insert(site(kind, path, name.as_str()));
                }
            }
            Some(serde_json::Value::String(_)) => {
                found.insert(site(kind, path, field));
            }
            _ => {}
        }
    }
}

pub fn verify_sites(found: &BTreeSet<Site>, manifest: &str) -> Result<usize, &'static str> {
    let mut lines = manifest.lines();
    if lines.next() != Some(HEADER) {
        return Err("invalid site inventory header");
    }
    let mut assigned = BTreeSet::new();
    for line in lines {
        let parsed = crate::inventory::fields(line).ok_or("invalid site inventory row")?;
        let [kind, path, key, task] = parsed.as_slice() else {
            return Err("invalid site inventory row");
        };
        if kind.is_empty()
            || path.is_empty()
            || key.is_empty()
            || !crate::inventory::migration_task(task)
        {
            return Err("invalid site assignment");
        }
        if !assigned.insert((kind.clone(), path.clone(), key.clone())) {
            return Err("duplicate site assignment");
        }
    }
    let discovered: BTreeSet<(String, String, String)> = found
        .iter()
        .map(|s| (s.kind.to_owned(), s.path.clone(), s.key.clone()))
        .collect();
    if discovered.iter().any(|site| !assigned.contains(site)) {
        return Err("unassigned embedded site or entry point");
    }
    if assigned.iter().any(|site| !discovered.contains(site)) {
        return Err("assigned site no longer exists");
    }
    Ok(assigned.len())
}

fn staged_files() -> Result<Vec<(String, String)>, &'static str> {
    let index = Command::new("git")
        .args(["ls-files", "--full-name", "--stage", "-z", "--", ":/"])
        .output()
        .map_err(|_| "cannot run Git")?;
    if !index.status.success() {
        return Err("cannot read the Git index");
    }
    let index = String::from_utf8(index.stdout).map_err(|_| "Git index output is not UTF-8")?;
    let mut entries = Vec::new();
    for entry in index.split('\0').filter(|e| !e.is_empty()) {
        let (metadata, path) = entry.split_once('\t').ok_or("invalid Git index entry")?;
        let mut parts = metadata.split(' ');
        let mode = parts.next().ok_or("invalid Git index entry")?;
        let oid = parts.next().ok_or("invalid Git index entry")?;
        if mode.starts_with("100") {
            entries.push((path.to_owned(), oid.to_owned()));
        }
    }
    let mut child = Command::new("git")
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| "cannot run Git")?;
    let mut stdin = child.stdin.take().ok_or("cannot run Git")?;
    let request: String = entries.iter().map(|(_, oid)| format!("{oid}\n")).collect();
    let writer = std::thread::spawn(move || stdin.write_all(request.as_bytes()));
    let mut out = Vec::new();
    child
        .stdout
        .take()
        .ok_or("cannot run Git")?
        .read_to_end(&mut out)
        .map_err(|_| "cannot read Git objects")?;
    let wrote = writer.join().map_err(|_| "cannot run Git")?;
    if wrote.is_err() || !child.wait().map_err(|_| "cannot run Git")?.success() {
        return Err("cannot read Git objects");
    }
    let mut files = Vec::new();
    let mut rest = out.as_slice();
    for (path, _) in entries {
        let end = rest
            .iter()
            .position(|b| *b == b'\n')
            .ok_or("invalid Git object output")?;
        let header = std::str::from_utf8(&rest[..end]).map_err(|_| "invalid Git object output")?;
        let size: usize = header
            .rsplit(' ')
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or("invalid Git object output")?;
        let body = rest
            .get(end + 1..end + 1 + size)
            .ok_or("invalid Git object output")?;
        rest = rest.get(end + 2 + size..).unwrap_or_default();
        if let Ok(text) = std::str::from_utf8(body) {
            files.push((path, text.to_owned()));
        }
    }
    Ok(files)
}

fn manifest() -> Result<String, &'static str> {
    let output = Command::new("git")
        .args(["show", &format!(":{SITES}")])
        .output()
        .map_err(|_| "cannot run Git")?;
    if !output.status.success() {
        return Err("cannot read the site inventory from the Git index");
    }
    String::from_utf8(output.stdout).map_err(|_| "site inventory is not UTF-8")
}

pub fn check() -> Result<usize, &'static str> {
    verify_sites(&discover(&staged_files()?), &manifest()?)
}

/// Print each discovered site as `kind,path,key` CSV, for review.
pub fn list() -> Result<usize, &'static str> {
    let found = discover(&staged_files()?);
    println!("kind,path,key");
    for s in &found {
        println!("{},{},\"{}\"", s.kind, s.path, s.key.replace('"', "\"\""));
    }
    Ok(found.len())
}

#[cfg(test)]
#[path = "sites_tests.rs"]
mod tests;
