//! `telemetry.to_event`: one event-log row as a PostHog event, with
//! allowlisted fields only. Sending, consent and the backlog stay in Python
//! until the network seam ports (rust-followup in the PR).

use crate::allowlists::*;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::sync::OnceLock;

const REGISTRY: &str = include_str!("../../cbcli/registry/registry.json");
const MAX_NUMBER: f64 = 1e12;

/// Python `re.match(pattern + "$")`: `$` also matches before one final newline.
fn py_match(pattern: &Regex, value: &str) -> bool {
    pattern.is_match(value)
        || value
            .strip_suffix('\n')
            .is_some_and(|v| pattern.is_match(v))
}

fn re(cell: &'static OnceLock<Regex>, source: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(source).expect("static pattern"))
}

fn version_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(&CELL, r"^\d+\.\d+\.\d+\z")
}

fn ts_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(&CELL, r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ\z")
}

fn num_key_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(&CELL, r"^[a-z][a-z0-9_]{0,31}\z")
}

fn story_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(&CELL, r"^us-\d{1,3}\z")
}

/// Every command name the CLI parser knows, from the generated registry:
/// each top-level name and each `<group>.<sub>` pair.
pub fn command_names() -> &'static BTreeSet<String> {
    static CELL: OnceLock<BTreeSet<String>> = OnceLock::new();
    CELL.get_or_init(|| {
        let registry: Value = serde_json::from_str(REGISTRY).expect("generated registry");
        let mut names = BTreeSet::new();
        for row in registry["rows"].as_array().into_iter().flatten() {
            let Some(name) = row["name"].as_str() else {
                continue;
            };
            let parts = name.split('.').collect::<Vec<_>>();
            names.insert(parts[0].to_owned());
            if parts.len() > 1 {
                names.insert(format!("{}.{}", parts[0], parts[1]));
            }
        }
        names
    })
}

fn has(set: &[&str], value: &str) -> bool {
    set.binary_search(&value).is_ok()
}

fn planted() -> bool {
    std::env::var("CAREERBREW_RUST24_PLANT").as_deref() == Ok("allowlist")
}

/// `telemetry._string_ok`: the value that may leave for one string field.
pub fn string_ok(key: &str, value: &str) -> Option<String> {
    let keep = |ok: bool| ok.then(|| value.to_owned());
    match key {
        "command" => keep(command_names().contains(value)),
        "status" => keep(has(STATUSES, value)),
        "story" => keep(py_match(story_re(), value)),
        "error" => {
            let lower = value.to_lowercase();
            Some(if has(ERRORS, &lower) {
                lower
            } else {
                "other".into()
            })
        }
        "source_kind" => keep(has(SOURCE_KINDS, value)),
        "tier" => keep(has(TIERS, value)),
        "route" => keep(has(VIEW_ROUTES, value) || has(WEB_ACTS, value)),
        "action" => keep(has(ACTIONS, value)),
        "error_class" => keep(has(STEP_ERRORS, value)),
        "via" => keep(has(VIA, value)),
        "step_kind" => keep(has(STEP_KINDS, value)),
        "signin" => keep(has(SIGNIN, value)),
        "source" => keep(has(ASK_SOURCES, value)),
        "recheck" => keep(has(RECHECKS, value)),
        "from_version" => keep(py_match(version_re(), value)),
        _ => planted().then(|| value.to_owned()),
    }
}

/// `telemetry.machine_fields`.
pub fn machine_fields() -> Map<String, Value> {
    let version = crate::version();
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        _ => "other",
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => "other",
    };
    let mut out = Map::new();
    out.insert(
        "app_version".into(),
        json!(if py_match(version_re(), &version) {
            version
        } else {
            "0.0.0".into()
        }),
    );
    out.insert("os".into(), json!(os));
    out.insert("arch".into(), json!(arch));
    // rust-followup: the label stays `python` until the native runtime ships.
    out.insert("runtime".into(), json!("python"));
    out
}

/// `telemetry.to_event`: None (JSON null) when the row may not leave.
pub fn to_event(record: &Value, distinct_id: &str, since: &str) -> Value {
    let Some(row) = record.as_object() else {
        return Value::Null;
    };
    let Some(name) = row.get("event").and_then(Value::as_str) else {
        return Value::Null;
    };
    if !has(EVENT_NAMES, name) {
        return Value::Null;
    }
    let Some(ts) = row.get("ts").and_then(Value::as_str) else {
        return Value::Null;
    };
    if !py_match(ts_re(), ts) || ts < since {
        return Value::Null;
    }
    let mut props = Map::new();
    for (key, value) in row {
        if matches!(key.as_str(), "event" | "ts" | "v") {
            continue;
        }
        match value {
            Value::String(text) => {
                if let Some(kept) = string_ok(key, text) {
                    props.insert(key.clone(), Value::String(kept));
                }
            }
            Value::Bool(_) | Value::Number(_) if py_match(num_key_re(), key) => {
                let ok = match value {
                    Value::Bool(_) => true,
                    Value::Number(n) => n
                        .as_f64()
                        .is_some_and(|x| x.is_finite() && x.abs() <= MAX_NUMBER),
                    _ => false,
                };
                if ok {
                    let key = if key == "ms" {
                        "duration_ms"
                    } else {
                        key.as_str()
                    };
                    props.insert(key.to_owned(), value.clone());
                }
            }
            _ => {}
        }
    }
    props.extend(machine_fields());
    props.insert("$process_person_profile".into(), json!(false));
    props.insert("$geoip_disable".into(), json!(true));
    if let Some(version) = row.get("v").and_then(Value::as_str) {
        if py_match(version_re(), version) {
            props.insert("app_version".into(), json!(version));
        }
    }
    json!({"event": name, "distinct_id": distinct_id, "timestamp": ts, "properties": props})
}

pub fn sorted(set: &[&str]) -> Value {
    let mut items = set.to_vec();
    items.sort_unstable();
    json!(items)
}
