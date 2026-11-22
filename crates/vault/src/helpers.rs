//! Component-only `store.outline`, `store.digest` and `store.cached` ports.
//! Every cache read/write uses the invocation's synthetic `Store`; no value is
//! retained between calls and no production route selects these helpers.

use crate::atomic::StoreError;
use crate::canon::Canon;
use crate::json_yaml;
use crate::store::Store;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outline {
    pub kind: String,
    pub sealed: bool,
    pub count: usize,
    pub keys: Vec<String>,
}

#[derive(Debug)]
pub enum OutlineError {
    Store(StoreError),
    Value(String),
}

impl From<StoreError> for OutlineError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

/// Python `str.splitlines`, including its non-LF line boundaries and no extra
/// item after a terminal boundary.
pub(crate) fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        let width = match ch {
            '\n' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}'
            | '\u{2029}' => ch.len_utf8(),
            '\r' => {
                if chars.peek().is_some_and(|(_, next)| *next == '\n') {
                    chars.next();
                    2
                } else {
                    1
                }
            }
            _ => continue,
        };
        lines.push(&text[start..index]);
        start = index + width;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

fn outline_key(key: &Canon) -> bool {
    let Canon::Str(key) = key else {
        return false;
    };
    // Python's `$` also matches immediately before one terminal newline.
    // The original key, including that newline, remains the outline text.
    let matched = key.strip_suffix('\n').unwrap_or(key);
    let bytes = matched.as_bytes();
    (1..=48).contains(&bytes.len())
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn record_type(types: &mut Vec<(String, Vec<String>)>, path: String, kind: String) {
    if let Some((_, known)) = types.iter_mut().find(|(known, _)| *known == path) {
        if !known.contains(&kind) {
            known.push(kind);
        }
    } else {
        types.push((path, vec![kind]));
    }
}

fn walk(
    value: &Canon,
    path: &str,
    types: &mut Vec<(String, Vec<String>)>,
    strict: bool,
    record: bool,
) {
    let merged = path.contains("[]") || path.contains('*');
    match value {
        Canon::Map(items) => {
            let data_keys = (strict && !record)
                || items.len() > 30
                || !items.iter().all(|(key, _)| outline_key(key));
            if !path.is_empty() {
                record_type(
                    types,
                    path.to_owned(),
                    if data_keys && !merged {
                        format!("map[{}]", items.len())
                    } else {
                        "map".into()
                    },
                );
            }
            for (key, value) in items {
                let key = if data_keys {
                    "*".to_owned()
                } else {
                    let Canon::Str(key) = key else {
                        unreachable!("a schema map contains only text keys")
                    };
                    key.clone()
                };
                let child = if path.is_empty() {
                    key
                } else {
                    format!("{path}.{key}")
                };
                walk(value, &child, types, strict, data_keys);
            }
        }
        Canon::List(items) => {
            record_type(
                types,
                if path.is_empty() { "(top)" } else { path }.to_owned(),
                if merged {
                    "list".into()
                } else {
                    format!("list[{}]", items.len())
                },
            );
            for value in items {
                walk(value, &format!("{path}[]"), types, strict, true);
            }
        }
        _ => {
            let kind = match value {
                Canon::Null => "null",
                Canon::Bool(_) => "bool",
                Canon::Int(_) => "int",
                Canon::Float(_) => "float",
                Canon::Str(_) => "str",
                Canon::Date(_) => "date",
                Canon::DateTime(_) => "datetime",
                Canon::Map(_) | Canon::List(_) => unreachable!(),
            };
            record_type(
                types,
                if path.is_empty() { "(top)" } else { path }.to_owned(),
                kind.into(),
            );
        }
    }
}

fn parse_json(text: &str) -> Result<Canon, OutlineError> {
    json_yaml::json_read(text).map_err(|_| {
        OutlineError::Value(match json_yaml::json_error_line(text) {
            Some(line) => format!("cannot parse as json (line {line})"),
            None => "cannot parse as json".into(),
        })
    })
}

pub fn outline(store: &Store, path: &Path, strict: bool) -> Result<Outline, OutlineError> {
    let text = store.read_text(path, Some(""), true)?.unwrap_or_default();
    let kind = match path
        .extension()
        .and_then(|suffix| suffix.to_str())
        .map(str::to_lowercase)
        .as_deref()
    {
        Some("yaml" | "yml") => "yaml",
        Some("json") => "json",
        Some("jsonl") => "jsonl",
        _ => "text",
    };
    let lines = splitlines(&text);
    let mut count = lines.len();
    let mut types = Vec::new();
    match kind {
        "jsonl" => {
            let mut rows = 0;
            for (index, line) in lines.iter().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let row = json_yaml::json_read(line).map_err(|_| {
                    OutlineError::Value(format!("cannot parse as jsonl (line {})", index + 1))
                })?;
                walk(&row, "[]", &mut types, strict, true);
                rows += 1;
            }
            count = rows;
        }
        "json" => {
            let value = parse_json(&text)?;
            walk(&value, "", &mut types, strict, true);
        }
        "yaml" => {
            let value = json_yaml::yaml_read(&text).map_err(|_| {
                OutlineError::Value(match json_yaml::yaml_error_line(&text) {
                    Some(line) => format!("cannot parse as yaml (line {line})"),
                    None => "cannot parse as yaml".into(),
                })
            })?;
            if value != Canon::Null {
                walk(&value, "", &mut types, strict, true);
            }
        }
        _ => {}
    }
    let sealed = std::fs::read(path)
        .map_err(StoreError::from)?
        .starts_with(crate::MAGIC);
    Ok(Outline {
        kind: kind.into(),
        sealed,
        count,
        keys: types
            .into_iter()
            .map(|(path, kinds)| format!("{path}: {}", kinds.join("|")))
            .collect(),
    })
}

