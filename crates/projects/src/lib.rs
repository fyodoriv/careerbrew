//! Component-only projects, priorities and promises for rust-09.
//!
//! No production route selects this crate. Every mutation is scoped to the
//! invocation's synthetic data root and the native project-file lock.

pub mod cli;
pub mod focus;
pub mod priorities;
pub mod project_priority;
pub mod projects;
pub mod promises;
pub mod synthetic;
pub mod team;

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectError {
    pub class: &'static str,
    pub message: String,
}

impl ProjectError {
    pub fn new(class: &'static str, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
        }
    }

    pub fn project(message: impl Into<String>) -> Self {
        Self::new("ProjectError", message)
    }

    pub fn value(message: impl Into<String>) -> Self {
        Self::new("ValueError", message)
    }

    pub fn team(message: impl Into<String>) -> Self {
        Self::new("TeamError", message)
    }
}

impl From<std::io::Error> for ProjectError {
    fn from(_: std::io::Error) -> Self {
        Self::new("OSError", "project file operation failed")
    }
}

impl From<careerbrew_vault::atomic::StoreError> for ProjectError {
    fn from(_: careerbrew_vault::atomic::StoreError) -> Self {
        Self::new("StoreError", "project storage operation failed")
    }
}

impl From<careerbrew_vault::transfer::TransferError> for ProjectError {
    fn from(error: careerbrew_vault::transfer::TransferError) -> Self {
        Self::new(error.class, error.message)
    }
}

impl From<serde_json::Error> for ProjectError {
    fn from(_: serde_json::Error) -> Self {
        Self::value("invalid JSON")
    }
}

pub type Result<T> = std::result::Result<T, ProjectError>;

pub fn context() -> Result<careerbrew_vault::transfer::Context> {
    Ok(careerbrew_vault::transfer::Context::from_env()?)
}

pub fn root() -> Result<PathBuf> {
    Ok(context()?.root)
}

pub fn now() -> DateTime<FixedOffset> {
    std::env::var("CAREERBREW_NOW")
        .ok()
        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
        .unwrap_or_else(|| Utc::now().fixed_offset())
}

pub fn stamp() -> String {
    now()
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub fn today() -> chrono::NaiveDate {
    now().date_naive()
}

pub fn read_yaml(root: &Path, relative: &str, default: Value) -> Result<Value> {
    let context = context()?;
    let store = context.store();
    let path = root.join(relative);
    let value = store.read_yaml(&path, careerbrew_setup::value::to_canon(&default), true)?;
    Ok(careerbrew_setup::value::from_canon(&value))
}

pub fn read_jsonl(root: &Path, relative: &str) -> Result<Vec<Value>> {
    let context = context()?;
    let store = context.store();
    Ok(store
        .read_jsonl(&root.join(relative), true)?
        .iter()
        .map(careerbrew_setup::value::from_canon)
        .collect())
}

pub fn write_yaml(root: &Path, relative: &str, value: &Value) -> Result<()> {
    let context = context()?;
    let store = context.store();
    store.write_yaml(
        &root.join(relative),
        &careerbrew_setup::value::to_canon(value),
    )?;
    Ok(())
}

pub fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    careerbrew_setup::value::get(value, key)
}

pub fn text(value: Option<&Value>) -> String {
    careerbrew_setup::value::text(value)
}

pub fn truthy(value: Option<&Value>) -> bool {
    careerbrew_setup::value::bool_value(value)
}

pub fn strings(value: Option<&Value>) -> Vec<String> {
    careerbrew_setup::value::strings(value)
}

pub fn typed(value: &Value) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::String(_) => value.clone(),
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            serde_json::json!({"int": number.to_string()})
        }
        Value::Number(number) => serde_json::json!({"float": number.to_string()}),
        Value::Array(items) => Value::Array(items.iter().map(typed).collect()),
        Value::Object(items) => Value::Object(
            items
                .iter()
                .map(|(key, value)| (key.clone(), typed(value)))
                .collect(),
        ),
    }
}

pub fn chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

pub fn one_line(value: &str, what: &str, limit: usize) -> Result<String> {
    let value = value.trim().to_owned();
    if value.contains('\n') || value.chars().count() > limit {
        return Err(ProjectError::project(format!(
            "{what} is one line of at most {limit} characters"
        )));
    }
    Ok(value)
}

pub fn float(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.parse().ok())
}

pub fn python_float(value: f64) -> String {
    if value == 0.0 {
        return "0.0".into();
    }
    if value.fract() == 0.0 {
        return format!("{value:.1}");
    }
    value.to_string()
}

pub fn python_g(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

pub fn python_round(value: f64, digits: i32) -> f64 {
    if !value.is_finite() {
        return value;
    }
    if digits < 0 {
        let factor = 10_f64.powi(-digits);
        return (value / factor).round_ties_even() * factor;
    }
    // Rust's exact decimal formatter and Python's round both use ties-even.
    // Parsing the formatted decimal avoids a binary multiplication changing
    // values such as 2.675 across the rounding boundary.
    let precision = digits as usize;
    format!("{value:.precision$}").parse().unwrap_or(value)
}

pub fn date(value: &str) -> Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| ProjectError::value(format!("invalid ISO date {value:?}")))
}
