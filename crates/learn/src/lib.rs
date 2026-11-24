//! Component-only project learning, replay, tours and explainers (rust-12).
//!
//! No production route selects this crate. Every mutation is scoped to the
//! invocation's synthetic data root or its synthetic replay checkout.

pub mod cards;
pub mod cli;
pub mod explain;
pub mod prd;
pub mod project;
pub mod replay;
pub mod synthetic;
pub mod tour;

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

pub fn stamp_at(at: DateTime<FixedOffset>) -> String {
    at.with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub fn stamp() -> String {
    stamp_at(now())
}

pub fn parse_at(value: &str) -> Result<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(value)
        .map_err(|_| ProjectError::value("invalid learning timestamp"))
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

pub fn read_yaml(root: &Path, relative: &str, default: Value) -> Result<Value> {
    careerbrew_projects::read_yaml(root, relative, default)
}

pub fn write_yaml(root: &Path, relative: &str, value: &Value) -> Result<()> {
    careerbrew_projects::write_yaml(root, relative, value)
}

pub fn read_jsonl(root: &Path, relative: &str) -> Result<Vec<Value>> {
    careerbrew_projects::read_jsonl(root, relative)
}

pub fn append_jsonl(root: &Path, relative: &str, rows: &[Value]) -> Result<()> {
    let rows = rows
        .iter()
        .map(careerbrew_setup::value::to_canon)
        .collect::<Vec<_>>();
    context()?
        .store()
        .append_jsonl(&root.join(relative), &rows)?;
    Ok(())
}

pub fn read_text(path: &Path) -> Result<Option<String>> {
    Ok(context()?.store().read_text(path, None, true)?)
}

pub fn write_text(path: &Path, text: &str) -> Result<()> {
    context()?.store().write_text(path, text)?;
    Ok(())
}

pub fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    careerbrew_projects::field(value, key)
}

pub fn text(value: Option<&Value>) -> String {
    careerbrew_projects::text(value)
}

pub fn strings(value: Option<&Value>) -> Vec<String> {
    careerbrew_projects::strings(value)
}

pub fn typed(value: &Value) -> Value {
    careerbrew_projects::typed(value)
}

pub fn digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let encoded = careerbrew_vault::json_yaml::json_compact_python(
        &careerbrew_vault::canon::Canon::Str(text.to_owned()),
    )
    .expect("a string always has a Python JSON representation");
    let mut hash = Sha256::new();
    hash.update(encoded.as_bytes());
    hash.update([0]);
    format!("{:x}", hash.finalize())[..24].to_owned()
}

pub fn error(class: &'static str, message: impl Into<String>) -> ProjectError {
    ProjectError::new(class, message)
}