pub fn digest(parts: &[Canon]) -> Result<String, StoreError> {
    let mut hash = Sha256::new();
    for part in parts {
        let text = json_yaml::json_compact_python(part).map_err(|_| StoreError::Codec("json"))?;
        hash.update(text.as_bytes());
        hash.update([0]);
    }
    Ok(format!("{:x}", hash.finalize())[..24].to_owned())
}

fn field<'a>(value: &'a Canon, name: &str) -> Option<&'a Canon> {
    let Canon::Map(items) = value else {
        return None;
    };
    items
        .iter()
        .find(|(key, _)| matches!(key, Canon::Str(key) if key == name))
        .map(|(_, value)| value)
}

fn number(value: &Canon) -> Option<f64> {
    match value {
        Canon::Float(value) => Some(*value),
        Canon::Int(value) => value.to_string().parse().ok(),
        _ => None,
    }
}

pub fn cache_path(root: &Path, key_parts: &[Canon]) -> Result<PathBuf, StoreError> {
    Ok(root
        .join("cache")
        .join(format!("{}.json", digest(key_parts)?)))
}

/// Return `(value, hit)`. The caller owns invocation-scoped clock and compute
/// adapters; a fresh hit never calls either compute or the clock for a write.
pub fn cached<F, C>(
    store: &Store,
    key_parts: &[Canon],
    ttl_seconds: Option<f64>,
    now: F,
    mut compute: C,
) -> Result<(Canon, bool), StoreError>
where
    F: Fn() -> f64,
    C: FnMut() -> Result<Canon, StoreError>,
{
    let path = cache_path(&store.root, key_parts)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() {
        let entry = store.read_json(&path, Canon::Null, true)?;
        let at = number(field(&entry, "at").ok_or(StoreError::Codec("cache-at"))?)
            .ok_or(StoreError::Codec("cache-at"))?;
        if ttl_seconds.is_none_or(|ttl| now() - at < ttl) {
            let value = field(&entry, "value")
                .ok_or(StoreError::Codec("cache-value"))?
                .clone();
            return Ok((value, true));
        }
    }
    let value = compute()?;
    let entry = Canon::Map(vec![
        (Canon::Str("at".into()), Canon::Float(now())),
        (Canon::Str("value".into()), value.clone()),
    ]);
    store.write_json(&path, &entry)?;
    Ok((value, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitlines_matches_python_boundaries() {
        assert_eq!(splitlines("a\n"), ["a"]);
        assert_eq!(splitlines("a\r\nb\rc\u{2028}"), ["a", "b", "c"]);
        assert_eq!(splitlines(""), Vec::<&str>::new());
    }

    #[test]
    fn digest_delimits_each_serialized_part() {
        let empty = digest(&[]).unwrap();
        assert_eq!(empty, "e3b0c44298fc1c149afbf4c8");
    }
}
