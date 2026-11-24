//! All-or-nothing project imports from CSV, JSON and recorded Sheets exports.

use crate::{error, field, read_text, text, Result};
use careerbrew_projects::{projects, ProjectError};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const COLUMNS: &[&str] = &["name", "title", "group", "parent", "links", "trackers"];
const MAX_ROWS: usize = 2000;
const FIX_SHARE: &str = "fix: use File > Download > CSV and import the file (recommended); use a shared link only when the sheet's owner and your company policy allow it";

fn project_error(message: impl Into<String>) -> ProjectError {
    error("ProjectError", message)
}

pub fn slug(value: &str) -> String {
    Regex::new(r"[^a-z0-9]+")
        .unwrap()
        .replace_all(&value.trim().to_lowercase(), "-")
        .trim_matches('-')
        .to_owned()
}

pub fn sheet_csv_url(value: &str) -> Result<String> {
    let value = value.trim();
    let found = Regex::new(r"^https://docs\.google\.com/spreadsheets/d/(e/)?([A-Za-z0-9_-]+)")
        .unwrap()
        .captures(value)
        .ok_or_else(|| project_error("import reads a .csv or .json file, or a Google Sheets link (https://docs.google.com/spreadsheets/d/<id>/...); for another site, download the sheet as CSV and import the file"))?;
    let tab = Regex::new(r"[#&?]gid=(\d+)")
        .unwrap()
        .captures(value)
        .map(|gid| format!("&gid={}", &gid[1]))
        .unwrap_or_default();
    if found.get(1).is_some() {
        Ok(format!(
            "https://docs.google.com/spreadsheets/d/e/{}/pub?output=csv{tab}",
            &found[2]
        ))
    } else {
        Ok(format!(
            "https://docs.google.com/spreadsheets/d/{}/export?format=csv{tab}",
            &found[2]
        ))
    }
}

fn fixture_root() -> PathBuf {
    std::env::var_os("CAREERBREW_HTTP_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/http"))
}

fn fetch(value: &str) -> Result<String> {
    let url = sheet_csv_url(value)?;
    let allowed =
        careerbrew_sources::allowed_hosts().map_err(|error| project_error(error.message))?;
    let raw = careerbrew_sources::http::get_bytes(&fixture_root(), &url, allowed.as_deref())
        .map_err(|error| {
            if let Some(status) = error.status {
                project_error(format!("the sheet answered HTTP {status} ({FIX_SHARE})"))
            } else {
                project_error(format!("could not reach docs.google.com ({}); download the sheet as CSV and import the file", error.class))
            }
        })?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    if text.trim_start().starts_with('<') {
        return Err(project_error(format!(
            "the link returned a sign-in page, not CSV: the sheet is private ({FIX_SHARE})"
        )));
    }
    Ok(text)
}

fn read_source(root: &Path, source: &str) -> Result<(String, String)> {
    if Regex::new(r"^[a-z]+://").unwrap().is_match(source) {
        return Ok((fetch(source)?, "csv".into()));
    }
    let path = PathBuf::from(source);
    let path = if let Some(stripped) = source.strip_prefix("~/") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or(path)
            .join(stripped)
    } else {
        path
    };
    if !path.is_file() {
        return Err(project_error(format!(
            "no file {source} (fix: give a .csv or .json file, or a Google Sheets link)"
        )));
    }
    let resolved_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let inside = path
        .canonicalize()
        .unwrap_or_else(|_| path.clone())
        .starts_with(resolved_root);
    let text = if inside {
        read_text(&path)?.unwrap_or_default()
    } else {
        let bytes = std::fs::read(&path)?;
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
        String::from_utf8(bytes.to_vec()).map_err(|_| project_error(format!("{} is not a text file (fix: save the sheet as CSV, File > Download > CSV, and import that)", path.file_name().unwrap_or_default().to_string_lossy())))?
    };
    Ok((
        text.trim_start_matches('\u{feff}').to_owned(),
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        {
            "json".into()
        } else {
            "csv".into()
        },
    ))
}

fn cell_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(value)) => value.trim().to_owned(),
        Some(Value::Bool(value)) => if *value { "True" } else { "False" }.into(),
        Some(value) => value.to_string().trim().to_owned(),
    }
}

fn many(value: Option<&Value>) -> Vec<String> {
    let values = match value {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| cell_text(Some(item)))
            .collect::<Vec<_>>(),
        _ => Regex::new(r"[,;|\s]+")
            .unwrap()
            .split(&cell_text(value))
            .map(str::to_owned)
            .collect(),
    };
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty() && seen.insert(item.clone()))
        .collect()
}

