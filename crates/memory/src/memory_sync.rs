//! Digest-based two-way project memory sync with owner-append preservation.

use crate::{
    digest, error, field, read_json, read_jsonl, read_text, research, strings, text, write_json,
    write_json_if_changed, write_text, Result,
};
use careerbrew_projects::{project_priority, projects};
use chrono::{DateTime, Duration, FixedOffset, SecondsFormat, TimeZone};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const NOTE_DAYS: i64 = 7;
const KEEP_DAYS: i64 = 60;
const MAX_NEW_SOURCES: usize = 20;
const OFF_BUSINESS: &str = "business reading is off here: no notes written (run it in your terminal, or a plugin sets agent_may_read_business)";
const OFF_SYNC: &str =
    "project_memory_sync is off (turn it on: careerbrew config set project_memory_sync true)";

pub fn enabled(root: &Path) -> Result<bool> {
    let settings = careerbrew_projects::read_yaml(root, "settings.yaml", json!({}))?;
    Ok(field(&settings, "project_memory_sync")
        .and_then(Value::as_bool)
        .unwrap_or(true))
}

fn with_lock<T>(root: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let path = root.join("work/memory-sync");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    careerbrew_vault::lock::with_lock(&path, f)?
}

fn path(root: &Path, memory: &str) -> PathBuf {
    let expanded = if memory == "~" || memory.starts_with("~/") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(memory.trim_start_matches("~/")))
            .unwrap_or_else(|| PathBuf::from(memory))
    } else {
        PathBuf::from(memory)
    };
    if expanded.is_absolute() {
        expanded
    } else {
        root.join(expanded)
    }
}

fn key(root: &Path, memory: &str) -> String {
    let path = path(root, memory);
    if let Ok(found) = path.canonicalize() {
        return found.to_string_lossy().into_owned();
    }
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        if let Ok(parent) = parent.canonicalize() {
            return parent.join(name).to_string_lossy().into_owned();
        }
    }
    path.to_string_lossy().into_owned()
}

fn inside(root: &Path, path: &Path) -> bool {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .starts_with(root)
}

fn read(root: &Path, path: &Path) -> Result<Option<String>> {
    if !path.is_file() {
        return Ok(None);
    }
    if inside(root, path) {
        return read_text(path);
    }
    Ok(Some(
        String::from_utf8_lossy(&std::fs::read(path)?).into_owned(),
    ))
}

fn append_file(root: &Path, path: &Path, before: &str, section: &str) -> Result<()> {
    if inside(root, path) {
        return write_text(path, &format!("{before}{section}"));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)?;
    file.write_all(section.as_bytes())?;
    Ok(())
}

fn rewrite(root: &Path, path: &Path, old: &str, new: &str) -> Result<bool> {
    rewrite_with_storage(root, path, old, new, || Ok(()))
}

fn rewrite_with_storage(
    root: &Path,
    path: &Path,
    old: &str,
    new: &str,
    before_replace: impl FnOnce() -> Result<()>,
) -> Result<bool> {
    let Some(now) = read(root, path)? else {
        return Ok(false);
    };
    if !now.starts_with(old) {
        return Ok(false);
    }
    let text = format!("{new}{}", &now[old.len()..]);
    if inside(root, path) {
        write_text(path, &text)?;
        return Ok(true);
    }
    let mut file = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| error("OSError", "memory file has no parent"))?,
    )?;
    let mode = std::fs::metadata(path)?.permissions().mode() & 0o777;
    file.as_file()
        .set_permissions(std::fs::Permissions::from_mode(mode))?;
    file.write_all(text.as_bytes())?;
    before_replace()?;
    let latest = read(root, path)?
        .ok_or_else(|| error("OSError", "memory file disappeared before replacement"))?;
    if latest == now {
        // No intervening write.
    } else if latest.starts_with(&now) {
        file.write_all(&latest.as_bytes()[now.len()..])?;
    } else {
        return Ok(false);
    }
    file.persist(path).map_err(|error| error.error)?;
    Ok(true)
}

pub fn rewrite_external(root: &Path, path: &Path, old: &str, new: &str) -> Result<bool> {
    rewrite(root, path, old, new)
}

fn record(root: &Path, path: &Path, expected: &str, rec: &mut Value) -> Result<()> {
    if read(root, path)?.as_deref() == Some(expected) {
        rec["hash"] = json!(digest(expected));
    }
    Ok(())
}

type MemoryFiles = BTreeMap<String, (PathBuf, Vec<(String, String)>)>;

fn files(root: &Path) -> Result<MemoryFiles> {
    let mut out = MemoryFiles::new();
    for (name, project) in projects::load(root)?
        .as_object()
        .into_iter()
        .flat_map(|rows| rows.iter())
    {
        let memory = text(field(project, "memory"));
        if !memory.is_empty() && text(field(project, "archived_at")).is_empty() {
            out.entry(key(root, &memory))
                .or_insert_with(|| (path(root, &memory), Vec::new()))
                .1
                .push((name.clone(), memory));
        }
    }
    Ok(out)
}

fn line_id(line: &str) -> String {
    digest(line.trim())
}

