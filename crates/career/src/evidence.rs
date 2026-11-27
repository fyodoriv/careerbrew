//! `work/evidence.py`: auto-tags, manual notes, forget, and the stored rows.
//!
//! The collectors that run `git`, `gh` and plugin commands are not ported in
//! this step (rust-followup: collect_git, collect_gh, collect_command, collect).

use crate::py;
use crate::Result;
use careerbrew_vault::canon::Canon;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

const KIND_TAGS: &[(&str, &[&str])] = &[
    ("fix", &["quality"]),
    ("feat", &["delivery"]),
    ("perf", &["quality", "impact"]),
    ("refactor", &["quality"]),
    ("docs", &["communication"]),
    ("test", &["quality"]),
    ("ci", &["tooling"]),
    ("chore", &[]),
];

const WORD_TAGS: &[(&str, &str)] = &[
    ("incident", "ownership"),
    ("mentor", "leadership"),
    ("design", "design"),
    ("rfc", "design"),
    ("migrat", "impact"),
    ("latency", "impact"),
    ("review", "collaboration"),
];

/// `evidence_path()`; `paths.sub` creates `work/`.
pub fn path() -> Result<PathBuf> {
    crate::sub(&["work", "evidence.jsonl"])
}

pub fn load() -> Result<Vec<Value>> {
    crate::read_jsonl(&path()?)
}

/// `tags_for(title)`.
pub fn tags_for(title: &str) -> Vec<String> {
    let low = py::lower(title);
    let head = low.split(':').next().unwrap_or("");
    let head = py::strip(head.split('(').next().unwrap_or(""));
    let mut tags: BTreeSet<String> = KIND_TAGS
        .iter()
        .find(|(kind, _)| *kind == head)
        .map(|(_, tags)| tags.iter().map(|t| (*t).to_owned()).collect())
        .unwrap_or_default();
    for (word, tag) in WORD_TAGS {
        if low.contains(word) {
            tags.insert((*tag).to_owned());
        }
    }
    tags.into_iter().collect()
}

/// `store.digest(*parts)` over strings.
pub fn digest(parts: &[&str]) -> Result<String> {
    let parts = parts
        .iter()
        .map(|p| Canon::Str((*p).to_owned()))
        .collect::<Vec<_>>();
    Ok(careerbrew_vault::helpers::digest(&parts)?)
}

/// `note(title, tags, impact, at, links)`: one manual evidence row, appended.
pub fn note(
    title: &str,
    tags: &[String],
    impact: &str,
    at: Option<&str>,
    links: &[String],
) -> Result<Value> {
    let when = match at {
        Some(at) => at.to_owned(),
        None => crate::when::iso_seconds(&crate::now()?),
    };
    let mut all: BTreeSet<String> = tags.iter().cloned().collect();
    all.extend(tags_for(title));
    let mut item = json!({
        "id": format!("note:{}", digest(&[title, &when])?),
        "kind": "note",
        "title": title,
        "impact": impact,
        "at": when,
        "tags": all.into_iter().collect::<Vec<_>>(),
        "origin": "user",
    });
    if !links.is_empty() {
        item["links"] = json!(links);
    }
    crate::append_jsonl(&path()?, &[careerbrew_setup::value::to_canon(&item)])?;
    Ok(item)
}

/// `forget(ids)`: remove rows by id; the file is rewritten only when one went.
pub fn forget(ids: &BTreeSet<String>) -> Result<usize> {
    let rows = load()?;
    let keep = rows
        .iter()
        .filter(|row| {
            !row.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| ids.contains(id))
        })
        .cloned()
        .collect::<Vec<_>>();
    if keep.len() != rows.len() {
        crate::write_jsonl(&path()?, &keep)?;
    }
    Ok(rows.len() - keep.len())
}
