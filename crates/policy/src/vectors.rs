//! The rust-04 vector sets this crate answers, in the oracle's answer shape
//! (`tests/oracles/rust04.py`): `canary-core`, `canary-match`, `leak-spans`,
//! `assert-redact`, `allowlists`, `public-scrub`, `private-path`,
//! `approvals`, `receipt-validate`, `access` and `assert-no-echo`. The
//! `policy-vectors` binary reads the file; this module only maps parsed
//! cases to outputs.

use crate::access;
use crate::allow::{self, PyError};
use crate::approvals;
use crate::canary::{self, Rules};
use crate::outputs;
use crate::scrub;
use crate::tables;
use crate::text;
use serde_json::Value;

/// Every rust-04 set (docs/rust-migration.md, rust-04 "Vector contract").
pub const SETS: &[&str] = &[
    "canary-core",
    "canary-match",
    "leak-spans",
    "assert-redact",
    "allowlists",
    "public-scrub",
    "private-path",
    "approvals",
    "receipt-validate",
    "access",
    "assert-no-echo",
];

/// The Unicode table version every rule here follows (Python's).
pub const UNICODE_VERSION: &str = tables::UNIDATA_VERSION;

/// A JSON value with Python's key order (insertion order).
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    Null,
    Bool(bool),
    Int(i64),
    /// A JSON number that is not an `i64`, in serde_json's text.
    Num(String),
    Str(String),
    List(Vec<Out>),
    Dict(Vec<(String, Out)>),
}

impl Out {
    /// A parsed JSON value, keys in the parser's order (the generator sorts them).
    pub fn from_value(v: &Value) -> Out {
        match v {
            Value::Null => Out::Null,
            Value::Bool(b) => Out::Bool(*b),
            Value::Number(n) => n.as_i64().map_or_else(|| Out::Num(n.to_string()), Out::Int),
            Value::String(s) => Out::Str(s.clone()),
            Value::Array(items) => Out::List(items.iter().map(Out::from_value).collect()),
            Value::Object(map) => Out::Dict(
                map.iter()
                    .map(|(k, v)| (k.clone(), Out::from_value(v)))
                    .collect(),
            ),
        }
    }

    fn str(s: impl Into<String>) -> Out {
        Out::Str(s.into())
    }

    fn strs(items: impl IntoIterator<Item = String>) -> Out {
        Out::List(items.into_iter().map(Out::Str).collect())
    }

    fn dict(items: Vec<(&str, Out)>) -> Out {
        Out::Dict(items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }

    fn int(n: usize) -> Out {
        Out::Int(i64::try_from(n).unwrap_or(i64::MAX))
    }

    /// `json.dumps(value)` (`ensure_ascii=True`) or, with `ascii` false,
    /// `json.dumps(value, ensure_ascii=False)`.
    pub fn dumps(&self, ascii: bool) -> String {
        let mut out = String::new();
        self.write(&mut out, ascii);
        out
    }

    fn write(&self, out: &mut String, ascii: bool) {
        match self {
            Out::Null => out.push_str("null"),
            Out::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Out::Int(n) => out.push_str(&n.to_string()),
            Out::Num(n) => out.push_str(n),
            Out::Str(s) => write_str(out, s, ascii),
            Out::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.write(out, ascii);
                }
                out.push(']');
            }
            Out::Dict(items) => {
                out.push('{');
                for (i, (key, value)) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write_str(out, key, ascii);
                    out.push_str(": ");
                    value.write(out, ascii);
                }
                out.push('}');
            }
        }
    }
}

fn write_str(out: &mut String, s: &str, ascii: bool) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (ascii && !(' '..='~').contains(&c)) => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

impl From<PyError> for Out {
    fn from(err: PyError) -> Out {
        Out::dict(vec![
            ("class", Out::str(err.class)),
            ("message", Out::Str(err.message)),
        ])
    }
}

fn key_error(key: &str) -> PyError {
    PyError::new("KeyError", format!("'{key}'"))
}

