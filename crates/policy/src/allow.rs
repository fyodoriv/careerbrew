//! The allowlists of `privacy.py` and the pure term filter.
//!
//! `filter_terms` is `privacy.sensitive_terms` over supplied raw inputs: the
//! term file text, the profile value and the plugin term file texts. Finding
//! and reading those files belongs to rust-07; this crate does no IO.

use crate::text;
use serde_json::Value;

/// `privacy.SENSITIVITIES`.
pub const SENSITIVITIES: &[&str] = &["public", "personal", "business"];

/// `privacy.TEMPLATE_PLACEHOLDERS`.
pub const TEMPLATE_PLACEHOLDERS: &[&str] = &["Your Name", "Your Employer"];

/// `privacy.TECH_WORDS`.
pub const TECH_WORDS: &[&str] = &[
    "OAuth2", "IPv4", "IPv6", "HTTP2", "HTTP3", "Python3", "ES2015", "ES2020", "UTF-8", "SHA256",
    "S3", "EC2", "ARM64", "x86_64", "AV1", "H264", "WCAG2",
];

/// `privacy.COMMON_COMPOUNDS`.
pub const COMMON_COMPOUNDS: &[&str] = &[
    "front-end",
    "back-end",
    "full-stack",
    "cross-team",
    "open-source",
    "high-impact",
    "low-latency",
    "self-serve",
    "self-service",
    "multi-region",
    "data-driven",
    "large-scale",
    "long-term",
    "short-term",
    "year-over-year",
    "follow-up",
    "user-facing",
    "customer-facing",
];

/// `privacy.COMMON_CAMEL`.
pub const COMMON_CAMEL: &[&str] = &[
    "JavaScript",
    "TypeScript",
    "GitHub",
    "GitLab",
    "PostgreSQL",
    "MySQL",
    "GraphQL",
    "DevOps",
    "OpenTelemetry",
    "LinkedIn",
    "PagerDuty",
    "BigQuery",
    "DynamoDB",
    "MongoDB",
    "CloudFormation",
    "OpenAI",
    "macOS",
    "iOS",
    "iPadOS",
    "NoSQL",
    "FastAPI",
    "PyTorch",
    "TensorFlow",
    "CircleCI",
    "SwiftUI",
    "WebSocket",
    "OAuth",
    "YouTube",
];

/// A set as Python's `sorted(...)` lists it.
pub fn sorted(set: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = set.iter().map(|s| (*s).to_owned()).collect();
    out.sort();
    out.dedup();
    out
}

/// A Python exception: its class name and message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PyError {
    pub class: &'static str,
    pub message: String,
}

impl PyError {
    pub fn new(class: &'static str, message: impl Into<String>) -> Self {
        PyError {
            class,
            message: message.into(),
        }
    }
}

/// Python's type name of a JSON value.
pub fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Python truthiness of a JSON value.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `privacy._read_terms` over the file's text.
pub fn read_terms(file_text: &str) -> Vec<String> {
    text::splitlines(file_text)
        .into_iter()
        .map(text::strip)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && text::len(l) >= 3)
        .map(str::to_owned)
        .collect()
}

