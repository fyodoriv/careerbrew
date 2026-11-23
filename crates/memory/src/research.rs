//! Exact project-source identity and owner-managed research writes.

use crate::{error, field, text, Result};
use careerbrew_projects::{projects, ProjectError};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const READING_OFF: &str = "reading business data is off here (run it in your terminal, or a plugin sets agent_may_read_business)";

fn read_error(message: impl Into<String>) -> ProjectError {
    error("ReadError", message)
}

struct Url<'a> {
    scheme: &'a str,
    authority: &'a str,
    path: &'a str,
}

fn split_url(value: &str) -> Option<Url<'_>> {
    let (scheme, rest) = value.split_once("://")?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let tail = &rest[authority_end..];
    let path = if tail.starts_with('/') {
        tail.split(['?', '#']).next().unwrap_or(tail)
    } else {
        ""
    };
    Some(Url {
        scheme,
        authority,
        path,
    })
}

fn hostname(value: &str) -> String {
    split_url(value)
        .map(|url| {
            url.authority
                .rsplit('@')
                .next()
                .unwrap_or(url.authority)
                .split(':')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase()
        })
        .unwrap_or_default()
}

pub fn github_item(reference: &str) -> Option<Vec<String>> {
    let url = split_url(reference)?;
    let host = hostname(reference);
    let parts = url
        .path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if (host == "github.com" || host.starts_with("github."))
        && parts.len() >= 4
        && matches!(parts[2], "pull" | "issues")
        && parts[3].bytes().all(|byte| byte.is_ascii_digit())
        && !parts[3].is_empty()
    {
        return Some(parts[..4].iter().map(|part| (*part).to_owned()).collect());
    }
    None
}

fn canonical_item(reference: &str, item: &[String]) -> String {
    let url = split_url(reference).expect("GitHub item is a URL");
    format!("{}://{}/{}", url.scheme, url.authority, item.join("/"))
}

pub fn detect(reference: &str) -> Result<(String, String)> {
    let raw = reference.trim();
    if Regex::new(r"^t\d{3,}$").unwrap().is_match(raw) {
        return Ok(("task".into(), raw.into()));
    }
    if raw.starts_with('#') && Regex::new(r"^#[\w.-]+$").unwrap().is_match(raw) {
        let id = Regex::new(r"^#[CGD][A-Z0-9]+$").unwrap().is_match(raw);
        return Ok(("chat".into(), if id { raw[1..].into() } else { raw.into() }));
    }
    if Regex::new(r"^[A-Z][A-Z0-9]+-\d+$").unwrap().is_match(raw) {
        return Ok(("tracker".into(), raw.into()));
    }
    if raw.contains("://") {
        let url = split_url(raw).ok_or_else(|| read_error("invalid URL"))?;
        if let Some(found) = Regex::new(r"/browse/([A-Z][A-Z0-9]+-\d+)")
            .unwrap()
            .captures(url.path)
        {
            return Ok(("tracker".into(), found[1].into()));
        }
        let host = hostname(raw);
        if host == "github.com" || host.starts_with("github.") {
            if let Some(item) = github_item(raw) {
                return Ok(("url".into(), canonical_item(raw, &item)));
            }
            let parts = url
                .path
                .split('/')
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            if parts.len() >= 2 {
                return Ok(("repo".into(), format!("{}/{}", parts[0], parts[1])));
            }
        }
        if host == "docs.google.com" {
            if let Some(found) = Regex::new(r"/d/([\w-]+)").unwrap().captures(url.path) {
                return Ok(("doc".into(), found[1].into()));
            }
        }
        if host.ends_with("slack.com") {
            if let Some(found) = Regex::new(r"/archives/(\w+)").unwrap().captures(url.path) {
                return Ok(("chat".into(), found[1].into()));
            }
        }
        return Ok(("url".into(), raw.into()));
    }
    let path = PathBuf::from(shellexpand(raw));
    if path.is_file() {
        return Ok((
            "doc".into(),
            path.canonicalize()?.to_string_lossy().into_owned(),
        ));
    }
    if Regex::new(r"^[\w.-]+/[\w.-]+$").unwrap().is_match(raw) {
        return Ok(("repo".into(), raw.into()));
    }
    Err(read_error(format!(
        "cannot tell what {raw:?} is: give a link, a file path, a task id (t001), a #channel, a ticket key, or owner/repo"
    )))
}

