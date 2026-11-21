//! Approval decisions (rust-04e) from `src/careerbrew/approvals.py` over
//! supplied records: the receipt validator (`validate_records`),
//! `request_item` (the record `request` saves), `decide_items` (Python's
//! check order), `pending`, `guidance` and `get`.
//!
//! The caller loads and saves the records (rust-05) and supplies `now` as
//! an ISO string and the new id. Records keep their key order; an update
//! keeps existing keys in place and appends new ones, like `dict.update`.

use crate::allow::PyError;
use crate::canary::Rules;
use crate::text;
use crate::vectors::Out;

/// `approvals.STATUSES`.
pub const STATUSES: &[&str] = &["pending", "approved", "rejected"];

/// `approvals.GUIDANCE_MAX`.
pub const GUIDANCE_MAX: i64 = 10;

fn record_error(message: String) -> PyError {
    PyError::new("ApprovalRecordError", message)
}

/// `str(KeyError(message))`: the message's `repr`.
pub fn key_error(message: &str) -> PyError {
    PyError::new("KeyError", py_repr(message))
}

/// Python `repr` of a str: a char `str.isprintable` refuses becomes
/// `\xNN`, `\uNNNN` or `\UNNNNNNNN`.
pub fn py_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::new();
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if !text::is_printable(c) => {
                let n = c as u32;
                out.push_str(&if n < 0x100 {
                    format!("\\x{n:02x}")
                } else if n < 0x10000 {
                    format!("\\u{n:04x}")
                } else {
                    format!("\\U{n:08x}")
                });
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn get<'a>(record: &'a Out, key: &str) -> Option<&'a Out> {
    match record {
        Out::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn get_str<'a>(record: &'a Out, key: &str) -> Option<&'a str> {
    match get(record, key) {
        Some(Out::Str(s)) => Some(s),
        _ => None,
    }
}

/// `dict.update(key=value)`: in place when present, else appended.
fn set(record: &mut Out, key: &str, value: Out) {
    if let Out::Dict(items) = record {
        match items.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => items.push((key.to_owned(), value)),
        }
    }
}

/// `approvals.validate_records`: deny by default; the records unchanged when valid.
pub fn validate_records(rules: &Rules, records: &Out) -> Result<Vec<Out>, PyError> {
    let Out::List(items) = records else {
        return Err(record_error("approvals file: not a list".into()));
    };
    let mut seen: Vec<&str> = Vec::new();
    for (i, a) in items.iter().enumerate() {
        if !matches!(a, Out::Dict(_)) {
            return Err(record_error(format!("approvals record {i}: not an object")));
        }
        for key in ["id", "status", "title", "at"] {
            let Some(value) = get_str(a, key) else {
                return Err(record_error(format!("approvals record {i}: missing {key}")));
            };
            if key == "status" && rules.status_check && !STATUSES.contains(&value) {
                return Err(record_error(format!(
                    "approvals record {i}: unknown status"
                )));
            }
        }
        if get_str(a, "status") != Some("pending") && get_str(a, "decided_at").is_none() {
            return Err(record_error(format!(
                "approvals record {i}: decided without decided_at"
            )));
        }
        let id = get_str(a, "id").unwrap_or_default();
        if seen.contains(&id) {
            return Err(record_error(format!("approvals record {i}: duplicate id")));
        }
        seen.push(id);
    }
    Ok(items.clone())
}

/// Python `s[:n]` over code points.
fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub fn request_title(title: &str) -> Result<String, PyError> {
    let title = text::split(title).join(" ");
    if title.is_empty() {
        return Err(PyError::new(
            "ValueError",
            "an approval request needs a title",
        ));
    }
    Ok(title)
}

/// The record `approvals.request` appends, and the list it saves.
/// `new_id` is `uuid4().hex`'s source; its first 8 chars are the id.
pub fn request_item(
    rules: &Rules,
    records: &Out,
    title: &str,
    detail: &str,
    by: &str,
    now: &str,
    new_id: &str,
) -> Result<(Out, Vec<Out>), PyError> {
    let title = request_title(title)?;
    let id: String = format!("{new_id}{}", "0".repeat(24))
        .chars()
        .take(8)
        .collect();
    let item = Out::Dict(vec![
        ("id".into(), Out::Str(id)),
        ("title".into(), Out::Str(cut(&title, 200))),
        ("detail".into(), Out::Str(cut(text::strip(detail), 4000))),
        ("by".into(), Out::Str(cut(by, 60))),
        ("at".into(), Out::Str(now.to_owned())),
        ("status".into(), Out::Str("pending".into())),
        ("note".into(), Out::Str(String::new())),
    ]);
    let mut saved = validate_records(rules, records)?;
    saved.push(item.clone());
    Ok((item, saved))
}

