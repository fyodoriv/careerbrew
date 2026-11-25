//! Component-only live listening, source health, review asks and notifications.
//!
//! No production route selects this crate. Every mutation stays below the
//! invocation-owned synthetic data root and uses the existing native storage,
//! feed, project, draft, memory and setup providers. Later-owned inbox, goal,
//! result and standup behavior crosses only explicit typed provider seams.

pub mod cli;
pub mod health;
pub mod listen;
pub mod live;
pub mod notify;
pub mod review;
pub mod synthetic;

pub use careerbrew_feed::{
    compact, context, emit, field, now, now_iso, now_utc, now_utc_iso, read_json, read_jsonl,
    read_text, read_yaml, strings, text, truthy, typed, write_json, write_json_if_changed,
    write_jsonl, write_text, write_yaml, FeedError, Result,
};
use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn root() -> Result<PathBuf> {
    Ok(context()?.root)
}

pub fn stamp() -> String {
    now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub fn utc_stamp() -> String {
    now_utc().to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub fn live_stamp() -> String {
    now_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub fn json_digest(value: &Value) -> Result<String> {
    let value = careerbrew_setup::value::to_canon(value);
    let text = careerbrew_vault::json_yaml::json_compact_python(&value)
        .map_err(|_| FeedError::value("cannot bind synthetic identity"))?;
    Ok(digest(&text))
}

pub fn error(class: &'static str, message: impl Into<String>) -> FeedError {
    FeedError::new(class, message)
}

pub fn live_error(message: impl Into<String>) -> FeedError {
    error("LiveError", message)
}

pub fn notify_error(message: impl Into<String>) -> FeedError {
    error("NotifyError", message)
}

pub fn review_error(message: impl Into<String>) -> FeedError {
    error("ReviewAskError", message)
}

pub fn projects(root: &Path) -> Result<serde_json::Map<String, Value>> {
    Ok(careerbrew_projects::projects::load(root)?
        .as_object()
        .cloned()
        .unwrap_or_default())
}

pub fn settings(root: &Path) -> Result<Value> {
    read_yaml(root, "settings.yaml", json!({}))
}

pub fn setting(root: &Path, key: &str, default: Value) -> Result<Value> {
    let values = settings(root)?;
    Ok(field(&values, key).cloned().unwrap_or(default))
}

pub fn setting_bool(root: &Path, key: &str, default: bool) -> Result<bool> {
    Ok(truthy(Some(&setting(root, key, json!(default))?)))
}

pub fn setting_i64(root: &Path, key: &str, default: i64) -> Result<i64> {
    let value = setting(root, key, json!(default))?;
    Ok(value.as_i64().unwrap_or(default))
}

pub fn setting_text(root: &Path, key: &str, default: &str) -> Result<String> {
    Ok(text(Some(&setting(root, key, json!(default))?)))
}

pub fn set_setting(root: &Path, key: &str, value: Value) -> Result<Value> {
    let path = root.join("settings.yaml");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    careerbrew_vault::lock::with_lock(&path, || -> Result<Value> {
        let mut settings = settings(root)?;
        match value.as_str() {
            Some("none") | Some("") => {
                settings
                    .as_object_mut()
                    .expect("settings object")
                    .remove(key);
            }
            _ => {
                settings[key] = value;
            }
        }
        write_yaml(root, "settings.yaml", &settings)?;
        Ok(field(&settings, key).cloned().unwrap_or(Value::Null))
    })?
}

pub fn set_setting_text(root: &Path, key: &str, value: &str) -> Result<Value> {
    let lower = value.to_ascii_lowercase();
    let parsed = match lower.as_str() {
        "true" => json!(true),
        "false" => json!(false),
        _ if matches!(key, "feed_tool_sources" | "project_priority") => {
            json!(value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>())
        }
        _ => value
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| json!(value)),
    };
    set_setting(root, key, parsed)
}

pub fn write_plain(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}

pub fn read_plain(path: &Path) -> Result<String> {
    Ok(std::fs::read_to_string(path)?)
}

pub fn parse_time(value: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(&value.replace("Z", "+00:00")).ok()
}

pub fn parse_utc(value: &str) -> Option<DateTime<Utc>> {
    parse_time(value).map(|value| value.with_timezone(&Utc))
}

pub fn local_day(value: &str) -> Option<String> {
    let zone = careerbrew_feed::local_zone();
    parse_time(value).map(|value| {
        value
            .with_timezone(&zone)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string()
    })
}

pub fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

pub fn json_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => text(Some(value)),
    }
}

pub fn redact(root: &Path, value: &str) -> Result<String> {
    let terms = careerbrew_feed::sources::sensitive_terms(root)?;
    Ok(careerbrew_policy::redact(value, &terms))
}