fn field<'a>(case: &'a Value, key: &str) -> Result<&'a Value, PyError> {
    case.get(key).ok_or_else(|| key_error(key))
}

/// Inputs outside the generator's shapes are out of contract; they are
/// refused with this fixed error instead of guessing Python's message.
fn unsupported() -> PyError {
    PyError::new("TypeError", "unsupported input shape")
}

fn string<'a>(case: &'a Value, key: &str) -> Result<&'a str, PyError> {
    field(case, key)?.as_str().ok_or_else(unsupported)
}

fn strings(case: &Value, key: &str) -> Result<Vec<String>, PyError> {
    field(case, key)?
        .as_array()
        .ok_or_else(unsupported)?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or_else(unsupported))
        .collect()
}

fn canary_core(case: &Value) -> Result<Out, PyError> {
    let op = string(case, "op")?;
    Ok(match op {
        "sensitivities" => Out::strs(allow::SENSITIVITIES.iter().map(|s| (*s).to_owned())),
        "term-id" => Out::Str(canary::term_id(string(case, "term")?)),
        "normalize" => Out::Str(text::normalize(string(case, "text")?)),
        "term-pattern" => {
            let source = canary::term_pattern_source(string(case, "term")?);
            Out::dict(vec![
                ("sha256", Out::Str(canary::sha256_hex(&source))),
                ("flags", Out::Int(i64::from(canary::PATTERN_FLAGS))),
            ])
        }
        other => return Err(PyError::new("ValueError", format!("unknown op {other}"))),
    })
}

fn canary_match(rules: &Rules, case: &Value) -> Result<Out, PyError> {
    let text_in = string(case, "text")?;
    let terms = strings(case, "terms")?;
    Ok(Out::strs(rules.find_leaks(text_in, &terms)))
}

fn leak_spans(rules: &Rules, case: &Value) -> Result<Out, PyError> {
    let text_in = string(case, "text")?;
    let terms = strings(case, "terms")?;
    // The matched text is for the owner's eyes only: report its length.
    Ok(Out::List(
        rules
            .leak_spans(text_in, &terms)
            .into_iter()
            .map(|span| {
                Out::List(vec![
                    Out::Str(span.term_id),
                    Out::int(span.line),
                    Out::Str(span.how),
                    Out::int(text::len(&span.matched)),
                ])
            })
            .collect(),
    ))
}

fn assert_redact(rules: &Rules, case: &Value) -> Result<Out, PyError> {
    let op = string(case, "op")?;
    let text_in = string(case, "text")?;
    if op == "assert-clean" {
        let place = string(case, "where")?;
        let terms = strings(case, "terms")?;
        outputs::assert_clean(rules, text_in, place, &terms)?;
        return Ok(Out::str("clean"));
    }
    let terms = strings(case, "terms")?;
    Ok(Out::Str(rules.redact(text_in, &terms)))
}

fn private_path(case: &Value) -> Result<Out, PyError> {
    let text_in = string(case, "text")?;
    Ok(Out::Bool(outputs::names_private_path(text_in)))
}

fn allowlists(case: &Value) -> Result<Out, PyError> {
    let op = string(case, "op")?;
    Ok(match op {
        "constants" => Out::dict(vec![
            (
                "template_placeholders",
                Out::strs(allow::sorted(allow::TEMPLATE_PLACEHOLDERS)),
            ),
            ("tech_words", Out::strs(allow::sorted(allow::TECH_WORDS))),
            (
                "common_compounds",
                Out::strs(allow::sorted(allow::COMMON_COMPOUNDS)),
            ),
            (
                "common_camel",
                Out::strs(allow::sorted(allow::COMMON_CAMEL)),
            ),
        ]),
        "filter-terms" => {
            let file = string(case, "file")?;
            let profile = field(case, "profile")?;
            let plugins = strings(case, "plugin_files")?;
            let plugins: Vec<&str> = plugins.iter().map(String::as_str).collect();
            // The oracle writes the term file only when its text is non-empty.
            let file = (!file.is_empty()).then_some(file);
            let terms = allow::filter_terms(file, profile, &plugins)?;
            Out::strs(terms.iter().map(|t| canary::term_id(t)))
        }
        "identifier-shaped" => Out::Bool(allow::identifier_shaped(string(case, "token")?)),
        "public-suspects" => Out::strs(allow::public_suspects(string(case, "text")?)),
        other => return Err(PyError::new("ValueError", format!("unknown op {other}"))),
    })
}

