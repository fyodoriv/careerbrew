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

/// Interpreters whose `-c`, `-e` or stdin heredoc form runs inline code.
const INTERPRETERS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "python", "python3", "node", "ruby", "perl", "deno", "bun",
];

/// Quoted inline-code flags in an argument array (`["sh", "-c", code]`).
const QUOTED_FLAGS: &[&str] = &["\"-c\"", "'-c'", "\"-e\"", "'-e'", "\"--eval\"", "'--eval'"];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Site {
    pub kind: &'static str,
    pub path: String,
    pub key: String,
}

fn interpreter(token: &str) -> bool {
    let token = token.trim_matches(|c: char| "\"'`[(,{$".contains(c));
    let name = token.rsplit('/').next().unwrap_or(token);
    INTERPRETERS.contains(&name)
}

/// True when one line starts inline code: a fixed marker, a quoted flag in
/// an argument array, or an interpreter word followed by `-c`, `-e` or a
/// stdin heredoc.
fn embedded(line: &str) -> bool {
    if MARKERS.iter().any(|m| line.contains(m)) {
        return true;
    }
    // `git -c key=value`, `grep -c` and `cat-file -e` take no code.
    let tool_flags = ["\"git\"", "\"grep\"", "_git("]
        .iter()
        .any(|t| line.contains(t));
    if !tool_flags && QUOTED_FLAGS.iter().any(|f| line.contains(f)) {
        return true;
    }
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let heredoc = line.contains("<<");
    tokens.iter().enumerate().any(|(i, token)| {
        interpreter(token)
            && (heredoc
                || tokens[i + 1..]
                    .iter()
                    .take(4)
                    .any(|next| matches!(*next, "-c" | "-e" | "--eval")))
    })
}

/// Collects sites; an identical key seen again in one file gets ` #n`.
struct Found {
    sites: BTreeSet<Site>,
}

impl Found {
    fn add(&mut self, kind: &'static str, path: &str, key: &str) {
        let mut n = 1;
        loop {
            let key = if n == 1 {
                key.to_owned()
            } else {
                format!("{key} #{n}")
            };
            let site = Site {
                kind,
                path: path.to_owned(),
                key,
            };
            if !self.sites.contains(&site) {
                self.sites.insert(site);
                return;
            }
            n += 1;
        }
    }
}

fn executed(path: &str) -> bool {
    EXECUTED_PREFIXES.iter().any(|p| path.starts_with(p)) || path == "lefthook.yml"
}

fn entry_file(path: &str) -> bool {
    path == "pyproject.toml"
        || path == "Agentfile.yaml"
        || (path.ends_with("package.json") && !path.contains("node_modules/"))
}

pub fn discover(files: &[(String, Vec<u8>)]) -> Result<BTreeSet<Site>, &'static str> {
    let mut texts = Vec::new();
    for (path, bytes) in files {
        match std::str::from_utf8(bytes) {
            Ok(text) => texts.push((path.as_str(), text)),
            // A NUL byte marks a binary file (an image), as Git decides.
            Err(_) if (executed(path) || entry_file(path)) && !bytes.contains(&0) => {
                return Err("non-UTF-8 file in an executed path");
            }
            Err(_) => {}
        }
    }
    let mut found = Found {
        sites: BTreeSet::new(),
    };
    let (command_dirs, skill_dirs) = agent_dirs(&texts)?;
    for &(path, text) in &texts {
        if executed(path) {
            for line in text.lines() {
                if embedded(line) {
                    found.add("embedded", path, line.trim());
                }
            }
        }
        if path == "pyproject.toml" {
            pyproject_scripts(path, text, &mut found);
        }
        if path.ends_with("package.json") && !path.contains("node_modules/") {
            package_entries(path, text, &mut found)?;
        }
        let workflow = path.starts_with(".github/workflows/")
            && (path.ends_with(".yml") || path.ends_with(".yaml"));
        if path == "lefthook.yml" || workflow {
            let kind = if workflow {
                "workflow-run"
            } else {
                "lefthook-run"
            };
            for line in text.lines().map(str::trim) {
                let line = line.strip_prefix("- ").unwrap_or(line);
                if line.starts_with("run:") {
                    found.add(kind, path, line);
                }
            }
        }
        if path == "Agentfile.yaml" {
            for line in text.lines().map(str::trim) {
                let line = line.strip_prefix("- ").unwrap_or(line);
                if line.starts_with("command:") {
                    found.add("agent-hook", path, line);
                }
            }
        }
        if command_dirs
            .iter()
            .any(|dir| path.starts_with(dir.as_str()))
        {
            found.add("agent-command", path, path);
        }
        if path.ends_with("/SKILL.md")
            && skill_dirs.iter().any(|dir| path.starts_with(dir.as_str()))
        {
            found.add("agent-skill", path, path);
        }
        if (path.starts_with("hooks/") && !path.contains('.'))
            || path.starts_with("hooks/lefthook/")
        {
            found.add("git-hook", path, path);
        }
    }
    Ok(found.sites)
}

