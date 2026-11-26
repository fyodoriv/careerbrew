//! Component-only tutor cards, public decks, and Notion imports (rust-20).
//!
//! No production route selects this crate. Native writes stay below the
//! invocation's synthetic data root or its explicitly supplied public deck.

pub mod cards;
pub mod engine;
pub mod fsrs;
pub mod mistakes;
pub mod notion;
pub mod public;
pub mod synthetic;

use careerbrew_vault::canon::Canon;
use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TutorError {
    pub class: &'static str,
    pub message: String,
}

impl TutorError {
    pub fn new(class: &'static str, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
        }
    }

    pub fn value(message: impl Into<String>) -> Self {
        Self::new("ValueError", message)
    }

    pub fn import(message: impl Into<String>) -> Self {
        Self::new("ImportError_", message)
    }
}

impl From<std::io::Error> for TutorError {
    fn from(_: std::io::Error) -> Self {
        Self::new("OSError", "tutor file operation failed")
    }
}

impl From<careerbrew_vault::atomic::StoreError> for TutorError {
    fn from(_: careerbrew_vault::atomic::StoreError) -> Self {
        Self::new("StoreError", "tutor storage operation failed")
    }
}

impl From<careerbrew_vault::transfer::TransferError> for TutorError {
    fn from(error: careerbrew_vault::transfer::TransferError) -> Self {
        Self::new(error.class, error.message)
    }
}

impl From<careerbrew_vault::bulk::BulkError> for TutorError {
    fn from(error: careerbrew_vault::bulk::BulkError) -> Self {
        Self::new(error.python_kind(), "tutor bundle sealing failed")
    }
}

impl From<careerbrew_setup::SetupError> for TutorError {
    fn from(error: careerbrew_setup::SetupError) -> Self {
        Self::new(error.class, error.message)
    }
}

impl From<careerbrew_projects::ProjectError> for TutorError {
    fn from(error: careerbrew_projects::ProjectError) -> Self {
        Self::new(error.class, error.message)
    }
}

impl From<serde_json::Error> for TutorError {
    fn from(_: serde_json::Error) -> Self {
        Self::value("invalid JSON")
    }
}

pub type Result<T> = std::result::Result<T, TutorError>;

pub fn context() -> Result<careerbrew_vault::transfer::Context> {
    Ok(careerbrew_vault::transfer::Context::from_env()?)
}

pub fn root() -> Result<PathBuf> {
    Ok(context()?.root)
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
    careerbrew_projects::typed(value)
}

pub fn compact(value: &Value) -> Result<String> {
    careerbrew_vault::json_yaml::json_compact_python(&careerbrew_setup::value::to_canon(value))
        .map_err(|_| TutorError::value("cannot encode synthetic report"))
}

pub fn casefold(value: &str) -> String {
    icu_casemap::CaseMapper::new()
        .fold_string(value)
        .into_owned()
}

pub fn chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

pub fn digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let encoded = careerbrew_vault::json_yaml::json_compact_python(&Canon::Str(text.to_owned()))
        .expect("a string always has a Python JSON representation");
    let mut hash = Sha256::new();
    hash.update(encoded.as_bytes());
    hash.update([0]);
    format!("{:x}", hash.finalize())[..24].to_owned()
}

pub fn now() -> DateTime<FixedOffset> {
    std::env::var("CAREERBREW_NOW")
        .ok()
        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
        .unwrap_or_else(|| Utc::now().fixed_offset())
}

pub fn parse_at(value: &str) -> Result<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(value).map_err(|_| TutorError::value("invalid tutor timestamp"))
}