/// `privacy.sensitive_terms` over supplied raw inputs. `file` is the term
/// file's text (None when there is no file), `profile` the parsed profile,
/// `plugin_files` each installed plugin's term file text, in order.
///
/// Python dedupes with a `set`, whose order for terms of equal length
/// follows the per-process string hash; this keeps the first one seen.
pub fn filter_terms(
    file: Option<&str>,
    profile: &Value,
    plugin_files: &[&str],
) -> Result<Vec<String>, PyError> {
    let mut terms = file.map(read_terms).unwrap_or_default();
    let empty = serde_json::Map::new();
    let profile = match profile {
        Value::Object(map) => map,
        other if !truthy(other) => &empty,
        other => {
            return Err(PyError::new(
                "AttributeError",
                format!("'{}' object has no attribute 'get'", type_name(other)),
            ))
        }
    };
    for key in ["name", "employer", "email"] {
        if let Some(Value::String(value)) = profile.get(key) {
            if text::len(value) >= 3 && !TEMPLATE_PLACEHOLDERS.contains(&value.as_str()) {
                terms.push(value.clone());
            }
        }
    }
    let extras: Vec<Value> = match profile.get("sensitive") {
        None => Vec::new(),
        Some(value) if !truthy(value) => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(Value::Object(map)) => map.keys().cloned().map(Value::String).collect(),
        Some(Value::String(s)) => s.chars().map(|c| Value::String(c.to_string())).collect(),
        Some(other) => {
            return Err(PyError::new(
                "TypeError",
                format!("'{}' object is not iterable", type_name(other)),
            ))
        }
    };
    for extra in extras {
        if let Value::String(s) = extra {
            if text::len(&s) >= 3 {
                terms.push(s);
            }
        }
    }
    for plugin in plugin_files {
        terms.extend(read_terms(plugin));
    }
    let mut seen = std::collections::HashSet::new();
    terms.retain(|t| seen.insert(t.clone()));
    // `sorted(key=len, reverse=True)` is stable.
    terms.sort_by_key(|t| std::cmp::Reverse(text::len(t)));
    Ok(terms)
}

/// `re.match(pattern + "$")`: the whole text, or the text before one final `\n`.
fn full_match(t: &str, pred: impl Fn(&str) -> bool) -> bool {
    pred(t) || t.strip_suffix('\n').is_some_and(pred)
}

/// `privacy._OK_SLASH`: `^(?:[A-Za-z]{1,12}(?:/[A-Za-z]{1,12})+|\d+/\d+)$`.
fn ok_slash(t: &str) -> bool {
    full_match(t, |s| {
        let parts: Vec<&str> = s.split('/').collect();
        let letters = parts.len() >= 2
            && parts
                .iter()
                .all(|p| (1..=12).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphabetic()));
        let digits = parts.len() == 2
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(text::is_decimal));
        letters || digits
    })
}

/// `privacy._VERSION`: `^v?\d+(?:\.\d+)+$`.
fn version(t: &str) -> bool {
    full_match(t, |s| {
        let s = s.strip_prefix('v').unwrap_or(s);
        let parts: Vec<&str> = s.split('.').collect();
        parts.len() >= 2
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(text::is_decimal))
    })
}

const PUNCT: &str = ".,;:!?()[]{}\"'";

/// `privacy._identifier_shaped`.
pub fn identifier_shaped(token: &str) -> bool {
    let t = text::strip_chars(token, PUNCT);
    if t.is_empty() {
        return false;
    }
    if ["://", ":/", "@", "\\"].iter().any(|x| t.contains(x)) || t.starts_with(['/', '~']) {
        return true;
    }
    if t.contains('/') && !ok_slash(t) {
        return true;
    }
    if t.contains('_') {
        return true;
    }
    let has_digit = t.chars().any(text::is_digit);
    let has_alpha = t.chars().any(text::is_alpha);
    if t.contains('-') && has_digit && has_alpha {
        return true;
    }
    has_digit && has_alpha && text::len(t) >= 6 && !version(t) && !TECH_WORDS.contains(&t)
}

/// `str.isalpha` of a whole string: non-empty, every char alphabetic.
fn isalpha(s: &str) -> bool {
    !s.is_empty() && s.chars().all(text::is_alpha)
}

/// `re.search(r"[a-z][A-Z]", t)`.
fn lower_then_upper(t: &str) -> bool {
    t.as_bytes()
        .windows(2)
        .any(|w| w[0].is_ascii_lowercase() && w[1].is_ascii_uppercase())
}

/// `privacy.public_suspects`.
pub fn public_suspects(text_in: &str) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    for raw in text::split(text_in) {
        let t = text::strip_chars(raw, PUNCT);
        let parts: Vec<&str> = t.split('-').collect();
        let odd_compound = parts.len() > 1
            && !COMMON_COMPOUNDS.contains(&text::lower(t).as_str())
            && parts.iter().all(|p| isalpha(p))
            && parts.iter().any(|p| text::len(p) >= 5);
        if odd_compound || (lower_then_upper(t) && !COMMON_CAMEL.contains(&t)) {
            out.insert(t.to_owned());
        }
    }
    out.into_iter().collect()
}