fn key_line(note: &str) -> bool {
    Regex::new(r"(?i)\b(decid\w*|agreed|scope|deadline|due|launch|ship|blocked|risk|owner|action|todo|open question|\d{4}-\d{2}-\d{2})\b|\?\s*$").unwrap().is_match(note)
}

fn writer_line(kind: &str, note: &str) -> bool {
    let pattern = match kind {
        "work tools" => r"^\d{4}-\d{2}-\d{2} \S+: .*(?:<https?://\S+>|\([^()]*\))$",
        "tracker" => r"^(?:status: .*; updated [\d-]*(?:; due .*)?|child [A-Z][A-Z0-9]+-\d+: .*)$",
        "repo" => r"^(?:\d+ PR\(s\) updated, \d+ merged|PR #\d+ .*)$",
        "gdoc" => r"^(?:modified [\d-]*; \d+ new comment\(s\), \d+ open|comment: .*)$",
        "chat" => r"^(?:\d+ message\(s\) since the last check|https?://\S+)$",
        _ => {
            return note.chars().count() <= 200
                && note == note.trim_matches([' ', '-', '*', '#', '\t'])
                && key_line(note)
        }
    };
    Regex::new(pattern).unwrap().is_match(note)
}

fn seed_own(text: &str) -> BTreeSet<String> {
    let heading = Regex::new(r"^#{1,6}\s").unwrap();
    let section = Regex::new(
        r"^##\s+\d{4}-\d{2}-\d{2}\s+(?:(doc|tracker|repo|chat|memory|url)\s+(\S+)|(work tools):)",
    )
    .unwrap();
    let mut own = BTreeSet::new();
    let mut kind = String::new();
    for line in text.lines() {
        if heading.is_match(line) {
            kind.clear();
            if let Some(found) = section.captures(line) {
                let raw = found.get(1).map(|value| value.as_str()).unwrap_or("");
                let reference = found.get(2).map(|value| value.as_str()).unwrap_or("");
                kind = if raw == "doc" && !reference.contains('/') {
                    "gdoc".into()
                } else if raw.is_empty() {
                    "work tools".into()
                } else {
                    raw.into()
                };
                own.insert(line_id(line));
            }
        } else if !line.trim().is_empty() && !line.starts_with("- ") {
            kind.clear();
        } else if !kind.is_empty()
            && !line.trim().is_empty()
            && writer_line(&kind, line[2..].trim())
        {
            own.insert(line_id(line));
        }
    }
    own
}

fn own(rec: &Value, text: &str) -> BTreeSet<String> {
    if rec.get("own").is_some() {
        return strings(rec.get("own")).into_iter().collect();
    }
    seed_own(text)
}

fn wrote(rec: &mut Value, before: &str, section: &str) {
    let mut lines = own(rec, before);
    lines.extend(
        section
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(line_id),
    );
    rec["own"] = json!(lines.into_iter().collect::<Vec<_>>());
}

#[derive(Default)]
struct Parsed {
    refs: Vec<(String, String)>,
    links: Vec<String>,
    status: String,
}

fn push_ref(out: &mut Vec<(String, String)>, kind: &str, reference: String) {
    let item = (kind.to_owned(), reference);
    if !out.contains(&item) {
        out.push(item);
    }
}

