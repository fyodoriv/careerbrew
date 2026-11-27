//! The work day's truth rules (`workday/gate.py`): the generic next action,
//! the overdue test, the plugin exec gate and the next pass over today's
//! running rows (one retry after a timeout, then failed).

use crate::{field, now, parse_iso, read_json, text, Result, Stamp};
use careerbrew_workday::status;
use chrono::{DateTime, Duration, FixedOffset, NaiveDate};
use serde_json::{json, Value};
use std::path::Path;

pub const ERROR_CLASSES: &[&str] = status::ERROR_CLASSES;
pub const MAX_ATTEMPTS: i64 = status::MAX_ATTEMPTS;
const ASK: &str = "answer the ask in the end-of-steps questions, then run {step} again";

fn next_for(class: &str) -> &'static str {
    match class {
        "env" => "install or fix the tool or setting {step} needs, then run {step} again",
        "auth" => "sign in again or renew the token {step} uses (careerbrew secret set <name>), then run {step} again",
        "network" => "run {step} again when the network or the service is back",
        "conflict" => "resolve the conflict by hand, then run {step} again",
        _ => "read the log of {step}, fix the tool or the step, then run {step} again",
    }
}

/// The generic next action for a blocked or failed row; empty otherwise.
pub fn next_action(step: &str, state: &str, error_class: &str) -> String {
    match state {
        "blocked" => ASK.replace("{step}", step),
        "failed" => next_for(error_class).replace("{step}", step),
        _ => String::new(),
    }
}

/// A plugin's commands run only when the owner added it with `--allow-exec`.
pub fn may_run(root: &Path, origin: &str) -> Result<bool> {
    let Some(name) = origin.strip_prefix("plugin:") else {
        return Ok(true);
    };
    let lock = read_json(root, "plugins.lock.json", json!({"plugins": {}}))?;
    Ok(field(&lock, "plugins")
        .and_then(Value::as_object)
        .and_then(|plugins| plugins.get(name))
        .and_then(|entry| field(entry, "exec_allowed"))
        .is_some_and(|allowed| allowed.as_bool().unwrap_or(false)))
}

/// A `running` row whose start is more than `minutes` ago, or that cannot prove its start.
pub fn overdue(row: &Value, minutes: i64, at: DateTime<FixedOffset>) -> bool {
    if text(field(row, "status")) != "running" {
        return false;
    }
    let started = text(field(row, "started"));
    if started.is_empty() {
        return true;
    }
    match parse_iso(&started) {
        Some(Stamp::Aware(started)) => at - started > Duration::minutes(minutes),
        // Python cannot subtract a naive start from an aware clock either; it cannot prove it is inside its limit.
        Some(Stamp::Naive(_)) | None => true,
    }
}

/// The next pass over today's rows: retry each overdue step once, then mark it failed.
pub fn check(root: &Path, steps: &[Value], day: NaiveDate) -> Result<Value> {
    let names = steps
        .iter()
        .map(|s| text(field(s, "step")))
        .collect::<Vec<_>>();
    let limits = steps
        .iter()
        .filter(|s| !field(s, "long_running").is_some_and(|v| v.as_bool().unwrap_or(false)))
        .map(|s| {
            (
                text(field(s, "step")),
                field(s, "timeout").and_then(Value::as_i64).unwrap_or(0),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let at = now();
    let mut checked = 0;
    let mut retry = Vec::new();
    let mut failed = Vec::new();
    for row in status::rows(root, &names, day)? {
        let step = text(field(&row, "step"));
        let Some(limit) = limits.get(&step) else {
            continue;
        };
        if text(field(&row, "status")) != "running" {
            continue;
        }
        checked += 1;
        if !overdue(&row, *limit, at) {
            continue;
        }
        let started = text(field(&row, "started"));
        if let Some(changed) = status::timed_out(root, &step, &names, &started, day)? {
            if text(field(&changed, "status")) == "waiting" {
                retry.push(step);
            } else {
                failed.push(step);
            }
        }
    }
    Ok(json!({"checked": checked, "retry": retry, "failed": failed}))
}
