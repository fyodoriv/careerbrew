//! Component CLI adapters. Production dispatch remains unchanged.

use crate::{feed, field, text, transcript, ComponentProvider, Result};
use chrono::TimeDelta;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub fn invocation(exit: i32, stdout: String) -> Value {
    json!({"exit": exit, "stdout": stdout})
}

pub fn transcript_command(
    root: &Path,
    path: &Path,
    since: &str,
    components: &dyn ComponentProvider,
) -> Value {
    match transcript::store(root, path, since, components) {
        Ok(result) => {
            let mut out = format!(
                "stored {} new event(s), {} updated, {} already stored, {} skipped (no id, time, or title)\n",
                result["new"].as_i64().unwrap_or_default(),
                result["updated"].as_i64().unwrap_or_default(),
                result["already"].as_i64().unwrap_or_default(),
                result["skipped"].as_i64().unwrap_or_default(),
            );
            let pairs = transcript::tool_map(root).unwrap_or_default();
            let replies = transcript::mapped_sources(&pairs)
                .iter()
                .map(|source| {
                    format!(
                        "{source}={}",
                        field(&result["replies"], source)
                            .and_then(Value::as_i64)
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let pointers = &result["pointers"];
            out.push_str(&format!(
                "replies: {replies}; {} error(s); saved-output files: {} read, {} refused (outside the transcript's folder or missing)\n",
                result["errors"].as_i64().unwrap_or_default(),
                pointers["followed"].as_i64().unwrap_or_default(),
                pointers["refused"].as_i64().unwrap_or_default(),
            ));
            for source in result["gaps"].as_array().into_iter().flatten() {
                out.push_str(&format!("gap  {}: nothing stored this cycle (fix: run its tool again, or check its replies have id, at, and title)\n", text(Some(source))));
            }
            invocation(
                if result["gaps"]
                    .as_array()
                    .is_some_and(|gaps| gaps.is_empty())
                {
                    0
                } else {
                    1
                },
                out,
            )
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

pub fn cycle_command(
    root: &Path,
    required: &[String],
    json_output: bool,
    components: &dyn ComponentProvider,
) -> Value {
    match feed::mark_cycle(root, Some(required), components) {
        Ok(result) => {
            if json_output {
                return match feed::cycle_json(&result) {
                    Ok(json) => invocation(0, json + "\n"),
                    Err(error) => invocation(2, format!("error: {}\n", error.message)),
                };
            }
            let required = result["required"].as_array().cloned().unwrap_or_default();
            let stored = result["stored"].as_array().cloned().unwrap_or_default();
            let gaps = result["not_stored"].as_array().cloned().unwrap_or_default();
            let mut out = format!(
                "cycle: stored {} of {} required source(s); coverage {} of {}\n",
                required
                    .iter()
                    .filter(|source| stored.contains(source))
                    .count(),
                required.len(),
                required.len() - gaps.len(),
                required.len(),
            );
            for source in &gaps {
                out.push_str(&format!("not stored  {}: required acquisition coverage is missing or invalid (fix: pull it again, then careerbrew work feed add or transcript)\n", text(Some(source))));
            }
            invocation(if gaps.is_empty() { 0 } else { 1 }, out)
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

pub fn team_list_command(root: &Path) -> Value {
    match careerbrew_projects::team::load(root) {
        Ok(rows) => {
            if rows.is_empty() {
                return invocation(0, "no teammates yet: careerbrew work team add \"Sam Lee\" --chat U0SAM --login samlee --email sam@example.com\n".into());
            }
            let mut out = format!("{} teammate(s)\n", rows.len());
            for row in rows {
                let ids = ["chat", "email", "login"]
                    .into_iter()
                    .filter(|key| !text(field(&row, key)).is_empty())
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(
                    "- (name hidden): {}\n",
                    if ids.is_empty() {
                        "name only".into()
                    } else {
                        ids
                    }
                ));
            }
            out.push_str("names hidden: stdout is not a terminal and no plugin policy allows agents to read business data\n");
            invocation(0, out)
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

pub fn team_feed_rows(root: &Path, days: i64) -> Result<Vec<Value>> {
    let cut = (crate::now_utc() - TimeDelta::days(days))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let every = careerbrew_projects::projects::load(root)?;
    let entries = careerbrew_projects::project_priority::entries(root)?;
    let globs = entries
        .iter()
        .map(|entry| {
            (
                text(field(entry, "name")),
                field(entry, "match")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|value| text(Some(value)))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut out = feed::load(root)?
        .into_iter()
        .filter(|row| {
            !field(row, "team")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty)
                && text(field(row, "triage")) != "noise"
                && !crate::truthy(field(row, "personal"))
                && text(field(row, "at")) >= cut
        })
        .map(|mut row| {
            let item = json!({
                "id": field(&row, "id").cloned().unwrap_or(Value::Null),
                "source": text(field(&row, "source")),
                "title": text(field(&row, "title")),
                "url": text(field(&row, "url")),
                "channel": text(field(&row, "channel")),
            });
            let projects = careerbrew_memory::memory_sync::targets(&item, &every, &globs)
                .into_iter()
                .filter(|name| !crate::truthy(field(&every[name], "archived_at")))
                .collect::<Vec<_>>();
            row["projects"] = json!(projects);
            row["to_research"] = json!(feed::needs_research(&row));
            row
        })
        .collect::<Vec<_>>();
    out.sort_by_key(|row| std::cmp::Reverse(text(field(row, "at"))));
    Ok(out)
}

pub fn team_feed_command(root: &Path, days: i64) -> Value {
    match team_feed_rows(root, days) {
        Ok(rows) => {
            if rows.is_empty() {
                return invocation(0, format!("no kept teammate events in the last {days} day(s) (add teammates: careerbrew work team add \"<name>\" --chat <id>)\n"));
            }
            let research = rows
                .iter()
                .map(|row| {
                    row["to_research"]
                        .as_array()
                        .map(Vec::len)
                        .unwrap_or_default()
                })
                .sum::<usize>();
            let matched = rows
                .iter()
                .filter(|row| !row["projects"].as_array().is_none_or(Vec::is_empty))
                .count();
            invocation(
                0,
                format!("{} teammate event(s) in the last {days} day(s): {matched} match a project (with project memory sync on, their notes go to its memory file), {} match none; {research} link(s) to research\ntitles hidden: stdout is not a terminal and no plugin policy allows agents to read business data\n", rows.len(), rows.len() - matched),
            )
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}
