//! Python value semantics the exercise runner relies on: `==`, `<`,
//! `sorted`, `float()`, `round(x, 2)`, the float `repr`, `json.dumps`
//! and the compensated float `sum` of CPython 3.12.
//!
//! Values are JSON values as Python's `json.loads` reads them: a number
//! written with a fraction or an exponent is a float, any other is an int.

use serde_json::{Number, Value};

/// A Python-level failure, named by its exception class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyError(pub &'static str);

/// Whether `json.loads` reads this number as a float.
pub fn is_float(number: &Number) -> bool {
    let text = number.to_string();
    text.contains(['.', 'e', 'E'])
}

pub fn number_f64(number: &Number) -> f64 {
    number.to_string().parse::<f64>().unwrap_or(f64::NAN)
}

fn int_text(number: &Number) -> String {
    let text = number.to_string();
    if text == "-0" {
        "0".to_owned()
    } else {
        text
    }
}

/// A JSON number from a Python float.
pub fn float_value(value: f64) -> Value {
    // The text form keeps the exact repr, so a float reads back as a float.
    serde_json::from_str::<Value>(&float_repr(value)).unwrap_or(Value::Null)
}

/// Python's `repr(float)`: the shortest round-trip digits, positional when
/// the decimal point falls within 16 digits and above 1e-4.
pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    let sci = format!("{:e}", x.abs());
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let point = exponent + 1;
    let sign = if x < 0.0 { "-" } else { "" };
    let body = if -4 < point && point <= 16 {
        let len = digits.len() as i32;
        if point <= 0 {
            format!("0.{}{}", "0".repeat((-point) as usize), digits)
        } else if point >= len {
            format!("{}{}.0", digits, "0".repeat((point - len) as usize))
        } else {
            format!(
                "{}.{}",
                &digits[..point as usize],
                &digits[point as usize..]
            )
        }
    } else {
        let rest = if digits.len() > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        let exp_sign = if exponent < 0 { "-" } else { "+" };
        format!("{}{}e{}{:02}", &digits[..1], rest, exp_sign, exponent.abs())
    };
    format!("{sign}{body}")
}

/// Python's `round(x, 2)`: the correctly rounded two-decimal value.
pub fn round2(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.2}").parse().unwrap_or(x)
}

/// CPython 3.12 `sum()` over floats (Neumaier compensated summation).
pub fn fsum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut total = 0.0_f64;
    let mut compensation = 0.0_f64;
    for x in values {
        let t = total + x;
        if total.abs() >= x.abs() {
            compensation += (total - t) + x;
        } else {
            compensation += (x - t) + total;
        }
        total = t;
    }
    if compensation != 0.0 && compensation.is_finite() {
        total += compensation;
    }
    total
}

fn numeric(value: &Value) -> Option<f64> {
    match value {
        Value::Bool(flag) => Some(f64::from(u8::from(*flag))),
        Value::Number(number) => Some(number_f64(number)),
        _ => None,
    }
}

/// Python `==` between two JSON-loaded values.
pub fn eq(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| eq(x, y))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, x)| b.get(key).is_some_and(|y| eq(x, y)))
        }
        (Value::Number(a), Value::Number(b)) if !is_float(a) && !is_float(b) => {
            int_text(a) == int_text(b)
        }
        _ => match (numeric(left), numeric(right)) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        },
    }
}

/// Python `<` between two JSON-loaded values (`TypeError` when unordered).
pub fn lt(left: &Value, right: &Value) -> Result<bool, PyError> {
    match (left, right) {
        (Value::String(a), Value::String(b)) => Ok(a < b),
        (Value::Array(a), Value::Array(b)) => {
            for (x, y) in a.iter().zip(b) {
                if !eq(x, y) {
                    return lt(x, y);
                }
            }
            Ok(a.len() < b.len())
        }
        _ => match (numeric(left), numeric(right)) {
            (Some(a), Some(b)) => Ok(a < b),
            _ => Err(PyError("TypeError")),
        },
    }
}

/// Python `sorted(items)`: a stable sort by `<`.
pub fn sorted(items: &[Value]) -> Result<Vec<Value>, PyError> {
    let mut out: Vec<Value> = Vec::with_capacity(items.len());
    for item in items {
        let mut at = out.len();
        while at > 0 && lt(item, &out[at - 1])? {
            at -= 1;
        }
        out.insert(at, item.clone());
    }
    Ok(out)
}

/// Python `float(value)`.
pub fn to_float(value: &Value) -> Result<f64, PyError> {
    match value {
        Value::Bool(_) | Value::Number(_) => Ok(numeric(value).unwrap_or(f64::NAN)),
        Value::String(text) => text
            .trim()
            .parse::<f64>()
            .map_err(|_| PyError("ValueError")),
        _ => Err(PyError("TypeError")),
    }
}

/// Python truthiness.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number_f64(number) != 0.0,
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(items)) => !items.is_empty(),
    }
}

fn push_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
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
                let mut units = [0_u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

/// Python `json.dumps(value, sort_keys=sort_keys, separators=(item, key))`
/// with the default `ensure_ascii=True`.
pub fn dumps_with(value: &Value, sort_keys: bool, item: &str, key: &str) -> String {
    let mut out = String::new();
    write(&mut out, value, sort_keys, item, key);
    out
}

/// Python `json.dumps(value, sort_keys=True)` (default separators).
pub fn dumps_sorted(value: &Value) -> String {
    dumps_with(value, true, ", ", ": ")
}

fn write(out: &mut String, value: &Value, sort_keys: bool, item: &str, key: &str) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) if is_float(number) => {
            let x = number_f64(number);
            if x.is_nan() {
                out.push_str("NaN");
            } else if x.is_infinite() {
                out.push_str(if x > 0.0 { "Infinity" } else { "-Infinity" });
            } else {
                out.push_str(&float_repr(x));
            }
        }
        Value::Number(number) => out.push_str(&int_text(number)),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (i, entry) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(item);
                }
                write(out, entry, sort_keys, item, key);
            }
            out.push(']');
        }
        Value::Object(items) => {
            let mut keys: Vec<&String> = items.keys().collect();
            if sort_keys {
                keys.sort();
            }
            out.push('{');
            for (i, name) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push_str(item);
                }
                push_string(out, name);
                out.push_str(key);
                write(out, &items[name], sort_keys, item, key);
            }
            out.push('}');
        }
    }
}

/// Python `repr(str)` for the plain strings exercise checks print.
pub fn repr_str(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
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
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python `repr(list_of_ints)`.
pub fn repr_ints(items: &[i64]) -> String {
    let parts: Vec<String> = items.iter().map(i64::to_string).collect();
    format!("[{}]", parts.join(", "))
}