/// Reads the Agentfile `commands:` and `skillDirectories:` lists, block or
/// flow style. A present key with no readable entry fails closed.
fn agent_dirs(texts: &[(&str, &str)]) -> Result<(Vec<String>, Vec<String>), &'static str> {
    let Some((_, text)) = texts.iter().find(|(path, _)| *path == "Agentfile.yaml") else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut lists = [Vec::new(), Vec::new()];
    let keys = ["commands:", "skillDirectories:"];
    let mut inside: Option<usize> = None;
    let mut seen = [false, false];
    for line in text.lines() {
        if !line.starts_with(' ') && !line.starts_with('-') && !line.trim().is_empty() {
            inside = None;
            let line = line.split(" #").next().unwrap_or(line).trim_end();
            for (i, key) in keys.iter().enumerate() {
                if let Some(rest) = line.strip_prefix(key) {
                    seen[i] = true;
                    let rest = rest.trim();
                    if rest.is_empty() {
                        inside = Some(i);
                    } else if let Some(flow) =
                        rest.strip_prefix('[').and_then(|r| r.strip_suffix(']'))
                    {
                        lists[i].extend(flow.split(',').map(str::to_owned));
                    }
                }
            }
            continue;
        }
        if let Some(i) = inside {
            if let Some(entry) = line.trim().strip_prefix("- ") {
                lists[i].push(entry.split(" #").next().unwrap_or(entry).to_owned());
            }
        }
    }
    let mut dirs = [Vec::new(), Vec::new()];
    for i in 0..2 {
        for entry in &lists[i] {
            let dir = entry
                .trim()
                .trim_matches(|c| c == '\'' || c == '"')
                .trim_start_matches("./")
                .trim_end_matches('/');
            if !dir.is_empty() {
                dirs[i].push(format!("{dir}/"));
            }
        }
        if seen[i] && dirs[i].is_empty() {
            return Err("unreadable Agentfile entry list");
        }
    }
    let [commands, skills] = dirs;
    Ok((commands, skills))
}

fn pyproject_scripts(path: &str, text: &str, found: &mut Found) {
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[project.scripts]"
                || line == "[project.gui-scripts]"
                || line.starts_with("[project.entry-points.");
        } else if inside {
            if let Some((name, _)) = line.split_once('=') {
                found.add("pyproject-script", path, name.trim().trim_matches('"'));
            }
        }
    }
}

fn package_entries(path: &str, text: &str, found: &mut Found) -> Result<(), &'static str> {
    let value =
        serde_json::from_str::<serde_json::Value>(text).map_err(|_| "unreadable package.json")?;
    for (field, kind) in [("scripts", "npm-script"), ("bin", "npm-bin")] {
        match value.get(field) {
            Some(serde_json::Value::Object(map)) => {
                for name in map.keys() {
                    found.add(kind, path, name.as_str());
                }
            }
            Some(serde_json::Value::String(_)) => found.add(kind, path, field),
            Some(_) => return Err("unreadable package.json"),
            None => {}
        }
    }
    Ok(())
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

fn staged_files() -> Result<Vec<(String, Vec<u8>)>, &'static str> {
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
        files.push((path, body.to_vec()));
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
    verify_sites(&discover(&staged_files()?)?, &manifest()?)
}

/// Discovered sites as manifest rows; the task comes from the current
/// manifest and is empty for a new site, to fill in by review.
fn list_rows(found: &BTreeSet<Site>, manifest: &str) -> String {
    let mut tasks = std::collections::BTreeMap::new();
    for line in manifest.lines().skip(1) {
        if let Some(parsed) = crate::inventory::fields(line) {
            if let [kind, path, key, task] = parsed.as_slice() {
                tasks.insert((kind.clone(), path.clone(), key.clone()), task.clone());
            }
        }
    }
    let mut out = format!("{HEADER}\n");
    for s in found {
        let task = tasks
            .get(&(s.kind.to_owned(), s.path.clone(), s.key.clone()))
            .map_or("", String::as_str);
        out.push_str(&format!(
            "{},{},\"{}\",{task}\n",
            s.kind,
            s.path,
            s.key.replace('"', "\"\"")
        ));
    }
    out
}

/// Print each discovered site as a manifest row, for review.
pub fn list() -> Result<usize, &'static str> {
    let found = discover(&staged_files()?)?;
    let manifest = manifest().unwrap_or_default();
    print!("{}", list_rows(&found, &manifest));
    Ok(found.len())
}

#[cfg(test)]
#[path = "sites_tests.rs"]
mod tests;