fn parse(text: &str, tracked: &BTreeSet<String>, own: &BTreeSet<String>) -> Result<Parsed> {
    let link = Regex::new(r#"https?://[^\s<>()\[\]{}"'`]+"#).unwrap();
    let key = Regex::new(r"(?i)^\s*(?:[-*+]\s+)?[*_]*(status|trackers?|channels?|links|related)[*_]*\s*:[*_]*\s*(.*?)\s*$").unwrap();
    let ticket = Regex::new(r"\b[A-Z][A-Z0-9]+-\d+\b").unwrap();
    let tracker_key = Regex::new(r"^[A-Z][A-Z0-9]+-\d+$").unwrap();
    let slug = Regex::new(r"[^a-z0-9]+").unwrap();
    let words = Regex::new(r"[,;|\s]+").unwrap();
    let channel = Regex::new(r"^#?[a-z0-9][a-z0-9._-]{0,79}$").unwrap();
    let mut out = Parsed::default();
    let mut bare = Vec::new();
    for line in text.lines().filter(|line| !own.contains(&line_id(line))) {
        for found in link.find_iter(line) {
            let raw = found
                .as_str()
                .trim_end_matches(['.', ',', ';', ':', '!', '?', '*', '_']);
            let (kind, reference) = research::detect(raw)?;
            push_ref(&mut out.refs, &kind, reference);
        }
        let rest = link.replace_all(line, " ");
        if let Some(found) = key.captures(line) {
            let name = found[1].to_lowercase();
            let value = found[2].to_owned();
            let value_without_links = link.replace_all(&value, " ");
            let values = words
                .split(&value_without_links)
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>();
            if name == "status" {
                let status = value.split_whitespace().collect::<Vec<_>>().join(" ");
                if !status.is_empty() {
                    out.status = status.chars().take(200).collect();
                }
            } else if name.starts_with("tracker") {
                for word in values {
                    let upper = word.to_uppercase();
                    if tracker_key.is_match(&upper) {
                        push_ref(&mut out.refs, "tracker", upper);
                    }
                }
            } else if name.starts_with("channel") {
                for word in values {
                    let lower = word.to_lowercase();
                    if channel.is_match(&lower) {
                        push_ref(
                            &mut out.refs,
                            "chat",
                            format!("#{}", lower.trim_start_matches('#')),
                        );
                    }
                }
            } else {
                for word in values {
                    let slug = slug
                        .replace_all(&word.to_lowercase(), "-")
                        .trim_matches('-')
                        .to_owned();
                    if !slug.is_empty() && !out.links.contains(&slug) {
                        out.links.push(slug);
                    }
                }
            }
        }
        bare.extend(
            ticket
                .find_iter(&rest)
                .map(|found| found.as_str().to_owned()),
        );
    }
    let mut tracked = tracked.clone();
    tracked.extend(
        out.refs
            .iter()
            .filter(|(kind, _)| kind == "tracker")
            .map(|(_, reference)| reference.split('-').next().unwrap_or_default().to_owned()),
    );
    for reference in bare {
        if tracked.contains(reference.split('-').next().unwrap_or_default()) {
            push_ref(&mut out.refs, "tracker", reference);
        }
    }
    Ok(out)
}

#[derive(Default)]
struct Counts {
    sources: usize,
    proposed: usize,
    links: usize,
    status: usize,
}

fn apply(every: &mut Value, name: &str, memory: &str, found: Parsed, stamp: &str) -> Counts {
    let known_projects = every
        .as_object()
        .map(|rows| rows.keys().cloned().collect::<BTreeSet<_>>())
        .unwrap_or_default();
    let project = &mut every[name];
    let have = project["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|source| (text(field(source, "kind")), text(field(source, "ref"))))
        .collect::<BTreeSet<_>>();
    let new = found
        .refs
        .into_iter()
        .filter(|reference| !have.contains(reference))
        .collect::<Vec<_>>();
    let add = new
        .iter()
        .take(MAX_NEW_SOURCES)
        .cloned()
        .collect::<Vec<_>>();
    let extra = new
        .iter()
        .skip(MAX_NEW_SOURCES)
        .cloned()
        .collect::<Vec<_>>();
    for (kind, reference) in &add {
        project["sources"]
            .as_array_mut()
            .expect("source list")
            .push(json!({"kind": kind, "ref": reference, "checked_at": null, "status": ""}));
    }
    if !add.is_empty() {
        let mut proposed = project["proposed"].as_array().cloned().unwrap_or_default();
        proposed
            .retain(|item| !add.contains(&(text(field(item, "kind")), text(field(item, "ref")))));
        project["proposed"] = Value::Array(proposed);
    }
    let known = project["proposed"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| (text(field(item, "kind")), text(field(item, "ref"))))
        .collect::<BTreeSet<_>>();
    let proposed = extra
        .into_iter()
        .filter(|item| !known.contains(item))
        .map(|(kind, reference)| json!({"kind": kind, "ref": reference, "via": "memory"}))
        .collect::<Vec<_>>();
    if !proposed.is_empty() {
        project["proposed"]
            .as_array_mut()
            .expect("proposal list")
            .extend(proposed.iter().cloned());
    }
    let existing_links = strings(project.get("links"));
    let links = found
        .links
        .into_iter()
        .filter(|link| {
            known_projects.contains(link) && link != name && !existing_links.contains(link)
        })
        .collect::<Vec<_>>();
    if !links.is_empty() {
        project["links"] = json!(existing_links
            .into_iter()
            .chain(links.iter().cloned())
            .collect::<Vec<_>>());
    }
    let sources = project["sources"].as_array_mut().expect("source list");
    let index = sources.iter().position(|source| {
        text(field(source, "kind")) == "memory" && text(field(source, "ref")) == memory
    });
    let index = match index {
        Some(index) => index,
        None => {
            sources
                .push(json!({"kind": "memory", "ref": memory, "checked_at": null, "status": ""}));
            sources.len() - 1
        }
    };
    sources[index]["checked_at"] = json!(stamp);
    sources[index]
        .as_object_mut()
        .expect("source object")
        .remove("stale_reason");
    let changed =
        !found.status.is_empty() && text(field(&sources[index], "status")) != found.status;
    if !found.status.is_empty() {
        sources[index]["status"] = json!(found.status);
    }
    Counts {
        sources: add.len(),
        proposed: proposed.len(),
        links: links.len(),
        status: usize::from(changed),
    }
}

fn parse_into(
    root: &Path,
    key: &str,
    users: &[(String, String)],
    content: &str,
    state: &mut Value,
) -> Result<Counts> {
    let rec = state["files"]
        .get(key)
        .cloned()
        .unwrap_or_else(|| json!({}));
    let own = own(&rec, content);
    let stamp = careerbrew_projects::now().to_rfc3339_opts(SecondsFormat::Secs, false);
    let mut total = Counts::default();
    projects::with_lock(root, || {
        let mut every = projects::load(root)?;
        for (name, memory) in users {
            if every.get(name).is_none() {
                continue;
            }
            let tracked = every[name]["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|source| text(field(source, "kind")) == "tracker")
                .map(|source| {
                    text(field(source, "ref"))
                        .to_uppercase()
                        .split('-')
                        .next()
                        .unwrap_or_default()
                        .to_owned()
                })
                .collect::<BTreeSet<_>>();
            let got = apply(
                &mut every,
                name,
                memory,
                parse(content, &tracked, &own)?,
                &stamp,
            );
            total.sources += got.sources;
            total.proposed += got.proposed;
            total.links += got.links;
            total.status += got.status;
        }
        projects::save(root, &every)
    })?;
    let mut rec = rec;
    rec["hash"] = json!(digest(content));
    let present = content.lines().map(line_id).collect::<BTreeSet<_>>();
    rec["own"] = json!(own.intersection(&present).cloned().collect::<Vec<_>>());
    state["files"][key] = rec;
    Ok(total)
}

fn empty_counts() -> Value {
    json!({"files": 0, "sources": 0, "proposed": 0, "links": 0, "status": 0, "errors": 0})
}

fn increment(out: &mut Value, key: &str, count: usize) {
    out[key] = json!(out[key].as_u64().unwrap_or(0) + count as u64);
}

fn catch_up(root: &Path, key: &str, text: Option<&str>, state: &mut Value) -> Result<()> {
    let Some(text) = text else { return Ok(()) };
    if !enabled(root)?
        || state["files"]
            .get(key)
            .and_then(|rec| rec.get("hash"))
            .and_then(Value::as_str)
            == Some(&digest(text))
    {
        return Ok(());
    }
    if let Some((_, users)) = files(root)?.get(key) {
        parse_into(root, key, users, text, state)?;
    }
    Ok(())
}

pub fn sync_in(root: &Path) -> Result<Value> {
    let mut out = empty_counts();
    if !enabled(root)? {
        return Ok(out);
    }
    with_lock(root, || {
        let mut state = read_json(root, "work/memory-sync.json", json!({"files": {}}))?;
        if state.get("files").is_none() {
            state["files"] = json!({});
        }
        for (key, (path, users)) in files(root)? {
            let text = match read(root, &path) {
                Ok(text) => text,
                Err(_) => {
                    increment(&mut out, "errors", 1);
                    continue;
                }
            };
            let Some(text) = text else { continue };
            if state["files"]
                .get(&key)
                .and_then(|rec| rec.get("hash"))
                .and_then(Value::as_str)
                == Some(&digest(&text))
            {
                continue;
            }
            increment(&mut out, "files", 1);
            let got = parse_into(root, &key, &users, &text, &mut state)?;
            for (name, count) in [
                ("sources", got.sources),
                ("proposed", got.proposed),
                ("links", got.links),
                ("status", got.status),
            ] {
                increment(&mut out, name, count);
            }
        }
        write_json_if_changed(root, "work/memory-sync.json", &state)?;
        Ok(out)
    })
}

fn local_zone(root: &Path) -> chrono_tz::Tz {
    let settings = careerbrew_projects::read_yaml(root, "settings.yaml", json!({}))
        .unwrap_or_else(|_| json!({}));
    let configured = text(field(&settings, "timezone"));
    let system = std::env::var("TZ").unwrap_or_default();
    let name = if !configured.is_empty() {
        configured
    } else if !system.is_empty() {
        system.trim_start_matches(':').to_owned()
    } else {
        std::fs::canonicalize("/etc/localtime")
            .ok()
            .and_then(|path| {
                path.to_string_lossy()
                    .split("/zoneinfo/")
                    .nth(1)
                    .map(str::to_owned)
            })
            .unwrap_or_default()
    };
    // Match Python's explicit warning when no valid zone can be resolved.
    name.parse().unwrap_or_else(|_| {
        eprintln!("careerbrew: cannot read this machine's time zone, so times use UTC; fix: careerbrew config set timezone <IANA name, for example Etc/UTC>");
        chrono_tz::UTC
    })
}

fn local_naive(zone: chrono_tz::Tz, naive: chrono::NaiveDateTime) -> Option<DateTime<FixedOffset>> {
    match zone.from_local_datetime(&naive) {
        chrono::LocalResult::Single(local) => Some(local.fixed_offset()),
        chrono::LocalResult::Ambiguous(first, _) => Some(first.fixed_offset()),
        chrono::LocalResult::None => {
            // Python's fold=0 uses the offset before a DST gap. Probe a bounded
            // three hours back, then apply that offset to the supplied time.
            for minutes in 1..=180 {
                let earlier = naive - Duration::minutes(minutes);
                let existing = match zone.from_local_datetime(&earlier) {
                    chrono::LocalResult::Single(local) => Some(local),
                    chrono::LocalResult::Ambiguous(first, _) => Some(first),
                    chrono::LocalResult::None => None,
                };
                if let Some(existing) = existing {
                    let offset = *existing.fixed_offset().offset();
                    return offset.from_local_datetime(&naive).single();
                }
            }
            None
        }
    }
}

fn parse_when(root: &Path, value: &str) -> Option<DateTime<FixedOffset>> {
    let value = research::normalize_timestamp(value.trim());
    let zone = local_zone(root);
    DateTime::parse_from_rfc3339(&value)
        .ok()
        .map(|aware| aware.with_timezone(&zone).fixed_offset())
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .and_then(|naive| local_naive(zone, naive))
        })
}

pub fn local_time(root: &Path, value: &str) -> Option<String> {
    parse_when(root, value).map(|when| when.to_rfc3339())
}

fn local_now(root: &Path) -> Result<DateTime<FixedOffset>> {
    let zone = local_zone(root);
    if let Ok(pinned) = std::env::var("CAREERBREW_NOW") {
        if !pinned.is_empty() {
            // Python calendar.now returns an aware pinned value unchanged; only
            // a naive value receives the current local zone.
            if let Ok(aware) = DateTime::parse_from_rfc3339(&pinned) {
                return Ok(aware);
            }
            for format in [
                "%Y-%m-%dT%H:%M:%S%.f",
                "%Y-%m-%d %H:%M:%S%.f",
                "%Y-%m-%dT%H:%M",
                "%Y-%m-%d %H:%M",
            ] {
                if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&pinned, format) {
                    return local_naive(zone, naive)
                        .ok_or_else(|| error("ValueError", "invalid CAREERBREW_NOW"));
                }
            }
            return Err(error("ValueError", "invalid CAREERBREW_NOW"));
        }
    }
    Ok(chrono::Utc::now().with_timezone(&zone).fixed_offset())
}

