//! Component-only work-day execution: step proofs, timeouts and retries,
//! per-step timing, the one watch loop with its pass lock, turn-start
//! catch-up and the movable graceful wrap-up (rust-16).
//!
//! No production route selects this crate. Mutable state has data-root
//! lifecycle: the loop store `day/loops.json` changes only under
//! `day/.loops.json.lock`, a pass holds `day/.loop.lock` for its whole run,
//! and the day file changes only under its canonical day lock. Nothing is
//! kept in process memory between calls.

pub mod cron_check;
pub mod end;
pub mod gate;
pub mod synthetic;
pub mod timing;
pub mod watch;

pub use careerbrew_projects::{ProjectError, Result};
pub use careerbrew_workday::{field, read_json, text, typed, write_json};

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use serde_json::{json, Value};
use std::path::Path;

pub fn now() -> DateTime<FixedOffset> {
    careerbrew_workday::now()
}

pub fn today() -> NaiveDate {
    now().date_naive()
}

pub fn iso_day(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

/// Python's `datetime.isoformat(timespec="minutes")`.
pub fn iso_minutes(at: &DateTime<FixedOffset>) -> String {
    at.format("%Y-%m-%dT%H:%M%:z").to_string()
}

/// Python's `datetime.isoformat(timespec="seconds")`.
pub fn iso_seconds(at: &DateTime<FixedOffset>) -> String {
    at.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

/// Python's `datetime.isoformat(timespec="microseconds")`.
pub fn iso_micros(at: &DateTime<FixedOffset>) -> String {
    at.format("%Y-%m-%dT%H:%M:%S%.6f%:z").to_string()
}

/// Python's default `datetime.isoformat()`: microseconds only when nonzero.
pub fn iso_auto(at: &DateTime<FixedOffset>) -> String {
    if at.nanosecond() / 1000 == 0 {
        iso_seconds(at)
    } else {
        iso_micros(at)
    }
}

/// A parsed `datetime.fromisoformat` value: aware or naive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stamp {
    Aware(DateTime<FixedOffset>),
    Naive(NaiveDateTime),
}

impl Stamp {
    /// The aware value, attaching `tz` to a naive one (Python's `replace(tzinfo=...)`).
    pub fn with_zone(self, tz: FixedOffset) -> Option<DateTime<FixedOffset>> {
        match self {
            Stamp::Aware(at) => Some(at),
            Stamp::Naive(naive) => naive.and_local_timezone(tz).single(),
        }
    }
}

fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
}

fn parse_offset(value: &str) -> Option<FixedOffset> {
    if value == "Z" {
        return FixedOffset::east_opt(0);
    }
    let sign = match value.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let rest = &value[1..];
    let parts: Vec<&str> = if rest.contains(':') {
        rest.split(':').collect()
    } else if rest.len() == 2 || rest.len() == 4 || rest.len() == 6 {
        rest.as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap_or(""))
            .collect()
    } else {
        return None;
    };
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| p.len() != 2 || !digits(p)) {
        return None;
    }
    let hours: i32 = parts[0].parse().ok()?;
    let minutes: i32 = parts.get(1).map_or(Some(0), |p| p.parse().ok())?;
    let seconds: i32 = parts.get(2).map_or(Some(0), |p| p.parse().ok())?;
    if hours > 23 || minutes > 59 || seconds > 59 {
        return None;
    }
    FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60 + seconds))
}

fn parse_time(value: &str) -> Option<NaiveTime> {
    let (clock, fraction) = match value.split_once(['.', ',']) {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (value, None),
    };
    let parts: Vec<&str> = clock.split(':').collect();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| p.len() != 2 || !digits(p)) {
        return None;
    }
    if fraction.is_some() && parts.len() != 3 {
        return None;
    }
    let hour: u32 = parts[0].parse().ok()?;
    let minute: u32 = parts.get(1).map_or(Some(0), |p| p.parse().ok())?;
    let second: u32 = parts.get(2).map_or(Some(0), |p| p.parse().ok())?;
    let micro = match fraction {
        None => 0,
        Some(f) if digits(f) => {
            let mut f = f.chars().take(6).collect::<String>();
            while f.len() < 6 {
                f.push('0');
            }
            f.parse().ok()?
        }
        Some(_) => return None,
    };
    NaiveTime::from_hms_micro_opt(hour, minute, second, micro)
}