#[derive(Clone, Debug)]
struct Row {
    where_: String,
    name: String,
    title: String,
    group: String,
    parent: String,
    links: Vec<String>,
    trackers: Vec<String>,
}

fn rows(text: &str, format: &str) -> Result<(Vec<Row>, Vec<String>, Vec<String>)> {
    let (raw, ignored) = if format == "json" {
        let data: Value = serde_json::from_str(text).map_err(|error| {
            project_error(format!(
                "not JSON ({}); use a list of objects, or {{\"projects\": [...]}}",
                error
            ))
        })?;
        let data = if data.is_object() {
            data.get("projects").cloned().unwrap_or(Value::Null)
        } else {
            data
        };
        let Value::Array(items) = data else {
            return Err(project_error(
                "a JSON import is a list of objects, or {\"projects\": [...]}",
            ));
        };
        if !items.iter().all(Value::is_object) {
            return Err(project_error(
                "a JSON import is a list of objects, or {\"projects\": [...]}",
            ));
        }
        let mut ignored = BTreeSet::new();
        let rows = items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut row = Map::new();
                for (key, value) in item.as_object().expect("JSON object") {
                    let key = key.trim().to_lowercase();
                    if COLUMNS.contains(&key.as_str()) {
                        row.insert(key, value.clone());
                    } else {
                        ignored.insert(key);
                    }
                }
                (format!("item {}", index + 1), Value::Object(row))
            })
            .collect::<Vec<_>>();
        (rows, ignored.into_iter().collect::<Vec<_>>())
    } else {
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(text.as_bytes());
        let headers = reader
            .headers()
            .map_err(|error| project_error(error.to_string()))?
            .iter()
            .map(|header| header.trim().to_lowercase())
            .collect::<Vec<_>>();
        if !headers.iter().any(|header| header == "name") {
            return Err(project_error(format!(
                "the sheet has no name column (fix: make the first row a header: {})",
                COLUMNS.join(", ")
            )));
        }
        let ignored = headers
            .iter()
            .filter(|header| !header.is_empty() && !COLUMNS.contains(&header.as_str()))
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut rows = Vec::new();
        for (index, record) in reader.records().enumerate() {
            let record = record.map_err(|error| project_error(error.to_string()))?;
            if !record.iter().any(|cell| !cell.trim().is_empty()) {
                continue;
            }
            let mut row = Map::new();
            for (key, value) in headers.iter().zip(record.iter()) {
                if COLUMNS.contains(&key.as_str()) {
                    row.insert(key.clone(), json!(value));
                }
            }
            rows.push((format!("row {}", index + 2), Value::Object(row)));
        }
        (rows, ignored)
    };
    if raw.len() > MAX_ROWS {
        return Err(project_error(format!(
            "the sheet has {} rows; import at most {MAX_ROWS} at a time",
            raw.len()
        )));
    }
    let mut out = Vec::new();
    let mut problems = Vec::new();
    let mut seen = BTreeMap::<String, String>::new();
    for (where_, value) in raw {
        let map = value.as_object().expect("row object");
        let name = slug(&cell_text(map.get("name")));
        if name.is_empty() {
            problems.push(format!(
                "{where_}: no name (fix: fill the name column, or delete the row)"
            ));
            continue;
        }
        if name.len() > 40 {
            problems.push(format!(
                "{where_}: the name is longer than 40 characters (fix: shorten it)"
            ));
            continue;
        }
        if let Some(other) = seen.get(&name) {
            problems.push(format!(
                "{where_}: {name} is also {other} (fix: keep one row per project)"
            ));
            continue;
        }
        seen.insert(name.clone(), where_.clone());
        let title = cell_text(map.get("title"));
        let group = cell_text(map.get("group"));
        if title.contains('\n')
            || title.chars().count() > 200
            || group.contains('\n')
            || group.chars().count() > 60
        {
            problems.push(format!(
                "{where_} ({name}): a label is one line and within its length limit"
            ));
            continue;
        }
        let trackers = many(map.get("trackers"));
        if trackers.iter().any(|tracker| tracker.chars().count() > 200) {
            problems.push(format!(
                "{where_} ({name}): a tracker is at most 200 characters"
            ));
            continue;
        }
        let mut link_seen = BTreeSet::new();
        let links = many(map.get("links"))
            .iter()
            .map(|item| slug(item))
            .filter(|item| !item.is_empty() && link_seen.insert(item.clone()))
            .collect();
        out.push(Row {
            where_,
            name,
            title,
            group,
            parent: slug(&cell_text(map.get("parent"))),
            links,
            trackers,
        });
    }
    Ok((out, ignored, problems))
}