pub fn stamp_at(at: DateTime<FixedOffset>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// Python datetime.isoformat() preserves six fractional digits when present.
pub fn stamp_state(at: DateTime<FixedOffset>) -> String {
    if at.timestamp_subsec_micros() == 0 {
        return stamp_at(at);
    }
    let base = at.format("%Y-%m-%dT%H:%M:%S%.6f").to_string();
    let offset = at.offset().local_minus_utc();
    let sign = if offset < 0 { '-' } else { '+' };
    let seconds = offset.unsigned_abs();
    format!(
        "{base}{sign}{:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60
    )
}

pub fn local_zone(root: &Path) -> chrono_tz::Tz {
    careerbrew_memory::memory_sync::local_zone(root)
}

pub fn system_zone() -> chrono_tz::Tz {
    careerbrew_memory::memory_sync::system_zone()
}

pub fn read_json(root: &Path, relative: &str, default: Value) -> Result<Value> {
    read_json_abs(&root.join(relative), default)
}

pub fn read_json_abs(path: &Path, default: Value) -> Result<Value> {
    let value =
        context()?
            .store()
            .read_json(path, careerbrew_setup::value::to_canon(&default), true)?;
    Ok(careerbrew_setup::value::from_canon(&value))
}

pub fn write_json(root: &Path, relative: &str, value: &Value) -> Result<()> {
    write_json_abs(&root.join(relative), value)
}

pub fn write_json_abs(path: &Path, value: &Value) -> Result<()> {
    context()?
        .store()
        .write_json(path, &careerbrew_setup::value::to_canon(value))?;
    Ok(())
}

pub fn read_yaml(root: &Path, relative: &str, default: Value) -> Result<Value> {
    read_yaml_abs(&root.join(relative), default)
}

pub fn read_yaml_abs(path: &Path, default: Value) -> Result<Value> {
    let value =
        context()?
            .store()
            .read_yaml(path, careerbrew_setup::value::to_canon(&default), true)?;
    Ok(careerbrew_setup::value::from_canon(&value))
}

pub fn write_yaml(root: &Path, relative: &str, value: &Value) -> Result<()> {
    write_yaml_abs(&root.join(relative), value)
}

pub fn write_yaml_abs(path: &Path, value: &Value) -> Result<()> {
    context()?
        .store()
        .write_yaml(path, &careerbrew_setup::value::to_canon(value))?;
    Ok(())
}

pub fn read_jsonl(root: &Path, relative: &str) -> Result<Vec<Value>> {
    read_jsonl_abs(&root.join(relative))
}

pub fn read_jsonl_abs(path: &Path) -> Result<Vec<Value>> {
    Ok(context()?
        .store()
        .read_jsonl(path, true)?
        .iter()
        .map(careerbrew_setup::value::from_canon)
        .collect())
}

pub fn write_jsonl(root: &Path, relative: &str, rows: &[Value]) -> Result<()> {
    write_jsonl_abs(&root.join(relative), rows)
}

pub fn write_jsonl_abs(path: &Path, rows: &[Value]) -> Result<()> {
    let rows = rows
        .iter()
        .map(careerbrew_setup::value::to_canon)
        .collect::<Vec<_>>();
    context()?.store().write_jsonl(path, &rows)?;
    Ok(())
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

pub fn read_text(root: &Path, relative: &str) -> Result<String> {
    read_text_abs(&root.join(relative))
}

pub fn read_text_abs(path: &Path) -> Result<String> {
    Ok(context()?
        .store()
        .read_text(path, Some(""), true)?
        .unwrap_or_default())
}

pub fn write_text(root: &Path, relative: &str, value: &str) -> Result<()> {
    write_text_abs(&root.join(relative), value)
}

pub fn write_text_abs(path: &Path, value: &str) -> Result<()> {
    context()?.store().write_text(path, value)?;
    Ok(())
}

struct EventClock;

impl careerbrew_vault::events::Clock for EventClock {
    fn stamp(&self) -> String {
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }

    fn version(&self) -> String {
        std::env::var("CAREERBREW_VERSION").unwrap_or_else(|_| "0.1.0".into())
    }
}

pub fn emit(root: &Path, event: &str, fields: Vec<(&str, Canon)>) -> Result<()> {
    let fields = fields
        .into_iter()
        .map(|(key, value)| (Canon::Str(key.into()), value))
        .collect::<Vec<_>>();
    careerbrew_vault::events::emit(
        root,
        std::env::var("CAREERBREW_EVENTS").ok().as_deref(),
        event,
        &Canon::Map(fields),
        &EventClock,
    )?;
    Ok(())
}

pub fn error(result: Result<()>) -> String {
    result
        .err()
        .map(|error| error.class)
        .unwrap_or("accepted")
        .to_owned()
}
