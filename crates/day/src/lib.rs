//! Component-only day records and sent-PM learning (rust-17, part 1).
//!
//! No production route selects this crate. Mutable state has data-root
//! lifecycle: every record lives in a file under the invocation's data root
//! and nothing is cached in process memory between calls. Lock order matches
//! Python: the canonical sent-day lock, then the PM history or project lock.
//! Sent-Tomorrow promise writes go through the typed `PromiseRecorder` seam.

pub mod difflib;
pub mod handoff;
pub mod pm_learn;
pub mod promises;
pub mod results;
pub mod synthetic;

pub use careerbrew_feed::{FeedError as DayError, Result};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use serde_json::Value;
use std::path::Path;

pub fn value_error(message: impl Into<String>) -> DayError {
    DayError::value(message)
}

pub fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_object().and_then(|map| map.get(key))
}

/// Python `str(value or "")` for JSON scalars that are strings or missing.
pub fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(Value::Bool(false)) => String::new(),
        Some(other) => other.to_string(),
    }
}

/// Python truthiness of a JSON value.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(items)) => !items.is_empty(),
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
    }
}

pub fn store_read_text(root: &Path, relative: &str) -> Result<Option<String>> {
    Ok(careerbrew_feed::context()?
        .store()
        .read_text(&root.join(relative), None, true)?)
}

pub fn store_write_text_if_changed(root: &Path, relative: &str, text: &str) -> Result<bool> {
    Ok(careerbrew_feed::context()?
        .store()
        .write_text_if_changed(&root.join(relative), text)?)
}

/// Python `store.read_json(path)`: None when missing, Err when malformed.
pub fn read_json_optional(root: &Path, relative: &str) -> Result<Option<Value>> {
    if !root.join(relative).exists() {
        return Ok(None);
    }
    Ok(Some(careerbrew_feed::read_json(
        root,
        relative,
        Value::Null,
    )?))
}

pub fn unlink(root: &Path, relative: &str) -> Result<()> {
    match std::fs::remove_file(root.join(relative)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Python `str.splitlines()` line boundaries.
pub fn splitlines(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push(std::mem::take(&mut current));
            }
            '\n' | '\u{0b}' | '\u{0c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}'
            | '\u{2029}' => out.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Python whitespace for `str.split()` and `str.strip()`.
pub fn is_py_space(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

pub fn strip(value: &str) -> &str {
    value.trim_matches(is_py_space)
}

pub fn words(value: &str) -> Vec<&str> {
    value.split(is_py_space).filter(|w| !w.is_empty()).collect()
}

/// Python `str.casefold()` for the characters these records use.
pub fn casefold(value: &str) -> String {
    value.to_lowercase().replace('ß', "ss")
}

/// `datetime.fromisoformat` for the stamp forms careerbrew writes.
pub enum Stamp {
    Aware(DateTime<FixedOffset>),
    Naive(NaiveDateTime),
}

pub fn parse_iso(value: &str) -> Option<Stamp> {
    let value = value.replace('Z', "+00:00");
    if let Ok(date) = NaiveDate::parse_from_str(&value, "%Y-%m-%d") {
        if value.len() == 10 {
            return Some(Stamp::Naive(date.and_time(NaiveTime::MIN)));
        }
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f%:z", "%Y-%m-%dT%H:%M%:z"] {
        if let Ok(stamp) = DateTime::parse_from_str(&value, format) {
            return Some(Stamp::Aware(stamp));
        }
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M"] {
        if let Ok(stamp) = NaiveDateTime::parse_from_str(&value, format) {
            return Some(Stamp::Naive(stamp));
        }
    }
    None
}

/// The local calendar day of a stamp; a naive stamp is local already.
pub fn local_day(value: &str) -> Option<NaiveDate> {
    let zone = careerbrew_feed::local_zone();
    match parse_iso(value)? {
        Stamp::Aware(stamp) => Some(stamp.with_timezone(&zone).date_naive()),
        Stamp::Naive(naive) => careerbrew_memory::memory_sync::local_naive(zone, naive)
            .map(|stamp| stamp.with_timezone(&zone).date_naive()),
    }
}

/// `date.fromisoformat(value)` for canonical `YYYY-MM-DD` days.
pub fn iso_day(value: &str) -> Result<NaiveDate> {
    if value.len() != 10 {
        return Err(value_error(format!("Invalid isoformat string: {value:?}")));
    }
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| value_error(format!("Invalid isoformat string: {value:?}")))
}

pub fn token_hex16() -> Result<String> {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn sha256_hex(value: &str) -> String {
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
