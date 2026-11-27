//! Component-only rust-18 port of career evidence, ladder grading, the
//! promotion case and the LinkedIn block.
//!
//! No production route selects this crate. Storage goes through the vault
//! store (sealed at rest, like `store.py`); reports in `out/` are plaintext by
//! design, like the Python modules. The standup report is a later slice.

pub mod evidence;
pub mod ladder_grade;
pub mod linkedin;
pub mod promo;
pub mod py;
pub mod synthetic;
pub mod when;

use careerbrew_setup::value::{from_canon, to_canon};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// The Python exception class name.
    pub class: &'static str,
    pub message: String,
}

impl Error {
    pub fn new(class: &'static str, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
        }
    }

    pub fn value(message: impl Into<String>) -> Self {
        Self::new("ValueError", message)
    }

    /// The oracle's `error()` spelling: `Class: message`.
    pub fn shown(&self) -> String {
        format!("{}: {}", self.class, self.message)
    }
}

impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::new("OSError", "career file operation failed")
    }
}

impl From<careerbrew_vault::atomic::StoreError> for Error {
    fn from(_: careerbrew_vault::atomic::StoreError) -> Self {
        Self::new("StoreError", "career storage operation failed")
    }
}

impl From<careerbrew_vault::transfer::TransferError> for Error {
    fn from(error: careerbrew_vault::transfer::TransferError) -> Self {
        Self::new(error.class, error.message)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The data root of this invocation (`CAREERBREW_HOME`), created with mode
/// 0700 like `paths.data_root()`.
pub fn root() -> Result<PathBuf> {
    let root = careerbrew_vault::transfer::Context::from_env()?.root;
    std::fs::create_dir_all(&root)?;
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700));
    Ok(root)
}

/// `paths.sub(*parts)`: the path under the data root, its parent created.
pub fn sub(parts: &[&str]) -> Result<PathBuf> {
    let mut path = root()?;
    for part in parts {
        path.push(part);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(path)
}

fn context() -> Result<careerbrew_vault::transfer::Context> {
    Ok(careerbrew_vault::transfer::Context::from_env()?)
}

pub fn read_jsonl(path: &Path) -> Result<Vec<Value>> {
    let context = context()?;
    Ok(context
        .store()
        .read_jsonl(path, true)?
        .iter()
        .map(from_canon)
        .collect())
}

pub fn write_jsonl(path: &Path, rows: &[Value]) -> Result<()> {
    let context = context()?;
    let rows = rows.iter().map(to_canon).collect::<Vec<_>>();
    context.store().write_jsonl(path, &rows)?;
    Ok(())
}

pub fn append_jsonl(path: &Path, rows: &[careerbrew_vault::canon::Canon]) -> Result<()> {
    let context = context()?;
    context.store().append_jsonl(path, rows)?;
    Ok(())
}

pub fn read_json(path: &Path, default: Value) -> Result<Value> {
    let context = context()?;
    let value = context.store().read_json(path, to_canon(&default), true)?;
    Ok(from_canon(&value))
}

pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    let context = context()?;
    context.store().write_json(path, &to_canon(value))?;
    Ok(())
}

pub fn read_yaml(path: &Path, default: Value) -> Result<Value> {
    let context = context()?;
    let value = context.store().read_yaml(path, to_canon(&default), true)?;
    Ok(from_canon(&value))
}

pub fn write_yaml(path: &Path, value: &Value) -> Result<()> {
    let context = context()?;
    context.store().write_yaml(path, &to_canon(value))?;
    Ok(())
}

pub fn read_text(path: &Path) -> Result<Option<String>> {
    let context = context()?;
    Ok(context.store().read_text(path, None, true)?)
}

pub fn write_text(path: &Path, text: &str) -> Result<()> {
    let context = context()?;
    context.store().write_text(path, text)?;
    Ok(())
}

/// The settings overrides (`config_io.read_settings`): a missing or empty
/// file reads as no overrides.
pub fn settings() -> Result<Value> {
    let value = read_yaml(
        &root()?.join("settings.yaml"),
        Value::Object(Default::default()),
    )?;
    Ok(if value.is_object() {
        value
    } else {
        Value::Object(Default::default())
    })
}

/// `calendar.now()`: the pinned clock (`CAREERBREW_NOW`), its zone kept; a
/// pinned time with no offset is in the local zone.
pub fn now() -> Result<chrono::DateTime<chrono::FixedOffset>> {
    match std::env::var("CAREERBREW_NOW") {
        Ok(pinned) if !pinned.is_empty() => {
            let (naive, offset) = when::parse(&pinned).map_err(Error::value)?;
            Ok(match offset {
                Some(offset) => when::fixed(naive, offset),
                None => when::attach(naive, when::local_tz()?),
            })
        }
        _ => Ok(chrono::Utc::now()
            .with_timezone(&when::local_tz()?)
            .fixed_offset()),
    }
}

/// The typed report spelling the oracle prints: integers as `{"int": "N"}`.
pub fn typed(value: &Value) -> Value {
    careerbrew_projects::typed(value)
}

/// A float in the typed report: `{"float": repr(value)}`.
pub fn float(value: f64) -> Value {
    serde_json::json!({ "float": py::float_repr(value) })
}

/// An integer in the typed report: `{"int": "N"}`.
pub fn int(value: usize) -> Value {
    serde_json::json!({ "int": value.to_string() })
}

pub fn text_of(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    }
}

/// Python truthiness of a stored value.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
    }
}

/// `str(value)` for the stored scalars a title or link holds.
pub fn py_str(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(true)) => "True".into(),
        Some(Value::Bool(false)) => "False".into(),
        Some(other) => other.to_string(),
    }
}