fn aliases(id: &str) -> BTreeSet<String> {
    Regex::new(r"^slack:[^:]+:(\d+\.\d+)$")
        .unwrap()
        .captures(id)
        .map(|found| BTreeSet::from([format!("slack:{}", &found[1])]))
        .unwrap_or_default()
}

fn candidates(root: &Path) -> Result<Vec<Value>> {
    let cut = local_now(root)? - Duration::days(NOTE_DAYS);
    let feed = read_jsonl(root, "work/feed/events.jsonl")?;
    let mut skip = feed
        .iter()
        .filter(|row| field(row, "personal") == Some(&Value::Bool(true)))
        .flat_map(|row| aliases(&text(field(row, "id"))))
        .collect::<BTreeSet<_>>();
    skip.extend(feed.iter().map(|row| text(field(row, "id"))));
    skip.extend(feed.iter().flat_map(|row| aliases(&text(field(row, "id")))));
    let mut out = Vec::new();
    for row in &feed {
        if field(row, "personal") != Some(&Value::Bool(true))
            && text(field(row, "triage")) != "noise"
        {
            out.push(json!({"id": text(field(row, "id")), "source": if text(field(row, "source")).is_empty() { "feed".into() } else { text(field(row, "source")) }, "title": text(field(row, "title")), "at": text(field(row, "at")), "url": text(field(row, "url")), "channel": text(field(row, "channel")), "team": row.get("team").cloned().unwrap_or_else(|| json!([]))}));
        }
    }
    for row in read_jsonl(root, "work/evidence.jsonl")? {
        let id = text(field(&row, "id"));
        if !skip.contains(&id) && field(&row, "personal") != Some(&Value::Bool(true)) {
            skip.insert(id.clone());
            out.push(json!({"id": id, "source": if text(field(&row, "kind")).is_empty() { "evidence".into() } else { text(field(&row, "kind")) }, "title": text(field(&row, "title")), "at": text(field(&row, "at")), "url": text(field(&row, "url")), "repo": text(field(&row, "repo"))}));
        }
    }
    for item in &mut out {
        let when = parse_when(root, &text(field(item, "at")));
        item["when"] = when
            .map(|value| json!(value.to_rfc3339()))
            .unwrap_or(Value::Null);
    }
    out.retain(|item| parse_when(root, &text(field(item, "when"))).is_some_and(|when| when >= cut));
    out.sort_by_key(|item| {
        (
            parse_when(root, &text(field(item, "when"))),
            text(field(item, "id")),
        )
    });
    Ok(out)
}