/// `approvals.decide` without the save: the decided record and the list to save.
pub fn decide_items(
    rules: &Rules,
    records: &Out,
    item_id: &str,
    decision: &str,
    note: &str,
    now: &str,
) -> Result<(Out, Vec<Out>), PyError> {
    decide_items_with(rules, records, item_id, decision, note, || now.to_owned())
}

/// The IO caller supplies its clock lazily, so an invalid or already decided
/// id is refused before a clock read, as in Python.
pub fn validate_decision(decision: &str) -> Result<(), PyError> {
    if decision != "approved" && decision != "rejected" {
        return Err(PyError::new(
            "ValueError",
            "decision must be approved or rejected",
        ));
    }
    Ok(())
}

pub fn decide_items_with(
    rules: &Rules,
    records: &Out,
    item_id: &str,
    decision: &str,
    note: &str,
    now: impl FnOnce() -> String,
) -> Result<(Out, Vec<Out>), PyError> {
    validate_decision(decision)?;
    let mut items = validate_records(rules, records)?;
    let Some(index) = items.iter().position(|a| get_str(a, "id") == Some(item_id)) else {
        return Err(key_error(&format!("unknown approval {item_id}")));
    };
    let status = get_str(&items[index], "status").unwrap_or_default();
    if status != "pending" {
        return Err(PyError::new("ValueError", format!("already {status}")));
    }
    let a = &mut items[index];
    set(a, "status", Out::Str(decision.to_owned()));
    set(a, "note", Out::Str(cut(text::strip(note), 1000)));
    set(a, "decided_at", Out::Str(now()));
    let decided = a.clone();
    Ok((decided, items))
}

/// `approvals.pending`.
pub fn pending(rules: &Rules, records: &Out) -> Result<Vec<Out>, PyError> {
    Ok(validate_records(rules, records)?
        .into_iter()
        .filter(|a| get_str(a, "status") == Some("pending"))
        .collect())
}

/// Python `str()` of a value read from a record.
fn py_str(value: Option<&Out>) -> String {
    match value {
        None | Some(Out::Null) => "None".into(),
        Some(Out::Str(s)) => s.clone(),
        Some(Out::Bool(b)) => (if *b { "True" } else { "False" }).into(),
        Some(Out::Int(n)) => n.to_string(),
        Some(other) => other.dumps(false),
    }
}

/// `a or b`: the first truthy value.
fn or<'a>(a: Option<&'a Out>, b: Option<&'a Out>) -> Option<&'a Out> {
    a.filter(|v| v.truthy()).or(b)
}

/// Python `list[:limit]` with a possibly negative limit.
fn head<T: Clone>(rows: &[T], limit: i64) -> Vec<T> {
    let n = rows.len() as i64;
    let end = if limit < 0 {
        (n + limit).max(0)
    } else {
        limit.min(n)
    };
    rows[..end as usize].to_vec()
}

/// `approvals.guidance(limit)`.
pub fn guidance(rules: &Rules, records: &Out, limit: i64) -> Result<Vec<Out>, PyError> {
    let mut rows: Vec<Out> = validate_records(rules, records)?
        .into_iter()
        .filter(|a| {
            matches!(get_str(a, "status"), Some("approved" | "rejected"))
                && get(a, "note").is_some_and(Out::truthy)
        })
        .collect();
    let key = |a: &Out| match or(get(a, "decided_at"), get(a, "at")).filter(|v| v.truthy()) {
        Some(v) => py_str(Some(v)),
        None => String::new(),
    };
    // `sort(reverse=True)` is stable: equal keys keep their order.
    rows.sort_by_key(|a| std::cmp::Reverse(key(a)));
    Ok(head(&rows, limit)
        .into_iter()
        .map(|a| {
            let at = match get(&a, "decided_at").filter(|v| v.truthy()) {
                Some(v) => py_str(Some(v)),
                None => String::new(),
            };
            Out::Dict(vec![
                (
                    "status".into(),
                    get(&a, "status").cloned().unwrap_or(Out::Null),
                ),
                (
                    "title".into(),
                    get(&a, "title").cloned().unwrap_or(Out::Null),
                ),
                ("note".into(), get(&a, "note").cloned().unwrap_or(Out::Null)),
                ("at".into(), Out::Str(at)),
            ])
        })
        .collect())
}

/// `approvals.get`: the first record with the id.
pub fn get_item(rules: &Rules, records: &Out, item_id: &str) -> Result<Out, PyError> {
    validate_records(rules, records)?
        .into_iter()
        .find(|a| get_str(a, "id") == Some(item_id))
        .ok_or_else(|| key_error(&format!("unknown approval {item_id}")))
}

impl Out {
    /// Python truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Out::Null => false,
            Out::Bool(b) => *b,
            Out::Int(n) => *n != 0,
            Out::Num(s) => s.parse::<f64>().is_ok_and(|f| f != 0.0),
            Out::Str(s) => !s.is_empty(),
            Out::List(v) => !v.is_empty(),
            Out::Dict(v) => !v.is_empty(),
        }
    }
}
