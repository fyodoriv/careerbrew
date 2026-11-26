//! Read-only declared/direct/adapter health and direct completion receipts.

use crate::{
    field, live, now_utc, parse_utc, read_json, read_jsonl, setting_i64, text, truthy, write_json,
    Result,
};
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::Path;

pub const DIRECT_COMMANDS: &[&str] = &[
    "careerbrew work feed transcript <session.jsonl> --since 30m",
    "careerbrew work feed cycle",
];

pub fn declared(root: &Path) -> Result<(Vec<Value>, Vec<String>, String)> {
    let sources = live::sources(root)?;
    let tools = crate::setting(root, "feed_tool_sources", json!([]))?
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(crate::json_string)
        .collect::<Vec<_>>();
    let required = tools
        .iter()
        .filter_map(|item| item.split_once('=').map(|(_, source)| source.to_owned()))
        .chain(
            sources
                .iter()
                .filter(|source| truthy(field(source, "required")))
                .map(|source| text(field(source, "name"))),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let binding = json!({
        "tools": tools,
        "sources": sources,
        "required": required,
        "max_age_minutes": setting_i64(root, "live_source_max_age_minutes", 15)?,
    });
    Ok((sources, required, crate::json_digest(&binding)?))
}

fn age(value: &str, at: DateTime<Utc>, highwater: Option<&str>) -> Option<f64> {
    let stamp = if value.is_empty() {
        None
    } else {
        parse_utc(value)
    }?;
    let previous = highwater.and_then(parse_utc);
    if stamp > at || highwater.is_some_and(|_| previous.is_none_or(|previous| previous > at)) {
        return None;
    }
    Some((at - stamp).num_milliseconds() as f64 / 1000.0)
}

pub fn complete(root: &Path, required: &[String], batch: &Value) -> Result<()> {
    let at = now_utc();
    let (_sources, declared_required, identity) = declared(root)?;
    let path = "work/feed/direct-cycle.json";
    let old = read_json(root, path, json!({}))?;
    let binding_matches = text(field(batch, "identity")) == identity;
    let received = if binding_matches {
        field(batch, "acquired")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    } else {
        Map::new()
    };
    let stored = field(batch, "stored")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| text(Some(value)))
        .collect::<BTreeSet<_>>();
    let missing = required
        .iter()
        .filter(|source| !stored.contains(*source))
        .collect::<BTreeSet<_>>();
    let failed_sources = field(batch, "failed_sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| text(Some(value)))
        .collect::<BTreeSet<_>>();
    let healthy = !declared_required.is_empty()
        && declared_required
            .iter()
            .all(|source| stored.contains(source))
        && missing.is_empty()
        && !truthy(field(batch, "errors"))
        && required
            .iter()
            .all(|source| !failed_sources.contains(source));
    let state = live::state(root)?;
    let valid_clock = declared_required.iter().all(|source| {
        received
            .get(source)
            .and_then(Value::as_str)
            .and_then(parse_utc)
            .is_some_and(|stamp| stamp <= at)
    }) && (old.as_object().is_none_or(Map::is_empty)
        || age(&text(field(&old, "checked_at")), at, None).is_some())
        && (text(field(&state, "checked_at")).is_empty()
            || age(&text(field(&state, "checked_at")), at, None).is_some());
    let status = if !declared_required.is_empty() && !binding_matches {
        "unknown"
    } else if !healthy {
        "failed"
    } else if !valid_clock {
        "unknown"
    } else {
        "ok"
    };
    let previous = parse_utc(&text(field(&old, "checked_at")));
    let checked = previous.map(|previous| previous.max(at)).unwrap_or(at);
    let acquired = declared_required
        .iter()
        .map(|source| {
            (
                source.clone(),
                received.get(source).cloned().unwrap_or(Value::Null),
            )
        })
        .collect::<Map<_, _>>();
    let record = json!({
        "cycles": old["cycles"].as_i64().unwrap_or_default() + 1,
        "identity": identity,
        "required": declared_required,
        "stored": stored.into_iter().collect::<Vec<_>>(),
        "status": status,
        "checked_at": checked.to_rfc3339_opts(SecondsFormat::Secs, false),
        "acquired": acquired,
        "errors": truthy(field(batch, "errors")),
        "failed_sources": field(batch, "failed_sources").cloned().unwrap_or_else(|| json!([])),
    });
    write_json(root, path, &record)?;
    // rust-17 results and rust-18 standup publish are typed completion effects,
    // not reimplemented here. Quiet/direct cases with no eligible evidence have
    // no effect. A standup with no saved rows uses the existing skipped result.
    let mut state = live::state(root)?;
    let _ = live::standup(root, &mut state, &[]);
    let fresh = live::state(root)?;
    for key in ["running", "started_at", "stopped_at"] {
        if let Some(value) = fresh.get(key) {
            state[key] = value.clone();
        }
    }
    live::save_state(root, &state)
}

fn invalid_snapshot(root: &Path) -> Result<Value> {
    let state = read_json(root, "work/live/state.json", json!({}))?;
    Ok(json!({
        "version": 1,
        "as_of": now_utc().to_rfc3339_opts(SecondsFormat::Secs, false),
        "status": "error",
        "reason": "invalid-config",
        "running": truthy(field(&state, "running")),
        "mode": "none",
        "required": 0,
        "healthy": 0,
        "sources": [],
        "next_expiry": Value::Null,
        "commands": ["careerbrew work live start --sources <file.yaml>"],
        "row_review": {"status": "unknown", "checked_at": Value::Null, "age_seconds": Value::Null, "max_age_minutes": 60},
    }))
}

pub fn snapshot(root: &Path) -> Result<Value> {
    match snapshot_inner(root) {
        Ok(value) => Ok(value),
        Err(error) if error.class == "LiveError" => invalid_snapshot(root),
        Err(error) => Err(error),
    }
}

fn snapshot_inner(root: &Path) -> Result<Value> {
    let at = now_utc();
    let (sources, required, identity) = declared(root)?;
    let state = read_json(root, "work/live/state.json", json!({}))?;
    let direct = read_json(root, "work/feed/direct-cycle.json", json!({}))?;
    let last = read_jsonl(root, "work/live/cycles.jsonl")?
        .last()
        .cloned()
        .unwrap_or_else(|| json!({}));
    let direct_bound = !required.is_empty()
        && text(field(&direct, "identity")) == identity
        && field(&direct, "required")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .map(|value| text(Some(value)))
                    .collect::<Vec<_>>()
            })
            == Some(required.clone());
    let by_name = sources
        .iter()
        .map(|source| (text(field(source, "name")), source))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut direct_rows = Vec::new();
    for name in &required {
        let limit = by_name
            .get(name)
            .and_then(|source| source.get("max_age_minutes"))
            .and_then(Value::as_i64)
            .unwrap_or(setting_i64(root, "live_source_max_age_minutes", 15)?);
        let stamp = text(Some(
            field(&direct, "acquired")
                .and_then(|rows| rows.get(name))
                .unwrap_or(&Value::Null),
        ));
        let mut age = direct_bound
            .then(|| age(&stamp, at, Some(&text(field(&direct, "checked_at")))))
            .flatten();
        let mut health = if age.is_none() {
            "unknown"
        } else if age.is_some_and(|age| age <= (limit * 60) as f64) {
            "healthy"
        } else {
            "stale"
        };
        if direct_bound && text(field(&direct, "status")) != "ok" {
            if text(field(&direct, "status")) == "unknown" {
                health = "unknown";
            } else if !field(&direct, "stored")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|row| text(Some(row)) == *name)
                || field(&direct, "failed_sources")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|row| text(Some(row)) == *name)
                || truthy(field(&direct, "errors"))
            {
                health = "error";
            }
        }
        let acquisition = parse_utc(&stamp);
        let checked = parse_utc(&text(field(&direct, "checked_at")));
        direct_rows.push(json!({
            "name": name,
            "required": true,
            "health": health,
            "acquired_at": if direct_bound { acquisition.map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, false)).map(Value::from).unwrap_or(Value::Null) } else { Value::Null },
            "checked_at": if direct_bound { checked.map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, false)).map(Value::from).unwrap_or(Value::Null) } else { Value::Null },
            "age_seconds": age.take().map(Value::from).unwrap_or(Value::Null),
            "max_age_minutes": limit,
        }));
    }
    let mut adapter_rows = Vec::new();
    let mut assessed_failure = false;
    for source in &sources {
        let name = text(field(source, "name"));
        let receipt = field(&state, "validated")
            .and_then(|rows| rows.get(&name))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let bound = text(field(&receipt, "identity")) == live::identity(root, source)?;
        let mut age = bound
            .then(|| {
                age(
                    &text(field(&receipt, "acquired_at")),
                    at,
                    Some(&text(field(&state, "checked_at"))),
                )
            })
            .flatten();
        let limit = source
            .get("max_age_minutes")
            .and_then(Value::as_i64)
            .unwrap_or(setting_i64(root, "live_source_max_age_minutes", 15)?);
        let prior = field(&last, "sources")
            .and_then(|rows| rows.get(&name))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let mut health = if age.is_none() {
            "unknown"
        } else if age.is_some_and(|age| age <= (limit * 60) as f64) {
            "healthy"
        } else {
            "stale"
        };
        if text(field(&prior, "source_identity")) == live::identity(root, source)? {
            assessed_failure |=
                required.contains(&name) && text(field(&prior, "health")) != "healthy";
            if matches!(text(field(&prior, "health")).as_str(), "error" | "missing") {
                health = if text(field(&prior, "health")) == "error" {
                    "error"
                } else {
                    "missing"
                };
            }
        }
        let acquisition = parse_utc(&text(field(&receipt, "acquired_at")));
        let checked = parse_utc(&text(field(&state, "checked_at")));
        adapter_rows.push(json!({
            "name": name.clone(),
            "required": required.contains(&name),
            "health": health,
            "acquired_at": if bound { acquisition.map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, false)).map(Value::from).unwrap_or(Value::Null) } else { Value::Null },
            "checked_at": checked.map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, false)).map(Value::from).unwrap_or(Value::Null),
            "age_seconds": age.take().map(Value::from).unwrap_or(Value::Null),
            "max_age_minutes": limit,
        }));
    }
    let present = adapter_rows
        .iter()
        .map(|row| text(field(row, "name")))
        .collect::<BTreeSet<_>>();
    for name in required.iter().filter(|name| !present.contains(*name)) {
        adapter_rows.push(json!({"name": name, "required": true, "health": "unknown", "acquired_at": Value::Null, "checked_at": Value::Null, "age_seconds": Value::Null, "max_age_minutes": setting_i64(root, "live_source_max_age_minutes", 15)?}));
    }
    let running = truthy(field(&state, "running"));
    let adapter_at = parse_utc(&text(field(&last, "at")));
    let direct_at = parse_utc(&text(field(&direct, "checked_at")));
    let ordering_unknown = !last.as_object().is_none_or(Map::is_empty)
        && (adapter_at.is_none_or(|adapter_at| adapter_at > at)
            || age(&text(field(&state, "checked_at")), at, None).is_none());
    if running && ordering_unknown {
        for row in &mut adapter_rows {
            if !matches!(text(field(row, "health")).as_str(), "error" | "missing") {
                row["health"] = json!("unknown");
                row["age_seconds"] = Value::Null;
            }
        }
    }
    let adapter_complete = !required.is_empty()
        && adapter_rows
            .iter()
            .filter(|row| truthy(field(row, "required")))
            .all(|row| text(field(row, "health")) == "healthy");
    let adapter_wins = running
        && direct_bound
        && (ordering_unknown
            || (adapter_at.is_some()
                && direct_at.is_some()
                && ((assessed_failure && adapter_at.unwrap() >= direct_at.unwrap())
                    || (adapter_complete && adapter_at.unwrap() > direct_at.unwrap()))));
    let use_direct = direct_bound && !adapter_wins;
    let direct_ok = use_direct
        && text(field(&direct, "status")) == "ok"
        && direct_rows
            .iter()
            .all(|row| text(field(row, "health")) == "healthy");
    let rows = if use_direct {
        direct_rows
            .into_iter()
            .chain(
                adapter_rows
                    .into_iter()
                    .filter(|row| !truthy(field(row, "required"))),
            )
            .collect::<Vec<_>>()
    } else {
        adapter_rows
    };
    let mut status = "healthy";
    let mut reason = "current";
    let mut commands = Vec::new();
    if sources.is_empty() && required.is_empty() {
        status = "missing";
        reason = "no-sources";
        commands.push("careerbrew work live start --sources <file.yaml>".into());
    } else if use_direct && !direct_ok {
        status = if text(field(&direct, "status")) == "failed" {
            "error"
        } else if rows.iter().any(|row| text(field(row, "health")) == "stale") {
            "stale"
        } else {
            "unknown"
        };
        reason = "direct-gap";
        commands.extend(DIRECT_COMMANDS.iter().map(|command| (*command).to_owned()));
    } else if sources.is_empty() && !required.is_empty() && !direct_ok {
        status = "unknown";
        reason = "direct-gap";
        commands.extend(DIRECT_COMMANDS.iter().map(|command| (*command).to_owned()));
    } else if !direct_ok && !running {
        status = "stopped";
        reason = "adapters-stopped";
        commands.push("careerbrew work live start".into());
    } else if !direct_ok
        && rows
            .iter()
            .any(|row| truthy(field(row, "required")) && text(field(row, "health")) != "healthy")
    {
        let failures = rows
            .iter()
            .filter(|row| truthy(field(row, "required")))
            .map(|row| text(field(row, "health")))
            .collect::<BTreeSet<_>>();
        status = ["error", "missing", "stale", "unknown"]
            .into_iter()
            .find(|health| failures.contains(*health))
            .unwrap_or("unknown");
        reason = "required-gap";
        commands.push("careerbrew work live cycle".into());
    } else if rows
        .iter()
        .any(|row| !truthy(field(row, "required")) && text(field(row, "health")) != "healthy")
    {
        reason = "optional-gap";
        commands.push("careerbrew work live cycle".into());
    }
    let mut expiries = Vec::new();
    for row in &rows {
        if text(field(row, "health")) == "healthy" {
            if let Some(stamp) = parse_utc(&text(field(row, "acquired_at"))) {
                expiries.push(
                    stamp
                        + TimeDelta::minutes(row["max_age_minutes"].as_i64().unwrap_or_default())
                        + TimeDelta::seconds(1),
                );
            }
        }
    }
    let saved = read_json(root, "work/standup.json", json!({}))?;
    let saved_at = parse_utc(&text(field(&saved, "saved_at")));
    let review_age = age(&text(field(&saved, "saved_at")), at, None);
    let review_status = if review_age.is_none() {
        "unknown"
    } else if review_age.is_some_and(|age| age <= 3600.0) {
        "current"
    } else {
        "due"
    };
    let review = json!({
        "status": review_status,
        "checked_at": if saved_at.is_some() && review_age.is_some() { saved_at.map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, false)).map(Value::from).unwrap_or(Value::Null) } else { Value::Null },
        "age_seconds": review_age.map(Value::from).unwrap_or(Value::Null),
        "max_age_minutes": 60,
    });
    if review_status == "current" {
        if let Some(saved_at) = saved_at {
            expiries.push(saved_at + TimeDelta::minutes(60) + TimeDelta::seconds(1));
        }
    }
    Ok(json!({
        "version": 1,
        "as_of": at.to_rfc3339_opts(SecondsFormat::Secs, false),
        "status": status,
        "reason": reason,
        "running": running,
        "mode": if use_direct { "direct" } else if !sources.is_empty() { "adapter" } else { "none" },
        "required": required.len(),
        "healthy": rows.iter().filter(|row| truthy(field(row, "required")) && text(field(row, "health")) == "healthy").count(),
        "sources": rows,
        "next_expiry": expiries.into_iter().min().map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, false)).map(Value::from).unwrap_or(Value::Null),
        "commands": commands,
        "row_review": review,
    }))
}

pub fn result_health(row: &Value, health: &Value) -> Value {
    health["sources"].as_array().into_iter().flatten().find(|source| text(field(source, "name")) == text(field(row, "feed_source"))).cloned().unwrap_or_else(|| json!({"health": "unknown", "age_seconds": Value::Null, "acquired_at": Value::Null, "checked_at": Value::Null}))
}