fn tickets(value: &str) -> BTreeSet<String> {
    Regex::new(r"\b[A-Z][A-Z0-9]+-\d+\b")
        .unwrap()
        .find_iter(value)
        .map(|found| found.as_str().to_owned())
        .collect()
}

pub fn score_item(item: &Value, project: &Value, globs: &[String]) -> usize {
    let id = text(field(item, "id"));
    let url = text(field(item, "url"));
    let raw_title = text(field(item, "title"));
    let title = raw_title.to_lowercase();
    let repo = {
        let found = project_priority::repo_of(&id, &url);
        if found.is_empty() {
            text(field(item, "repo")).to_lowercase()
        } else {
            found.to_lowercase()
        }
    };
    let channel = text(field(item, "channel")).to_lowercase();
    let tickets = tickets(&format!("{raw_title} {url}"));
    let prefixes = tickets
        .iter()
        .map(|ticket| ticket.split('-').next().unwrap_or_default().to_owned())
        .collect::<BTreeSet<_>>();
    let keys = project_priority::keys(&title, &repo, &url, "", &[]);
    let mut best = 0usize;
    for source in project["sources"].as_array().into_iter().flatten() {
        let kind = text(field(source, "kind"));
        let reference = text(field(source, "ref"));
        if reference.is_empty() {
            continue;
        }
        if kind == "tracker" {
            let upper = reference.to_uppercase();
            best = best.max(if tickets.contains(&upper) {
                3
            } else if prefixes.contains(upper.split('-').next().unwrap_or_default()) {
                1
            } else {
                0
            });
        } else if kind == "repo" && !repo.is_empty() {
            let lower = reference.to_lowercase();
            if repo == lower
                || (!repo.contains('/') && repo == lower.rsplit('/').next().unwrap_or_default())
            {
                best = best.max(2);
            }
        } else if kind == "chat" {
            if let Some(stripped) = reference.strip_prefix('#') {
                let name = stripped.to_lowercase();
                let pattern = Regex::new(&format!(
                    r"(?:^|[^\w-])#{}(?:$|[^\w-])",
                    regex::escape(&name)
                ))
                .unwrap();
                if channel == name || pattern.is_match(&title) {
                    best = best.max(2);
                }
            } else if Regex::new(&format!(
                r"/archives/{}(?:[/?]|$)",
                regex::escape(&reference)
            ))
            .unwrap()
            .is_match(&url)
                || id.starts_with(&format!("slack:{reference}:"))
            {
                best = best.max(2);
            }
        } else if kind == "doc" && !reference.contains('/') && url.contains(&reference) {
            best = best.max(2);
        } else if kind == "url"
            && !url.is_empty()
            && url
                .trim_end_matches('/')
                .starts_with(reference.trim_end_matches('/'))
        {
            if research::github_item(&reference).is_some() {
                let suffix = &url[reference.trim_end_matches('/').len()..];
                if !suffix.is_empty() && !suffix.starts_with(['/', '?', '#']) {
                    continue;
                }
            }
            best = best.max(2);
        }
    }
    if !globs.is_empty() {
        let entry = json!({"name": "", "rank": 0, "match": globs});
        if project_priority::matching(&keys, &[entry]).is_some() {
            best = best.max(2);
        }
    }
    best
}

