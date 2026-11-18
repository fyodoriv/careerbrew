//! Set `hash`: the `changes._h` input hash primitive, reused by rust-24.
//!
//! Python: `sha256(json.dumps(obj, sort_keys=True, default=str).encode())
//! .hexdigest()[:16]`. The digest is the `sha2` crate. The text follows
//! Python's `json.dumps` defaults: `", "` and `": "` separators,
//! `ensure_ascii` escapes (UTF-16 surrogate pairs above the BMP), float
//! `repr`, `NaN` and `Infinity`, keys sorted by Python's own ordering before
//! they become strings, and a `TypeError` for keys that cannot be compared or
//! are not str, int, float, bool or None. `default=str` covers dates,
//! datetimes and paths.
//!
//! The input is a typed [`PyValue`], not a JSON value, so an int against a
//! float, a big int and a non-string key stay visible.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fmt::Write as _;

/// Python's default limit on the digits of an int it turns into text
/// (`sys.get_int_max_str_digits()`).
const INT_MAX_STR_DIGITS: usize = 4300;

/// A Python value as `json.dumps` sees it.
#[derive(Clone, Debug)]
pub enum PyValue {
    None,
    Bool(bool),
    /// Normalized decimal digits, with a leading `-` when negative.
    Int(String),
    Float(f64),
    Str(String),
    /// A leaf that `default=str` turns into this text (date, datetime, path).
    Text(String),
    List(Vec<PyValue>),
    /// The pairs of a dict, in insertion order, with equal keys merged.
    Map(Vec<(PyValue, PyValue)>),
}

/// Why a value cannot be hashed (Python raises).
#[derive(Debug, PartialEq)]
pub enum HashError {
    /// Keys of types that do not compare, or a key type JSON refuses.
    Type,
    /// An int with more digits than Python turns into text.
    Value,
}

/// The 16 hex digit `changes._h` digest of `value`.
pub fn input_hash(value: &PyValue) -> Result<String, HashError> {
    Ok(digest(&dumps(value)?))
}

/// `json.dumps(value, sort_keys=True, default=str)`.
pub fn dumps(value: &PyValue) -> Result<String, HashError> {
    let mut out = String::new();
    encode(value, &mut out)?;
    Ok(out)
}

fn digest(text: &str) -> String {
    let hex = format!("{:x}", Sha256::digest(text.as_bytes()));
    hex[..16].to_owned()
}

