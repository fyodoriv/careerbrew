//! Component-only canonical work-day records and project-session CLI behavior.
//!
//! No production route selects this crate. Mutable state has data-root
//! lifecycle. Every day-file writer uses the canonical day lock, then the
//! history lock when needed; question and session writes preserve foreign keys.

pub mod questions;
pub mod stages;
pub mod status;
pub mod steps;
pub mod synthetic;
pub mod work_session;

use careerbrew_projects::{ProjectError, Result};
use chrono::{DateTime, FixedOffset, SecondsFormat};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn context() -> Result<careerbrew_vault::transfer::Context> {
    Ok(careerbrew_vault::transfer::Context::from_env()?)
}

pub fn root() -> Result<PathBuf> {
    Ok(context()?.root)
}

pub fn now() -> DateTime<FixedOffset> {
    careerbrew_projects::now()
}

pub fn stamp() -> String {
    now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub fn today() -> chrono::NaiveDate {
    now().date_naive()
}

pub fn day(value: &str) -> Result<chrono::NaiveDate> {
    careerbrew_projects::date(value)
}

pub fn read_json(root: &Path, relative: &str, default: Value) -> Result<Value> {
    let value = context()?.store().read_json(
        &root.join(relative),
        careerbrew_setup::value::to_canon(&default),
        true,
    )?;
    Ok(careerbrew_setup::value::from_canon(&value))
}

pub fn write_json(root: &Path, relative: &str, value: &Value) -> Result<()> {
    context()?.store().write_json(
        &root.join(relative),
        &careerbrew_setup::value::to_canon(value),
    )?;
    Ok(())
}

pub fn write_json_if_changed(root: &Path, relative: &str, value: &Value) -> Result<bool> {
    Ok(context()?.store().write_json_if_changed(
        &root.join(relative),
        &careerbrew_setup::value::to_canon(value),
    )?)
}

pub fn read_yaml(root: &Path, relative: &str, default: Value) -> Result<Value> {
    careerbrew_projects::read_yaml(root, relative, default)
}

pub fn write_text(root: &Path, relative: &str, text: &str) -> Result<()> {
    context()?.store().write_text(&root.join(relative), text)?;
    Ok(())
}

pub fn read_text(root: &Path, relative: &str) -> Result<Option<String>> {
    Ok(context()?
        .store()
        .read_text(&root.join(relative), None, true)?)
}

pub fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    careerbrew_projects::field(value, key)
}

pub fn text(value: Option<&Value>) -> String {
    careerbrew_projects::text(value)
}

pub fn typed(value: &Value) -> Value {
    careerbrew_projects::typed(value)
}

pub fn compact(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

pub fn error(class: &'static str, message: impl Into<String>) -> ProjectError {
    ProjectError::new(class, message)
}

pub fn value_error(message: impl Into<String>) -> ProjectError {
    error("ValueError", message)
}

pub fn step_error(message: impl Into<String>) -> ProjectError {
    error("StepError", message)
}

pub fn session_error(message: impl Into<String>) -> ProjectError {
    error("SessionError", message)
}

pub fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn chars(value: &str, limit: usize) -> String {
    careerbrew_projects::chars(value, limit)
}

pub fn setting(root: &Path, key: &str, default: i64) -> Result<i64> {
    let settings = read_yaml(root, "settings.yaml", serde_json::json!({}))?;
    Ok(field(&settings, key)
        .and_then(Value::as_i64)
        .unwrap_or(default))
}

pub fn emit(root: &Path, event: &str, fields: Value) -> Result<()> {
    struct Clock;
    impl careerbrew_vault::events::Clock for Clock {
        fn stamp(&self) -> String {
            crate::now()
                .with_timezone(&chrono::Utc)
                .to_rfc3339_opts(SecondsFormat::Secs, false)
        }
        fn version(&self) -> String {
            std::env::var("CAREERBREW_VERSION").unwrap_or_else(|_| "0.1.0".into())
        }
    }
    let fields = careerbrew_setup::value::to_canon(&fields);
    let events_env = std::env::var("CAREERBREW_EVENTS").ok();
    careerbrew_vault::events::emit(root, events_env.as_deref(), event, &fields, &Clock)?;
    Ok(())
}

pub fn digest(value: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub fn write_count(base: &Path) -> usize {
    std::fs::read_to_string(base.join("writes.log"))
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

pub fn reset_write_count(base: &Path) -> Result<()> {
    std::fs::write(base.join("writes.log"), "")?;
    Ok(())
}
