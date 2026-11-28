//! Native CLI handlers owned by `rust-04` (see `steps/mod.rs`): `draft
//! route` and `draft judge`, the two access-ask commands (issue #314) over
//! the rust-04e decisions in `careerbrew_policy::access`. They read an ask
//! or draft file and a channel directory file, print ids, and write only
//! the `cli.run` row.
//!
//! Both serve only inputs whose meaning is certain in both languages:
//! - no file Python's `privacy.redact` reads is present (`say` redacts);
//! - no path is empty or starts with `~` (`expanduser` on the ask file);
//! - the ask or draft text is printable ASCII plus newline and tab (no
//!   carriage return, which Python's universal newlines rewrite);
//! - the directory file is printable ASCII plus newline, parses as JSON
//!   with no key given twice, every string printable ASCII, every number a
//!   plain integer, and every `\u` escape four hex digits, so
//!   `json.loads` reads exactly the same value;
//! - every read succeeds, except a missing file, whose error line is fixed
//!   text (plus the ask file's plain name).
//!
//! Anything else returns None before any write, and Python runs it.
use super::Ctx;
use crate::args::{Parsed, Value};
use crate::registry::{self, Json};
use crate::Outcome;
use careerbrew_policy::access;
use careerbrew_policy::vectors::Out;
use careerbrew_policy::Rules;

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust04.rs`.
pub const COMMANDS: &[&str] = &["draft.route", "draft.judge"];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.redact_inputs_present() {
        return None;
    }
    match name {
        "draft.route" => draft_route(args),
        "draft.judge" => draft_judge(args),
        _ => None,
    }
}

fn served(stdout: String, code: i32) -> Option<Outcome> {
    Some(Outcome {
        stdout,
        stderr: String::new(),
        code,
        emit_event: true,
    })
}

/// `cli._draft_fail`: `error: <msg> (breaks US-98)`.
fn fail(message: &str, code: i32) -> Option<Outcome> {
    served(format!("error: {message} (breaks US-98)\n"), code)
}

fn printable(ch: char) -> bool {
    (' '..='~').contains(&ch)
}

fn plain_text(text: &str) -> bool {
    text.chars()
        .all(|ch| ch == '\n' || ch == '\t' || printable(ch))
}

/// A path whose reading Python does not change (`expanduser`, `Path("")`).
fn plain_path(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('~')
}

/// `Path(path).name` for a path whose last part is a plain name, else None.
fn file_name(path: &str) -> Option<&str> {
    if path.contains("//") || path.contains("/./") {
        return None;
    }
    let last = path.rsplit('/').next()?;
    (!matches!(last, "" | "." | "..") && last.chars().all(printable)).then_some(last)
}

enum Read {
    Bytes(Vec<u8>),
    Missing,
}

/// The file's bytes, `Missing` for ENOENT, None for any other error.
fn read(path: &str) -> Option<Read> {
    match std::fs::read(path) {
        Ok(bytes) => Some(Read::Bytes(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(Read::Missing),
        Err(_) => None,
    }
}

/// `cli._draft_text`: the text, or the `ValueError` message of a missing file.
fn draft_text(path: &str) -> Option<Result<String, String>> {
    if !plain_path(path) {
        return None;
    }
    match read(path)? {
        Read::Bytes(bytes) => {
            let text = String::from_utf8(bytes).ok()?;
            plain_text(&text).then_some(Ok(text))
        }
        Read::Missing => Some(Err(format!(
            "could not read {}: FileNotFoundError",
            file_name(path)?
        ))),
    }
}

/// True when every `\u` escape in the raw JSON text has four hex digits.
fn plain_escapes(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'\\' {
            if bytes.get(at + 1) == Some(&b'u') {
                let hex = bytes.get(at + 2..at + 6);
                if !hex.is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)) {
                    return false;
                }
            }
            at += 2;
        } else {
            at += 1;
        }
    }
    true
}

/// `-?(0|[1-9][0-9]*)` with at most 18 digits: an `i64` both sides print alike.
fn plain_int(text: &str) -> Option<i64> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let ok = (1..=18).contains(&digits.len())
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'));
    ok.then(|| text.parse().ok()).flatten()
}

/// The JSON value as `json.loads` reads it, for the plain shapes only.
fn to_out(value: &Json) -> Option<Out> {
    Some(match value {
        Json::Null => Out::Null,
        Json::Bool(b) => Out::Bool(*b),
        Json::Number(text) => Out::Int(plain_int(text)?),
        Json::Str(text) => {
            if !text.chars().all(printable) {
                return None;
            }
            Out::Str(text.clone())
        }
        Json::Array(items) => Out::List(items.iter().map(to_out).collect::<Option<_>>()?),
        Json::Object(pairs) => {
            let mut out: Vec<(String, Out)> = Vec::new();
            for (key, value) in pairs {
                if !key.chars().all(printable) || out.iter().any(|(k, _)| k == key) {
                    return None;
                }
                out.push((key.clone(), to_out(value)?));
            }
            Out::Dict(out)
        }
    })
}

/// `access.load`: the validated directory, or the `AccessError` message.
fn load(path: &str) -> Option<Result<Out, String>> {
    if !plain_path(path) {
        return None;
    }
    let bytes = match read(path)? {
        Read::Bytes(bytes) => bytes,
        Read::Missing => {
            return Some(Err(
                "could not read the directory file: FileNotFoundError".into()
            ))
        }
    };
    let raw = String::from_utf8(bytes).ok()?;
    if !raw.chars().all(|ch| ch == '\n' || printable(ch)) || !plain_escapes(&raw) {
        return None;
    }
    let mut directory = to_out(&registry::parse(&raw).ok()?)?;
    // `d.setdefault("threads", [])`.
    if let Out::Dict(items) = &mut directory {
        if !items.iter().any(|(k, _)| k == "threads") {
            items.push(("threads".into(), Out::List(Vec::new())));
        }
    }
    match access::validate_directory(&directory) {
        Ok(directory) => Some(Ok(directory)),
        Err(error) if error.class == "AccessError" => Some(Err(error.message)),
        Err(_) => None,
    }
}

/// The text of an optional `--flag` value (`a.flag or ""`).
fn optional_text<'a>(args: &'a Parsed, name: &str) -> Option<&'a str> {
    match args.get(name) {
        Value::Missing => Some(""),
        Value::Text(text) => Some(text),
        _ => None,
    }
}

fn field<'a>(r: &'a Out, key: &str) -> Option<&'a Out> {
    access::get(r, key)
}

fn text<'a>(r: &'a Out, key: &str) -> Option<&'a str> {
    match field(r, key)? {
        Out::Str(s) => Some(s),
        _ => None,
    }
}

// ---------- draft route ----------

fn draft_route(args: &Parsed) -> Option<Outcome> {
    let ask = args.text("ask")?;
    let path = args.text("directory")?;
    let teammate = optional_text(args, "teammate")?;
    if !teammate.chars().all(printable) {
        return None;
    }
    let ask = match draft_text(ask)? {
        Ok(text) => text,
        Err(message) => return fail(&message, 2),
    };
    let directory = match load(path)? {
        Ok(directory) => directory,
        Err(message) => return fail(&message, 2),
    };
    let r = access::route(&Rules::python(), &ask, &directory, teammate).ok()?;
    if args.flag("json") {
        // `print(json.dumps(r))`: ASCII escapes, `, ` and `: ` separators.
        return served(format!("{}\n", r.dumps(true)), 0);
    }
    let action = text(&r, "action")?;
    let where_ = match action {
        "link" => format!(
            "link the open thread {} in {}",
            text(&r, "thread")?,
            text(&r, "channel")?
        ),
        "open" => format!("draft the ask in {}", text(&r, "channel")?),
        other => other.to_owned(),
    };
    let mut line = format!("access route: {where_} ({})", text(&r, "reason")?);
    match field(&r, "tag") {
        None => {}
        Some(Out::Str(tag)) if tag.is_empty() => {}
        Some(Out::Str(tag)) => line.push_str(&format!("; teammate: {tag}")),
        Some(_) => return None,
    }
    line.push('\n');
    line.chars()
        .all(|ch| ch == '\n' || printable(ch))
        .then_some(())?;
    served(line, 0)
}

// ---------- draft judge ----------

fn draft_judge(args: &Parsed) -> Option<Outcome> {
    let draft = args.text("draft")?;
    let channel = args.text("channel")?;
    let path = args.text("directory")?;
    if !channel.chars().all(printable) {
        return None;
    }
    let draft = match draft_text(draft)? {
        Ok(text) => text,
        Err(message) => return fail(&message, 2),
    };
    let directory = match load(path)? {
        Ok(directory) => directory,
        Err(message) => return fail(&message, 2),
    };
    let fails = access::judge(
        &Rules::python(),
        &draft,
        channel,
        &directory,
        args.flag("new_thread"),
    )
    .ok()?;
    if fails.is_empty() {
        return served(format!("draft judge: passes for {channel}\n"), 0);
    }
    let message = format!(
        "draft judge: {} (fix the draft and judge it again; never send)",
        fails.join("; ")
    );
    if !message.chars().all(printable) {
        return None;
    }
    fail(&message, 1)
}
