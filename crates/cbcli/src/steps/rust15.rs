//! Native CLI handlers owned by `rust-15` (see `steps/mod.rs`): the work
//! day's `work session handoff`, over `careerbrew_sessions`.
//!
//! The handler checks every input Python's `sessions.write_handoff` and
//! `day/handoff.add` read before it writes anything: the text and link, the
//! session store (Python's validation is wider than the crate's), the day's
//! handoff file and the clock. It serves only a UTC clock: the crate stamps
//! `handoff_at` and picks the handoff day in UTC, Python in the local zone.
//! The `Canon` and clock helpers are shared with `rust14`.
use super::rust14::{
    event_version_ok, get, inputs_ok, int, iso, list, read_json, read_yaml, string, written,
};
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;
use careerbrew_vault::canon::Canon;
use std::path::Path;
use std::time::Instant;

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust15.rs`.
pub const COMMANDS: &[&str] = &["work.session.handoff"];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let started = Instant::now();
    if !inputs_ok(ctx) || !event_version_ok() {
        return None;
    }
    match name {
        "work.session.handoff" => handoff(args, ctx, started),
        _ => None,
    }
}

/// `MAX_TEXT` in `day/handoff.py`.
const MAX_TEXT: usize = 200;

/// Characters Python's `str.split()` treats as whitespace and Rust's
/// `split_whitespace` does not.
fn odd_space(text: &str) -> bool {
    text.chars().any(|c| ('\u{1c}'..='\u{1f}').contains(&c))
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The clock is UTC for both sides: a pinned `+00:00` stamp, or a local
/// zone at offset zero now.
fn utc_clock() -> bool {
    match std::env::var("CAREERBREW_NOW") {
        Ok(value) => iso(&value) == Some(Some(0)) && value.ends_with("+00:00"),
        Err(std::env::VarError::NotPresent) => {
            careerbrew_feed::now().offset().local_minus_utc() == 0
        }
        Err(_) => false,
    }
}

/// A stored `YYYY-MM-DD` day, as `date.fromisoformat(day).isoformat() == day`.
fn plain_day(day: &str) -> bool {
    day.len() == 10 && iso(&format!("{day}T00:00:00")).is_some()
}

/// One saved session as Python's `_load` accepts it and the crate reads it
/// the same way.
fn session_ok(session: &Canon) -> bool {
    if !matches!(session, Canon::Map(_))
        || ["id", "project", "day", "started", "ended"]
            .iter()
            .any(|key| string(session, key).is_none())
    {
        return false;
    }
    let started = string(session, "started").unwrap_or("");
    let ended = string(session, "ended").unwrap_or("");
    matches!(iso(started), Some(Some(_)))
        && plain_day(string(session, "day").unwrap_or(""))
        && (ended.is_empty() || iso(ended).is_some())
}

/// One saved claim as Python's `_load` accepts it; an aware `until`, so
/// `_prune` compares it the same way.
fn claim_ok(claim: &Canon) -> bool {
    matches!(claim, Canon::Map(_))
        && ["resource", "session", "until"]
            .iter()
            .all(|key| string(claim, key).is_some_and(|text| !text.is_empty()))
        && matches!(string(claim, "until").and_then(iso), Some(Some(_)))
}

/// Python's `find`: the newest open session with this id, else the newest
/// open session of this project.
fn find<'a>(sessions: &'a [Canon], target: &str) -> Option<&'a Canon> {
    let open = || {
        sessions
            .iter()
            .rev()
            .filter(|s| string(s, "ended").is_some_and(str::is_empty))
    };
    open()
        .find(|s| string(s, "id") == Some(target))
        .or_else(|| open().find(|s| string(s, "project") == Some(target)))
}

/// The day's handoff file in the shape `handoff._save` writes, so the
/// crate's in-place edit leaves the same file.
fn day_file_ok(root: &Path, day: &str) -> bool {
    let rel = format!("day/handoff-{day}.json");
    if !root.join(&rel).exists() {
        return true;
    }
    let Some(file) = read_json(root, &rel) else {
        return false;
    };
    let Canon::Map(pairs) = &file else {
        return false;
    };
    pairs
        .iter()
        .all(|(k, _)| matches!(k, Canon::Str(key) if key == "day" || key == "items"))
        && string(&file, "day") == Some(day)
        && list(get(&file, "items")).is_some_and(|items| {
            items
                .iter()
                .all(|item| string(item, "id").is_some_and(|id| !id.is_empty()))
        })
}

fn handoff(args: &Parsed, ctx: &Ctx, started: Instant) -> Option<Outcome> {
    let root = &ctx.root;
    let target = args.text("id")?;
    let raw = args.text("text")?;
    let link = args.text("link").unwrap_or("");
    if !utc_clock() || [target, raw, link].iter().any(|text| odd_space(text)) {
        return None;
    }
    // Python's own refusals (an empty text, a link that is not http(s))
    // print an error: they stay with Python.
    let text = one_line(raw);
    if text.is_empty()
        || !(link.is_empty() || link.starts_with("https://") || link.starts_with("http://"))
    {
        return None;
    }
    if !root.join("work/sessions.json").exists()
        || careerbrew_sessions::sessions::load(root).is_err()
    {
        return None;
    }
    let store = read_json(root, "work/sessions.json")?;
    let sessions = list(get(&store, "sessions"))?;
    let claims = match get(&store, "claims") {
        None => &[][..],
        value => list(value)?,
    };
    if !sessions.iter().all(session_ok) || !claims.iter().all(claim_ok) {
        return None;
    }
    let session = find(sessions, target)?;
    let project = string(session, "project")?;
    if odd_space(project) || one_line(project) != project {
        return None;
    }
    if !matches!(
        get(session, "handoff_id"),
        None | Some(Canon::Null) | Some(Canon::Str(_))
    ) {
        return None;
    }
    match get(session, "wrap_requested_at") {
        None | Some(Canon::Null) => {}
        Some(Canon::Str(at)) if at.is_empty() => {}
        // `max(now, requested + 1us)` keeps the winner's offset in Python.
        Some(Canon::Str(at)) if iso(at) == Some(Some(0)) && at.ends_with("+00:00") => {}
        Some(_) => return None,
    }
    if format!("{project}: {text}").chars().count() > MAX_TEXT {
        return None;
    }
    let settings = read_yaml(root, "settings.yaml")?;
    if get(&settings, "agent_keep_days").is_some()
        && int(get(&settings, "agent_keep_days")).is_none()
    {
        return None;
    }
    let day = careerbrew_sessions::today().format("%Y-%m-%d").to_string();
    if !day_file_ok(root, &day) {
        return None;
    }
    let saved = careerbrew_sessions::sessions::write_handoff(root, target, &text, link).ok()?;
    let saved = careerbrew_setup::value::to_canon(&saved);
    let id = string(&saved, "id")?.to_owned();
    let _ = careerbrew_feed::emit(
        root,
        "work.session",
        vec![
            ("action", Canon::Str("handoff".into())),
            ("status", Canon::Str("ok".into())),
        ],
    );
    let line = format!(
        "handoff written for session {id} (Project sessions in careerbrew day handoff show); end it: careerbrew work session end {id}\n"
    );
    Some(written(ctx, "work.session", started, line))
}
