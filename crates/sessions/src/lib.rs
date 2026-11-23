//! Component-only sessions, claims, agents and reply drafts for rust-10.
//!
//! No production route selects this crate. Every mutation uses the
//! invocation's synthetic data root and the same atomic storage and advisory
//! lock providers as the Python oracle.

pub mod agents;
pub mod cli;
pub mod drafts;
pub mod prior_selection;
pub mod sessions;
pub mod synthetic;

use careerbrew_projects::{ProjectError, Result};
use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
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
    now()
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub fn stamp_at(at: DateTime<FixedOffset>) -> String {
    at.with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub fn micro_stamp(at: DateTime<FixedOffset>) -> String {
    at.with_timezone(&Utc)
        .format("%Y-%m-%dT%H:%M:%S%.6f%:z")
        .to_string()
}

pub fn today() -> chrono::NaiveDate {
    now().date_naive()
}

pub fn read_json(root: &Path, relative: &str, default: Value) -> Result<Value> {
    let context = context()?;
    let value = context.store().read_json(
        &root.join(relative),
        careerbrew_setup::value::to_canon(&default),
        true,
    )?;
    Ok(careerbrew_setup::value::from_canon(&value))
}

pub fn write_json(root: &Path, relative: &str, value: &Value) -> Result<()> {
    let context = context()?;
    context.store().write_json(
        &root.join(relative),
        &careerbrew_setup::value::to_canon(value),
    )?;
    Ok(())
}

pub fn read_jsonl(root: &Path, relative: &str) -> Result<Vec<Value>> {
    careerbrew_projects::read_jsonl(root, relative)
}

pub fn write_jsonl(root: &Path, relative: &str, rows: &[Value]) -> Result<()> {
    let context = context()?;
    let rows = rows
        .iter()
        .map(careerbrew_setup::value::to_canon)
        .collect::<Vec<_>>();
    context.store().write_jsonl(&root.join(relative), &rows)?;
    Ok(())
}

pub fn read_text(root: &Path, relative: &str) -> Result<Option<String>> {
    let context = context()?;
    Ok(context
        .store()
        .read_text(&root.join(relative), None, true)?)
}

pub fn write_text(root: &Path, relative: &str, text: &str) -> Result<()> {
    let context = context()?;
    context.store().write_text(&root.join(relative), text)?;
    Ok(())
}

pub fn setting(root: &Path, key: &str, default: i64) -> Result<i64> {
    let settings = careerbrew_projects::read_yaml(root, "settings.yaml", serde_json::json!({}))?;
    Ok(settings.get(key).and_then(Value::as_i64).unwrap_or(default))
}

pub fn set_setting(root: &Path, key: &str, value: i64) -> Result<()> {
    let mut settings =
        careerbrew_projects::read_yaml(root, "settings.yaml", serde_json::json!({}))?;
    settings[key] = Value::from(value);
    careerbrew_vault::lock::with_lock(&root.join("settings.yaml"), || {
        careerbrew_projects::write_yaml(root, "settings.yaml", &settings)
    })?
}

pub fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    careerbrew_projects::field(value, key)
}

pub fn text(value: Option<&Value>) -> String {
    careerbrew_projects::text(value)
}

pub fn error(class: &'static str, message: impl Into<String>) -> ProjectError {
    ProjectError::new(class, message)
}

pub fn typed(value: &Value) -> Value {
    careerbrew_projects::typed(value)
}

pub fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
