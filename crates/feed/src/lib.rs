//! Component-only feed ingestion and transcript normalization (rust-13).
//!
//! No production route selects this crate. Mutations stay below the
//! invocation-owned synthetic data root. Health/live completion and
//! promise/evidence integration use explicit invocation-scoped providers.

pub mod cli;
pub mod feed;
pub mod sources;
pub mod synthetic;
pub mod transcript;

use careerbrew_vault::canon::Canon;
use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedError {
    pub class: &'static str,
    pub message: String,
}

impl FeedError {
    pub fn new(class: &'static str, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
        }
    }

    pub fn feed(message: impl Into<String>) -> Self {
        Self::new("FeedError", message)
    }

    pub fn value(message: impl Into<String>) -> Self {
        Self::new("ValueError", message)
    }
}

impl From<std::io::Error> for FeedError {
    fn from(_: std::io::Error) -> Self {
        Self::new("OSError", "feed file operation failed")
    }
}

impl From<careerbrew_vault::atomic::StoreError> for FeedError {
    fn from(_: careerbrew_vault::atomic::StoreError) -> Self {
        Self::new("StoreError", "feed storage operation failed")
    }
}

impl From<careerbrew_vault::transfer::TransferError> for FeedError {
    fn from(error: careerbrew_vault::transfer::TransferError) -> Self {
        Self::new(error.class, error.message)
    }
}

impl From<careerbrew_sources::SourceError> for FeedError {
    fn from(error: careerbrew_sources::SourceError) -> Self {
        Self::new(error.class, error.message)
    }
}

impl From<careerbrew_projects::ProjectError> for FeedError {
    fn from(error: careerbrew_projects::ProjectError) -> Self {
        Self::new(error.class, error.message)
    }
}

impl From<careerbrew_setup::SetupError> for FeedError {
    fn from(error: careerbrew_setup::SetupError) -> Self {
        Self::new(error.class, error.message)
    }
}

impl From<serde_json::Error> for FeedError {
    fn from(_: serde_json::Error) -> Self {
        Self::value("invalid JSON")
    }
}

pub type Result<T> = std::result::Result<T, FeedError>;

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
        .map_err(|_| FeedError::value("cannot encode synthetic report"))
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
    let value = context()?.store().read_yaml(
        &root.join(relative),
        careerbrew_setup::value::to_canon(&default),
        true,
    )?;
    Ok(careerbrew_setup::value::from_canon(&value))
}

pub fn write_yaml(root: &Path, relative: &str, value: &Value) -> Result<()> {
    context()?.store().write_yaml(
        &root.join(relative),
        &careerbrew_setup::value::to_canon(value),
    )?;
    Ok(())
}

pub fn read_jsonl(root: &Path, relative: &str) -> Result<Vec<Value>> {
    Ok(context()?
        .store()
        .read_jsonl(&root.join(relative), true)?
        .iter()
        .map(careerbrew_setup::value::from_canon)
        .collect())
}

pub fn write_jsonl(root: &Path, relative: &str, rows: &[Value]) -> Result<()> {
    let rows = rows
        .iter()
        .map(careerbrew_setup::value::to_canon)
        .collect::<Vec<_>>();
    context()?
        .store()
        .write_jsonl(&root.join(relative), &rows)?;
    Ok(())
}

pub fn read_text(root: &Path, relative: &str) -> Result<String> {
    Ok(context()?
        .store()
        .read_text(&root.join(relative), Some(""), true)?
        .unwrap_or_default())
}

pub fn write_text(root: &Path, relative: &str, value: &str) -> Result<()> {
    context()?.store().write_text(&root.join(relative), value)?;
    Ok(())
}

pub fn local_zone() -> chrono_tz::Tz {
    match context() {
        Ok(context) => careerbrew_memory::memory_sync::local_zone(&context.root),
        Err(_) => careerbrew_memory::memory_sync::local_zone(Path::new("")),
    }
}

pub fn now() -> DateTime<FixedOffset> {
    std::env::var("CAREERBREW_NOW")
        .ok()
        .and_then(|value| {
            DateTime::parse_from_rfc3339(&value).ok().or_else(|| {
                chrono::NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M:%S%.f")
                    .ok()
                    .and_then(|naive| {
                        careerbrew_memory::memory_sync::local_naive(local_zone(), naive)
                    })
            })
        })
        .unwrap_or_else(|| Utc::now().with_timezone(&local_zone()).fixed_offset())
}

pub fn now_utc() -> DateTime<Utc> {
    now().with_timezone(&Utc)
}

/// Python calendar.now().isoformat(timespec="seconds") in the configured zone.
pub fn now_iso() -> String {
    now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// Python calendar.now_utc().isoformat() for acquisition stamps.
pub fn now_utc_iso() -> String {
    now_utc().to_rfc3339_opts(SecondsFormat::Secs, false)
}

struct EventClock;

impl careerbrew_vault::events::Clock for EventClock {
    fn stamp(&self) -> String {
        // Python events._now() reads the wall clock, not CAREERBREW_NOW.
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

/// Typed declaration snapshot supplied by the not-yet-native health/live seam.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Declaration {
    pub required: Vec<String>,
    pub identity: String,
}

/// Invocation-scoped provider boundary for integrations outside rust-13.
pub trait ComponentProvider {
    fn declaration(&self, root: &Path) -> Result<Declaration>;
    fn complete(&self, root: &Path, required: &[String], batch: &Value) -> Result<()>;
    fn required_gaps(&self, root: &Path) -> Result<Vec<String>>;
    fn track_fresh(&self, fresh: &[(Value, String)]) -> Result<()>;
}