fn graph_problems(every: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let empty = Map::new();
    let rows = every.as_object().unwrap_or(&empty);
    for (name, project) in rows {
        let parent = text(field(project, "parent"));
        if parent == *name {
            out.push((name.clone(), "a project cannot be its own parent".into()));
        } else if !parent.is_empty() && !rows.contains_key(&parent) {
            out.push((name.clone(), format!("parent {parent} is not a project (fix: add {parent} first, or leave parent empty)")));
        }
        for link in crate::strings(field(project, "links")) {
            if link == *name {
                out.push((name.clone(), "a project cannot link to itself".into()));
            } else if !rows.contains_key(&link) {
                out.push((
                    name.clone(),
                    format!(
                        "link {link} is not a project (fix: add {link} first, or drop the link)"
                    ),
                ));
            }
        }
    }
    let mut loops = BTreeSet::new();
    for name in rows.keys() {
        let mut chain = vec![name.clone()];
        let mut current = text(field(&every[name], "parent"));
        while rows.contains_key(&current) && !chain.contains(&current) {
            chain.push(current.clone());
            current = text(field(&every[&current], "parent"));
        }
        if chain.contains(&current) && chain.last() != Some(&current) {
            let start = chain.iter().position(|item| item == &current).unwrap_or(0);
            let loop_ = chain[start..].to_vec();
            let key = loop_
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join("\0");
            if loops.insert(key) {
                out.push((
                    loop_.iter().min().cloned().unwrap_or_default(),
                    format!(
                        "parents loop: {} -> {current} (fix: clear one of these parents)",
                        loop_.join(" -> ")
                    ),
                ));
            }
        }
    }
    out
}

pub fn run(root: &Path, source: &str, dry_run: bool) -> Result<Value> {
    let (source_text, format) = read_source(root, source)?;
    let (parsed, ignored, mut problems) = rows(&source_text, &format)?;
    projects::with_lock(root, || {
        let mut merged = projects::load(root)?;
        let mut added = Vec::new();
        let mut updated = Vec::new();
        let mut unchanged = Vec::new();
        let mut sources = 0usize;
        for row in &parsed {
            let new = merged.get(&row.name).is_none();
            if new {
                merged[&row.name] = projects::new_record();
            }
            let project = merged.get_mut(&row.name).expect("inserted project");
            let before = project.clone();
            for (key, value) in [
                ("title", &row.title),
                ("group", &row.group),
                ("parent", &row.parent),
            ] {
                if !value.is_empty() {
                    project[key] = json!(value);
                }
            }
            if !row.links.is_empty() {
                project["links"] = json!(row.links);
            }
            for reference in &row.trackers {
                let list = project["sources"].as_array_mut().expect("source list");
                if !list.iter().any(|source| {
                    text(field(source, "kind")) == "tracker"
                        && text(field(source, "ref")) == *reference
                }) {
                    list.push(json!({"kind": "tracker", "ref": reference, "checked_at": null, "status": ""}));
                    sources += 1;
                }
            }
            if new {
                added.push(row.name.clone());
            } else if *project != before {
                updated.push(row.name.clone());
            } else {
                unchanged.push(row.name.clone());
            }
        }
        let where_ = parsed
            .iter()
            .map(|row| (row.name.clone(), row.where_.clone()))
            .collect::<BTreeMap<_, _>>();
        problems.extend(graph_problems(&merged).into_iter().map(|(name, message)| {
            if let Some(where_) = where_.get(&name) {
                format!("{where_} ({name}): {message}")
            } else {
                format!("{name}: {message}")
            }
        }));
        problems.sort_by_key(|problem| {
            Regex::new(r"^(?:row|item) (\d+)")
                .unwrap()
                .captures(problem)
                .and_then(|found| found[1].parse::<usize>().ok())
                .unwrap_or(0)
        });
        if !problems.is_empty() {
            return Err(project_error(format!(
                "{} problem(s), nothing imported:\n  {}",
                problems.len(),
                problems.join("\n  ")
            )));
        }
        if parsed.is_empty() {
            return Err(project_error(format!(
                "no project rows (fix: one row per project under a header: {})",
                COLUMNS.join(", ")
            )));
        }
        if !dry_run {
            projects::save(root, &merged)?;
        }
        Ok(
            json!({"rows": parsed.len(), "added": added, "updated": updated, "unchanged": unchanged, "sources": sources, "ignored": ignored, "dry_run": dry_run}),
        )
    })
}