/// Python's `date.fromisoformat` for the extended `YYYY-MM-DD` form.
pub fn parse_date(value: &str) -> Option<NaiveDate> {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if !digits(&value[0..4]) || !digits(&value[5..7]) || !digits(&value[8..10]) {
        return None;
    }
    NaiveDate::from_ymd_opt(
        value[0..4].parse().ok()?,
        value[5..7].parse().ok()?,
        value[8..10].parse().ok()?,
    )
}

/// Python's `datetime.fromisoformat` for the extended forms careerbrew writes.
pub fn parse_iso(value: &str) -> Option<Stamp> {
    if !value.is_ascii() || value.len() < 10 {
        return None;
    }
    let day = parse_date(&value[..10])?;
    if value.len() == 10 {
        return Some(Stamp::Naive(day.and_hms_opt(0, 0, 0)?));
    }
    let rest = &value[11..];
    let split = rest.find(['+', '-', 'Z']).unwrap_or(rest.len());
    let time = parse_time(&rest[..split])?;
    let naive = day.and_time(time);
    if split == rest.len() {
        return Some(Stamp::Naive(naive));
    }
    let offset = parse_offset(&rest[split..])?;
    naive.and_local_timezone(offset).single().map(Stamp::Aware)
}

/// A setting from `settings.yaml`, or its default.
pub fn setting_text(root: &Path, key: &str, default: &str) -> Result<String> {
    let settings = careerbrew_workday::read_yaml(root, "settings.yaml", json!({}))?;
    Ok(match field(&settings, key) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Null) | None => default.to_owned(),
        Some(other) => other.to_string(),
    })
}

pub fn setting_int(root: &Path, key: &str, default: i64) -> Result<i64> {
    careerbrew_workday::setting(root, key, default)
}

/// The day file of `day`, like Python's `status._data`.
pub fn day_data(root: &Path, day: NaiveDate) -> Result<Value> {
    let data = read_json(root, &day_relative(day), json!({}))?;
    Ok(if data.is_object() { data } else { json!({}) })
}

pub fn day_relative(day: NaiveDate) -> String {
    format!("day/work-{}.json", iso_day(day))
}

/// Run `f` under the canonical day-file lock (`.work-<day>.json.lock`).
pub fn with_day_lock<T>(root: &Path, day: NaiveDate, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let path = root.join(day_relative(day));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    careerbrew_vault::lock::with_lock(&path, f)?
}

pub fn error(class: &'static str, message: impl Into<String>) -> ProjectError {
    ProjectError::new(class, message)
}

pub fn value_error(message: impl Into<String>) -> ProjectError {
    error("ValueError", message)
}

pub fn emit(root: &Path, event: &str, fields: Value) -> Result<()> {
    careerbrew_workday::emit(root, event, fields)
}

pub fn digest16(value: &str) -> String {
    let mut out = careerbrew_workday::digest(value);
    out.truncate(16);
    out
}

/// Python's `round(x)` for a nonnegative ratio `numerator / denominator`: halves go to the even neighbour.
pub fn round_half_even(numerator: i64, denominator: i64) -> i64 {
    let quotient = numerator.div_euclid(denominator);
    let twice = 2 * numerator.rem_euclid(denominator);
    if twice > denominator || (twice == denominator && quotient % 2 == 1) {
        quotient + 1
    } else {
        quotient
    }
}

pub fn minutes(value: i64) -> Duration {
    Duration::minutes(value)
}
