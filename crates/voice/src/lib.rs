//! Component-only model routes, the eval judge adapter and local voice (rust-23).
//!
//! No production route selects this crate. Model calls go through the typed
//! [`models::Transport`] seam; the only network client speaks plain HTTP to a
//! caller-named URL and is exercised against a local fixture server. Practice
//! records go through the [`voice::Practice`] seam, whose default backs onto
//! the rust-20 tutor engine until rust-21 lands the native teacher.

pub mod http;
pub mod judge;
pub mod models;
pub mod shlex;
pub mod synthetic;
pub mod voice;

use serde_json::{Map, Value};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub class: String,
    pub message: String,
}

impl Error {
    pub fn new(class: &str, message: impl Into<String>) -> Self {
        Self {
            class: class.to_owned(),
            message: message.into(),
        }
    }

    pub fn llm(message: impl Into<String>) -> Self {
        Self::new("LLMError", message)
    }

    pub fn value(message: impl Into<String>) -> Self {
        Self::new("ValueError", message)
    }

    /// `KeyError(text)`: `str()` of a KeyError quotes its argument.
    pub fn key(text: &str) -> Self {
        Self::new("KeyError", py_repr_str(text))
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        let class = match error.kind() {
            std::io::ErrorKind::NotFound => "FileNotFoundError",
            std::io::ErrorKind::PermissionDenied => "PermissionError",
            _ => "OSError",
        };
        Self::new(class, "voice file operation failed")
    }
}

impl From<careerbrew_tutor::TutorError> for Error {
    fn from(error: careerbrew_tutor::TutorError) -> Self {
        Self::new(error.class, error.message)
    }
}

impl From<careerbrew_feed::FeedError> for Error {
    fn from(error: careerbrew_feed::FeedError) -> Self {
        Self::new(error.class, error.message)
    }
}

impl From<careerbrew_setup::SetupError> for Error {
    fn from(error: careerbrew_setup::SetupError) -> Self {
        Self::new(error.class, error.message)
    }
}

impl From<careerbrew_policy::allow::PyError> for Error {
    fn from(error: careerbrew_policy::allow::PyError) -> Self {
        Self::new(error.class, error.message)
    }
}

impl From<careerbrew_vault::atomic::StoreError> for Error {
    fn from(_: careerbrew_vault::atomic::StoreError) -> Self {
        Self::new("StoreError", "voice storage operation failed")
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_object().and_then(|map| map.get(key))
}

/// Python truthiness of a JSON-like value.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(items)) => !items.is_empty(),
    }
}

/// `repr()` of a Python str (the common printable cases).
pub fn py_repr_str(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::new();
    out.push(quote);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn py_repr(value: &Value) -> String {
    match value {
        Value::String(text) => py_repr_str(text),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(py_repr).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|(key, item)| format!("{}: {}", py_repr_str(key), py_repr(item)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => py_str(other),
    }
}

/// `str()` of a JSON-decoded Python value.
pub fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        other => py_repr(other),
    }
}

/// The oracle's typed report: integers and floats keep their kind.
pub fn typed(value: &Value) -> Value {
    match value {
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
        other => other.clone(),
    }
}

/// Python `json.dumps(value, sort_keys=True)`.
pub fn dumps(value: &Value) -> String {
    careerbrew_vault::json_yaml::json_compact_python(&careerbrew_setup::value::to_canon(value))
        .expect("JSON values always serialize")
}

/// Python `json.dumps(value, sort_keys=True, ensure_ascii=False)`.
pub fn dumps_unicode(value: &Value) -> String {
    match value {
        Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(dumps_unicode)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(items) => {
            let mut keys = items.keys().collect::<Vec<_>>();
            keys.sort();
            format!(
                "{{{}}}",
                keys.iter()
                    .map(|key| format!(
                        "{}: {}",
                        serde_json::to_string(key).expect("strings serialize"),
                        dumps_unicode(&items[*key])
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        other => {
            if other.is_string() {
                serde_json::to_string(other).expect("strings serialize")
            } else {
                dumps(other)
            }
        }
    }
}

pub fn sha16(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))[..16].to_owned()
}

pub fn sha_full(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// The checkout this component reads its repository data from.
pub fn repo_root() -> PathBuf {
    let raw = std::env::var_os("CAREERBREW_RUST23_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    raw.canonicalize().unwrap_or(raw)
}

/// `paths.data_root(create=False)` without the create side effect.
pub fn data_root() -> Result<PathBuf> {
    match std::env::var("CAREERBREW_HOME") {
        Ok(value) if value.trim().is_empty() => Err(Error::new(
            "RuntimeError",
            "CAREERBREW_HOME is set but empty; unset it to use private/, or point it at a folder",
        )),
        Ok(value) => Ok(expand_user(&value)),
        Err(_) => Ok(repo_root().join("private")),
    }
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// `os.path.expanduser` for `~` and `~/...`.
pub fn expand_user(value: &str) -> PathBuf {
    if value == "~" {
        return home();
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return home().join(rest);
    }
    PathBuf::from(value)
}

/// `str(Path(value))`: repeated and trailing slashes and `.` parts drop.
pub fn pure_path(value: &str) -> String {
    if value.is_empty() {
        return ".".into();
    }
    let absolute = value.starts_with('/');
    let parts = value
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>();
    let joined = parts.join("/");
    match (absolute, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".into(),
        (false, false) => joined,
    }
}

/// `Path.resolve()` (non-strict): the longest existing prefix is
/// canonicalized and the rest is appended lexically.
pub fn resolve(path: &Path) -> std::io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut lexical = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                lexical.pop();
            }
            other => lexical.push(other.as_os_str()),
        }
    }
    let mut existing = lexical.clone();
    let mut tail = Vec::new();
    loop {
        match existing.canonicalize() {
            Ok(real) => {
                let mut out = real;
                for part in tail.iter().rev() {
                    out.push(part);
                }
                return Ok(out);
            }
            Err(_) => {
                let Some(name) = existing.file_name().map(|name| name.to_os_string()) else {
                    return Ok(lexical);
                };
                tail.push(name);
                if !existing.pop() {
                    return Ok(lexical);
                }
            }
        }
    }
}

/// Replace invocation-owned paths and temp names with the oracle's fixed
/// placeholders, in every string and key.
pub fn clean_report(value: &mut Value, owned: &str, temp: &str) {
    match value {
        Value::String(text) => *text = clean_text(text, owned, temp),
        Value::Array(items) => {
            for item in items {
                clean_report(item, owned, temp);
            }
        }
        Value::Object(items) => {
            let old = std::mem::take(items);
            let mut out = Map::new();
            for (key, mut item) in old {
                clean_report(&mut item, owned, temp);
                out.insert(clean_text(&key, owned, temp), item);
            }
            *items = out;
        }
        _ => {}
    }
}

pub fn clean_text(text: &str, owned: &str, temp: &str) -> String {
    let mut out = text.to_owned();
    if !owned.is_empty() {
        out = out.replace(owned, "<data>");
    }
    if !temp.is_empty() {
        out = out.replace(temp, "<tmp>");
    }
    let pattern = regex::Regex::new(r"careerbrew-voice-[A-Za-z0-9_]+").expect("valid pattern");
    pattern
        .replace_all(&out, "careerbrew-voice-<id>")
        .into_owned()
}
