//! Agent activity records, restart/quiet lifecycle and grouped views.

use crate::{
    error, field, now, one_line, read_jsonl, setting, stamp_at, text, write_jsonl, Result,
};
use chrono::{DateTime, Duration, FixedOffset, SecondsFormat, Utc};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const STATUSES: &[&str] = &["running", "done", "failed"];
const PATH: &str = "work/agents/events.jsonl";

pub fn load(root: &Path) -> Result<Vec<Value>> {
    read_jsonl(root, PATH)
}

fn valid_name(value: &str) -> bool {
    Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,59}$")
        .expect("name regex")
        .is_match(value)
}

fn valid_url(value: &str) -> bool {
    Regex::new(r"(?i)^https?://[^/]+\S*$")
        .expect("url regex")
        .is_match(value)
}

#[allow(clippy::too_many_arguments)] // Mirrors the six-field agent event plus root and clock.
pub fn record_at(
    root: &Path,
    agent: &str,
    title: &str,
    status: &str,
    url: &str,
    project: &str,
    run: &str,
    at: DateTime<FixedOffset>,
) -> Result<Value> {
    let agent = agent.trim();
    let run = if run.is_empty() { agent } else { run.trim() };
    let title = one_line(title);
    let project = one_line(project);
    let url = url.trim();
    if !valid_name(agent) {
        return Err(error("AgentError", format!("bad agent name {}: use 1 to 60 letters, digits, and _ . : / - (for example review-bot)", careerbrew_projects::chars(agent, 60))));
    }
    if !valid_name(run) {
        return Err(error(
            "AgentError",
            "bad run name: use 1 to 60 letters, digits, and _ . : / -",
        ));
    }
    if !STATUSES.contains(&status) {
        return Err(error(
            "AgentError",
            format!("bad status {status:?}: use one of running, done, failed"),
        ));
    }
    if title.is_empty() {
        return Err(error(
            "AgentError",
            "the title is empty: say in one line what the agent did or does now",
        ));
    }
    if title.chars().count() > 200 {
        return Err(error(
            "AgentError",
            format!(
                "the title has {} characters; the limit is 200: keep it to one short line",
                title.chars().count()
            ),
        ));
    }
    if project.chars().count() > 80 {
        return Err(error(
            "AgentError",
            format!(
                "the project name has {} characters; the limit is 80",
                project.chars().count()
            ),
        ));
    }
    if !url.is_empty() && !valid_url(url) {
        return Err(error("AgentError", "the link must be a full http(s) URL, for example https://github.example.com/org/repo/pull/7"));
    }
    let row = json!({
        "at": at.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Secs, false),
        "agent": agent, "run": run, "status": status, "title": title,
        "url": url, "project": project,
    });
    let path = root.join(PATH);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(root.join("work/agents/.lock"))?;
    lock.lock()?;
    let cutoff = stamp_at(at - Duration::days(setting(root, "agent_keep_days", 7)?));
    let mut kept = load(root)?
        .into_iter()
        .filter(|row| text(field(row, "at")) >= cutoff)
        .collect::<Vec<_>>();
    kept.push(row.clone());
    write_jsonl(root, PATH, &kept)?;
    drop(lock);
    Ok(row)
}

pub fn record(
    root: &Path,
    agent: &str,
    title: &str,
    status: &str,
    url: &str,
    project: &str,
    run: &str,
) -> Result<Value> {
    record_at(root, agent, title, status, url, project, run, now())
}

pub fn runs_at(root: &Path, at: DateTime<FixedOffset>) -> Result<Vec<Value>> {
    let cutoff = stamp_at(at - Duration::days(setting(root, "agent_keep_days", 7)?));
    let quiet = stamp_at(at - Duration::minutes(setting(root, "agent_quiet_minutes", 60)?));
    let mut events = load(root)?;
    events.sort_by_key(|row| text(field(row, "at")));
    let mut out = BTreeMap::<(String, String), Value>::new();
    for row in events {
        let row_at = text(field(&row, "at"));
        if row_at < cutoff {
            continue;
        }
        let key = (text(field(&row, "agent")), text(field(&row, "run")));
        let previous = out.get(&key).cloned();
        let status = text(field(&row, "status"));
        let restart = previous.is_none()
            || (previous.as_ref().is_some_and(|old| {
                matches!(text(field(old, "status")).as_str(), "done" | "failed")
            }) && status == "running");
        let old_project = previous
            .as_ref()
            .filter(|_| !restart)
            .map(|old| text(field(old, "project")))
            .unwrap_or_default();
        let project = {
            let supplied = text(field(&row, "project"));
            if supplied.is_empty() {
                old_project
            } else {
                supplied
            }
        };
        let started_at = if restart {
            row_at.clone()
        } else {
            text(field(previous.as_ref().expect("previous"), "started_at"))
        };
        let count = if restart {
            1
        } else {
            field(previous.as_ref().expect("previous"), "events")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                + 1
        };
        out.insert(
            key.clone(),
            json!({
                "agent": key.0, "run": key.1, "project": project,
                "status": status, "title": text(field(&row, "title")),
                "url": text(field(&row, "url")), "at": row_at,
                "started_at": started_at, "events": count,
            }),
        );
    }
    let mut rows = out.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        let lk = (
            text(field(left, "at")),
            text(field(left, "agent")),
            text(field(left, "run")),
        );
        let rk = (
            text(field(right, "at")),
            text(field(right, "agent")),
            text(field(right, "run")),
        );
        rk.cmp(&lk)
    });
    for row in &mut rows {
        if text(field(row, "status")) == "running" && text(field(row, "at")) < quiet {
            row["status"] = json!("quiet");
        }
    }
    Ok(rows)
}

pub fn runs(root: &Path) -> Result<Vec<Value>> {
    runs_at(root, now())
}

pub fn counts(rows: &[Value]) -> Value {
    let mut out = Map::new();
    for status in ["running", "quiet", "done", "failed"] {
        out.insert(
            status.into(),
            json!(rows
                .iter()
                .filter(|row| text(field(row, "status")) == status)
                .count()),
        );
    }
    Value::Object(out)
}

pub fn view_at(root: &Path, at: DateTime<FixedOffset>) -> Result<Value> {
    let rows = runs_at(root, at)?;
    let mut groups = BTreeMap::<String, Vec<Value>>::new();
    for row in &rows {
        groups
            .entry(text(field(row, "project")))
            .or_default()
            .push(row.clone());
    }
    let mut ranked = Vec::new();
    for (project, mut items) in groups {
        items.sort_by_key(|row| text(field(row, "status")) != "running");
        let running = items
            .iter()
            .filter(|row| text(field(row, "status")) == "running")
            .count();
        let last_at = items
            .iter()
            .map(|row| text(field(row, "at")))
            .max()
            .unwrap_or_default();
        ranked.push(
            json!({"project": project, "running": running, "last_at": last_at, "runs": items}),
        );
    }
    ranked.sort_by_key(|group| std::cmp::Reverse(text(field(group, "last_at"))));
    ranked.sort_by_key(|group| field(group, "running").and_then(Value::as_u64) == Some(0));
    Ok(
        json!({"counts": counts(&rows), "quiet_minutes": setting(root, "agent_quiet_minutes", 60)?, "groups": ranked}),
    )
}