fn shellexpand(value: &str) -> String {
    if value == "~" || value.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}{}", PathBuf::from(home).to_string_lossy(), &value[1..]);
        }
    }
    value.to_owned()
}

fn tokens(kind: &str, reference: &str) -> BTreeSet<String> {
    match kind {
        "tracker" => BTreeSet::from([format!(
            "tracker:{}",
            reference.split('-').next().unwrap_or_default()
        )]),
        "repo" => BTreeSet::from([
            format!(
                "repo:{}",
                reference
                    .split('/')
                    .next()
                    .unwrap_or_default()
                    .to_lowercase()
            ),
            format!("repo:{}", reference.to_lowercase()),
        ]),
        "url" => {
            let mut out = BTreeSet::from([format!("host:{}", hostname(reference))]);
            if let Some(item) = github_item(reference) {
                out.extend(tokens("repo", &item[..2].join("/")));
            }
            out
        }
        "chat" => BTreeSet::from([format!("chat:{}", reference.to_lowercase())]),
        _ => BTreeSet::from([format!("{kind}:{reference}")]),
    }
}

pub fn match_project(root: &Path, kind: &str, reference: &str, content: &str) -> Result<String> {
    let mine = tokens(kind, reference);
    let mut scores = BTreeMap::<String, usize>::new();
    for (name, project) in projects::load(root)?
        .as_object()
        .into_iter()
        .flat_map(|rows| rows.iter())
    {
        if !text(field(project, "archived_at")).is_empty() {
            continue;
        }
        let mut score = project["sources"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|source| {
                mine.intersection(&tokens(
                    &text(field(source, "kind")),
                    &text(field(source, "ref")),
                ))
                .count()
            })
            .sum::<usize>();
        let name_pattern = Regex::new(&format!(r"\b{}\b", regex::escape(name))).unwrap();
        if reference.to_lowercase().contains(name)
            || (!content.is_empty() && name_pattern.is_match(content))
        {
            score += 1;
        }
        if score > 0 {
            scores.insert(name.clone(), score);
        }
    }
    if scores.is_empty() {
        return Err(ProjectError::project(format!(
            "no project matches {reference} (fix: name it: careerbrew work project absorb <project> {reference})"
        )));
    }
    let best = scores.values().max().copied().unwrap_or(0);
    let top = scores
        .iter()
        .filter(|(_, score)| **score == best)
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    if top.len() != 1 {
        return Err(ProjectError::project(format!(
            "{reference} matches {} equally (fix: name one: careerbrew work project absorb <project> {reference})",
            top.join(", ")
        )));
    }
    Ok(top[0].clone())
}

