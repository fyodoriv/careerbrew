//! Component-only content-free report, metric and export builders (rust-24).
//!
//! No production route selects this crate. It ports the public repo health
//! export and its allowlist, the telemetry event allowlist, time saved, field
//! totals, the inline-SVG chart builder and the Getting started checklist.
//! Event rows reuse `careerbrew_vault::events` and leak checks reuse
//! `careerbrew_policy::find_leaks`; neither is written twice here.
//!
//! Mutable state: none at module scope. Every reader takes the invocation's
//! data root from `CAREERBREW_HOME`, and every function is invocation-scoped.

pub mod allowlists;
pub mod charts;
pub mod checklist;
pub mod field;
pub mod repo_health;
pub mod synthetic;
pub mod telemetry;
pub mod time_saved;

use careerbrew_vault::canon::Canon;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::{Number, Value};
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, String>;

/// The careerbrew version, from the same `pyproject.toml` Python reads.
const PYPROJECT: &str = include_str!("../../../pyproject.toml");

pub fn version() -> String {
    PYPROJECT
        .lines()
        .skip_while(|line| line.trim() != "[project]")
        .find_map(|line| {
            let rest = line.trim().strip_prefix("version")?.trim_start();
            let rest = rest.strip_prefix('=')?.trim();
            Some(rest.trim_matches('"').to_owned())
        })
        .unwrap_or_default()
}

pub fn context() -> Result<careerbrew_vault::transfer::Context> {
    careerbrew_vault::transfer::Context::from_env().map_err(|error| error.message)
}

pub fn data_root() -> Result<PathBuf> {
    Ok(context()?.root)
}

/// Python `repr(float)`.
pub fn py_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.into();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let text = format!("{x:e}");
    let (sign, text) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text.as_str()),
    };
    let (mantissa, exponent) = text.split_once('e').expect("LowerExp has an exponent");
    let exponent: i32 = exponent.parse().expect("LowerExp exponent is an integer");
    let digits = mantissa.replace('.', "");
    if (-4..16).contains(&exponent) {
        if exponent >= 0 {
            let point = exponent as usize + 1;
            let (int, frac) = if digits.len() > point {
                (digits[..point].to_owned(), digits[point..].to_owned())
            } else {
                (
                    format!("{digits}{}", "0".repeat(point - digits.len())),
                    "0".to_owned(),
                )
            };
            format!("{sign}{int}.{frac}")
        } else {
            format!("{sign}0.{}{digits}", "0".repeat((-exponent - 1) as usize))
        }
    } else {
        let mantissa = if digits.len() > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits
        };
        let esign = if exponent < 0 { '-' } else { '+' };
        format!("{sign}{mantissa}e{esign}{:02}", exponent.abs())
    }
}

/// Python `round(x, n)` for a float.
pub fn py_round(x: f64, n: usize) -> f64 {
    format!("{x:.n$}").parse().unwrap_or(x)
}

/// A JSON float with Python's spelling kept in the number text.
pub fn float(x: f64) -> Value {
    serde_json::from_str::<Number>(&py_repr(x)).map_or(Value::Null, Value::Number)
}

pub fn int(n: i64) -> Value {
    Value::Number(n.into())
}

/// Whether a JSON number came from a Python float (it has a point or exponent).
pub fn is_float(number: &Number) -> bool {
    let text = number.to_string();
    text.contains(['.', 'e', 'E'])
}

pub fn is_int(value: &Value) -> bool {
    matches!(value, Value::Number(n) if !is_float(n))
}

/// A Python `int | float` (a bool counts too, as `isinstance(True, int)`).
pub fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Bool(flag) => Some(f64::from(u8::from(*flag))),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// Python truthiness for a JSON-shaped value.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(items)) => !items.is_empty(),
    }
}

