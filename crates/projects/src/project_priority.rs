//! Ranked project identity, configured ticket keys and score bonuses.

use crate::{field, read_yaml, text, write_yaml, ProjectError, Result};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

pub const BONUS: f64 = 3.0;
const STANDARDS: &[&str] = &[
    "AES", "ANSI", "CVE", "CWE", "ECMA", "IEC", "IEEE", "ISO", "PEP", "RFC", "RSA", "SHA", "TLS",
    "UCS", "UTF",
];

pub fn entries(root: &Path) -> Result<Vec<Value>> {
    let settings = read_yaml(root, "settings.yaml", json!({}))?;
    let raw = field(&settings, "project_priority")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let values = match raw {
        Value::Array(rows) => crate::strings(Some(&Value::Array(rows))),
        Value::String(value) => value
            .split(',')
            .map(|item| item.trim().to_owned())
            .filter(|item| !item.is_empty())
            .collect(),
        _ => Vec::new(),
    };
    Ok(values
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let (name, patterns) = raw.split_once('=').unwrap_or((raw.as_str(), ""));
            let matches = if patterns.is_empty() {
                vec![name.to_owned()]
            } else {
                patterns
                    .split('|')
                    .filter(|item| !item.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            };
            json!({"name": name, "rank": index + 1, "match": matches})
        })
        .collect())
}