fn local_path(root: &Path, reference: &str) -> PathBuf {
    let path = PathBuf::from(shellexpand(reference));
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

pub fn after(stamp: &str, since: Option<&str>) -> bool {
    let Some(since) = since.filter(|value| !value.is_empty()) else {
        return true;
    };
    let parse = |value: &str| {
        let normalized = if value.ends_with('Z') {
            format!("{}+00:00", &value[..value.len() - 1])
        } else if value.len() >= 5
            && matches!(&value[value.len() - 5..value.len() - 4], "+" | "-")
            && !value[value.len() - 2..].contains(':')
        {
            format!(
                "{}:{}",
                &value[..value.len() - 2],
                &value[value.len() - 2..]
            )
        } else {
            value.to_owned()
        };
        DateTime::parse_from_rfc3339(&normalized)
            .map(|value| value.with_timezone(&Utc))
            .or_else(|_| {
                DateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f%:z")
                    .map(|value| value.with_timezone(&Utc))
            })
    };
    match (parse(stamp), parse(since)) {
        (Ok(left), Ok(right)) => left > right,
        _ => true,
    }
}

fn file_change(root: &Path, kind: &str, reference: &str, since: Option<&str>) -> Option<bool> {
    if !matches!(kind, "doc" | "memory") {
        return None;
    }
    let path = local_path(root, reference);
    if kind == "memory" && !path.exists() {
        return Some(false);
    }
    if !path.is_file() {
        return None;
    }
    let modified = path.metadata().ok()?.modified().ok()?;
    let stamp = DateTime::<Utc>::from(modified).to_rfc3339();
    Some(after(&stamp, since))
}

pub fn changes(
    root: &Path,
    name: &str,
    reference: Option<&str>,
    read_allowed: bool,
) -> Result<Vec<Value>> {
    let every = projects::load(root)?;
    let project = every.get(name).ok_or_else(|| {
        ProjectError::project(format!(
            "unknown project {name} (fix: `careerbrew work project add {name}`)"
        ))
    })?;
    let sources = project["sources"].as_array().cloned().unwrap_or_default();
    let selected = sources
        .into_iter()
        .filter(|source| reference.is_none_or(|wanted| text(field(source, "ref")) == wanted))
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err(ProjectError::project(if let Some(reference) = reference {
            format!("{name} has no source {reference} (see `careerbrew work project show {name}`)")
        } else {
            format!("{name} has no sources yet (fix: `careerbrew work project source {name} <kind> <ref>`)")
        }));
    }
    let mut rows = Vec::new();
    for source in selected {
        let kind = text(field(&source, "kind"));
        let reference = text(field(&source, "ref"));
        let since_value = field(&source, "checked_at").cloned().unwrap_or(Value::Null);
        let since = since_value.as_str().filter(|value| !value.is_empty());
        let reason = projects::reference_reason(&source);
        let mut row = json!({"kind": kind, "ref": reference, "since": since_value, "count": null, "reason": ""});
        if !reason.is_empty() {
            row["state"] = json!("reference");
            row["reason"] = json!(reason);
        } else if since.is_none() {
            row["state"] = json!("never read");
        } else if let Some(changed) = file_change(root, &kind, &reference, since) {
            row["state"] = json!(if changed { "changed" } else { "unchanged" });
            row["count"] = json!(usize::from(changed));
        } else if kind == "url" {
            row["state"] = json!("unknown");
            row["reason"] = json!("a url source has no change date: read it");
        } else if !read_allowed {
            row["state"] = json!("unknown");
            row["reason"] = json!(READING_OFF);
        } else {
            row["state"] = json!("unknown");
            row["reason"] = json!(format!("{kind} could not be read (FileNotFoundError)"));
        }
        rows.push(row);
    }
    Ok(rows)
}

fn add_source_exact(root: &Path, name: &str, kind: &str, reference: &str) -> Result<()> {
    projects::with_lock(root, || {
        let mut every = projects::load(root)?;
        let Some(project) = every.get_mut(name) else {
            return Err(ProjectError::project(format!("unknown project {name}")));
        };
        let sources = project["sources"].as_array_mut().expect("source list");
        if !sources.iter().any(|source| {
            text(field(source, "kind")) == kind && text(field(source, "ref")) == reference
        }) {
            sources.push(json!({"kind": kind, "ref": reference, "checked_at": null, "status": ""}));
        }
        projects::save(root, &every)
    })
}