fn encode(value: &PyValue, out: &mut String) -> Result<(), HashError> {
    match value {
        PyValue::None => out.push_str("null"),
        PyValue::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
        PyValue::Int(digits) => out.push_str(int_text(digits)?),
        PyValue::Float(v) => out.push_str(&float_text(*v)),
        PyValue::Str(v) | PyValue::Text(v) => string(v, out),
        PyValue::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                encode(item, out)?;
            }
            out.push(']');
        }
        PyValue::Map(pairs) => {
            let mut sorted: Vec<&(PyValue, PyValue)> = pairs.iter().collect();
            let mut failed = false;
            for (key, _) in &sorted {
                if !matches!(
                    key,
                    PyValue::None
                        | PyValue::Bool(_)
                        | PyValue::Int(_)
                        | PyValue::Float(_)
                        | PyValue::Str(_)
                ) {
                    failed = true;
                }
            }
            // Python sorts first, then checks each key type as it writes.
            for pair in sorted.windows(2) {
                if compare(&pair[0].0, &pair[1].0).is_none() {
                    failed = true;
                }
            }
            if failed {
                return Err(HashError::Type);
            }
            sorted.sort_by(|a, b| compare(&a.0, &b.0).unwrap_or(Ordering::Equal));
            out.push('{');
            for (i, (key, item)) in sorted.into_iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let key = match key {
                    PyValue::Str(v) => v.clone(),
                    PyValue::None => "null".to_owned(),
                    PyValue::Bool(v) => (if *v { "true" } else { "false" }).to_owned(),
                    PyValue::Int(digits) => int_text(digits)?.to_owned(),
                    PyValue::Float(v) => float_text(*v),
                    _ => return Err(HashError::Type),
                };
                string(&key, out);
                out.push_str(": ");
                encode(item, out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

fn int_text(digits: &str) -> Result<&str, HashError> {
    if digits.trim_start_matches('-').len() > INT_MAX_STR_DIGITS {
        return Err(HashError::Value);
    }
    Ok(digits)
}

/// `json.dumps` for a float: `float.__repr__`, or `NaN`, `Infinity`,
/// `-Infinity`. Rust's `{:e}` gives the same shortest round-trip digits as
/// Python's `repr`; this only lays them out the way Python does: fixed
/// notation while the decimal point position is in -3..=16, else
/// `d.ddde+XX` with at least two exponent digits.
pub fn float_text(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return (if v > 0.0 { "Infinity" } else { "-Infinity" }).into();
    }
    let sci = format!("{:e}", v.abs());
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let point = exponent + 1;
    let mut out = String::new();
    if v.is_sign_negative() {
        out.push('-');
    }
    let n = digits.len() as i32;
    if (-3..=16).contains(&point) {
        if point <= 0 {
            out.push_str("0.");
            out.push_str(&"0".repeat((-point) as usize));
            out.push_str(&digits);
        } else if point < n {
            out.push_str(&digits[..point as usize]);
            out.push('.');
            out.push_str(&digits[point as usize..]);
        } else {
            out.push_str(&digits);
            out.push_str(&"0".repeat((point - n) as usize));
            out.push_str(".0");
        }
    } else {
        out.push_str(&digits[..1]);
        if n > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let e = point - 1;
        let sign = if e < 0 { '-' } else { '+' };
        write!(out, "e{sign}{:02}", e.abs()).unwrap();
    }
    out
}

/// A string with `ensure_ascii`: `"`, `\`, `\n`, `\r`, `\t`, `\b`, `\f`
/// get short escapes; anything else outside `' '..='~'` gets `\uXXXX`
/// (lowercase hex, a surrogate pair above the BMP).
fn string(value: &str, out: &mut String) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(ch),
            _ => {
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    write!(out, "\\u{unit:04x}").unwrap();
                }
            }
        }
    }
    out.push('"');
}

/// A number key as Python orders it: bools are 0 and 1.
enum Num<'a> {
    Int(&'a str),
    Float(f64),
}

fn num(value: &PyValue) -> Option<Num<'_>> {
    match value {
        PyValue::Bool(false) => Some(Num::Int("0")),
        PyValue::Bool(true) => Some(Num::Int("1")),
        PyValue::Int(digits) => Some(Num::Int(digits)),
        PyValue::Float(v) => Some(Num::Float(*v)),
        _ => None,
    }
}

/// Python's `<` between two dict keys: `None` when Python raises
/// `TypeError` (str against a number, `None` against anything) or the order
/// is undefined (`NaN`).
fn compare(a: &PyValue, b: &PyValue) -> Option<Ordering> {
    match (a, b) {
        (PyValue::Str(x), PyValue::Str(y)) => Some(x.cmp(y)),
        _ => match (num(a)?, num(b)?) {
            (Num::Int(x), Num::Int(y)) => Some(int_cmp(x, y)),
            (Num::Float(x), Num::Float(y)) => x.partial_cmp(&y),
            (Num::Int(x), Num::Float(y)) => int_float_cmp(x, y),
            (Num::Float(x), Num::Int(y)) => int_float_cmp(y, x).map(Ordering::reverse),
        },
    }
}

/// Two normalized decimal ints.
fn int_cmp(x: &str, y: &str) -> Ordering {
    let (xn, xd) = x.strip_prefix('-').map_or((false, x), |d| (true, d));
    let (yn, yd) = y.strip_prefix('-').map_or((false, y), |d| (true, d));
    let magnitude = xd.len().cmp(&yd.len()).then_with(|| xd.cmp(yd));
    match (xn, yn) {
        (false, false) => magnitude,
        (true, true) => magnitude.reverse(),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
    }
}

