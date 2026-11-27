//! Native CLI handlers owned by `rust-14` (see `steps/mod.rs`): the live
//! feed's `work live stop` and `notify list`, over `careerbrew_live`.
//!
//! Each handler checks, before any write, that the Python code would read
//! the same inputs the same way; any doubt refuses (exit 64) and Python runs
//! the command. The checks read data files as `Canon` (this crate links no
//! JSON crate) and never create a key.
//!
//! `rust15` shares the small `Canon` and clock helpers marked `pub(super)`.
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;
use careerbrew_vault::canon::Canon;
use std::path::Path;
use std::time::Instant;

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust14.rs`.
pub const COMMANDS: &[&str] = &["work.live.stop", "notify.list"];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let started = Instant::now();
    if !inputs_ok(ctx) {
        return None;
    }
    match name {
        "work.live.stop" => live_stop(ctx, started),
        "notify.list" => notify_list(args, ctx),
        _ => None,
    }
}

// ---------- shared checks (also used by rust15) ----------

/// The checks every handler here needs: no redaction input (Python's
/// `say` would read it), a readable data context, and a clock and time
/// zone that Python and the domain crates read the same way.
pub(super) fn inputs_ok(ctx: &Ctx) -> bool {
    !ctx.redact_inputs_present()
        && careerbrew_vault::transfer::Context::from_env().is_ok()
        && pinned_clock_ok()
        && zone_ok(&ctx.root)
}

/// The domain crates stamp event rows with `CAREERBREW_VERSION` or
/// `0.1.0`; Python stamps its own version. Refuse when they could differ.
pub(super) fn event_version_ok() -> bool {
    std::env::var_os("CAREERBREW_VERSION").is_none() && crate::events::version() == "0.1.0"
}

/// `CAREERBREW_NOW`, when set, in a form both Python's `fromisoformat` and
/// the domain crates parse to the same instant.
fn pinned_clock_ok() -> bool {
    match std::env::var("CAREERBREW_NOW") {
        Ok(value) => iso(&value).is_some(),
        Err(std::env::VarError::NotPresent) => true,
        Err(_) => false,
    }
}

/// The `timezone` setting, when set, names the zone the domain crates use
/// (Python raises on a zone it cannot load; the crates fall back to UTC).
pub(super) fn zone_ok(root: &Path) -> bool {
    let Some(settings) = read_yaml(root, "settings.yaml") else {
        return false;
    };
    match get(&settings, "timezone") {
        None | Some(Canon::Null) => true,
        Some(Canon::Str(name)) if name.is_empty() => true,
        Some(Canon::Str(name)) => careerbrew_feed::local_zone().name() == name,
        Some(_) => false,
    }
}

/// A data file as `Canon` (the default when missing), never creating a key.
/// None on any read or decode error.
pub(super) fn read_json(root: &Path, rel: &str) -> Option<Canon> {
    let context = careerbrew_vault::transfer::Context::from_env().ok()?;
    context
        .store()
        .read_json(&root.join(rel), Canon::Map(Vec::new()), false)
        .ok()
}

pub(super) fn read_yaml(root: &Path, rel: &str) -> Option<Canon> {
    let context = careerbrew_vault::transfer::Context::from_env().ok()?;
    let value = context
        .store()
        .read_yaml(&root.join(rel), Canon::Map(Vec::new()), false)
        .ok()?;
    match value {
        // An empty YAML file reads as None; Python's `or {}` makes it a mapping.
        Canon::Null => Some(Canon::Map(Vec::new())),
        Canon::Map(_) => Some(value),
        _ => None,
    }
}

pub(super) fn get<'a>(map: &'a Canon, key: &str) -> Option<&'a Canon> {
    let Canon::Map(pairs) = map else {
        return None;
    };
    pairs
        .iter()
        .find(|(k, _)| matches!(k, Canon::Str(name) if name == key))
        .map(|(_, v)| v)
}

pub(super) fn string<'a>(map: &'a Canon, key: &str) -> Option<&'a str> {
    match get(map, key) {
        Some(Canon::Str(text)) => Some(text),
        _ => None,
    }
}

pub(super) fn int(value: Option<&Canon>) -> Option<i64> {
    match value {
        Some(Canon::Int(number)) => number.to_string().parse().ok(),
        _ => None,
    }
}

pub(super) fn list(value: Option<&Canon>) -> Option<&[Canon]> {
    match value {
        Some(Canon::List(items)) => Some(items),
        _ => None,
    }
}

/// A stamp `YYYY-MM-DDTHH:MM:SS[.f{1,6}][(+|-)HH:MM]` that Python's
/// `fromisoformat` and RFC 3339 read the same way. Returns the offset in
/// seconds (None inside the option for a naive stamp).
pub(super) fn iso(text: &str) -> Option<Option<i32>> {
    let b = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| range.clone().all(|i| b[i].is_ascii_digit());
    if b.len() < 19
        || !digits(0..4)
        || b[4] != b'-'
        || !digits(5..7)
        || b[7] != b'-'
        || !digits(8..10)
        || b[10] != b'T'
        || !digits(11..13)
        || b[13] != b':'
        || !digits(14..16)
        || b[16] != b':'
        || !digits(17..19)
        || b[17] > b'5'
    {
        return None;
    }
    let mut at = 19;
    if b.get(at) == Some(&b'.') {
        let start = at + 1;
        let mut end = start;
        while end < b.len() && b[end].is_ascii_digit() {
            end += 1;
        }
        if end == start || end - start > 6 {
            return None;
        }
        at = end;
    }
    let rest = &text[at..];
    let aware = match rest.len() {
        0 => false,
        6 => {
            let r = rest.as_bytes();
            if !(r[0] == b'+' || r[0] == b'-')
                || !r[1].is_ascii_digit()
                || !r[2].is_ascii_digit()
                || r[3] != b':'
                || !r[4].is_ascii_digit()
                || !r[5].is_ascii_digit()
            {
                return None;
            }
            true
        }
        _ => return None,
    };
    let probe = if aware {
        text.to_owned()
    } else {
        format!("{text}+00:00")
    };
    let parsed = careerbrew_live::parse_time(&probe)?;
    Some(aware.then(|| parsed.offset().local_minus_utc()))
}

/// A one-line result with no domain `cli.run` row of its own: the handler
/// writes Python's row (`work.live`, not the route key) itself.
pub(super) fn written(ctx: &Ctx, command: &str, started: Instant, stdout: String) -> Outcome {
    let ms = started.elapsed().as_millis() as u64;
    crate::events::emit_command(&ctx.env.caps, command, true, ms);
    Outcome {
        stdout,
        stderr: String::new(),
        code: 0,
        emit_event: false,
    }
}

// ---------- work live stop ----------

fn live_stop(ctx: &Ctx, started: Instant) -> Option<Outcome> {
    if !event_version_ok() {
        return None;
    }
    let root = &ctx.root;
    // Python's `_state` raises on a file it cannot read; read it first.
    let state = read_json(root, "work/live/state.json")?;
    if !matches!(state, Canon::Map(_)) {
        return None;
    }
    // The count is printed as Python prints it: an int only.
    if get(&state, "cycles").is_some() && int(get(&state, "cycles")).is_none() {
        return None;
    }
    let result = careerbrew_live::live::stop(root).ok()?;
    let canon = careerbrew_setup::value::to_canon(&result);
    let cycles = int(get(&canon, "cycles"))?;
    let was_running = matches!(get(&canon, "was_running"), Some(Canon::Bool(true)));
    let line = if was_running {
        format!(
            "live feed stopped after {cycles} cycle(s); cursors kept, so the next start continues\n"
        )
    } else {
        "the live feed was not running\n".to_owned()
    };
    Some(written(ctx, "work.live", started, line))
}

// ---------- notify list ----------

const OPEN: &[&str] = &["new", "snoozed", "read"];
const ROW_STRINGS: &[&str] = &["id", "kind", "source", "headline", "why", "next", "status"];
const ROW_PRESENT: &[&str] = &["priority_why", "snooze_until", "resolved"];

fn null_or_str(value: Option<&Canon>) -> bool {
    matches!(value, None | Some(Canon::Null) | Some(Canon::Str(_)))
}

fn null_or_map(value: Option<&Canon>) -> bool {
    matches!(value, None | Some(Canon::Null) | Some(Canon::Map(_)))
}

/// The settings `notify.settings`, `notify.backend` and
/// `project_priority.entries` read, in the types Python's code reads
/// without raising and the domain crates read the same way.
fn notify_settings_ok(settings: &Canon) -> bool {
    let ints = [
        "notify_min_priority",
        "notify_daily_cap",
        "notify_batch_minutes",
        "notify_snooze_minutes",
    ];
    if ints
        .iter()
        .any(|key| get(settings, key).is_some() && int(get(settings, key)).is_none())
    {
        return false;
    }
    if !matches!(get(settings, "notify_events"), None | Some(Canon::Bool(_))) {
        return false;
    }
    match get(settings, "notify_quiet_hours") {
        None => {}
        Some(Canon::Str(spec)) if spec.is_empty() => {}
        Some(Canon::Str(spec)) => {
            let b = spec.as_bytes();
            let clock = |at: usize| {
                b[at].is_ascii_digit()
                    && b[at + 1].is_ascii_digit()
                    && b[at + 2] == b':'
                    && b[at + 3].is_ascii_digit()
                    && b[at + 4].is_ascii_digit()
            };
            if b.len() != 11 || b[5] != b'-' || !clock(0) || !clock(6) {
                return false;
            }
        }
        Some(_) => return false,
    }
    match get(settings, "notify_backend") {
        None => {}
        Some(Canon::Str(name))
            if ["auto", "alerter", "terminal-notifier", "osascript"].contains(&name.as_str()) => {}
        Some(_) => return false,
    }
    match get(settings, "project_priority") {
        None | Some(Canon::Null) => true,
        Some(Canon::List(items)) => items.iter().all(|item| matches!(item, Canon::Str(_))),
        Some(_) => false,
    }
}

/// One stored notification as `notify.view` reads it without raising.
fn item_ok(row: &Canon) -> bool {
    if !matches!(row, Canon::Map(_))
        || ROW_STRINGS.iter().any(|key| string(row, key).is_none())
        || ROW_PRESENT.iter().any(|key| get(row, key).is_none())
        || int(get(row, "priority")).is_none()
        || int(get(row, "count")).is_none()
        || !null_or_str(get(row, "popup_id"))
        || !null_or_str(get(row, "popup"))
    {
        return false;
    }
    // `_dt(at)` sorts the rows: an aware stamp sorts the same everywhere.
    if !matches!(string(row, "at").and_then(iso), Some(Some(_))) {
        return false;
    }
    if string(row, "kind") == Some("suggestion") {
        return string(row, "item").is_some();
    }
    string(row, "key").is_some()
        && matches!(list(get(row, "refs")), Some(refs) if matches!(refs.last(), Some(Canon::Str(_))))
}

fn popup_ok(popup: &Canon) -> bool {
    if !matches!(popup, Canon::Map(_))
        || string(popup, "at").and_then(iso).is_none()
        || !null_or_str(get(popup, "launch_state"))
    {
        return false;
    }
    let launching = matches!(
        string(popup, "launch_state"),
        Some("reserved" | "launching")
    );
    !launching || string(popup, "id").is_some()
}

/// An inbox item as `inbox.explain` reads it without raising.
fn inbox_item_ok(item: &Canon) -> bool {
    matches!(item, Canon::Map(_))
        && string(item, "id").is_some()
        && null_or_str(get(item, "kind"))
        && null_or_map(get(item, "data"))
        && get(item, "data").is_none_or(|data| null_or_map(get(data, "rule")))
}

/// Every input `notify.view` and `notify.status` read, checked before the
/// domain view runs. The work feed is left to Python: its rows decide which
/// asks are still open, and this handler does not check them.
fn notify_inputs_ok(root: &Path) -> bool {
    if root.join("work/feed/events.jsonl").exists() {
        return false;
    }
    let Some(settings) = read_yaml(root, "settings.yaml") else {
        return false;
    };
    if !notify_settings_ok(&settings) {
        return false;
    }
    let Some(state) = read_json(root, "day/notifications.json") else {
        return false;
    };
    if !matches!(state, Canon::Map(_)) {
        return false;
    }
    let items = match get(&state, "items") {
        None => &[][..],
        value => match list(value) {
            Some(items) => items,
            None => return false,
        },
    };
    let popups = match get(&state, "popups") {
        None => &[][..],
        value => match list(value) {
            Some(popups) => popups,
            None => return false,
        },
    };
    if !items.iter().all(item_ok) || !popups.iter().all(popup_ok) {
        return false;
    }
    let Some(inbox) = read_json(root, "day/inbox.json") else {
        return false;
    };
    if !matches!(inbox, Canon::Map(_)) {
        return false;
    }
    match get(&inbox, "items") {
        None => true,
        value => list(value).is_some_and(|items| items.iter().all(inbox_item_ok)),
    }
}

fn notify_list(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let all = args.flag("all");
    let show = args.flag("show");
    // Titles show only on a terminal (or under a plugin policy, which
    // `REDACT_INPUTS` already refuses); that path stays with Python.
    if show && ctx.env.stdout_is_terminal {
        return None;
    }
    let root = &ctx.root;
    if !notify_inputs_ok(root) {
        return None;
    }
    let view = careerbrew_live::notify::view(root).ok()?;
    let view = careerbrew_setup::value::to_canon(&view);
    let status = get(&view, "status")?;
    let open = int(get(status, "open"))?;
    let unread = int(get(status, "unread"))?;
    let enabled = match get(status, "enabled") {
        Some(Canon::Bool(value)) => *value,
        _ => return None,
    };
    let mut out = format!(
        "notifications: {open} open ({unread} unread), {}\n",
        if enabled { "on" } else { "off" }
    );
    if show {
        out.push_str("titles hidden: stdout is not a terminal and no plugin policy allows agents to read business data\n");
    }
    let mut shown = 0;
    for row in list(get(&view, "items"))? {
        let status = string(row, "status")?;
        if !(all || OPEN.contains(&status)) {
            continue;
        }
        let id = string(row, "id")?;
        let priority = int(get(row, "priority"))?;
        let count = int(get(row, "count"))?;
        out.push_str(&format!(
            "- {id} [{status}] p{priority} {}",
            string(row, "headline")?
        ));
        if count > 1 {
            out.push_str(&format!(" ({count}x)"));
        }
        out.push('\n');
        let mut actions = Vec::new();
        for action in list(get(row, "actions"))? {
            match action {
                Canon::Str(name) => actions.push(name.as_str()),
                _ => return None,
            }
        }
        let actions = if actions.is_empty() {
            "read".to_owned()
        } else {
            actions.join("|")
        };
        out.push_str(&format!(
            "    why: {}. next: {}. act: careerbrew notify act {id} {actions}\n",
            string(row, "why")?,
            string(row, "next")?
        ));
        shown += 1;
    }
    if shown == 0 {
        out.push_str(if enabled {
            "nothing open\n"
        } else {
            "nothing open: careerbrew config set notify_events true turns notifications on\n"
        });
    }
    Some(Outcome {
        stdout: out,
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}

#[cfg(test)]
mod tests {
    use super::iso;

    #[test]
    fn iso_reads_only_forms_both_parsers_share() {
        assert_eq!(iso("2026-11-20T09:00:00"), Some(None));
        assert_eq!(iso("2026-11-20T09:00:00.123456+00:00"), Some(Some(0)));
        assert_eq!(iso("2026-11-20T09:00:00+02:00"), Some(Some(7200)));
        for bad in [
            "2026-11-20T09:00",
            "2026-11-20 09:00:00",
            "2026-11-20T09:00:60",
            "2026-11-20T09:00:00Z",
            "2026-11-20T09:00:00.1234567",
            "2026-02-30T09:00:00",
            "2026-11-20T25:00:00",
            "",
        ] {
            assert_eq!(iso(bad), None, "{bad}");
        }
    }
}