pub fn absorb(
    root: &Path,
    reference: &str,
    name: Option<&str>,
    read_allowed: bool,
) -> Result<Value> {
    let (kind, normalized) = detect(reference)?;
    let name = match name {
        Some(name) => name.to_owned(),
        None => match_project(root, &kind, &normalized, "")?,
    };
    if kind == "task" {
        return Err(ProjectError::project(
            "task absorb is outside this component workload",
        ));
    }
    add_source_exact(root, &name, &kind, &normalized)?;
    let mut result = json!({"project": name, "kind": kind, "ref": normalized, "read": false, "notes": 0, "memory": false, "reason": ""});
    let every = projects::load(root)?;
    let project = every
        .get(result["project"].as_str().unwrap_or_default())
        .ok_or_else(|| ProjectError::project("unknown project"))?;
    let source = project["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|source| {
            text(field(source, "kind")) == result["kind"]
                && text(field(source, "ref")) == result["ref"]
        })
        .ok_or_else(|| ProjectError::project("source missing after add"))?;
    let reason = projects::reference_reason(source);
    if !reason.is_empty() {
        result["reason"] = json!(format!(
            "kept as a reference ({reason}): careerbrew never reads it and it never goes stale"
        ));
        return Ok(result);
    }
    if !read_allowed {
        projects::check(
            root,
            result["project"].as_str().unwrap_or_default(),
            Some(result["ref"].as_str().unwrap_or_default()),
            "absorbed: ref only (reading business data is off)",
        )?;
        result["reason"] = json!("reading business data is off here: stored the ref and date only (run it in your terminal, or a plugin sets agent_may_read_business)");
        return Ok(result);
    }
    Err(read_error(
        "the synthetic component reads only local files and ref-only absorbs",
    ))
}

pub fn write_memory(root: &Path, name: &str, header: &str, notes: &[String]) -> Result<bool> {
    let every = projects::load(root)?;
    let memory = projects::notes_path(&every, name);
    if memory.is_empty() || notes.is_empty() {
        return Ok(false);
    }
    let header = if text(field(&every[name], "memory")).is_empty() {
        format!("{header} (project {name})")
    } else {
        header.to_owned()
    };
    crate::memory_sync::append(root, &memory, &header, notes)?;
    Ok(true)
}

/// Repair legacy refs inside their saved kind, without widening item or
/// reference-only conversation scope. Duplicate refs retain their first order
/// and any explicit reference choice.
pub fn normalize_sources(root: &Path) -> Result<()> {
    projects::with_lock(root, || {
        let mut every = projects::load(root)?;
        let before = every.clone();
        for (_, project) in every
            .as_object_mut()
            .into_iter()
            .flat_map(|rows| rows.iter_mut())
        {
            let mut normalized = Vec::<Value>::new();
            for source in project["sources"].as_array().cloned().unwrap_or_default() {
                let mut source = source;
                let kind = text(field(&source, "kind"));
                let original = text(field(&source, "ref"));
                let reference_only = field(&source, "reference") == Some(&Value::Bool(true));
                let replacement = if reference_only && kind != "tracker" {
                    original.clone()
                } else if kind == "chat"
                    && Regex::new(r"^#[CGD][A-Z0-9]+$")
                        .unwrap()
                        .is_match(&original)
                {
                    original[1..].to_owned()
                } else if kind == "memory" {
                    original.clone()
                } else {
                    match detect(&original) {
                        Ok((detected, value)) if detected == kind => value,
                        _ => original.clone(),
                    }
                };
                if replacement != original {
                    source["ref"] = json!(replacement);
                    source["checked_at"] = Value::Null;
                    source["status"] = json!("");
                    source
                        .as_object_mut()
                        .expect("source object")
                        .remove("stale_reason");
                }
                let identity = (kind, text(field(&source, "ref")));
                if let Some(existing) = normalized.iter_mut().find(|existing| {
                    (text(field(existing, "kind")), text(field(existing, "ref"))) == identity
                }) {
                    if reference_only {
                        existing["reference"] = Value::Bool(true);
                    }
                } else {
                    normalized.push(source);
                }
            }
            project["sources"] = Value::Array(normalized);
        }
        if every != before {
            projects::save(root, &every)?;
        }
        Ok(())
    })
}