/// An int against a float, exactly, as Python compares them.
fn int_float_cmp(x: &str, y: f64) -> Option<Ordering> {
    if y.is_nan() {
        return None;
    }
    if y.is_infinite() {
        return Some(if y > 0.0 {
            Ordering::Less
        } else {
            Ordering::Greater
        });
    }
    // `{:.0}` of an integral float prints its exact digits.
    let whole = y.trunc();
    let digits = format!("{whole:.0}");
    let digits = if digits == "-0" {
        "0".to_owned()
    } else {
        digits
    };
    Some(int_cmp(x, &digits).then_with(|| {
        let rest = y - whole;
        if rest > 0.0 {
            Ordering::Less
        } else if rest < 0.0 {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }))
}

/// Normalized decimal digits for an int, or `None` for text Python's
/// `int()` refuses here (only ASCII digits and one leading sign).
fn int_digits(text: &str) -> Option<String> {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if rest.len() > INT_MAX_STR_DIGITS {
        return None;
    }
    let trimmed = rest.trim_start_matches('0');
    if trimmed.is_empty() {
        return Some("0".into());
    }
    Some(if negative {
        format!("-{trimmed}")
    } else {
        trimmed.to_owned()
    })
}

/// `str(PurePosixPath(text))`: empty and `.` parts dropped, a trailing
/// slash dropped, exactly two leading slashes kept, `.` when nothing is left.
fn path_text(text: &str) -> String {
    let lead = if text.starts_with("//") && !text.starts_with("///") {
        "//"
    } else if text.starts_with('/') {
        "/"
    } else {
        ""
    };
    let parts: Vec<&str> = text
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    if lead.is_empty() && parts.is_empty() {
        return ".".into();
    }
    format!("{lead}{}", parts.join("/"))
}

/// Python dict equality for keys: numbers by value, strings by text.
fn same_key(a: &PyValue, b: &PyValue) -> bool {
    match (a, b) {
        (PyValue::Str(x), PyValue::Str(y)) => x == y,
        (PyValue::None, PyValue::None) => true,
        _ => compare(a, b) == Some(Ordering::Equal) && num(a).is_some() && num(b).is_some(),
    }
}

/// Reads one typed input value (the `canon.rs` form, plus `tuple` and
/// `path`). Text with no stable Python reading is refused.
pub fn typed(value: &Value) -> Result<PyValue, String> {
    let v = &value["v"];
    let text = || v.as_str().ok_or_else(|| "not text".to_owned());
    Ok(match value["t"].as_str().ok_or("no type")? {
        "null" => PyValue::None,
        "bool" => PyValue::Bool(v.as_bool().ok_or("not a bool")?),
        "str" => PyValue::Str(text()?.to_owned()),
        "int" => PyValue::Int(int_digits(text()?).ok_or("not an int")?),
        "float" => {
            let bits = u64::from_str_radix(text()?, 16).map_err(|_| "not float bits")?;
            PyValue::Float(f64::from_bits(bits))
        }
        // `default=str` of a date or datetime is its `isoformat` with a
        // space; the vectors give the canonical `isoformat` text.
        "date" => PyValue::Text(text()?.to_owned()),
        "datetime" => PyValue::Text(text()?.replacen('T', " ", 1)),
        "path" => PyValue::Text(path_text(text()?)),
        "list" | "tuple" => PyValue::List(
            v.as_array()
                .ok_or("not a list")?
                .iter()
                .map(typed)
                .collect::<Result<_, _>>()?,
        ),
        "map" => {
            let mut pairs: Vec<(PyValue, PyValue)> = Vec::new();
            for pair in v.as_array().ok_or("not a map")? {
                let key = typed(&pair[0])?;
                let item = typed(&pair[1])?;
                // A list or map key is unhashable in Python.
                if matches!(key, PyValue::List(_) | PyValue::Map(_)) {
                    return Err("unhashable key".into());
                }
                match pairs.iter_mut().find(|(k, _)| same_key(k, &key)) {
                    // Python keeps the first key and the last value.
                    Some(slot) => slot.1 = item,
                    None => pairs.push((key, item)),
                }
            }
            PyValue::Map(pairs)
        }
        _ => return Err("unknown type".into()),
    })
}

/// The set's one op, `input-hash`: `[serialized text, digest]`.
pub fn run(op: &str, input: &Value) -> Result<Value, String> {
    if op != "input-hash" {
        return Err("unknown op".into());
    }
    let value = typed(input)?;
    let text = dumps(&value).map_err(|e| format!("{e:?}"))?;
    Ok(json!({"t": "list", "v": [
        {"t": "str", "v": text},
        {"t": "str", "v": digest(&text)},
    ]}))
}
