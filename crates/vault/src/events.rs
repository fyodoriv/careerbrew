//! Content-free JSONL event rows. The invocation supplies its data root,
//! clock, version and environment switch; no production dispatch uses this.

use crate::atomic::StoreError;
use crate::canon::Canon;
use crate::json_yaml::json_line_with_float;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

const ALLOWED_STRING_KEYS: &[&str] = &[
    "event",
    "story",
    "command",
    "status",
    "route",
    "action",
    "source_kind",
    "tier",
    "error",
    "saves",
    "step",
    "step_kind",
    "from_version",
    "error_class",
    "via",
    "signin",
    "source",
    "recheck",
];

pub trait Clock {
    fn stamp(&self) -> String;
    fn version(&self) -> String;
}

pub fn log_path(root: &Path) -> PathBuf {
    root.join("logs/events.jsonl")
}

fn identifier(value: &str) -> bool {
    // Python's `$` also matches immediately before one terminal newline.
    // Keep the original value when that boundary matches.
    let matched = value.strip_suffix('\n').unwrap_or(value);
    (1..=48).contains(&matched.len())
        && matched.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'_' | b'.' | b':' | b'-')
        })
}

fn python_event_float(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value == f64::INFINITY {
        return "Infinity".into();
    }
    if value == f64::NEG_INFINITY {
        return "-Infinity".into();
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if value == 0.0 {
        return format!("{sign}0.0");
    }

    // Serde/Ryu supplies the shortest round-tripping digits. Re-layout those
    // digits with Python repr's fixed/scientific thresholds and exponent pad.
    let text = serde_json::Number::from_f64(value)
        .expect("finite floats have a JSON number")
        .to_string();
    let unsigned = text.strip_prefix('-').unwrap_or(&text);
    let exponent_at = unsigned.find(['e', 'E']);
    let (mantissa, exponent) = match exponent_at {
        Some(index) => (
            &unsigned[..index],
            unsigned[index + 1..]
                .parse::<i32>()
                .expect("Serde float exponents are integers"),
        ),
        None => (unsigned, 0),
    };
    let point = mantissa.find('.').unwrap_or(mantissa.len());
    let raw = mantissa.replace('.', "");
    let leading = raw.bytes().take_while(|byte| *byte == b'0').count();
    let digits = raw[leading..].trim_end_matches('0');
    let decimal_exponent = point as i32 + exponent - leading as i32 - 1;

    if !(-4..16).contains(&decimal_exponent) {
        let mut out = String::from(sign);
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let exponent_sign = if decimal_exponent < 0 { '-' } else { '+' };
        out.push_str(&format!("e{exponent_sign}{:02}", decimal_exponent.abs()));
        return out;
    }

    let mut out = String::from(sign);
    if decimal_exponent < 0 {
        out.push_str("0.");
        out.push_str(&"0".repeat((-decimal_exponent - 1) as usize));
        out.push_str(digits);
        return out;
    }
    let integer_digits = (decimal_exponent + 1) as usize;
    if digits.len() <= integer_digits {
        out.push_str(digits);
        out.push_str(&"0".repeat(integer_digits - digits.len()));
        out.push_str(".0");
    } else {
        out.push_str(&digits[..integer_digits]);
        out.push('.');
        out.push_str(&digits[integer_digits..]);
    }
    out
}

fn event_json_float(value: f64) -> Result<String, String> {
    Ok(python_event_float(value))
}

pub fn clean(fields: &Canon) -> Result<Canon, StoreError> {
    let Canon::Map(items) = fields else {
        return Err(StoreError::Codec("event-fields"));
    };
    let mut out = Vec::new();
    for (key, value) in items {
        let Canon::Str(key) = key else {
            return Err(StoreError::Codec("event-key"));
        };
        let value = if key == "error" {
            match value {
                Canon::Str(value) => Canon::Str(value.to_lowercase()),
                value => value.clone(),
            }
        } else {
            value.clone()
        };
        let keep = match &value {
            Canon::Bool(_) | Canon::Int(_) | Canon::Float(_) => true,
            Canon::Str(value) => ALLOWED_STRING_KEYS.contains(&key.as_str()) && identifier(value),
            _ => false,
        };
        if keep {
            out.push((Canon::Str(key.clone()), value));
        }
    }
    Ok(Canon::Map(out))
}

fn insert(items: &mut Vec<(Canon, Canon)>, key: &str, value: Canon) {
    if let Some((_, old)) = items
        .iter_mut()
        .find(|(name, _)| matches!(name, Canon::Str(name) if name == key))
    {
        *old = value;
    } else {
        items.push((Canon::Str(key.into()), value));
    }
}

pub fn record(
    event: &str,
    fields: &Canon,
    stamp: &str,
    version: &str,
) -> Result<Canon, StoreError> {
    let Canon::Map(input) = fields else {
        return Err(StoreError::Codec("event-fields"));
    };
    let mut with_event = Vec::with_capacity(input.len() + 1);
    with_event.push((Canon::Str("event".into()), Canon::Str(event.into())));
    with_event.extend(input.iter().cloned());
    let Canon::Map(cleaned) = clean(&Canon::Map(with_event))? else {
        unreachable!()
    };
    let mut row = vec![
        (Canon::Str("ts".into()), Canon::Str(stamp.into())),
        (Canon::Str("v".into()), Canon::Str(version.into())),
    ];
    for (key, value) in cleaned {
        let Canon::Str(key) = key else {
            return Err(StoreError::Codec("event-key"));
        };
        insert(&mut row, &key, value);
    }
    Ok(Canon::Map(row))
}

/// Append one sorted, unencrypted JSON line without a storage lock, like the
/// Python provider. Event rows are plaintext by design.
pub fn write_row(root: &Path, row: &Canon) -> Result<(), StoreError> {
    let path = log_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().append(true).create(true).open(path)?;
    file.write_all(
        json_line_with_float(row, event_json_float)
            .map_err(|_| StoreError::Codec("jsonl"))?
            .as_bytes(),
    )?;
    Ok(())
}

pub fn emit(
    root: &Path,
    events_env: Option<&str>,
    event: &str,
    fields: &Canon,
    clock: &dyn Clock,
) -> Result<(), StoreError> {
    if events_env == Some("off") {
        return Ok(());
    }
    let row = record(event, fields, &clock.stamp(), &clock.version())?;
    write_row(root, &row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_keep_python_terminal_newline_boundary() {
        assert!(identifier(&"a".repeat(48)));
        assert!(!identifier(&"a".repeat(49)));
        assert!(identifier("ok\n"));
        assert!(!identifier("ok\n\n"));
    }

    #[test]
    fn event_floats_use_python_json_spellings() {
        for (value, expected) in [
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (-0.0, "-0.0"),
            (1e-7, "1e-07"),
            (1e-4, "0.0001"),
            (1e-5, "1e-05"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (1e20, "1e+20"),
        ] {
            assert_eq!(python_event_float(value), expected);
        }
    }
}