fn public_scrub(rules: &Rules, case: &Value) -> Result<Out, PyError> {
    let text_in = string(case, "text")?;
    let terms = strings(case, "terms")?;
    let replacement = match case.get("replacement") {
        Some(_) => string(case, "replacement")?,
        None => scrub::REPLACEMENT,
    };
    Ok(Out::Str(scrub::scrub_public(
        rules,
        text_in,
        &terms,
        replacement,
    )?))
}

fn opt_string<'a>(case: &'a Value, key: &str, default: &'a str) -> Result<&'a str, PyError> {
    match case.get(key) {
        Some(_) => string(case, key),
        None => Ok(default),
    }
}

/// The oracle's `_approvals_call`: `{"value", "written"}` over supplied records.
fn approvals_call(rules: &Rules, case: &Value) -> Result<Out, PyError> {
    let records = Out::from_value(field(case, "records")?);
    let now = opt_string(case, "now", "2026-01-01T00:00:00+00:00")?;
    let new_id = opt_string(case, "new_id", "00000000")?;
    let op = string(case, "op")?;
    let (value, written) = match op {
        "request" => {
            let (item, saved) = approvals::request_item(
                rules,
                &records,
                string(case, "title")?,
                string(case, "detail")?,
                string(case, "by")?,
                now,
                new_id,
            )?;
            (item, vec![Out::List(saved)])
        }
        "decide" => {
            let (item, saved) = approvals::decide_items(
                rules,
                &records,
                string(case, "item")?,
                string(case, "decision")?,
                string(case, "note")?,
                now,
            )?;
            (item, vec![Out::List(saved)])
        }
        "pending" => (Out::List(approvals::pending(rules, &records)?), vec![]),
        "guidance" => {
            let limit = match case.get("limit") {
                Some(v) => v.as_i64().ok_or_else(unsupported)?,
                None => approvals::GUIDANCE_MAX,
            };
            (
                Out::List(approvals::guidance(rules, &records, limit)?),
                vec![],
            )
        }
        "get" => (
            approvals::get_item(rules, &records, string(case, "item")?)?,
            vec![],
        ),
        "validate-records" => (
            Out::List(approvals::validate_records(rules, &records)?),
            vec![],
        ),
        other => return Err(PyError::new("ValueError", format!("unknown op {other}"))),
    };
    Ok(Out::dict(vec![
        ("value", value),
        ("written", Out::List(written)),
    ]))
}

fn access_call(rules: &Rules, case: &Value) -> Result<Out, PyError> {
    let op = string(case, "op")?;
    let dir = |case: &Value| field(case, "directory").map(Out::from_value);
    Ok(match op {
        "blocker" => Out::Bool(access::blocker(rules, string(case, "text")?)?),
        "owner" => {
            let (channel, topic) = access::owner(string(case, "text")?, &dir(case)?)?;
            let id = match channel {
                Some(c) => access::get(&c, "id")
                    .cloned()
                    .ok_or_else(|| PyError::new("KeyError", "'id'"))?,
                None => Out::Null,
            };
            Out::dict(vec![("channel", id), ("topic", Out::Str(topic))])
        }
        "route" => access::route(
            rules,
            string(case, "text")?,
            &dir(case)?,
            string(case, "teammate")?,
        )?,
        "topic" => access::topic(
            string(case, "text")?,
            &Out::from_value(field(case, "channel")?),
        )?,
        "existing" => access::existing(
            &Out::from_value(field(case, "channel")?),
            string(case, "topic")?,
            &dir(case)?,
        )?
        .unwrap_or(Out::Null),
        "mentions" => Out::strs(access::mentions(rules, string(case, "text")?)?),
        "judge" => {
            let new_thread = field(case, "new_thread")?
                .as_bool()
                .ok_or_else(unsupported)?;
            Out::strs(access::judge(
                rules,
                string(case, "draft")?,
                string(case, "channel")?,
                &dir(case)?,
                new_thread,
            )?)
        }
        "validate-directory" => access::validate_directory(&dir(case)?)?,
        other => return Err(PyError::new("ValueError", format!("unknown op {other}"))),
    })
}

