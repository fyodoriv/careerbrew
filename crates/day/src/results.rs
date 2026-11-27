//! The results-of-the-day list: one sealed JSONL file per local day.
//!
//! Appends are idempotent by id; a row seen again with a changed version
//! field replaces its stored copy (Python `day/results.py`).

use crate::{field, local_day, Result};
use chrono::NaiveDate;
use serde_json::{json, Map, Value};
use std::path::Path;

pub const VERSION_FIELDS: &[&str] = &[
    "kind",
    "title",
    "draft",
    "state",
    "merged_at",
    "feed_source",
];

pub fn relative(day: NaiveDate) -> String {
    format!("day/{}.jsonl", day.format("%Y-%m-%d"))
}

pub fn load(root: &Path, day: NaiveDate) -> Result<Vec<Value>> {
    careerbrew_feed::read_jsonl(root, &relative(day))
}

/// Added plus replaced rows; one write per batch, none when nothing changed.
pub fn append(root: &Path, rows: &[Value], day: NaiveDate) -> Result<usize> {
    let rows = rows
        .iter()
        .map(careerbrew_setup::value::to_canon)
        .collect::<Vec<_>>();
    let (added, replaced) = careerbrew_feed::context()?.store().upsert_jsonl(
        &root.join(relative(day)),
        &rows,
        VERSION_FIELDS,
    )?;
    Ok(added + replaced)
}

/// Convert already eligible evidence for its local day.
pub fn evidence_rows(items: &[Value], day: NaiveDate) -> Vec<Value> {
    let mut rows = Vec::new();
    for item in items {
        let at = match field(item, "at") {
            Some(Value::String(text)) => text.clone(),
            None => String::new(),
            Some(other) => other.to_string(),
        };
        if local_day(&at) != Some(day) {
            continue;
        }
        let get = |key: &str, default: &str| {
            field(item, key)
                .cloned()
                .unwrap_or_else(|| Value::String(default.into()))
        };
        let mut row = Map::new();
        row.insert("id".into(), item["id"].clone());
        row.insert("at".into(), item["at"].clone());
        row.insert("kind".into(), get("kind", "other"));
        row.insert("title".into(), get("title", ""));
        row.insert("source".into(), json!("evidence"));
        row.insert("origin".into(), get("origin", "user"));
        for key in ["draft", "state", "merged_at", "feed_source"] {
            if let Some(value) = field(item, key) {
                row.insert(key.into(), value.clone());
            }
        }
        rows.push(Value::Object(row));
    }
    rows
}

pub fn summary(root: &Path, day: NaiveDate) -> Result<Value> {
    let mut out = Map::new();
    for row in load(root, day)? {
        let kind = match field(&row, "kind") {
            Some(Value::String(kind)) => kind.clone(),
            None => "other".into(),
            Some(other) => other.to_string(),
        };
        let count = out.get(&kind).and_then(Value::as_i64).unwrap_or(0) + 1;
        out.insert(kind, json!(count));
    }
    Ok(Value::Object(out))
}
