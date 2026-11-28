//! Native CLI handlers owned by `rust-10` (see `steps/mod.rs`): `draft fit`
//! and `draft check`, the two reply-draft commands that only read files and
//! append event rows.
//!
//! Both serve only inputs whose meaning is certain in both languages:
//! - every text they judge is printable ASCII plus newline and tab (no
//!   carriage return, which Python's universal newlines would rewrite on
//!   read, and none of the separators `str.isspace` and Rust disagree on);
//! - no path starts with `~` (Python's `expanduser`);
//! - no file Python's `privacy.redact` reads is present (`say` redacts);
//! - every read succeeds, except a missing `draft check` file, whose error
//!   line is fixed text plus the file name.
//!
//! Anything else returns None before any write, and Python runs it.
//! The work itself is the rust-10 component (`careerbrew_sessions`).
use super::Ctx;
use crate::args::Parsed;
use crate::registry::{self, Json};
use crate::{events, Outcome};
use std::io::Write;
use std::path::Path;

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust10.rs`.
pub const COMMANDS: &[&str] = &["draft.fit", "draft.check"];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.redact_inputs_present() {
        return None;
    }
    match name {
        "draft.fit" => draft_fit(args, ctx),
        "draft.check" => draft_check(args),
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

/// A character both Python and Rust read the same way in these commands.
fn plain_char(ch: char) -> bool {
    ch == '\n' || ch == '\t' || (' '..='~').contains(&ch)
}

fn plain_text(text: &str) -> bool {
    text.chars().all(plain_char)
}

/// A text `draft check` may compare: Python's `str.rstrip` and Rust's
/// `trim_end` strip the same characters, and `len` counts the same units.
fn comparable(text: &str) -> bool {
    text.chars()
        .all(|ch| !('\u{1c}'..='\u{1f}').contains(&ch) && (ch.is_ascii() || !ch.is_whitespace()))
}

/// A path whose reading Python does not change (`expanduser`).
fn plain_path(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('~')
}

/// `Path(path).name` for a path whose last part is a plain name, else None.
fn file_name(path: &str) -> Option<&str> {
    let last = path.rsplit('/').next()?;
    (!matches!(last, "" | "." | "..") && last.bytes().all(|b| (b' '..=b'~').contains(&b)))
        .then_some(last)
}

enum Read {
    Text(String),
    Missing,
}

/// The file as UTF-8 text, `Missing` for ENOENT, None for anything else.
fn read(path: &str) -> Option<Read> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).ok().map(Read::Text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(Read::Missing),
        Err(_) => None,
    }
}

fn read_text(path: &str) -> Option<String> {
    match read(path)? {
        Read::Text(text) => Some(text),
        Read::Missing => None,
    }
}

// ---------- draft check ----------

fn draft_check(args: &Parsed) -> Option<Outcome> {
    let sent = args.text("sent")?;
    let back = args.text("read_back")?;
    if !plain_path(sent) || !plain_path(back) {
        return None;
    }
    let mut texts = Vec::new();
    for path in [sent, back] {
        match read(path)? {
            Read::Text(text) if comparable(&text) => texts.push(text),
            Read::Text(_) => return None,
            Read::Missing => {
                let name = file_name(path)?;
                return served(
                    format!("error: could not read {name}: FileNotFoundError (breaks US-98)\n"),
                    2,
                );
            }
        }
    }
    let row = careerbrew_sessions::drafts::compare(&texts[0], Some(&texts[1]));
    let sent_chars = row["sent_chars"].as_u64()?;
    let read_chars = row["read_chars"].as_u64()?;
    if row["same"].as_bool()? {
        return served(
            format!("draft check: the read-back matches ({sent_chars} characters)\n"),
            0,
        );
    }
    let line = row["line"].as_u64()?;
    served(
        format!("error: the read-back differs at line {line} ({read_chars} vs {sent_chars} characters): the chat tool kept other text (breaks US-98)\n"),
        1,
    )
}

// ---------- draft fit ----------

/// True for a JSON object with no key given twice (Python keeps the last).
fn unique_keys(pairs: &[(String, Json)]) -> bool {
    pairs
        .iter()
        .enumerate()
        .all(|(i, (key, _))| pairs[..i].iter().all(|(other, _)| other != key))
}

/// A decimal ts both sides read as the same float: digits, an optional
/// fraction, few enough digits to stay exact.
fn plain_ts_text(text: &str) -> bool {
    let mut parts = text.splitn(2, '.');
    let whole = parts.next().unwrap_or("");
    let frac = parts.next();
    (1..=15).contains(&whole.len())
        && whole.bytes().all(|b| b.is_ascii_digit())
        && frac.is_none_or(|f| (1..=6).contains(&f.len()) && f.bytes().all(|b| b.is_ascii_digit()))
}

/// A JSON integer `json.dumps` and serde print with the same digits.
fn plain_ts_number(text: &str) -> bool {
    (1..=15).contains(&text.len())
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'))
}

/// One thread message whose reading is certain on both sides.
fn plain_message(message: &Json) -> bool {
    let Json::Object(pairs) = message else {
        return false;
    };
    if !unique_keys(pairs) {
        return false;
    }
    let text_ok = match message.get("text") {
        None => true,
        Some(Json::Str(text)) => plain_text(text),
        Some(_) => false,
    };
    let ts_ok = match message.get("ts") {
        None => true,
        Some(Json::Str(text)) => plain_ts_text(text),
        Some(Json::Number(text)) => plain_ts_number(text),
        Some(_) => false,
    };
    let mine_ok = matches!(message.get("mine"), None | Some(Json::Bool(_)));
    // `author` only counts when it is the string "me" on both sides.
    let author_ok = matches!(message.get("author"), None | Some(Json::Str(_)));
    text_ok && ts_ok && mine_ok && author_ok
}

/// The thread file's JSON, when every message is plain.
fn plain_thread(raw: &str) -> bool {
    let Ok(thread) = registry::parse(raw) else {
        return false;
    };
    let messages = match &thread {
        Json::Object(pairs) => {
            if !unique_keys(pairs) {
                return false;
            }
            thread.get("messages")
        }
        Json::Array(_) => Some(&thread),
        _ => None,
    };
    match messages.and_then(Json::as_array) {
        Some(items) if !items.is_empty() => items.iter().all(plain_message),
        _ => false,
    }
}

fn bool_of(row: &Json, key: &str) -> Option<bool> {
    match row.get(key)? {
        Json::Bool(value) => Some(*value),
        _ => None,
    }
}

/// The `draft.fit` row `events.emit` writes, keys sorted, no `via`.
fn fit_row(kind: &str, stale: bool, started: bool) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut out = format!(
        "{{\"deploy_started\": {started}, \"event\": \"draft.fit\", \"stale\": {stale}, \"status\": "
    );
    crate::render::py_string(kind, &mut out);
    out.push_str(", \"ts\": ");
    crate::render::py_string(&events::stamp(secs), &mut out);
    out.push_str(", \"v\": ");
    crate::render::py_string(events::version(), &mut out);
    out.push_str("}\n");
    out
}

fn append_event(ctx: &Ctx, line: &str) {
    if !ctx.env.caps.events_on {
        return;
    }
    let logs = ctx.root.join("logs");
    if std::fs::create_dir_all(&logs).is_err() {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("events.jsonl"))
    {
        let _ = file.write_all(line.as_bytes());
    }
}

fn draft_fit(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let thread = args.text("thread")?;
    if !plain_path(thread) || !plain_thread(&read_text(thread)?) {
        return None;
    }
    // Python reads `--draft ""` as no draft.
    let draft = args.text("draft").filter(|path| !path.is_empty());
    if let Some(path) = draft {
        // `plain_text` also leaves out `\r`, which Python's read would rewrite.
        if !plain_path(path) || !plain_text(&read_text(path)?) {
            return None;
        }
    }
    let seen = args.text("seen");
    if seen.is_some_and(|value| !plain_ts_text(value)) {
        return None;
    }
    let major = args.text("major");
    if major.is_some_and(|value| {
        !value
            .chars()
            .all(|ch| ch == '\t' || (' '..='~').contains(&ch))
    }) {
        return None;
    }
    let (code, json) = careerbrew_sessions::cli::draft_fit(
        &ctx.root,
        Path::new(thread),
        draft.map(Path::new),
        seen,
        major,
        true,
    );
    if code != 0 || !json.is_ascii() {
        return None;
    }
    let row = registry::parse(&json).ok()?;
    let kind = row.get("kind")?.as_str()?;
    let reason = row.get("reason")?.as_str()?;
    let stale = bool_of(&row, "stale")?;
    let started = bool_of(&row, "deploy_started")?;
    let keep = bool_of(&row, "keep_draft")?;
    let emoji = row.get("emoji")?.as_str().filter(|e| !e.is_empty());
    let text = row.get("text")?.as_str().filter(|t| !t.is_empty());
    let Json::Number(newer) = row.get("newer")? else {
        return None;
    };
    if !matches!(kind, "none" | "emoji" | "ack" | "stop" | "reply") {
        return None;
    }
    let stdout = if args.flag("json") {
        json
    } else {
        let mut out = format!("draft: {kind} ({reason})\n");
        if let Some(emoji) = emoji {
            out.push_str(&format!("emoji: :{emoji}:\n"));
        }
        if let Some(text) = text {
            out.push_str(&format!("text: {text}\n"));
        }
        if stale {
            out.push_str(&format!(
                "stale: {newer} newer message(s) since the one your draft answers\n"
            ));
        }
        if matches!(kind, "ack" | "stop" | "reply") && (text.is_some() || keep) {
            out.push_str("save it: careerbrew draft replace <target> --file <draft file>\n");
        }
        out
    };
    append_event(ctx, &fit_row(kind, stale, started));
    served(stdout, 0)
}
