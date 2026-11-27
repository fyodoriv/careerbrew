//! The end-of-day handoff (Python `day/handoff.py`), without the live promise
//! projection (`with_promises`, which reads rust-11 promise records).
//!
//! Each load-change-save holds the day's `.handoff-<day>.json.lock`.

use crate::{field, sha256_hex, text, truthy, value_error, words, Result};
use chrono::NaiveDate;
use serde_json::{json, Map, Value};
use std::path::Path;

pub const SECTIONS: &[&str] = &[
    "calendar",
    "prs",
    "waiting",
    "questions",
    "promises",
    "unverified",
    "lessons",
    "gotchas",
    "projects",
];
pub const MAX_TEXT: usize = 200;

pub fn relative(day: NaiveDate) -> String {
    format!("day/handoff-{}.json", day.format("%Y-%m-%d"))
}

pub fn load(root: &Path, day: NaiveDate) -> Result<Vec<Value>> {
    let stored = careerbrew_feed::read_json(root, &relative(day), json!({}))?;
    if !truthy(Some(&stored)) {
        return Ok(Vec::new());
    }
    Ok(field(&stored, "items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

fn save(root: &Path, items: Vec<Value>, day: NaiveDate) -> Result<()> {
    careerbrew_feed::write_json_if_changed(
        root,
        &relative(day),
        &json!({"day": day.format("%Y-%m-%d").to_string(), "items": items}),
    )?;
    Ok(())
}

fn locked<T>(root: &Path, day: NaiveDate, f: impl FnOnce() -> Result<T>) -> Result<T> {
    careerbrew_vault::lock::with_lock(&root.join(relative(day)), f)?
}

/// A checked item, not saved. `extra` keeps only truthy values.
pub fn item(section: &str, value: &str, link: &str, extra: &[(&str, Value)]) -> Result<Value> {
    if !SECTIONS.contains(&section) {
        let quoted = format!("'{section}'");
        return Err(value_error(format!(
            "unknown section {quoted}; use one of: {}",
            SECTIONS.join(", ")
        )));
    }
    let value = words(value).join(" ");
    if value.is_empty() {
        return Err(value_error("the item text is empty"));
    }
    let length = value.chars().count();
    if length > MAX_TEXT {
        return Err(value_error(format!(
            "keep an item short: {length} characters, at most {MAX_TEXT} (put the detail behind a link)"
        )));
    }
    let id = format!("h-{}", &sha256_hex(&format!("{section}\n{value}"))[..8]);
    let mut out = Map::new();
    out.insert("id".into(), json!(id));
    out.insert("section".into(), json!(section));
    out.insert("text".into(), json!(value));
    if !link.is_empty() {
        out.insert("link".into(), json!(link));
    }
    for (key, extra_value) in extra {
        if truthy(Some(extra_value)) {
            out.insert((*key).into(), extra_value.clone());
        }
    }
    Ok(Value::Object(out))
}

fn id_of(value: &Value) -> String {
    text(field(value, "id"))
}

pub fn add(root: &Path, section: &str, value: &str, link: &str, day: NaiveDate) -> Result<Value> {
    let new = item(section, value, link, &[])?;
    locked(root, day, || {
        let mut items = load(root, day)?
            .into_iter()
            .filter(|row| id_of(row) != id_of(&new))
            .collect::<Vec<_>>();
        items.push(new.clone());
        save(root, items, day)
    })?;
    Ok(new)
}

/// Replace the items `owner` wrote; every other item stays.
pub fn replace_owned(root: &Path, owner: &str, items: &[Value], day: NaiveDate) -> Result<()> {
    let ids = items.iter().map(id_of).collect::<Vec<_>>();
    locked(root, day, || {
        let mut kept = load(root, day)?
            .into_iter()
            .filter(|row| text(field(row, "owner")) != owner && !ids.contains(&id_of(row)))
            .collect::<Vec<_>>();
        for item in items {
            let mut item = item.clone();
            item["owner"] = json!(owner);
            kept.push(item);
        }
        save(root, kept, day)
    })
}

pub fn done(root: &Path, item_id: &str, day: NaiveDate) -> Result<bool> {
    locked(root, day, || {
        let items = load(root, day)?;
        let before = items.len();
        let kept = items
            .into_iter()
            .filter(|row| id_of(row) != item_id)
            .collect::<Vec<_>>();
        if kept.len() == before {
            return Ok(false);
        }
        save(root, kept, day)?;
        Ok(true)
    })
}

/// The newest non-empty handoff written on a day before `day`.
pub fn latest_before(root: &Path, day: NaiveDate) -> Result<(Option<NaiveDate>, Vec<Value>)> {
    let folder = root.join("day");
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(stem) = name
                .strip_prefix("handoff-")
                .and_then(|rest| rest.strip_suffix(".json"))
            {
                found.push(crate::iso_day(stem)?);
            }
        }
    }
    found.sort_by(|a, b| b.cmp(a));
    for candidate in found {
        if candidate < day {
            let items = load(root, candidate)?;
            if !items.is_empty() {
                return Ok((Some(candidate), items));
            }
        }
    }
    Ok((None, Vec::new()))
}
