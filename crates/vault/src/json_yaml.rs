//! Vector set `json-yaml`: what `store.py` does with JSON and YAML text.
//!
//! Ops (each vector names one):
//! - `json-read`: text to value, as `json.loads` (`store.read_json`);
//! - `json-write`: value to the exact text `store._json_text` writes
//!   (`indent=2`, sorted keys, `ensure_ascii`, `default=str`);
//! - `yaml-read`: old data text to value, as `yaml.load` with the safe
//!   loader (`store.read_yaml`): YAML 1.1 plain scalars, so `yes` is a bool
//!   and `2026-11-18` is a date;
//! - `yaml-roundtrip`: value to YAML text and back to a value, each side with
//!   its own writer and reader (`store.write_yaml`, then `store.read_yaml`).
//!
//! The parser and the emitter are `yaml-rust2`. This file only maps their
//! events to values with the YAML 1.1 rules PyYAML's safe loader uses.
use crate::canon::{insert, Canon};
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::{Marker, TScalarStyle};
use yaml_rust2::yaml::{Hash, Yaml};
use yaml_rust2::YamlEmitter;

pub fn run(op: &str, input: &Value) -> Result<Value, String> {
    match op {
        "json-read" => Ok(json_read(text(input)?)?.to_json()),
        "json-write" => Ok(Value::String(json_write(&Canon::from_json(input)?)?)),
        "yaml-read" => Ok(yaml_read(text(input)?)?.to_json()),
        "yaml-roundtrip" => {
            let written = yaml_write(&Canon::from_json(input)?)?;
            Ok(yaml_read(&written)?.to_json())
        }
        _ => Err("unknown op".into()),
    }
}

fn text(input: &Value) -> Result<&str, String> {
    input.as_str().ok_or_else(|| "input is not text".to_owned())
}

// ---- JSON ----

struct JsonCanon(Canon);

