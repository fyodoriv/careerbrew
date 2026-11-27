//! `field.compute`: this machine's real-use totals from its own content-free
//! event log. The machine label is a typed seam: its random salt and host name
//! stay with the caller (rust-followup: port `field.machine_label`).

use crate::{float, int, number, py_round, Result};
use serde_json::{json, Map, Value};
use std::path::Path;

/// Supplies the salted machine label.
pub trait MachineLabel {
    fn label(&self) -> Result<String>;
}

/// The synthetic component's fixed label.
pub struct Pinned(pub String);

impl MachineLabel for Pinned {
    fn label(&self) -> Result<String> {
        Ok(self.0.clone())
    }
}

fn rows(log: &Path) -> Result<Vec<Map<String, Value>>> {
    if !log.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(log).map_err(|_| "cannot read the event log".to_owned())?;
    let mut out = Vec::new();
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Value::Object(row) = value else {
            return Err("AttributeError".into());
        };
        if matches!(
            row.get("event").and_then(Value::as_str),
            Some("cli.run" | "cli.error")
        ) {
            out.push(row);
        }
    }
    Ok(out)
}

pub fn compute(log: &Path, machine: &dyn MachineLabel) -> Result<Value> {
    let rows = rows(log)?;
    let event =
        |row: &Map<String, Value>| row.get("event").and_then(Value::as_str).map(str::to_owned);
    let timed = rows
        .iter()
        .filter(|row| event(row).as_deref() == Some("cli.error") || row.contains_key("ms"))
        .collect::<Vec<_>>();
    let ok = timed
        .iter()
        .filter(|row| {
            event(row).as_deref() == Some("cli.run")
                && row.get("status").and_then(Value::as_str) == Some("ok")
        })
        .count() as i64;
    let failed = timed.len() as i64 - ok;
    let seconds = timed
        .iter()
        .filter_map(|row| row.get("ms").and_then(number))
        .map(|ms| ms / 1000.0)
        .collect::<Vec<_>>();
    let saved = crate::time_saved::compute(log)?;
    let since = timed
        .iter()
        .map(|row| {
            let ts = row.get("ts").and_then(Value::as_str).unwrap_or_default();
            ts.chars().take(10).collect::<String>()
        })
        .min();
    let mut by_week = Map::new();
    for week in saved["weeks"].as_array().into_iter().flatten() {
        by_week.insert(
            week["week"].as_str().unwrap_or_default().to_owned(),
            week["minutes_saved"].clone(),
        );
    }
    let total = timed.len() as f64;
    Ok(json!({
        "machine": machine.label()?,
        "since": since,
        "runs": int(timed.len() as i64),
        "runs_ok": int(ok),
        "runs_failed": int(failed),
        "success_rate": if timed.is_empty() { Value::Null } else { float(py_round(100.0 * ok as f64 / total, 1)) },
        "mean_seconds": if seconds.is_empty() { Value::Null } else {
            float(py_round(seconds.iter().sum::<f64>() / seconds.len() as f64, 2))
        },
        "action_runs": saved["runs_total"],
        "minutes_saved": saved["minutes_saved_total"],
        "minutes_saved_by_week": by_week,
        "measured_baselines": saved["measured_baselines"],
        "baselines": int(saved["baselines"].as_object().map_or(0, Map::len) as i64),
    }))
}
