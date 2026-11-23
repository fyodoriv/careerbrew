//! Read-only prior-session classification; status never implies completion.

use crate::{error, Result};
use chrono::{Duration, NaiveDate};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const LIMIT: usize = 10_000;

fn id(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(|value| {
        value.len() == 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn day(value: Option<&Value>) -> Option<NaiveDate> {
    let value = value?.as_str()?;
    if value.len() != 10
        || value.bytes().enumerate().any(|(index, byte)| {
            if matches!(index, 4 | 7) {
                byte != b'-'
            } else {
                !byte.is_ascii_digit()
            }
        })
    {
        return None;
    }
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .filter(|parsed| parsed.format("%Y-%m-%d").to_string() == value)
}

pub fn select(today: NaiveDate, rows: &[Value], projects: &[String]) -> Result<Vec<Value>> {
    if today == NaiveDate::MIN
        || rows.len() > LIMIT
        || projects.len() > LIMIT
        || projects.iter().any(|value| !id(Some(&json!(value))))
    {
        return Err(error("ValueError", "invalid selection input"));
    }
    let yesterday = today - Duration::days(1);
    let mut project_counts = BTreeMap::<&str, usize>::new();
    for project in projects {
        *project_counts.entry(project).or_default() += 1;
    }
    let mut record_counts = BTreeMap::<&str, usize>::new();
    for row in rows {
        if let Some(record) = row
            .get("record_id")
            .and_then(Value::as_str)
            .filter(|value| {
                value.len() == 32
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
        {
            *record_counts.entry(record).or_default() += 1;
        }
    }
    let mut out = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let mut outcome = "attention";
        let mut reason = "invalid_row";
        if row.is_object() {
            let row_day = day(row.get("day"));
            let record = row.get("record_id");
            let project = row.get("project_incarnation").and_then(Value::as_str);
            if row_day.is_none() {
                reason = "invalid_day";
            } else if !id(record) {
                reason = "invalid_record_id";
            } else if record_counts.get(record.and_then(Value::as_str).expect("valid id"))
                != Some(&1)
            {
                reason = "duplicate_record_id";
            } else if !id(row.get("project_incarnation")) {
                reason = "invalid_project_id";
            } else if project_counts.get(project.expect("valid project")) != Some(&1) {
                reason = "unknown_project_id";
            } else if row_day != Some(yesterday) {
                outcome = "out_of_day";
                reason = "outside_day";
            } else {
                outcome = "candidate";
                reason = "prior_day";
            }
        }
        out.push(json!({"index": index, "outcome": outcome, "reason": reason}));
    }
    Ok(out)
}