impl<'de> Deserialize<'de> for JsonCanon {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(JsonVisitor).map(JsonCanon)
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Canon;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_unit<E>(self) -> Result<Canon, E> {
        Ok(Canon::Null)
    }
    fn visit_bool<E>(self, v: bool) -> Result<Canon, E> {
        Ok(Canon::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Canon, E> {
        Ok(Canon::Int(v.into()))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Canon, E> {
        Ok(Canon::Int(v.into()))
    }
    fn visit_f64<E>(self, v: f64) -> Result<Canon, E> {
        Ok(Canon::Float(v))
    }
    fn visit_str<E>(self, v: &str) -> Result<Canon, E> {
        Ok(Canon::Str(v.to_owned()))
    }
    fn visit_string<E>(self, v: String) -> Result<Canon, E> {
        Ok(Canon::Str(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Canon, A::Error> {
        let mut items = Vec::new();
        while let Some(JsonCanon(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(Canon::List(items))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Canon, A::Error> {
        let mut pairs = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            let JsonCanon(value) = map.next_value()?;
            insert(&mut pairs, Canon::Str(key), value);
        }
        Ok(Canon::Map(pairs))
    }
}

pub fn json_read(text: &str) -> Result<Canon, String> {
    serde_json::from_str::<JsonCanon>(text)
        .map(|JsonCanon(value)| value)
        .map_err(|_| "invalid JSON".to_owned())
}

/// Python's `ensure_ascii` over one already quoted JSON string.
fn json_string(text: &str) -> Result<String, String> {
    let quoted = serde_json::to_string(text).map_err(|_| "cannot write JSON".to_owned())?;
    let mut out = String::with_capacity(quoted.len());
    for ch in quoted.chars() {
        if ch == '\x7f' || !ch.is_ascii() {
            let mut units = [0u16; 2];
            for unit in ch.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        } else {
            out.push(ch);
        }
    }
    Ok(out)
}

fn write_json_value(
    value: &Canon,
    out: &mut String,
    level: usize,
    pretty: bool,
) -> Result<(), String> {
    match value {
        Canon::Null => out.push_str("null"),
        Canon::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
        Canon::Int(v) => out.push_str(&v.to_string()),
        Canon::Float(v) => out.push_str(
            &serde_json::Number::from_f64(*v)
                .ok_or("NaN and infinity are not supported")?
                .to_string(),
        ),
        Canon::Str(v) | Canon::Date(v) => out.push_str(&json_string(v)?),
        // `default=str` writes a datetime with a space, not a `T`.
        Canon::DateTime(v) => out.push_str(&json_string(&v.replacen('T', " ", 1))?),
        Canon::List(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return Ok(());
            }
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                if pretty {
                    out.push('\n');
                    out.push_str(&" ".repeat(level + 2));
                } else if index > 0 {
                    out.push(' ');
                }
                write_json_value(item, out, level + 2, pretty)?;
            }
            if pretty {
                out.push('\n');
                out.push_str(&" ".repeat(level));
            }
            out.push(']');
        }
        Canon::Map(pairs) => {
            let mut sorted = std::collections::BTreeMap::<&str, &Canon>::new();
            for (key, value) in pairs {
                let Canon::Str(key) = key else {
                    return Err("only text keys are supported".into());
                };
                sorted.insert(key, value);
            }
            if sorted.is_empty() {
                out.push_str("{}");
                return Ok(());
            }
            out.push('{');
            for (index, (key, value)) in sorted.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                if pretty {
                    out.push('\n');
                    out.push_str(&" ".repeat(level + 2));
                } else if index > 0 {
                    out.push(' ');
                }
                out.push_str(&json_string(key)?);
                out.push_str(": ");
                write_json_value(value, out, level + 2, pretty)?;
            }
            if pretty {
                out.push('\n');
                out.push_str(&" ".repeat(level));
            }
            out.push('}');
        }
    }
    Ok(())
}

/// `json.dumps(data, indent=2, sort_keys=True, default=str)` with arbitrary
/// size integers. Map order is sorted without Serde's `preserve_order`.
pub fn json_write(value: &Canon) -> Result<String, String> {
    let mut out = String::new();
    write_json_value(value, &mut out, 0, true)?;
    Ok(out)
}

/// One JSONL row: `json.dumps(row, sort_keys=True, default=str)`.
pub fn json_line(value: &Canon) -> Result<String, String> {
    let mut out = String::new();
    write_json_value(value, &mut out, 0, false)?;
    out.push('\n');
    Ok(out)
}

// ---- YAML read ----

enum Frame {
    Seq(usize, Vec<Canon>),
    Map(usize, Vec<(Canon, Canon)>, Option<Canon>),
}

#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    anchors: HashMap<usize, Canon>,
    docs: Vec<Canon>,
    error: Option<String>,
}

impl Builder {
    fn fail(&mut self, reason: &str) {
        self.error.get_or_insert_with(|| reason.to_owned());
    }

    fn node(&mut self, anchor: usize, value: Canon) {
        if anchor > 0 {
            self.anchors.insert(anchor, value.clone());
        }
        let collection = matches!(value, Canon::List(_) | Canon::Map(_));
        let mut bad_key = false;
        match self.stack.last_mut() {
            None => self.docs.push(value),
            Some(Frame::Seq(_, items)) => items.push(value),
            Some(Frame::Map(_, pairs, key)) => match key.take() {
                None => {
                    bad_key = collection;
                    *key = Some(value);
                }
                Some(k) => insert(pairs, k, value),
            },
        }
        if bad_key {
            self.fail("a list or a map as a key is not supported");
        }
    }
}

impl MarkedEventReceiver for Builder {
    fn on_event(&mut self, event: Event, _mark: Marker) {
        if self.error.is_some() {
            return;
        }
        match event {
            Event::Scalar(text, style, anchor, tag) => {
                let value = match (tag, style) {
                    (Some(tag), _)
                        if matches!(tag.handle.as_str(), "!!" | "tag:yaml.org,2002:")
                            && tag.suffix == "str" =>
                    {
                        Ok(Canon::Str(text))
                    }
                    (Some(_), _) => Err("tags other than !!str are not supported".to_owned()),
                    (None, TScalarStyle::Plain) => resolve(&text),
                    (None, _) => Ok(Canon::Str(text)),
                };
                match value {
                    Ok(value) => self.node(anchor, value),
                    Err(reason) => self.fail(&reason),
                }
            }
            Event::SequenceStart(anchor, None) => self.stack.push(Frame::Seq(anchor, Vec::new())),
            Event::MappingStart(anchor, None) => {
                self.stack.push(Frame::Map(anchor, Vec::new(), None))
            }
            Event::SequenceStart(_, Some(_)) | Event::MappingStart(_, Some(_)) => {
                self.fail("tagged collections are not supported")
            }
            Event::SequenceEnd => match self.stack.pop() {
                Some(Frame::Seq(anchor, items)) => self.node(anchor, Canon::List(items)),
                _ => self.fail("unbalanced sequence"),
            },
            Event::MappingEnd => match self.stack.pop() {
                Some(Frame::Map(anchor, pairs, None)) => self.node(anchor, Canon::Map(pairs)),
                _ => self.fail("unbalanced mapping"),
            },
            Event::Alias(id) => match self.anchors.get(&id).cloned() {
                Some(value) => self.node(0, value),
                None => self.fail("unknown alias"),
            },
            _ => {}
        }
    }
}

/// `yaml.load(text, Loader=SafeLoader)`: no document is None, more than one
/// is an error.
pub fn yaml_read(text: &str) -> Result<Canon, String> {
    let mut builder = Builder::default();
    Parser::new_from_str(text)
        .load(&mut builder, true)
        .map_err(|_| "invalid YAML".to_owned())?;
    if let Some(reason) = builder.error {
        return Err(reason);
    }
    match builder.docs.len() {
        0 => Ok(Canon::Null),
        1 => Ok(builder.docs.remove(0)),
        _ => Err("more than one document".into()),
    }
}

/// PyYAML's implicit resolvers for a plain scalar, in its order: bool,
/// float, int, merge, null, timestamp, value; anything else is text.
fn resolve(s: &str) -> Result<Canon, String> {
    const BOOLS: &[&str] = &[
        "yes", "Yes", "YES", "no", "No", "NO", "true", "True", "TRUE", "false", "False", "FALSE",
        "on", "On", "ON", "off", "Off", "OFF",
    ];
    if BOOLS.contains(&s) {
        let lower = s.to_ascii_lowercase();
        return Ok(Canon::Bool(matches!(lower.as_str(), "yes" | "true" | "on")));
    }
    let b = s.as_bytes();
    if is_float(b) {
        return float(s).map(Canon::Float);
    }
    if is_int(b) {
        return int(s).map(Canon::Int);
    }
    if s == "<<" {
        return Err("merge keys are not supported".into());
    }
    if matches!(s, "~" | "null" | "Null" | "NULL" | "") {
        return Ok(Canon::Null);
    }
    if let Some(stamp) = timestamp(b) {
        return stamp;
    }
    if s == "=" {
        return Err("the value tag has no safe constructor".into());
    }
    Ok(Canon::Str(s.to_owned()))
}

fn sign(b: &[u8], i: usize) -> usize {
    if matches!(b.get(i), Some(b'-' | b'+')) {
        i + 1
    } else {
        i
    }
}

fn digit(b: &[u8], i: usize) -> bool {
    b.get(i).is_some_and(u8::is_ascii_digit)
}

/// `[0-9_]*` and friends: the end of a run of bytes that pass `ok`.
fn run_of(b: &[u8], mut i: usize, ok: impl Fn(u8) -> bool) -> usize {
    while b.get(i).is_some_and(|c| ok(*c)) {
        i += 1;
    }
    i
}

fn digits_us(b: &[u8], i: usize) -> usize {
    run_of(b, i, |c| c.is_ascii_digit() || c == b'_')
}

/// An optional `[eE][-+][0-9]+`.
fn exponent(b: &[u8], i: usize) -> usize {
    if matches!(b.get(i), Some(b'e' | b'E'))
        && matches!(b.get(i + 1), Some(b'-' | b'+'))
        && digit(b, i + 2)
    {
        run_of(b, i + 2, |c| c.is_ascii_digit())
    } else {
        i
    }
}

/// One or more `:[0-5]?[0-9]`; None when there is none.
fn sexagesimal(b: &[u8], mut i: usize) -> Option<usize> {
    let start = i;
    while b.get(i) == Some(&b':') {
        if matches!(b.get(i + 1), Some(b'0'..=b'5')) && digit(b, i + 2) {
            i += 3;
        } else if digit(b, i + 1) {
            i += 2;
        } else {
            return None;
        }
    }
    (i > start).then_some(i)
}

fn is_float(b: &[u8]) -> bool {
    let n = b.len();
    // [-+]?[0-9][0-9_]*\.[0-9_]*(?:[eE][-+][0-9]+)?
    let i = sign(b, 0);
    if digit(b, i) {
        let j = digits_us(b, i + 1);
        if b.get(j) == Some(&b'.') && exponent(b, digits_us(b, j + 1)) == n {
            return true;
        }
        // [-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*
        if let Some(k) = sexagesimal(b, j) {
            if b.get(k) == Some(&b'.') && digits_us(b, k + 1) == n {
                return true;
            }
        }
    }
    // \.[0-9][0-9_]*(?:[eE][-+][0-9]+)?
    if b.first() == Some(&b'.') && digit(b, 1) && exponent(b, digits_us(b, 2)) == n {
        return true;
    }
    // [-+]?\.(?:inf|Inf|INF) and \.(?:nan|NaN|NAN)
    matches!(&b[sign(b, 0)..], b".inf" | b".Inf" | b".INF")
        || matches!(b, b".nan" | b".NaN" | b".NAN")
}

/// `construct_yaml_float`.
fn float(s: &str) -> Result<f64, String> {
    let mut value = s.replace('_', "").to_ascii_lowercase();
    let mut sign = 1.0;
    if value.starts_with('-') {
        sign = -1.0;
    }
    if value.starts_with(['-', '+']) {
        value.remove(0);
    }
    if value == ".inf" {
        return Ok(sign * f64::INFINITY);
    }
    if value == ".nan" {
        return Ok(f64::NAN);
    }
    let parse = |part: &str| part.parse::<f64>().map_err(|_| "bad float".to_owned());
    if value.contains(':') {
        let (mut base, mut total) = (1.0, 0.0);
        for part in value.split(':').rev() {
            total += parse(part)? * base;
            base *= 60.0;
        }
        return Ok(sign * total);
    }
    Ok(sign * parse(&value)?)
}

fn is_int(b: &[u8]) -> bool {
    let n = b.len();
    let i = sign(b, 0);
    let rest = &b[i..];
    // [-+]?0b[0-1_]+ and [-+]?0x[0-9a-fA-F_]+
    if let Some(bits) = rest.strip_prefix(b"0b") {
        return !bits.is_empty() && bits.iter().all(|c| matches!(c, b'0' | b'1' | b'_'));
    }
    if let Some(hex) = rest.strip_prefix(b"0x") {
        return !hex.is_empty() && hex.iter().all(|c| c.is_ascii_hexdigit() || *c == b'_');
    }
    // [-+]?0[0-7_]+ and [-+]?0
    if rest.first() == Some(&b'0') {
        return rest.len() == 1 || rest[1..].iter().all(|c| matches!(c, b'0'..=b'7' | b'_'));
    }
    // [-+]?[1-9][0-9_]* and [-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+
    if matches!(rest.first(), Some(b'1'..=b'9')) {
        let j = digits_us(b, i + 1);
        return j == n || sexagesimal(b, j) == Some(n);
    }
    false
}

/// `construct_yaml_int`, including Python's arbitrary-size integers.
fn int(s: &str) -> Result<num_bigint::BigInt, String> {
    use num_bigint::BigInt;
    let mut value = s.replace('_', "");
    let negative = value.starts_with('-');
    if value.starts_with(['-', '+']) {
        value.remove(0);
    }
    let parse = |digits: &str, radix: u32| {
        BigInt::parse_bytes(digits.as_bytes(), radix).ok_or_else(|| "bad int".to_owned())
    };
    let mut magnitude = if value == "0" {
        BigInt::from(0)
    } else if let Some(bits) = value.strip_prefix("0b") {
        parse(bits, 2)?
    } else if let Some(hex) = value.strip_prefix("0x") {
        parse(hex, 16)?
    } else if value.starts_with('0') {
        parse(&value, 8)?
    } else if value.contains(':') {
        let (mut base, mut total) = (BigInt::from(1), BigInt::from(0));
        for part in value.split(':').rev() {
            total += parse(part, 10)? * &base;
            base *= 60;
        }
        total
    } else {
        parse(&value, 10)?
    };
    if negative {
        magnitude = -magnitude;
    }
    Ok(magnitude)
}

/// PyYAML's timestamp resolver and `construct_yaml_timestamp`. None when the
/// text is not a timestamp; an invalid date or time is an error, as Python's
/// `datetime` raises.
fn timestamp(b: &[u8]) -> Option<Result<Canon, String>> {
    let number = |from: usize, to: usize| -> u32 {
        b[from..to]
            .iter()
            .fold(0, |n, c| n * 10 + u32::from(c - b'0'))
    };
    if b.len() < 8 || !(0..4).all(|i| digit(b, i)) || b[4] != b'-' {
        return None;
    }
    let year = number(0, 4);
    let mut i = 5;
    let field = |i: &mut usize| -> Option<u32> {
        let start = *i;
        if !digit(b, *i) {
            return None;
        }
        *i += 1;
        if digit(b, *i) {
            *i += 1;
        }
        Some(number(start, *i))
    };
    let month_start = i;
    let month = field(&mut i)?;
    let month_len = i - month_start;
    if b.get(i) != Some(&b'-') {
        return None;
    }
    i += 1;
    let day_start = i;
    let day = field(&mut i)?;
    let day_len = i - day_start;
    if i == b.len() {
        // The date-only form needs two digits each.
        if month_len != 2 || day_len != 2 {
            return None;
        }
        return Some(date(year, month, day).map(Canon::Date));
    }
    // (?:[Tt]|[ \t]+)
    if matches!(b[i], b'T' | b't') {
        i += 1;
    } else if matches!(b[i], b' ' | b'\t') {
        i = run_of(b, i, |c| c == b' ' || c == b'\t');
    } else {
        return None;
    }
    let hour = field(&mut i)?;
    let two = |i: &mut usize| -> Option<u32> {
        if b.get(*i) == Some(&b':') && digit(b, *i + 1) && digit(b, *i + 2) {
            *i += 3;
            Some(number(*i - 2, *i))
        } else {
            None
        }
    };
    let minute = two(&mut i)?;
    let second = two(&mut i)?;
    let mut fraction = 0u32;
    if b.get(i) == Some(&b'.') {
        let start = i + 1;
        i = run_of(b, start, |c| c.is_ascii_digit());
        let digits: String = b[start..i].iter().take(6).map(|c| char::from(*c)).collect();
        if !digits.is_empty() {
            fraction = format!("{digits:0<6}").parse().unwrap_or(0);
        }
    }
    let after_fraction = i;
    i = run_of(b, i, |c| c == b' ' || c == b'\t');
    let mut offset: Option<i64> = None;
    if i < b.len() {
        if b[i] == b'Z' {
            offset = Some(0);
            i += 1;
        } else if matches!(b[i], b'-' | b'+') {
            let negative = b[i] == b'-';
            i += 1;
            let tz_hour = field(&mut i)?;
            let tz_minute = if b.get(i) == Some(&b':') {
                two(&mut i)?
            } else {
                0
            };
            let minutes = i64::from(tz_hour) * 60 + i64::from(tz_minute);
            offset = Some(if negative { -minutes } else { minutes });
        } else {
            return None;
        }
    } else {
        i = after_fraction;
    }
    if i != b.len() {
        return None;
    }
    Some(datetime(
        year, month, day, hour, minute, second, fraction, offset,
    ))
}

fn date(year: u32, month: u32, day: u32) -> Result<String, String> {
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err("month out of range".into()),
    };
    if year == 0 || day == 0 || day > days {
        return Err("day out of range".into());
    }
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

/// `datetime(...).isoformat()`, with `timezone(timedelta(minutes=offset))`.
#[allow(clippy::too_many_arguments)]
fn datetime(
    year: u32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    fraction: u32,
    offset: Option<i64>,
) -> Result<Canon, String> {
    let date = date(year, month, day)?;
    if hour > 23 || minute > 59 || second > 59 {
        return Err("time out of range".into());
    }
    let mut text = format!("{date}T{hour:02}:{minute:02}:{second:02}");
    if fraction > 0 {
        text.push_str(&format!(".{fraction:06}"));
    }
    if let Some(minutes) = offset {
        if minutes.abs() >= 24 * 60 {
            return Err("offset out of range".into());
        }
        let sign = if minutes < 0 { '-' } else { '+' };
        let minutes = minutes.abs();
        text.push_str(&format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60));
    }
    Ok(Canon::DateTime(text))
}

// ---- YAML write ----

/// A float in the form PyYAML reads back as a float: a dot in the mantissa
/// and a signed exponent.
fn yaml_float(v: f64) -> String {
    if v.is_nan() {
        return ".nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { ".inf" } else { "-.inf" }.into();
    }
    let text = format!("{v:?}");
    match text.split_once('e') {
        None => text,
        Some((mantissa, exp)) => {
            let mantissa = if mantissa.contains('.') {
                mantissa.to_owned()
            } else {
                format!("{mantissa}.0")
            };
            let exp = if exp.starts_with('-') {
                exp.to_owned()
            } else {
                format!("+{exp}")
            };
            format!("{mantissa}e{exp}")
        }
    }
}

fn to_yaml(value: &Canon) -> Result<Yaml, String> {
    Ok(match value {
        Canon::Null => Yaml::Null,
        Canon::Bool(v) => Yaml::Boolean(*v),
        Canon::Int(v) => Yaml::Integer(i64::try_from(v).map_err(|_| "int out of range")?),
        // `Real` is written as is, unquoted.
        Canon::Float(v) => Yaml::Real(yaml_float(*v)),
        Canon::Date(v) => Yaml::Real(v.clone()),
        // PyYAML writes a datetime as an unquoted timestamp with a space.
        Canon::DateTime(v) => Yaml::Real(v.replacen('T', " ", 1)),
        Canon::Str(v) => Yaml::String(v.clone()),
        Canon::List(items) => Yaml::Array(items.iter().map(to_yaml).collect::<Result<_, _>>()?),
        Canon::Map(pairs) => {
            let mut hash = Hash::new();
            for (k, v) in pairs {
                hash.insert(to_yaml(k)?, to_yaml(v)?);
            }
            Yaml::Hash(hash)
        }
    })
}

pub fn yaml_write(value: &Canon) -> Result<String, String> {
    let mut out = String::new();
    YamlEmitter::new(&mut out)
        .dump(&to_yaml(value)?)
        .map_err(|_| "cannot write YAML".to_owned())?;
    out.push('\n');
    Ok(out)
}
