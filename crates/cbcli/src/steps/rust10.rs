//! Native CLI handlers owned by `rust-10` (see `steps/mod.rs`): `draft fit`
//! and `draft check`, the two reply-draft commands that only read files and
//! append event rows, and (in `work`, below) `work agent event` and `work
//! claim take` / `release`, which write the agent log and the session store.
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
pub const COMMANDS: &[&str] = &[
    "draft.fit",
    "draft.check",
    "work.agent.event",
    "work.claim.take",
    "work.claim.release",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.redact_inputs_present() {
        return None;
    }
    match name {
        "draft.fit" => draft_fit(args, ctx),
        "draft.check" => draft_check(args),
        "work.agent.event" => work::agent_event(args, ctx),
        "work.claim.take" => work::claim_take(args, ctx),
        "work.claim.release" => work::claim_release(args, ctx),
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

// ---------- work agent event, work claim take / release ----------

/// `work agent event`, `work claim take` and `work claim release`.
///
/// They serve only inputs whose reading is certain on both sides:
/// - every argument and every stored text they print or compare is printable
///   ASCII (Python's `strip`, `split`, `len` and regexes then read it as Rust
///   does);
/// - every stored stamp is `YYYY-MM-DDTHH:MM:SS` with an optional `+HH:MM`
///   offset (a claim's `until` must have one), and every date `YYYY-MM-DD`;
/// - the settings they read are plain integers or absent;
/// - an error is served only when its text is fixed or built from such
///   input; an error Python prints with `repr`, one that names a claim
///   holder (its text depends on `may_show_business`), and one Python does
///   not catch all refuse.
///
/// Every refusal comes before any write. A store that changed between the
/// first read and the read under its lock refuses after the lock file is
/// made, a file Python makes too.
mod work {
    use super::super::Ctx;
    use crate::args::Parsed;
    use crate::Outcome;
    use careerbrew_career::when;
    use careerbrew_vault::canon::Canon;
    use careerbrew_vault::store::Store;
    use std::io::Write;
    use std::path::Path;

    type Pairs = Vec<(Canon, Canon)>;

    const SESSIONS: &str = "work/sessions.json";
    const AGENT_LOG: &str = "work/agents/events.jsonl";
    const USE_KINDS: &[&str] = &["browser", "slack-draft", "file", "checkout"];

    fn served(stdout: String, code: i32) -> Option<Outcome> {
        Some(Outcome {
            stdout,
            stderr: String::new(),
            code,
            emit_event: true,
        })
    }

    /// Printable ASCII: no character Python and Rust classify differently.
    fn plain(text: &str) -> bool {
        text.bytes().all(|b| (b' '..=b'~').contains(&b))
    }

    /// `str.strip()` for printable ASCII.
    fn strip(text: &str) -> &str {
        text.trim_matches(' ')
    }

    /// `" ".join(str(value).split())` for printable ASCII.
    fn one_line(text: &str) -> String {
        text.split(' ')
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    // ---- stored values ----

    /// Every key is text and none is given twice (Python keeps the last).
    fn keys_unique(pairs: &Pairs) -> bool {
        pairs.iter().enumerate().all(|(i, (key, _))| {
            matches!(key, Canon::Str(_)) && pairs[..i].iter().all(|(other, _)| other != key)
        })
    }

    fn map(value: &Canon) -> Option<&Pairs> {
        match value {
            Canon::Map(pairs) if keys_unique(pairs) => Some(pairs),
            _ => None,
        }
    }

    fn get<'a>(pairs: &'a [(Canon, Canon)], key: &str) -> Option<&'a Canon> {
        pairs
            .iter()
            .find(|(k, _)| matches!(k, Canon::Str(k) if k == key))
            .map(|(_, v)| v)
    }

    fn text<'a>(pairs: &'a [(Canon, Canon)], key: &str) -> Option<&'a str> {
        match get(pairs, key)? {
            Canon::Str(value) => Some(value),
            _ => None,
        }
    }

    fn list_mut<'a>(pairs: &'a mut Pairs, key: &str) -> Option<&'a mut Vec<Canon>> {
        pairs.iter_mut().find_map(|(k, v)| match (k, v) {
            (Canon::Str(k), Canon::List(items)) if k == key => Some(items),
            _ => None,
        })
    }

    fn list<'a>(pairs: &'a [(Canon, Canon)], key: &str) -> &'a [Canon] {
        match get(pairs, key) {
            Some(Canon::List(items)) => items,
            _ => &[],
        }
    }

    /// Python truthiness of a stored value.
    fn truthy(value: &Canon) -> bool {
        match value {
            Canon::Null => false,
            Canon::Bool(b) => *b,
            Canon::Int(n) => n.to_string() != "0",
            Canon::Float(f) => *f != 0.0,
            Canon::Str(s) | Canon::Date(s) | Canon::DateTime(s) => !s.is_empty(),
            Canon::List(items) => !items.is_empty(),
            Canon::Map(items) => !items.is_empty(),
        }
    }

    /// A float anywhere in the value: its printed form and its equality may
    /// differ from Python's (`repr`, NaN), so a store holding one refuses.
    fn has_float(value: &Canon) -> bool {
        match value {
            Canon::Float(_) => true,
            Canon::List(items) => items.iter().any(has_float),
            Canon::Map(pairs) => pairs.iter().any(|(k, v)| has_float(k) || has_float(v)),
            _ => false,
        }
    }

    /// The file's text, `Some(None)` when it does not exist, None on any read
    /// error (a sealed file is read without making a key).
    fn read_text(store: &Store, path: &Path) -> Option<Option<String>> {
        if !path.exists() {
            return Some(None);
        }
        store.read_text(path, None, false).ok()
    }

    /// `store.read_yaml(path, {}) or {}` as a mapping with text keys.
    fn read_map_yaml(store: &Store, path: &Path) -> Option<Pairs> {
        match store.read_yaml(path, Canon::Map(Vec::new()), false).ok()? {
            Canon::Map(pairs) if keys_unique(&pairs) => Some(pairs),
            other if !truthy(&other) => Some(Vec::new()),
            _ => None,
        }
    }

    /// `int(config.get(key))` for a setting that is absent or an integer.
    fn setting_int(settings: &Pairs, key: &str, default: i64) -> Option<i64> {
        match get(settings, key) {
            None => Some(default),
            Some(Canon::Int(n)) => n.to_string().parse().ok(),
            Some(_) => None,
        }
    }

    // ---- the clock ----

    /// The zone name `calendar.local_tz()` uses (copied from `rust18`, which
    /// keeps it private): the `timezone` setting, else `TZ`, else the
    /// `/etc/localtime` link; None when the Rust zone table lacks it.
    fn zone_name(settings: &Pairs) -> Option<String> {
        let setting = get(settings, "timezone");
        let name = if setting.is_some_and(truthy) {
            match setting? {
                Canon::Str(name) => name.clone(),
                _ => return None,
            }
        } else {
            let env = std::env::var("TZ").unwrap_or_default();
            let env = env.trim_start_matches(':').to_owned();
            if env.is_empty() {
                let link = std::fs::canonicalize("/etc/localtime").ok()?;
                let link = link.to_str()?.to_owned();
                link.split_once("/zoneinfo/")?.1.to_owned()
            } else {
                env
            }
        };
        if name.is_empty() || when::zone(&name).is_err() {
            return None;
        }
        Some(name)
    }

    /// `calendar.now()`, and the zone it carries (None for a pinned clock
    /// with its own offset, which Python keeps as a fixed zone).
    struct Clock {
        at: when::At,
        zone: Option<String>,
    }

    fn clock(settings: &Pairs) -> Option<Clock> {
        let pinned = std::env::var("CAREERBREW_NOW")
            .ok()
            .filter(|value| !value.is_empty());
        if let Some(pinned) = pinned {
            let (naive, offset) = when::parse(&pinned).ok()?;
            return Some(match offset {
                Some(offset) => Clock {
                    at: when::fixed(naive, offset),
                    zone: None,
                },
                None => {
                    let zone = zone_name(settings)?;
                    Clock {
                        at: when::attach(naive, when::zone(&zone).ok()?),
                        zone: Some(zone),
                    }
                }
            });
        }
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        let (naive, offset) = when::parse(&crate::events::stamp(secs)).ok()?;
        let zone = zone_name(settings)?;
        Some(Clock {
            at: when::into(&when::fixed(naive, offset?), when::zone(&zone).ok()?),
            zone: Some(zone),
        })
    }

    fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    fn civil_from_days(days: i64) -> (i64, i64, i64) {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        (yoe + era * 400 + i64::from(month <= 2), month, day)
    }

    fn date_text(days: i64) -> String {
        let (y, m, d) = civil_from_days(days);
        format!("{y:04}-{m:02}-{d:02}")
    }

    /// `YYYY-MM-DDTHH:MM:SS` for seconds since the epoch (no offset).
    fn naive_text(secs: i64) -> String {
        let rest = secs.rem_euclid(86_400);
        format!(
            "{}T{:02}:{:02}:{:02}",
            date_text(secs.div_euclid(86_400)),
            rest / 3600,
            rest % 3600 / 60,
            rest % 60
        )
    }

    fn digits(text: &str, from: usize, to: usize) -> Option<i64> {
        let part = text.get(from..to)?;
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    }

    /// A `YYYY-MM-DD` that `date.fromisoformat` reads and prints back
    /// unchanged, as days since the epoch.
    fn day_of(text: &str) -> Option<i64> {
        let b = text.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let (y, m, d) = (
            digits(text, 0, 4)?,
            digits(text, 5, 7)?,
            digits(text, 8, 10)?,
        );
        let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let len = match m {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            1..=12 => 31,
            _ => return None,
        };
        (y >= 1 && (1..=len).contains(&d)).then(|| days_from_civil(y, m, d))
    }

    /// `YYYY-MM-DDTHH:MM:SS`, then nothing or `+HH:MM` / `-HH:MM`: the wall
    /// clock in seconds since the epoch, and the offset in seconds.
    fn stamp_of(text: &str) -> Option<(i64, Option<i64>)> {
        let b = text.as_bytes();
        if (b.len() != 19 && b.len() != 25) || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
            return None;
        }
        let day = day_of(text.get(..10)?)?;
        let (h, m, s) = (
            digits(text, 11, 13)?,
            digits(text, 14, 16)?,
            digits(text, 17, 19)?,
        );
        if h > 23 || m > 59 || s > 59 {
            return None;
        }
        let wall = day * 86_400 + h * 3600 + m * 60 + s;
        if b.len() == 19 {
            return Some((wall, None));
        }
        let sign = match b[19] {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let (oh, om) = (digits(text, 20, 22)?, digits(text, 23, 25)?);
        if b[22] != b':' || oh > 23 || om > 59 {
            return None;
        }
        Some((wall, Some(sign * (oh * 3600 + om * 60))))
    }

    /// A stamp with an offset as seconds since the epoch.
    fn instant(text: &str) -> Option<i64> {
        let (wall, offset) = stamp_of(text)?;
        Some(wall - offset?)
    }

    /// `now + timedelta(minutes=n)`, in `now`'s zone (fold 0).
    fn plus_minutes(clock: &Clock, minutes: i64) -> Option<when::At> {
        let (wall, _) = stamp_of(&when::strftime(&clock.at, "%Y-%m-%dT%H:%M:%S"))?;
        let (naive, _) = when::parse(&naive_text(wall + minutes * 60)).ok()?;
        Some(match &clock.zone {
            Some(zone) => when::attach(naive, when::zone(zone).ok()?),
            None => when::fixed(naive, *clock.at.offset()),
        })
    }

    // ---- events and locks ----

    fn quote(text: &str) -> String {
        let mut out = String::new();
        crate::render::py_string(text, &mut out);
        out
    }

    /// One `events.emit(event, **fields)` row; each value is already a JSON
    /// token (a quoted lowercase id, or a number).
    fn emit(ctx: &Ctx, event: &str, fields: &[(&str, String)]) {
        if !ctx.env.caps.events_on {
            return;
        }
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let mut pairs: Vec<(&str, String)> = vec![
            ("event", quote(event)),
            ("ts", quote(&crate::events::stamp(secs))),
            ("v", quote(crate::events::version())),
        ];
        pairs.extend(fields.iter().cloned());
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        let mut line = String::from("{");
        for (i, (key, value)) in pairs.iter().enumerate() {
            if i > 0 {
                line.push_str(", ");
            }
            line.push_str(&quote(key));
            line.push_str(": ");
            line.push_str(value);
        }
        line.push_str("}\n");
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

    /// Runs `f` under the fcntl lock Python takes on `lock`.
    fn locked<T>(lock: &Path, f: impl FnOnce() -> Option<T>) -> Option<T> {
        careerbrew_vault::lock::with_exact_lock(lock, f).ok()?
    }

    // ---- the session store ----

    /// `sessions._load()` (with its `setdefault`s), or None when Python
    /// would raise or read a stamp in a form this module does not.
    fn load_sessions(store: &Store, path: &Path) -> Option<(Option<String>, Pairs)> {
        let raw = read_text(store, path)?;
        let mut st = match &raw {
            None => Vec::new(),
            Some(text) => match careerbrew_vault::json_yaml::json_read(text).ok()? {
                value @ Canon::Map(_) if has_float(&value) => return None,
                Canon::Map(pairs) if keys_unique(&pairs) => pairs,
                _ => return None,
            },
        };
        for key in ["sessions", "claims"] {
            match get(&st, key) {
                None => st.push((Canon::Str(key.into()), Canon::List(Vec::new()))),
                Some(Canon::List(_)) => {}
                Some(_) => return None,
            }
        }
        let mut records: Vec<&str> = Vec::new();
        for session in list(&st, "sessions") {
            let s = map(session)?;
            let field = |key| text(s, key);
            let (id, project, day, started, ended) = (
                field("id")?,
                field("project")?,
                field("day")?,
                field("started")?,
                field("ended")?,
            );
            if id.is_empty() || project.is_empty() || day_of(day).is_none() {
                return None;
            }
            stamp_of(started)?;
            if !ended.is_empty() {
                stamp_of(ended)?;
            }
            match (get(s, "record_id"), get(s, "project_incarnation")) {
                (None, None) => {}
                (Some(Canon::Str(record)), Some(_)) => {
                    let hex = record
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
                    if record.len() != 32 || !hex || records.contains(&record.as_str()) {
                        return None;
                    }
                    records.push(record);
                }
                _ => return None,
            }
        }
        for claim in list(&st, "claims") {
            let c = map(claim)?;
            for key in ["resource", "session", "until"] {
                if text(c, key)?.is_empty() {
                    return None;
                }
            }
            instant(text(c, "until")?)?;
        }
        Some((raw, st))
    }

    /// The ids of the sessions not ended.
    fn open_ids(st: &Pairs) -> Vec<String> {
        list(st, "sessions")
            .iter()
            .filter_map(|s| match s {
                Canon::Map(s) if text(s, "ended") == Some("") => text(s, "id").map(str::to_owned),
                _ => None,
            })
            .collect()
    }

    /// `_valid(c, now) and c["session"] in open_ids`.
    fn holds(claim: &Canon, now: i64, open: &[String]) -> bool {
        let Canon::Map(c) = claim else { return false };
        let until = text(c, "until").and_then(instant);
        until.is_some_and(|until| until > now)
            && text(c, "session").is_some_and(|s| open.iter().any(|id| id == s))
    }

    /// `sessions._prune`: expired or orphaned claims, and ended sessions
    /// older than `keep` (a date), are dropped.
    fn prune(st: &mut Pairs, now: i64, keep: &str) -> Option<()> {
        let open = open_ids(st);
        list_mut(st, "claims")?.retain(|c| holds(c, now, &open));
        list_mut(st, "sessions")?.retain(|s| match s {
            Canon::Map(s) => text(s, "ended") == Some("") || text(s, "day").unwrap_or("") >= keep,
            _ => true,
        });
        Some(())
    }

    /// The date `agent_keep_days` before today.
    fn keep_date(today: &str, days: i64) -> Option<String> {
        Some(date_text(day_of(today)? - days))
    }

    enum Live {
        Session(Pairs),
        /// A `SessionError` whose text is fixed or built from plain store text.
        Refused(String),
    }

    /// `sessions.live(target)`, or None when Python would raise an error this
    /// module does not print (an unknown session, an unknown project, a bad
    /// project record).
    fn live(target: &str, st: &Pairs, today: &str, store: &Store, root: &Path) -> Option<Live> {
        let open: Vec<&Pairs> = list(st, "sessions")
            .iter()
            .rev()
            .filter_map(|s| match s {
                Canon::Map(s) if text(s, "ended") == Some("") => Some(s),
                _ => None,
            })
            .collect();
        let s = open
            .iter()
            .find(|s| text(s, "id") == Some(target))
            .or_else(|| open.iter().find(|s| text(s, "project") == Some(target)))?;
        let (id, project, day) = (text(s, "id")?, text(s, "project")?, text(s, "day")?);
        if !plain(id) || !plain(project) {
            return None;
        }
        if day != today {
            return Some(Live::Refused(format!(
                "session {id} was opened on {day}: end it (careerbrew work session end {id}) and open today's"
            )));
        }
        // `projects.quiet(project)`: the project's record and its quiet date.
        let top = read_map_yaml(store, &root.join("work").join("projects.yaml"))?;
        let projects = match get(&top, "projects") {
            None => Vec::new(),
            Some(value) if !truthy(value) => Vec::new(),
            Some(value) => map(value)?.clone(),
        };
        let record = map(get(&projects, project)?)?;
        let quiet = match get(record, "quiet_until") {
            None => false,
            Some(value) if !truthy(value) => false,
            Some(Canon::Str(until)) if day_of(until).is_some() => today < until.as_str(),
            Some(_) => return None,
        };
        if quiet {
            return Some(Live::Refused(
                "this project is quiet until its saved date; clear --quiet-until to resume it"
                    .into(),
            ));
        }
        Some(Live::Session((*s).clone()))
    }

    /// `sessions.resource(value)` for a value it accepts; None for an error
    /// (its text has a `repr`) or a form this module does not read.
    fn resource(raw: &str) -> Option<String> {
        if !plain(raw) {
            return None;
        }
        let value = strip(raw);
        if value.len() > 200 {
            return None;
        }
        if let Some(rest) = value.strip_prefix("pr:") {
            if raw != value {
                return None;
            }
            let (owner, rest) = rest.split_once('/')?;
            let (repo, number) = rest.split_once('#')?;
            let part = |s: &str| {
                !s.is_empty()
                    && !matches!(s, "." | "..")
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            };
            let number_ok = number.bytes().all(|b| b.is_ascii_digit())
                && number.bytes().next().is_some_and(|b| b != b'0');
            if !part(owner) || !part(repo) || !number_ok {
                return None;
            }
            return Some(format!(
                "pr:{}/{}#{number}",
                owner.to_ascii_lowercase(),
                repo.to_ascii_lowercase()
            ));
        }
        let (kind, name) = value.split_once(':')?;
        (USE_KINDS.contains(&kind) && !name.is_empty() && !name.contains(' '))
            .then(|| value.to_owned())
    }

    /// `sessions.check_id(ident)` for an id it accepts.
    fn check_id(raw: &str) -> Option<String> {
        if !plain(raw) {
            return None;
        }
        let id = strip(raw);
        let b = id.as_bytes();
        let ok = !b.is_empty()
            && b.len() <= 64
            && b[0].is_ascii_alphanumeric()
            && b.iter()
                .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(c));
        ok.then(|| id.to_owned())
    }

    /// The settings, the clock and the keep days every command here reads.
    struct Base {
        settings: Pairs,
        clock: Clock,
        today: String,
        keep_days: i64,
    }

    fn base(store: &Store, ctx: &Ctx) -> Option<Base> {
        let settings = read_map_yaml(store, &ctx.root.join("settings.yaml"))?;
        let clock = clock(&settings)?;
        let today = when::strftime(&clock.at, "%Y-%m-%d");
        let keep_days = setting_int(&settings, "agent_keep_days", 7)?;
        if !(0..=36_500).contains(&keep_days) {
            return None;
        }
        Some(Base {
            settings,
            clock,
            today,
            keep_days,
        })
    }

    fn vault() -> Option<careerbrew_vault::transfer::Context> {
        careerbrew_vault::transfer::Context::from_env().ok()
    }

    // ---------- work claim take ----------

    pub(super) fn claim_take(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
        let session = args.text("session")?;
        if !plain(session) {
            return None;
        }
        let res = resource(args.text("resource")?)?;
        let vault = vault()?;
        let store = vault.store();
        let b = base(&store, ctx)?;
        let minutes = match args.int("minutes") {
            Some(minutes) => minutes,
            None => setting_int(&b.settings, "work_claim_minutes", 30)?,
        };
        if !(1..=480).contains(&minutes) {
            return served(
                "error: --minutes: a claim lasts 1 to 480 minutes (breaks US-107)\n".into(),
                2,
            );
        }
        let path = ctx.root.join(SESSIONS);
        let (raw, st) = load_sessions(&store, &path)?;
        let s = match live(session, &st, &b.today, &store, &ctx.root)? {
            Live::Session(s) => s,
            Live::Refused(message) => {
                return served(format!("error: {message} (breaks US-107)\n"), 2)
            }
        };
        let sid = text(&s, "id")?.to_owned();
        let now = b.clock.at.timestamp();
        let at = when::iso_seconds(&b.clock.at);
        let until = when::iso_seconds(&plus_minutes(&b.clock, minutes)?);
        let keep = keep_date(&b.today, b.keep_days)?;
        let lock = ctx.root.join("work").join(".sessions.json.lock");
        let renewed = locked(&lock, || {
            let (again, mut st) = load_sessions(&store, &path)?;
            if again != raw {
                return None;
            }
            let before = st.clone();
            let open = open_ids(&st);
            let held = list(&st, "claims")
                .iter()
                .find(|c| {
                    matches!(c, Canon::Map(c) if text(c, "resource") == Some(res.as_str()))
                        && holds(c, now, &open)
                })
                .cloned();
            if let Some(Canon::Map(c)) = &held {
                // Another session holds it: Python's message shows the
                // resource only where business data may show.
                if text(c, "session") != Some(sid.as_str()) {
                    return None;
                }
            }
            let claims = list_mut(&mut st, "claims")?;
            claims.retain(
                |c| !matches!(c, Canon::Map(c) if text(c, "resource") == Some(res.as_str())),
            );
            let s = |value: &str| Canon::Str(value.to_owned());
            claims.push(Canon::Map(vec![
                (s("resource"), s(&res)),
                (s("session"), s(&sid)),
                (s("at"), s(&at)),
                (s("until"), s(&until)),
            ]));
            prune(&mut st, now, &keep)?;
            if st != before {
                store.write_json(&path, &Canon::Map(st)).ok()?;
            }
            Some(held.is_some())
        })?;
        let status = if renewed { "renewed" } else { "taken" };
        emit(
            ctx,
            "work.claim",
            &[("status", quote(status)), ("minutes", minutes.to_string())],
        );
        let verb = if renewed { "renewed" } else { "claimed" };
        let hhmm = until.get(11..16)?;
        served(
            format!("{verb} {res} for session {sid} until {hhmm}; release it when done: careerbrew work claim release {res} --session {sid}\n"),
            0,
        )
    }

    // ---------- work claim release ----------

    pub(super) fn claim_release(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
        let (raw_res, raw_session) = (args.text("resource")?, args.text("session")?);
        let res = resource(raw_res)?;
        let ident = check_id(raw_session)?;
        let vault = vault()?;
        let store = vault.store();
        let b = base(&store, ctx)?;
        let path = ctx.root.join(SESSIONS);
        let (raw, _) = load_sessions(&store, &path)?;
        let now = b.clock.at.timestamp();
        let keep = keep_date(&b.today, b.keep_days)?;
        let lock = ctx.root.join("work").join(".sessions.json.lock");
        let gone = locked(&lock, || {
            let (again, mut st) = load_sessions(&store, &path)?;
            if again != raw {
                return None;
            }
            let before = st.clone();
            let claims = list_mut(&mut st, "claims")?;
            let n = claims.len();
            claims.retain(|c| {
                !matches!(c, Canon::Map(c)
                    if text(c, "resource") == Some(res.as_str())
                        && text(c, "session") == Some(ident.as_str()))
            });
            let gone = claims.len() < n;
            prune(&mut st, now, &keep)?;
            if st != before {
                store.write_json(&path, &Canon::Map(st)).ok()?;
            }
            Some(gone)
        })?;
        let status = if gone { "released" } else { "none" };
        emit(ctx, "work.claim", &[("status", quote(status))]);
        if !gone {
            return served(
                format!("error: session {raw_session} holds no claim on {raw_res} (careerbrew work claim list) (breaks US-107)\n"),
                2,
            );
        }
        served(format!("released {raw_res}\n"), 0)
    }

    // ---------- work agent event ----------

    /// `agents.NAME`: `^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,59}$`.
    fn name_ok(value: &str) -> bool {
        let b = value.as_bytes();
        !b.is_empty()
            && b.len() <= 60
            && b[0].is_ascii_alphanumeric()
            && b.iter()
                .all(|c| c.is_ascii_alphanumeric() || b"_.:/-".contains(c))
    }

    /// `urlsplit(url)` has an http(s) scheme and a host; None when Python's
    /// `urlsplit` could raise (brackets: an IPv6 host).
    fn url_ok(url: &str) -> Option<bool> {
        if url.contains('[') || url.contains(']') {
            return None;
        }
        let Some(colon) = url.find(':') else {
            return Some(false);
        };
        let scheme = &url[..colon];
        let is_scheme = scheme
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic())
            && scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b));
        if !is_scheme || !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
            return Some(false);
        }
        let Some(after) = url[colon + 1..].strip_prefix("//") else {
            return Some(false);
        };
        Some(after.find(['/', '?', '#']).unwrap_or(after.len()) > 0)
    }

    /// The agent log's raw text and its rows with their `at` text.
    type Rows = (Option<String>, Vec<(String, Canon)>);

    /// The agent log, when every line is a JSON object Python splits and
    /// reads the same way.
    fn agent_rows(store: &Store, path: &Path) -> Option<Rows> {
        let raw = read_text(store, path)?;
        let mut rows = Vec::new();
        if let Some(text) = &raw {
            // `str.splitlines` and `str.strip` know more separators than Rust.
            let odd = |c: char| {
                matches!(c, '\r' | '\u{b}' | '\u{c}' | '\u{1c}'..='\u{1f}')
                    || (!c.is_ascii() && c.is_whitespace())
            };
            if text.chars().any(odd) {
                return None;
            }
            for line in text.split('\n') {
                if line.trim().is_empty() {
                    continue;
                }
                let row = careerbrew_vault::json_yaml::json_read(line).ok()?;
                if has_float(&row) {
                    return None;
                }
                let at = match get(map(&row)?, "at") {
                    None => String::new(),
                    Some(Canon::Str(at)) => at.clone(),
                    Some(_) => return None,
                };
                rows.push((at, row));
            }
        }
        Some((raw, rows))
    }

    /// Seconds since the epoch as `isoformat(timespec="seconds")` in UTC.
    fn utc_iso(secs: i64) -> Option<String> {
        let secs = u64::try_from(secs).ok()?;
        Some(crate::events::stamp(secs).replace('Z', "+00:00"))
    }

    fn agent_fail(message: &str) -> Option<Outcome> {
        served(format!("error: {message} (breaks US-91)\n"), 2)
    }

    pub(super) fn agent_event(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
        let (agent, title, status) = (
            args.text("agent")?,
            args.text("title")?,
            args.text("status")?,
        );
        let given = |name| args.text(name).unwrap_or("");
        let (project, url, run, session) = (
            given("project"),
            given("url"),
            given("run"),
            given("session"),
        );
        if ![agent, title, status, project, url, run, session]
            .iter()
            .all(|text| plain(text))
            || !["running", "done", "failed"].contains(&status)
        {
            return None;
        }
        let vault = vault()?;
        let store = vault.store();
        let b = base(&store, ctx)?;
        let (mut run, mut project) = (run.to_owned(), project.to_owned());
        if !session.is_empty() {
            let (_, st) = load_sessions(&store, &ctx.root.join(SESSIONS))?;
            match live(session, &st, &b.today, &store, &ctx.root)? {
                Live::Refused(message) => {
                    return served(format!("error: {message} (breaks US-107)\n"), 2)
                }
                Live::Session(s) => {
                    let (id, own) = (text(&s, "id")?, text(&s, "project")?);
                    if (!run.is_empty() && run != id) || (!project.is_empty() && project != own) {
                        return agent_fail(
                            "--session sets the run and the project: leave out --run and --project",
                        );
                    }
                    (run, project) = (id.to_owned(), own.to_owned());
                }
            }
        }
        // `agents._check`, in its order.
        let agent = strip(agent);
        let run = strip(if run.is_empty() { agent } else { &run }).to_owned();
        let (title, project, url) = (one_line(title), one_line(&project), strip(url));
        if !name_ok(agent) || !name_ok(&run) {
            return None;
        }
        if title.is_empty() {
            return agent_fail(
                "the title is empty: say in one line what the agent did or does now",
            );
        }
        if title.len() > 200 {
            return agent_fail(&format!(
                "the title has {} characters; the limit is 200: keep it to one short line",
                title.len()
            ));
        }
        if project.len() > 80 {
            return agent_fail(&format!(
                "the project name has {} characters; the limit is 80",
                project.len()
            ));
        }
        if !url.is_empty() && !url_ok(url)? {
            return agent_fail("the link must be a full http(s) URL, for example https://github.example.com/org/repo/pull/7");
        }
        // `agents.record`.
        let utc = when::into(&b.clock.at, when::zone("UTC").ok()?);
        let at = when::iso_seconds(&utc);
        let cutoff = utc_iso(utc.timestamp() - b.keep_days * 86_400)?;
        let path = ctx.root.join(AGENT_LOG);
        let (raw, _) = agent_rows(&store, &path)?;
        let s = |value: &str| Canon::Str(value.to_owned());
        let row = Canon::Map(vec![
            (s("at"), s(&at)),
            (s("agent"), s(agent)),
            (s("run"), s(&run)),
            (s("status"), s(status)),
            (s("title"), s(&title)),
            (s("url"), s(url)),
            (s("project"), s(&project)),
        ]);
        let lock = ctx.root.join("work").join("agents").join(".lock");
        locked(&lock, || {
            let (again, rows) = agent_rows(&store, &path)?;
            if again != raw {
                return None;
            }
            let mut kept: Vec<Canon> = rows
                .into_iter()
                .filter(|(at, _)| at.as_str() >= cutoff.as_str())
                .map(|(_, row)| row)
                .collect();
            kept.push(row);
            store.write_jsonl(&path, &kept).ok()
        })?;
        emit(
            ctx,
            "work.agent.event",
            &[("status", quote(status)), ("story", quote("us-91"))],
        );
        let shown_run = if run != agent {
            format!(" (run {run})")
        } else {
            String::new()
        };
        served(
            format!("event stored: {agent} {status}{shown_run}; Agent activity on Work shows it within seconds\n"),
            0,
        )
    }
}