fn setting_text(entries: &[Value]) -> String {
    entries
        .iter()
        .map(|entry| {
            let name = text(field(entry, "name"));
            let matches = crate::strings(field(entry, "match"));
            if matches == [name.clone()] {
                name
            } else {
                format!("{name}={}", matches.join("|"))
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn write_entries(root: &Path, entries: &[Value]) -> Result<()> {
    let path = root.join("settings.yaml");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    careerbrew_vault::lock::with_lock(&path, || -> Result<()> {
        let mut settings = read_yaml(root, "settings.yaml", json!({}))?;
        if entries.is_empty() {
            settings
                .as_object_mut()
                .expect("settings object")
                .remove("project_priority");
        } else {
            settings["project_priority"] = json!(setting_text(entries)
                .split(", ")
                .map(str::to_owned)
                .collect::<Vec<_>>());
        }
        write_yaml(root, "settings.yaml", &settings)
    })?
}

pub fn set_project(
    root: &Path,
    name: &str,
    rank: Option<usize>,
    matches: Option<Vec<String>>,
) -> Result<Vec<Value>> {
    if rank == Some(0) {
        return Err(ProjectError::value("rank starts at 1"));
    }
    let entries = entries(root)?;
    let current = entries
        .iter()
        .find(|entry| text(field(entry, "name")) == name)
        .cloned();
    let mut others = entries
        .iter()
        .filter(|entry| text(field(entry, "name")) != name)
        .cloned()
        .collect::<Vec<_>>();
    let position = rank
        .map(|rank| rank - 1)
        .or_else(|| {
            current
                .as_ref()
                .and_then(|entry| field(entry, "rank"))
                .and_then(Value::as_u64)
                .map(|rank| rank as usize - 1)
        })
        .unwrap_or(others.len())
        .min(others.len());
    let patterns = matches
        .or_else(|| {
            current
                .as_ref()
                .map(|entry| crate::strings(field(entry, "match")))
        })
        .unwrap_or_else(|| vec![name.to_owned()]);
    others.insert(position, json!({"name": name, "match": patterns}));
    write_entries(root, &others)?;
    self::entries(root)
}

pub fn remove(root: &Path, name: &str) -> Result<Vec<Value>> {
    let entries = entries(root)?;
    if !entries
        .iter()
        .any(|entry| text(field(entry, "name")) == name)
    {
        return Err(ProjectError::value("unknown project in project_priority"));
    }
    let rest = entries
        .into_iter()
        .filter(|entry| text(field(entry, "name")) != name)
        .collect::<Vec<_>>();
    write_entries(root, &rest)?;
    self::entries(root)
}

pub fn repo_of(id: &str, url: &str) -> String {
    for (value, pattern) in [
        (id, r"(?:^|:)pr:([\w.-]+/[\w.-]+)#\d+$"),
        (id, r"(?:^|:)git:([\w.-]+):[0-9a-f]+$"),
        (url, r"https?://[^/\s]*github[^/\s]*/([\w.-]+/[\w.-]+)"),
    ] {
        if let Some(found) = Regex::new(pattern)
            .ok()
            .and_then(|regex| regex.captures(value))
        {
            return found[1].to_owned();
        }
    }
    String::new()
}

pub fn ticket_projects(entries: &[Value]) -> BTreeSet<String> {
    let named = Regex::new(r"^([A-Z][A-Z0-9]+)-[\d*?!\[\]-]*$").expect("ticket project regex");
    let bare = Regex::new(r"^[A-Z][A-Z0-9]+$").expect("bare project regex");
    let patterns = entries
        .iter()
        .flat_map(|entry| crate::strings(field(entry, "match")))
        .collect::<Vec<_>>();
    let named_set = patterns
        .iter()
        .filter_map(|pattern| named.captures(pattern).map(|found| found[1].to_owned()))
        .collect::<BTreeSet<_>>();
    if named_set.is_empty() {
        return named_set;
    }
    named_set
        .into_iter()
        .chain(
            patterns
                .into_iter()
                .filter(|pattern| bare.is_match(pattern)),
        )
        .collect()
}

fn tracker_prefixes(link: &str) -> BTreeSet<String> {
    Regex::new(r"/browse/([A-Z][A-Z0-9]+)-\d+\b")
        .expect("tracker regex")
        .captures_iter(link)
        .map(|found| found[1].to_owned())
        .collect()
}

fn ticket_matches(
    value: &str,
    named: &BTreeSet<String>,
    linked: &BTreeSet<String>,
) -> Vec<(String, String)> {
    let ticket = Regex::new(r"\b([A-Z][A-Z0-9]+)-(\d+)\b").expect("ticket regex");
    ticket
        .captures_iter(value)
        .filter_map(|found| {
            let prefix = found[1].to_owned();
            let accepted = if named.is_empty() {
                (!STANDARDS.contains(&prefix.as_str())
                    && prefix
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()))
                    || linked.contains(&prefix)
            } else {
                named.contains(&prefix)
            };
            accepted.then(|| (found[0].to_owned(), prefix))
        })
        .collect()
}

pub fn keys(title: &str, repo: &str, link: &str, project: &str, entries: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    if !repo.is_empty() {
        out.push(repo.to_owned());
        out.push(repo.rsplit('/').next().unwrap_or(repo).to_owned());
    }
    let named = ticket_projects(entries);
    let linked = tracker_prefixes(link);
    for value in [title, link] {
        for (key, prefix) in ticket_matches(value, &named, &linked) {
            out.push(key);
            out.push(prefix);
        }
    }
    out.extend(
        [project.to_owned(), title.to_owned()]
            .into_iter()
            .filter(|value| !value.is_empty()),
    );
    out.into_iter().map(|value| value.to_lowercase()).collect()
}

pub fn matching(keys: &[String], entries: &[Value]) -> Option<Value> {
    for entry in entries {
        for pattern in crate::strings(field(entry, "match")) {
            if let Ok(pattern) = glob::Pattern::new(&pattern.to_lowercase()) {
                if keys.iter().any(|key| pattern.matches(key)) {
                    return Some(entry.clone());
                }
            }
        }
    }
    None
}

pub fn of_task(
    task: &Value,
    linked: &serde_json::Map<String, Value>,
    entries: &[Value],
) -> Option<Value> {
    let urls = crate::priorities::links(task)
        .into_iter()
        .map(|link| text(field(&link, "url")))
        .collect::<Vec<_>>();
    let repo = urls
        .iter()
        .find_map(|url| {
            let found = repo_of("", url);
            (!found.is_empty()).then_some(found)
        })
        .unwrap_or_default();
    let project = linked
        .get(&text(field(task, "id")))
        .map(|value| text(Some(value)))
        .unwrap_or_default();
    matching(
        &keys(
            &text(field(task, "title")),
            &repo,
            &urls.join(" "),
            &project,
            entries,
        ),
        entries,
    )
}

pub fn bonus(rank: usize, count: usize) -> f64 {
    if count > 0 && (1..=count).contains(&rank) {
        crate::python_round(BONUS * (count - rank + 1) as f64 / count as f64, 2)
    } else {
        0.0
    }
}

pub fn open_task_counts(
    tasks: &[Value],
    linked: &serde_json::Map<String, Value>,
    entries: &[Value],
) -> serde_json::Map<String, Value> {
    let mut counts = entries
        .iter()
        .map(|entry| (text(field(entry, "name")), json!(0)))
        .collect::<serde_json::Map<_, _>>();
    for task in tasks.iter().filter(|task| crate::priorities::is_open(task)) {
        if let Some(hit) = of_task(task, linked, entries) {
            let name = text(field(&hit, "name"));
            if let Some(count) = counts.get_mut(&name) {
                *count = json!(count.as_u64().unwrap_or(0) + 1);
            }
        }
    }
    counts
}

pub fn day_result_counts(
    rows: &[Value],
    evidence_urls: &serde_json::Map<String, Value>,
    entries: &[Value],
) -> serde_json::Map<String, Value> {
    let mut counts = entries
        .iter()
        .map(|entry| (text(field(entry, "name")), json!(0)))
        .collect::<serde_json::Map<_, _>>();
    for row in rows {
        let id = text(field(row, "id"));
        let url = evidence_urls
            .get(&id)
            .map(|value| text(Some(value)))
            .unwrap_or_default();
        let keys = keys(
            &text(field(row, "title")),
            &repo_of(&id, &url),
            &url,
            "",
            entries,
        );
        if let Some(hit) = matching(&keys, entries) {
            let name = text(field(&hit, "name"));
            if let Some(count) = counts.get_mut(&name) {
                *count = json!(count.as_u64().unwrap_or(0) + 1);
            }
        }
    }
    counts
}

pub fn summary(root: &Path) -> Result<Vec<Value>> {
    // Python's day-results reader creates its directory during this summary.
    std::fs::create_dir_all(root.join("day"))?;
    let entries = entries(root)?;
    let linked = crate::projects::task_projects(root)?;
    let tasks = crate::priorities::load(root)?;
    let counts = open_task_counts(&tasks, &linked, &entries);
    let evidence = crate::read_jsonl(root, "work/evidence.jsonl")?;
    let urls = evidence
        .iter()
        .map(|row| (text(field(row, "id")), json!(text(field(row, "url")))))
        .collect::<serde_json::Map<_, _>>();
    let results = crate::read_jsonl(
        root,
        &format!("day/{}.jsonl", crate::today().format("%Y-%m-%d")),
    )?;
    let today_counts = day_result_counts(&results, &urls, &entries);
    Ok(entries
        .into_iter()
        .map(|entry| {
            let name = text(field(&entry, "name"));
            let open = counts.get(&name).cloned().unwrap_or_else(|| json!(0));
            let today = today_counts.get(&name).cloned().unwrap_or_else(|| json!(0));
            let mut row = entry.as_object().expect("entry object").clone();
            row.insert("open_tasks".into(), open);
            row.insert("today".into(), today);
            Value::Object(row)
        })
        .collect())
}