fn no_echo(case: &Value) -> Result<Out, PyError> {
    let terms = field(case, "terms")?.as_array().ok_or_else(unsupported)?;
    Ok(Out::dict(vec![
        ("terms", Out::int(terms.len())),
        ("checked_by", Out::str("every-other-set")),
    ]))
}

/// A run that must stop: a set this crate does not answer, or an output
/// that holds term text (the oracle's exit 3).
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    UnknownSet(String),
    Echo(String),
    Malformed,
}

/// Each case's `{"id", "output"}` row for one set, then the no-echo check
/// against every term of the file, like `rust04.run_set`.
pub fn run_set(
    rules: &Rules,
    name: &str,
    cases: &[Value],
    terms: &[String],
) -> Result<Vec<Out>, Refusal> {
    let mut rows = Vec::new();
    for case in cases {
        let id = case
            .get("id")
            .and_then(Value::as_str)
            .ok_or(Refusal::Malformed)?;
        let answer = match name {
            "canary-core" => canary_core(case),
            "canary-match" => canary_match(rules, case),
            "leak-spans" => leak_spans(rules, case),
            "assert-redact" => assert_redact(rules, case),
            "allowlists" => allowlists(case),
            "private-path" => private_path(case),
            "public-scrub" => public_scrub(rules, case),
            "approvals" | "receipt-validate" => approvals_call(rules, case),
            "access" => access_call(rules, case),
            "assert-no-echo" => no_echo(case),
            other => return Err(Refusal::UnknownSet(other.to_owned())),
        };
        let output = match answer {
            Ok(value) => Out::dict(vec![("ok", value)]),
            Err(err) => Out::dict(vec![("error", err.into())]),
        };
        rows.push((id.to_owned(), output));
    }
    if name != "assert-no-echo" {
        for (id, output) in &rows {
            let text_out = output.dumps(false);
            if !rules.find_leaks(&text_out, terms).is_empty() {
                return Err(Refusal::Echo(format!(
                    "{name}/{id}: output holds term text"
                )));
            }
        }
    }
    Ok(rows
        .into_iter()
        .map(|(id, output)| Out::dict(vec![("id", Out::Str(id)), ("output", output)]))
        .collect())
}

/// The whole answer line for one set of a parsed vector file:
/// `{"set", "unidata_version", "results"}`.
pub fn answer(rules: &Rules, name: &str, file: &Value) -> Result<String, Refusal> {
    let sets = file.get("sets").and_then(Value::as_object);
    let terms = file.get("terms").and_then(Value::as_array);
    let (Some(sets), Some(terms)) = (sets, terms) else {
        return Err(Refusal::Malformed);
    };
    let terms: Vec<String> = terms
        .iter()
        .map(|t| t.as_str().map(str::to_owned).ok_or(Refusal::Malformed))
        .collect::<Result<_, _>>()?;
    let cases = sets
        .get(name)
        .filter(|_| SETS.contains(&name))
        .ok_or_else(|| Refusal::UnknownSet(name.to_owned()))?
        .as_array()
        .ok_or(Refusal::Malformed)?;
    let results = run_set(rules, name, cases, &terms)?;
    Ok(Out::dict(vec![
        ("set", Out::str(name)),
        ("unidata_version", Out::str(UNICODE_VERSION)),
        ("results", Out::List(results)),
    ])
    .dumps(true))
}