/// The oracle's typed report value: ints and floats keep their Python kind.
pub fn typed(value: &Value) -> Value {
    match value {
        Value::Number(n) if is_float(n) => {
            serde_json::json!({"float": py_repr(n.as_f64().unwrap_or(f64::NAN))})
        }
        Value::Number(n) => serde_json::json!({"int": n.to_string()}),
        Value::Array(items) => Value::Array(items.iter().map(typed).collect()),
        Value::Object(items) => Value::Object(
            items
                .iter()
                .map(|(key, item)| (key.clone(), typed(item)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `json.dumps(typed(value), ensure_ascii=False, separators=(",", ":"), sort_keys=True)`.
pub fn compact(value: &Value) -> String {
    serde_json::to_string(&typed(value)).unwrap_or_default() + "\n"
}

/// `json.dumps(value, indent=1, sort_keys=True)` for ASCII values.
pub fn pretty(value: &Value) -> String {
    use serde::Serialize;
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    value
        .serialize(&mut serializer)
        .expect("JSON values serialize");
    String::from_utf8(out).unwrap_or_default()
}

/// Python's `repr` of a list of strings, as `sorted(KEYS)` prints.
pub fn py_list(items: &[&str]) -> String {
    let inner = items
        .iter()
        .map(|item| format!("'{item}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{inner}]")
}

/// The saved settings, through the native storage provider.
pub fn settings() -> Result<Value> {
    let context = context()?;
    let value = careerbrew_vault::settings::read_settings(&context.store())
        .map_err(|_| "cannot read settings".to_owned())?;
    Ok(careerbrew_setup::value::from_canon(&value))
}

pub fn setting<'a>(settings: &'a Value, key: &str) -> Option<&'a Value> {
    settings.get(key)
}

/// `calendar.system_tz`: `TZ`, else the /etc/localtime link, else UTC.
fn system_tz() -> String {
    let mut name = std::env::var("TZ")
        .unwrap_or_default()
        .trim_start_matches(':')
        .to_owned();
    if name.is_empty() {
        name = std::fs::canonicalize("/etc/localtime")
            .ok()
            .and_then(|link| {
                let link = link.to_string_lossy().into_owned();
                link.split_once("/zoneinfo/")
                    .map(|(_, zone)| zone.to_owned())
            })
            .unwrap_or_default();
    }
    if name.parse::<Tz>().is_ok() {
        name
    } else {
        "UTC".into()
    }
}

/// `calendar.local_tz()`: the `timezone` setting, else the machine's zone.
pub fn local_tz() -> Result<Tz> {
    let settings = settings()?;
    let name = match settings.get("timezone") {
        Some(value) if truthy(Some(value)) => value.as_str().unwrap_or_default().to_owned(),
        _ => system_tz(),
    };
    name.parse::<Tz>()
        .map_err(|_| format!("unknown time zone {name}"))
}

/// Python `datetime.fromisoformat` for the forms careerbrew writes: a date,
/// or a date, one separator and `HH[:MM[:SS[.ffffff]]]`, with an optional
/// `+HH:MM` offset. The offset is None for a naive value.
pub fn from_iso(text: &str) -> Option<(NaiveDateTime, Option<FixedOffset>)> {
    let date = |part: &str| {
        let bytes = part.as_bytes();
        (part.len() == 10
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && part
                .bytes()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit()))
        .then(|| NaiveDate::parse_from_str(part, "%Y-%m-%d").ok())
        .flatten()
    };
    if text.len() == 10 {
        return date(text).map(|day| (day.and_time(NaiveTime::MIN), None));
    }
    if text.len() < 13 || !text.is_char_boundary(10) || !text.is_char_boundary(11) {
        return None;
    }
    let day = date(&text[..10])?;
    let rest = &text[11..];
    let (clock, offset) = match rest.find(['+', '-']) {
        Some(index) => (&rest[..index], Some(&rest[index..])),
        None => (rest, None),
    };
    let offset = match offset {
        None => None,
        Some(value) => {
            let sign = if value.starts_with('-') { -1 } else { 1 };
            let body = &value[1..];
            let (hours, minutes) = match body.len() {
                5 if body.as_bytes()[2] == b':' => (&body[..2], &body[3..]),
                4 => (&body[..2], &body[2..]),
                _ => return None,
            };
            if !hours
                .bytes()
                .chain(minutes.bytes())
                .all(|b| b.is_ascii_digit())
            {
                return None;
            }
            let seconds = hours.parse::<i32>().ok()? * 3600 + minutes.parse::<i32>().ok()? * 60;
            if seconds >= 86400 {
                return None;
            }
            Some(FixedOffset::east_opt(sign * seconds)?)
        }
    };
    let (clock, fraction) = match clock.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (clock, None),
    };
    let parts = clock.split(':').collect::<Vec<_>>();
    if parts.is_empty()
        || parts.len() > 3
        || parts
            .iter()
            .any(|part| part.len() != 2 || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    if fraction.is_some() && parts.len() != 3 {
        return None;
    }
    let nanos = match fraction {
        None => 0,
        Some(digits) => {
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let kept = &digits[..digits.len().min(6)];
            let padded = format!("{kept:0<9}");
            padded.parse::<u32>().ok()?
        }
    };
    let number = |index: usize| {
        parts
            .get(index)
            .map_or(Some(0), |part| part.parse::<u32>().ok())
    };
    let time = NaiveTime::from_hms_nano_opt(number(0)?, number(1)?, number(2)?, nanos)?;
    Some((day.and_time(time), offset))
}

/// `calendar.now()`: the pinned `CAREERBREW_NOW` (a naive value takes the
/// local zone), else the wall clock in the local zone.
pub fn now() -> Result<DateTime<FixedOffset>> {
    let tz = local_tz()?;
    match std::env::var("CAREERBREW_NOW") {
        Ok(pinned) if !pinned.is_empty() => {
            let (naive, offset) = from_iso(&pinned).ok_or("CAREERBREW_NOW is not an ISO time")?;
            Ok(match offset {
                Some(offset) => offset
                    .from_local_datetime(&naive)
                    .single()
                    .ok_or("invalid pinned time")?,
                None => tz
                    .from_local_datetime(&naive)
                    .earliest()
                    .ok_or("pinned time is not in the local zone")?
                    .fixed_offset(),
            })
        }
        _ => Ok(Utc::now().with_timezone(&tz).fixed_offset()),
    }
}

/// `Path.resolve()` (non-strict): the longest existing prefix is made canonical.
pub fn resolve(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut existing = absolute.clone();
    let mut rest = Vec::new();
    loop {
        if let Ok(canonical) = existing.canonicalize() {
            let mut out = canonical;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (
            existing.file_name().map(ToOwned::to_owned),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return absolute,
        }
    }
}

/// Convert JSON into the vault value codec, for the shared event cleaner.
pub fn canon(value: &Value) -> Canon {
    careerbrew_setup::value::to_canon(value)
}

pub fn from_canon(value: &Canon) -> Value {
    careerbrew_setup::value::from_canon(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repr_and_round_follow_python() {
        for (value, expected) in [
            (0.5, "0.5"),
            (1.0, "1.0"),
            (12.5, "12.5"),
            (30.36, "30.36"),
            (1e16, "1e+16"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (123456789.0, "123456789.0"),
            (-2.5, "-2.5"),
        ] {
            assert_eq!(py_repr(value), expected);
        }
        assert_eq!(py_round(0.25, 1), 0.2);
        assert_eq!(py_round(2.675, 2), 2.67);
    }

    #[test]
    fn iso_times_follow_python_fromisoformat() {
        assert!(from_iso("2026-10-19").is_some());
        assert!(from_iso("2026-10-19T01:15").is_some());
        assert!(from_iso("2026-10-18 09:00:00").is_some());
        assert!(from_iso("2026-10-18T09:00:00-04:00").unwrap().1.is_some());
        assert!(from_iso("2026-02-30T09:00:00").is_none());
        assert!(from_iso("bad").is_none());
    }
}