pub fn targets(item: &Value, every: &Value, globs: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let scores = every
        .as_object()
        .into_iter()
        .flat_map(|rows| rows.iter())
        .map(|(name, project)| {
            (
                name.clone(),
                score_item(
                    item,
                    project,
                    globs.get(name).map(Vec::as_slice).unwrap_or(&[]),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let best = scores.values().max().copied().unwrap_or(0);
    scores
        .into_iter()
        .filter(|(_, score)| *score > 0 && *score == best)
        .map(|(name, _)| name)
        .collect()
}

fn note_line(root: &Path, item: &Value) -> String {
    let when = parse_when(root, &text(field(item, "when")))
        .or_else(|| parse_when(root, &text(field(item, "at"))));
    let day = when
        .map(|value| value.date_naive().to_string())
        .unwrap_or_default();
    let mut title = text(field(item, "title"))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect::<String>();
    let team = strings(item.get("team"));
    if !team.is_empty() {
        title = format!("{}: {title}", team.join(", "));
    }
    let url = text(field(item, "url"));
    let link = if url.is_empty() {
        format!("({})", text(field(item, "id")))
    } else {
        format!("<{url}>")
    };
    format!("{day} {}: {title} {link}", text(field(item, "source")))
}

fn section(header: &str, lines: &[String]) -> String {
    format!(
        "\n## {header}\n\n{}",
        lines
            .iter()
            .map(|line| format!("- {line}\n"))
            .collect::<String>()
    )
}

fn header_for(names: &BTreeSet<String>) -> String {
    if names.contains("") {
        return String::new();
    }
    let kids = names.iter().cloned().collect::<Vec<_>>();
    format!(
        " (project{} {})",
        if kids.len() == 1 { "" } else { "s" },
        kids.join(", ")
    )
}

#[derive(Default)]
struct Work {
    path: PathBuf,
    projects: BTreeSet<String>,
    items: Vec<(String, Value)>,
    for_: BTreeMap<String, BTreeSet<String>>,
}

fn personal_ids(root: &Path) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    for row in read_jsonl(root, "work/feed/events.jsonl")? {
        if field(&row, "personal") == Some(&Value::Bool(true)) {
            let id = text(field(&row, "id"));
            ids.insert(id.clone());
            ids.extend(aliases(&id));
        }
    }
    Ok(ids)
}

fn empty_out() -> Value {
    json!({"notes": 0, "projects": 0, "removed": 0, "errors": 0, "reason": ""})
}

fn prune(root: &Path, notes: &mut Value) -> Result<()> {
    let cut = (local_now(root)? - Duration::days(KEEP_DAYS))
        .date_naive()
        .to_string();
    if let Some(map) = notes.as_object_mut() {
        map.retain(|_, value| {
            let day = value.get(1).and_then(Value::as_str).unwrap_or_default();
            day.is_empty() || day >= cut.as_str()
        });
    }
    Ok(())
}

pub fn sync_out(root: &Path, allowed: bool) -> Result<Value> {
    sync_out_with(root, allowed, |_| Ok(()))
}

pub fn sync_out_with(
    root: &Path,
    allowed: bool,
    mut observer: impl FnMut(&Path) -> Result<()>,
) -> Result<Value> {
    let mut out = empty_out();
    out["removed"] = json!(forget_with(root, &personal_ids(root)?, &mut observer)?);
    if !enabled(root)? {
        out["reason"] = json!(OFF_SYNC);
        return Ok(out);
    }
    if !allowed {
        out["reason"] = json!(OFF_BUSINESS);
        return Ok(out);
    }
    let every = projects::load(root)?;
    let globs = project_priority::entries(root)?
        .into_iter()
        .map(|entry| (text(field(&entry, "name")), strings(field(&entry, "match"))))
        .collect::<BTreeMap<_, _>>();
    let mut plan = BTreeMap::<String, Work>::new();
    for item in candidates(root)? {
        for name in targets(&item, &every, &globs) {
            if !text(field(&every[&name], "archived_at")).is_empty() {
                continue;
            }
            let memory = projects::notes_path(&every, &name);
            if memory.is_empty()
                || Path::new(&memory)
                    .file_name()
                    .is_some_and(|name| name == "MEMORY.md")
            {
                continue;
            }
            let key = key(root, &memory);
            let work = plan.entry(key).or_insert_with(|| Work {
                path: path(root, &memory),
                ..Default::default()
            });
            work.projects.insert(name.clone());
            let id = text(field(&item, "id"));
            if !work.items.iter().any(|(existing, _)| existing == &id) {
                work.items.push((id.clone(), item.clone()));
            }
            work.for_.entry(id).or_default().insert(
                if text(field(&every[&name], "memory")).is_empty() {
                    name
                } else {
                    String::new()
                },
            );
        }
    }
    if plan.is_empty() {
        return Ok(out);
    }
    let header = format!("{} work tools", local_now(root)?.date_naive());
    let mut written = BTreeSet::new();
    with_lock(root, || {
        let mut state = read_json(root, "work/memory-sync.json", json!({"files": {}}))?;
        for (key, work) in plan {
            let mut rec = state["files"]
                .get(&key)
                .cloned()
                .unwrap_or_else(|| json!({}));
            let mut notes = rec.get("notes").cloned().unwrap_or_else(|| json!({}));
            let result = (|| -> Result<()> {
                let before = read(root, &work.path)?;
                catch_up(root, &key, before.as_deref(), &mut state)?;
                if let Some(current) = state["files"].get(&key).cloned() {
                    rec = current;
                }
                notes = rec.get("notes").cloned().unwrap_or(notes);
                let present = before
                    .as_deref()
                    .unwrap_or_default()
                    .lines()
                    .map(|line| line.trim().to_owned())
                    .collect::<BTreeSet<_>>();
                let mut groups = BTreeMap::<String, Vec<String>>::new();
                let mut pending = Map::new();
                for (id, item) in &work.items {
                    let id_digest = digest(id);
                    if notes.get(&id_digest).is_some() {
                        continue;
                    }
                    let line = note_line(root, item);
                    let day = parse_when(root, &text(field(item, "when")))
                        .map(|when| when.date_naive().to_string())
                        .unwrap_or_default();
                    pending.insert(id_digest, json!([digest(&format!("- {line}")), day]));
                    if !present.contains(&format!("- {line}")) {
                        groups
                            .entry(header_for(&work.for_[id]))
                            .or_default()
                            .push(line);
                    }
                }
                if !groups.is_empty() {
                    let section_text = groups
                        .iter()
                        .map(|(tag, lines)| {
                            section(
                                &format!("{header}: {} new item(s){tag}", lines.len()),
                                lines,
                            )
                        })
                        .collect::<String>();
                    append_file(
                        root,
                        &work.path,
                        before.as_deref().unwrap_or_default(),
                        &section_text,
                    )?;
                    record(
                        root,
                        &work.path,
                        &format!("{}{}", before.as_deref().unwrap_or_default(), section_text),
                        &mut rec,
                    )?;
                    wrote(
                        &mut rec,
                        before.as_deref().unwrap_or_default(),
                        &section_text,
                    );
                    increment(&mut out, "notes", groups.values().map(Vec::len).sum());
                    written.extend(work.projects.iter().cloned());
                }
                for (id, value) in pending {
                    notes[id] = value;
                }
                prune(root, &mut notes)?;
                rec["notes"] = notes;
                state["files"][&key] = rec;
                Ok(())
            })();
            if result.is_err() {
                increment(&mut out, "errors", 1);
            }
        }
        write_json_if_changed(root, "work/memory-sync.json", &state)?;
        Ok(())
    })?;
    out["projects"] = json!(written.len());
    Ok(out)
}

fn drop_empty_sections(text: &str) -> String {
    let heading = Regex::new(r"(?m)^#{1,6}\s").unwrap();
    let notes = Regex::new(r"^##\s+\d{4}-\d{2}-\d{2}\s+work tools:").unwrap();
    let mut starts = heading
        .find_iter(text)
        .map(|found| found.start())
        .collect::<Vec<_>>();
    if starts.first() != Some(&0) {
        starts.insert(0, 0);
    }
    starts.push(text.len());
    let mut out = String::new();
    for pair in starts.windows(2) {
        let block = &text[pair[0]..pair[1]];
        let body = block
            .split_once('\n')
            .map(|(_, body)| body)
            .unwrap_or_default();
        if notes.is_match(block) && body.trim().is_empty() {
            continue;
        }
        out.push_str(block);
    }
    out
}

fn forget_with(
    root: &Path,
    ids: &BTreeSet<String>,
    observer: &mut impl FnMut(&Path) -> Result<()>,
) -> Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    let want = ids.iter().map(|id| digest(id)).collect::<BTreeSet<_>>();
    let mut removed = 0usize;
    with_lock(root, || {
        let mut state = read_json(root, "work/memory-sync.json", json!({"files": {}}))?;
        let keys = state["files"]
            .as_object()
            .map(|rows| rows.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for key in keys {
            let mut rec = state["files"][&key].clone();
            let mut notes = rec.get("notes").cloned().unwrap_or_else(|| json!({}));
            let hit = notes
                .as_object()
                .into_iter()
                .flat_map(|rows| rows.keys())
                .filter(|id| want.contains(*id))
                .cloned()
                .collect::<Vec<_>>();
            if hit.is_empty() {
                continue;
            }
            let drop = hit
                .iter()
                .filter_map(|id| notes[id].get(0).and_then(Value::as_str))
                .map(str::to_owned)
                .collect::<BTreeSet<_>>();
            let path = PathBuf::from(&key);
            let result = (|| -> Result<bool> {
                let Some(before) = read(root, &path)? else {
                    return Ok(false);
                };
                catch_up(root, &key, Some(&before), &mut state)?;
                if let Some(current) = state["files"].get(&key).cloned() {
                    rec = current;
                }
                observer(&path)?;
                let lines = before.split_inclusive('\n').collect::<Vec<_>>();
                let kept = lines
                    .iter()
                    .filter(|line| !drop.contains(&digest(line.trim())))
                    .cloned()
                    .collect::<Vec<_>>();
                if kept.len() >= lines.len() {
                    return Ok(true);
                }
                let new = drop_empty_sections(&kept.join(""));
                if !rewrite(root, &path, &before, &new)? {
                    return Ok(false);
                }
                if enabled(root)? {
                    record(root, &path, &new, &mut rec)?;
                }
                removed += lines.len() - kept.len();
                Ok(true)
            })();
            if result.unwrap_or(false) {
                for id in hit {
                    if let Some(map) = notes.as_object_mut() {
                        map.remove(&id);
                    }
                }
                rec["notes"] = notes;
                state["files"][&key] = rec;
            }
        }
        write_json_if_changed(root, "work/memory-sync.json", &state)?;
        Ok(removed)
    })
}

pub fn append(root: &Path, memory: &str, header: &str, notes: &[String]) -> Result<bool> {
    if memory.is_empty() || notes.is_empty() {
        return Ok(false);
    }
    let path = path(root, memory);
    let key = key(root, memory);
    let section = section(header, notes);
    with_lock(root, || {
        let mut state = read_json(root, "work/memory-sync.json", json!({"files": {}}))?;
        let before = read(root, &path)?;
        if enabled(root)? {
            catch_up(root, &key, before.as_deref(), &mut state)?;
        }
        append_file(root, &path, before.as_deref().unwrap_or_default(), &section)?;
        let mut rec = state["files"]
            .get(&key)
            .cloned()
            .unwrap_or_else(|| json!({}));
        if enabled(root)? {
            record(
                root,
                &path,
                &format!("{}{}", before.as_deref().unwrap_or_default(), section),
                &mut rec,
            )?;
        }
        wrote(&mut rec, before.as_deref().unwrap_or_default(), &section);
        state["files"][&key] = rec;
        write_json(root, "work/memory-sync.json", &state)?;
        Ok(true)
    })
}

pub fn run(root: &Path, allowed: bool) -> Result<Value> {
    research::normalize_sources(root)?;
    Ok(json!({"enabled": enabled(root)?, "in": sync_in(root)?, "out": sync_out(root, allowed)?}))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RestoreEnv(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for RestoreEnv {
        fn drop(&mut self) {
            for (name, value) in &self.0 {
                if let Some(value) = value {
                    std::env::set_var(name, value);
                } else {
                    std::env::remove_var(name);
                }
            }
        }
    }

    #[test]
    fn external_rewrite_preserves_mode_and_carries_an_owner_append() {
        let scratch = tempfile::tempdir().unwrap();
        let data = scratch.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let path = scratch.path().join("external/notes.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let restore = RestoreEnv(
            [
                "HOME",
                "CAREERBREW_HOME",
                "CAREERBREW_KEY_FILE",
                "CAREERBREW_ENCRYPT",
            ]
            .into_iter()
            .map(|name| (name, std::env::var_os(name)))
            .collect(),
        );
        std::env::set_var("HOME", scratch.path().join("home"));
        std::env::set_var("CAREERBREW_HOME", &data);
        std::env::set_var(
            "CAREERBREW_KEY_FILE",
            scratch.path().join("keys/backup.key"),
        );
        std::env::set_var("CAREERBREW_ENCRYPT", "off");
        std::fs::write(&path, "# notes\n- old\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let old = std::fs::read_to_string(&path).unwrap();
        let appended = path.clone();
        rewrite_with_storage(&data, &path, &old, "# notes\n", || {
            let mut file = std::fs::OpenOptions::new().append(true).open(&appended)?;
            file.write_all(b"- owner\n")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# notes\n- owner\n"
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(restore);
    }
}
