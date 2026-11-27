//! rust-19 slice 1: job search postings, ATS boards, criteria, Want scores and
//! LinkedIn alert postings. Synthetic-only; no production route selects it.
//!
//! Storage, clock, digest and event helpers come from the rust-20 crate
//! (`careerbrew-tutor`) so the two steps share one store boundary.

pub mod criteria;
pub mod linkedin;
pub mod prep;
pub mod sources;
pub mod synthetic;
pub mod want;

pub use careerbrew_tutor::{Result, TutorError as JobsError};

use serde_json::Value;
use std::path::Path;

/// Python truthiness of an optional JSON value.
pub(crate) fn truthy(value: Option<&Value>) -> bool {
    careerbrew_tutor::truthy(value)
}

/// `str(value)` for the scalar ids that board APIs return.
pub(crate) fn py_str(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".into(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(true)) => "True".into(),
        Some(Value::Bool(false)) => "False".into(),
        Some(other) => other.to_string(),
    }
}

/// `d.get(key, "")` for a text field: missing or null gives the empty text.
pub(crate) fn get_text(value: &Value, key: &str) -> String {
    match value.get(key) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// `paths.sub(*parts)`: Python creates the parent folder even on a read.
pub(crate) fn sub(root: &Path, relative: &str) -> Result<std::path::PathBuf> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(path)
}

/// A float from a JSON number or numeric text, like Python `float(v)`.
pub(crate) fn float(value: &Value) -> Result<f64> {
    careerbrew_projects::float(value)
        .ok_or_else(|| JobsError::value("could not convert value to float"))
}

/// An int from a JSON number, like Python `int(v)` (floats truncate).
pub(crate) fn int(value: &Value) -> Result<i64> {
    if let Some(number) = value.as_i64() {
        return Ok(number);
    }
    if let Some(number) = value.as_f64() {
        return Ok(number.trunc() as i64);
    }
    value
        .as_str()
        .and_then(|text| text.trim().parse().ok())
        .ok_or_else(|| JobsError::value("invalid literal for int()"))
}
