//! Native CLI handlers owned by `rust-14` (see `steps/mod.rs`): the live
//! feed's `work live stop`, `notify list`, `notify status` and `notify act`
//! over `careerbrew_live`, and the project sessions' `work session list`
//! and the argument error of `work session end`. Wave 4 adds the narrow
//! cases of `work live start` (no source anywhere: its error), `work live
//! cycle` (the feed is stopped: its error), `work live status` (no source
//! anywhere) and `notify check` (notifications off).
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
pub const COMMANDS: &[&str] = &[
    "work.live.stop",
    "notify.list",
    "notify.status",
    "notify.act",
    "work.session.list",
    "work.session.end",
    "work.live.start",
    "work.live.cycle",
    "work.live.status",
    "notify.check",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let started = Instant::now();
    if !inputs_ok(ctx) {
        return None;
    }
    match name {
        "work.live.stop" => live_stop(ctx, started),
        "notify.list" => notify_list(args, ctx),
        "notify.status" => notify_status(ctx),
        "notify.act" => notify_act(args, ctx, started),
        "work.session.list" => session_list(args, ctx),
        "work.session.end" => session_end(args),
        "work.live.start" => live_start(args, ctx),
        "work.live.cycle" => live_cycle(ctx),
        "work.live.status" => live_status(args, ctx),
        "notify.check" => notify_check(ctx),
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

// ---------- notify status ----------

/// True when `CAREERBREW_NOW` is pinned with a UTC offset: Python's clock
/// then carries a fixed offset, as the crates' clock does. Otherwise it
/// carries the local zone, whose offset can change (daylight saving time).
fn clock_is_fixed() -> bool {
    std::env::var("CAREERBREW_NOW").is_ok_and(|value| matches!(iso(&value), Some(Some(_))))
}

/// The notifier lookup reads `PATH` the same way in Python (`shutil.which`)
/// and the crate: every entry absolute, and every notifier found there an
/// executable file (the crate does not check the mode). The crate's test
/// platform hook must be unset.
fn notifier_path_ok() -> bool {
    use std::os::unix::fs::PermissionsExt;
    if std::env::var_os("CAREERBREW_RUST14_PLATFORM").is_some() {
        return false;
    }
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    for dir in path.split(':') {
        if !dir.starts_with('/') {
            return false;
        }
        for name in ["alerter", "terminal-notifier", "osascript"] {
            let file = Path::new(dir).join(name);
            if let Ok(meta) = std::fs::metadata(&file) {
                if !meta.is_file() || meta.permissions().mode() & 0o111 != 0o111 {
                    return false;
                }
            }
        }
    }
    true
}

/// The stored notifications as `notify.load` reads them without raising:
/// a mapping whose `items` and `popups` are lists (or absent).
fn notify_state(root: &Path) -> Option<(Canon, Vec<Canon>, Vec<Canon>)> {
    let state = read_json(root, "day/notifications.json")?;
    if !matches!(state, Canon::Map(_)) {
        return None;
    }
    let items = match get(&state, "items") {
        None => Vec::new(),
        value => list(value)?.to_vec(),
    };
    let popups = match get(&state, "popups") {
        None => Vec::new(),
        value => list(value)?.to_vec(),
    };
    Some((state, items, popups))
}

/// One pop-up's `at` is aware, and its day is the same in the clock's
/// fixed offset (the crate) and in the local zone (Python, unless the
/// clock is pinned with an offset).
fn popup_day_ok(popup: &Canon, fixed_clock: bool) -> bool {
    let Some(at) = string(popup, "at") else {
        return false;
    };
    if !matches!(iso(at), Some(Some(_))) {
        return false;
    }
    if fixed_clock {
        return true;
    }
    let Some(time) = careerbrew_live::parse_time(at) else {
        return false;
    };
    let now = careerbrew_feed::now();
    let zone = careerbrew_feed::local_zone();
    time.with_timezone(&now.timezone()).date_naive() == time.with_timezone(&zone).date_naive()
}

fn notify_status(ctx: &Ctx) -> Option<Outcome> {
    let root = &ctx.root;
    if !notifier_path_ok() {
        return None;
    }
    let settings = read_yaml(root, "settings.yaml")?;
    if !notify_settings_ok(&settings) {
        return None;
    }
    let (_, items, popups) = notify_state(root)?;
    if !items
        .iter()
        .all(|row| matches!(row, Canon::Map(_)) && string(row, "status").is_some())
    {
        return None;
    }
    let fixed = clock_is_fixed();
    if !popups.iter().all(|popup| popup_day_ok(popup, fixed)) {
        return None;
    }
    let state = careerbrew_live::notify::status(root).ok()?;
    let state = careerbrew_setup::value::to_canon(&state);
    let flag = |key: &str| match get(&state, key) {
        Some(Canon::Bool(value)) => Some(*value),
        _ => None,
    };
    let number = |key: &str| int(get(&state, key));
    let enabled = flag("enabled")?;
    let quiet = string(&state, "quiet_hours")?;
    let mut out = format!(
        "notifications: {}\n",
        if enabled {
            "on"
        } else {
            "off (opt in: careerbrew config set notify_events true)"
        }
    );
    out.push_str(&format!("pop-ups: {}\n", string(&state, "backend_note")?));
    out.push_str(&format!(
        "rules: priority >= {} (5 a page on you, 4 a DM or review request, 3 a mention, important mail, or suggestion), \
         quiet hours {}{}, cap {} a day ({} today), batch {} min, snooze {} min\n",
        number("min_priority")?,
        if quiet.is_empty() { "none" } else { quiet },
        if flag("quiet_now")? { " (now)" } else { "" },
        number("daily_cap")?,
        number("sent_today")?,
        number("batch_minutes")?,
        number("snooze_minutes")?,
    ));
    out.push_str(&format!(
        "open: {} ({} unread): careerbrew notify list, or the web app's Notifications view\n",
        number("open")?,
        number("unread")?
    ));
    Some(Outcome {
        stdout: out,
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}

// ---------- notify act ----------

/// A JSON value the crate's `serde_json` round trip writes back as Python's
/// `json.dumps` does: no floats and no integers past `i64`.
fn plain(value: &Canon) -> bool {
    match value {
        Canon::Null | Canon::Bool(_) | Canon::Str(_) => true,
        Canon::Int(_) => int(Some(value)).is_some(),
        Canon::List(items) => items.iter().all(plain),
        Canon::Map(pairs) => pairs
            .iter()
            .all(|(k, v)| matches!(k, Canon::Str(_)) && plain(v)),
        _ => false,
    }
}

pub(super) fn printable(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| (' '..='~').contains(&c))
}

/// The local zone's UTC offset at `secs` since the epoch.
fn zone_offset_at(secs: i64) -> Option<i32> {
    let stamp = crate::events::stamp(u64::try_from(secs).ok()?);
    let time = careerbrew_live::parse_time(&stamp)?;
    let zone = careerbrew_feed::local_zone();
    Some(
        time.with_timezone(&zone)
            .fixed_offset()
            .offset()
            .local_minus_utc(),
    )
}

/// Python adds the snooze to the wall clock of a zoned `now` and takes the
/// zone's offset then; the crate keeps `now`'s offset. They agree when the
/// zone keeps one offset over the snooze (or the clock is pinned with an
/// offset).
fn snooze_clock_ok(minutes: i64) -> bool {
    if clock_is_fixed() {
        return true;
    }
    let now = careerbrew_feed::now();
    let at = now.timestamp();
    let offset = now.offset().local_minus_utc();
    [at, at + (minutes + 120) * 60]
        .iter()
        .all(|secs| zone_offset_at(*secs) == Some(offset))
}

fn notify_act(args: &Parsed, ctx: &Ctx, started: Instant) -> Option<Outcome> {
    if !event_version_ok() {
        return None;
    }
    let id = args.text("id")?;
    let action = args.text("action")?;
    // Accept, and Dismiss of a suggestion, decide an inbox item: Python.
    if !printable(id) || !["read", "snooze", "dismiss"].contains(&action) {
        return None;
    }
    let root = &ctx.root;
    let settings = read_yaml(root, "settings.yaml")?;
    if !notify_settings_ok(&settings) {
        return None;
    }
    let (state, items, _) = notify_state(root)?;
    if !plain(&state) {
        return None;
    }
    if !items
        .iter()
        .all(|row| matches!(row, Canon::Map(_)) && string(row, "id").is_some())
    {
        return None;
    }
    // Every refusal Python raises (`NotifyError`) stays with Python.
    let row = items.iter().find(|row| string(row, "id") == Some(id))?;
    let status = string(row, "status")?;
    let kind = string(row, "kind")?;
    if action != "read" && !OPEN.contains(&status) {
        return None;
    }
    if kind == "suggestion" && action == "dismiss" {
        return None;
    }
    let refs_ok = match get(row, "held_refs") {
        None | Some(Canon::Null) => true,
        Some(Canon::List(refs)) => refs.iter().all(|r| matches!(r, Canon::Str(_))),
        Some(_) => false,
    };
    if !refs_ok
        || !null_or_map(get(row, "snoozed_until_by_ref"))
        || !matches!(get(row, "snoozes"), None | Some(Canon::List(_)))
    {
        return None;
    }
    if action == "snooze" {
        let minutes = match get(&settings, "notify_snooze_minutes") {
            None => 60,
            value => int(value)?,
        };
        if !snooze_clock_ok(minutes) {
            return None;
        }
    }
    let done = careerbrew_live::notify::act(root, id, action, "cli", None).ok()?;
    let done = careerbrew_setup::value::to_canon(&done);
    let status = string(&done, "status")?;
    let mut line = format!("{id}: {status}");
    if status == "snoozed" {
        let until = string(&done, "snooze_until")?;
        line.push_str(" until ");
        line.push_str(until.get(11..16)?);
    }
    line.push('\n');
    Some(written(ctx, "notify.act", started, line))
}

// ---------- wave 4: work live start, cycle, status; notify check ----------

/// `cli.main` on a caught `ValueError`: one `cli.error` row, `error: <message>`,
/// exit 2, and no `cli.run` row. `command` is Python's two-word name and
/// `class` the lowercase class name; the message is fixed text.
pub(super) fn cli_error(ctx: &Ctx, command: &str, class: &str, message: &str) -> Outcome {
    let _ = careerbrew_feed::emit(
        &ctx.root,
        "cli.error",
        vec![
            ("command", Canon::Str(command.into())),
            ("error", Canon::Str(class.into())),
            ("via", Canon::Str(ctx.env.caps.via.into())),
        ],
    );
    Outcome {
        stdout: format!("error: {message}\n"),
        stderr: String::new(),
        code: 2,
        emit_event: false,
    }
}

/// True when nothing (a file, a folder or a link) is at `rel`.
fn absent(root: &Path, rel: &str) -> bool {
    matches!(
        std::fs::symlink_metadata(root.join(rel)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    )
}

/// No live source can exist: no owner config and no plugin (a plugin's
/// lock file is also a redaction input, refused before this).
fn no_live_sources(root: &Path) -> bool {
    absent(root, "work/live-sources.yaml") && absent(root, "plugins.lock.json")
}

/// `work live start` with no `--sources` and no source anywhere: Python's
/// `live.start` raises `LiveError` before it writes anything.
fn live_start(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if !event_version_ok() {
        return None;
    }
    // `if a.sources:` reads a given config (or stdin): Python.
    if args.text("sources").is_some_and(|text| !text.is_empty()) {
        return None;
    }
    if !no_live_sources(&ctx.root) {
        return None;
    }
    Some(cli_error(
        ctx,
        "work.live",
        "liveerror",
        "no live sources (fix: careerbrew work live start --sources <file.yaml>, or install a plugin with live sources)",
    ))
}

/// `work live cycle` while the feed is stopped: Python takes the cycle lock
/// (creating `work/live/.cycle.lock`), reads the state and raises
/// `LiveError`. A running feed stays with Python.
fn live_cycle(ctx: &Ctx) -> Option<Outcome> {
    if !event_version_ok() {
        return None;
    }
    let root = &ctx.root;
    let state = read_json(root, "work/live/state.json")?;
    match &state {
        Canon::Null => {}
        Canon::Map(_) => {
            if !matches!(
                get(&state, "running"),
                None | Some(Canon::Null) | Some(Canon::Bool(false))
            ) {
                return None;
            }
        }
        _ => return None,
    }
    let dir = root.join("work/live");
    std::fs::create_dir_all(&dir).ok()?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(dir.join(".cycle.lock"))
        .ok()?;
    let message = match lock.try_lock() {
        Ok(()) => "the live feed is stopped (fix: careerbrew work live start)",
        Err(std::fs::TryLockError::WouldBlock) => {
            "another live cycle is running (wait for it to finish, then run the next one)"
        }
        Err(_) => return None,
    };
    let out = cli_error(ctx, "work.live", "liveerror", message);
    drop(lock);
    Some(out)
}

/// What `work live status` prints from the state file when no source exists.
struct LiveState {
    running: bool,
    cycles: i64,
    started_at: String,
}

/// The state and settings `live.status` and `health.snapshot` read, in the
/// shapes whose output this handler copies: no source, no cycle record, no
/// direct-cycle receipt and no saved standup (whose age is a float).
fn live_status_inputs(root: &Path) -> Option<LiveState> {
    if !no_live_sources(root)
        || ![
            "work/live/cycles.jsonl",
            "work/feed/direct-cycle.json",
            "work/standup.json",
        ]
        .iter()
        .all(|rel| absent(root, rel))
    {
        return None;
    }
    let settings = read_yaml(root, "settings.yaml")?;
    match get(&settings, "feed_tool_sources") {
        None => {}
        Some(Canon::List(items)) if items.is_empty() => {}
        Some(_) => return None,
    }
    if get(&settings, "live_source_max_age_minutes").is_some()
        && int(get(&settings, "live_source_max_age_minutes")).is_none()
    {
        return None;
    }
    let state = read_json(root, "work/live/state.json")?;
    let state = match state {
        Canon::Null => Canon::Map(Vec::new()),
        Canon::Map(_) => state,
        _ => return None,
    };
    let running = match get(&state, "running") {
        None => false,
        Some(Canon::Bool(value)) => *value,
        Some(_) => return None,
    };
    let cycles = match get(&state, "cycles") {
        None => 0,
        value => int(value)?,
    };
    let started_at = match get(&state, "started_at") {
        None => String::new(),
        Some(Canon::Str(text)) => text.clone(),
        Some(_) => return None,
    };
    match get(&state, "last") {
        None | Some(Canon::Null) => {}
        Some(Canon::Map(pairs)) if pairs.is_empty() => {}
        Some(_) => return None,
    }
    Some(LiveState {
        running,
        cycles,
        started_at,
    })
}

/// `calendar.now_utc().isoformat()` for a pinned clock with whole seconds;
/// None for the wall clock (its microseconds differ on every run).
fn pinned_utc_iso() -> Option<String> {
    let pinned = std::env::var("CAREERBREW_NOW").ok()?;
    if pinned.contains('.') || iso(&pinned).is_none() {
        return None;
    }
    Some(
        careerbrew_feed::now_utc()
            .format("%Y-%m-%dT%H:%M:%S+00:00")
            .to_string(),
    )
}

const START_FIX: &str = "careerbrew work live start --sources <file.yaml>";

fn live_status(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let root = &ctx.root;
    let state = live_status_inputs(root)?;
    let stdout = if args.flag("json") {
        let as_of = pinned_utc_iso()?;
        let running = if state.running { "true" } else { "false" };
        let mut out = String::from("{\n  \"actions\": [],\n  \"cycles\": ");
        out.push_str(&state.cycles.to_string());
        out.push_str(",\n  \"health\": {\n    \"as_of\": ");
        crate::render::py_string(&as_of, &mut out);
        out.push_str(",\n    \"commands\": [\n      ");
        crate::render::py_string(START_FIX, &mut out);
        out.push_str(concat!(
            "\n    ],\n    \"healthy\": 0,\n    \"mode\": \"none\",\n",
            "    \"next_expiry\": null,\n    \"reason\": \"no-sources\",\n",
            "    \"required\": 0,\n    \"row_review\": {\n",
            "      \"age_seconds\": null,\n      \"checked_at\": null,\n",
            "      \"max_age_minutes\": 60,\n      \"status\": \"unknown\"\n",
            "    },\n    \"running\": ",
        ));
        out.push_str(running);
        out.push_str(concat!(
            ",\n    \"sources\": [],\n    \"status\": \"missing\",\n",
            "    \"version\": 1\n  },\n  \"last\": {},\n  \"running\": ",
        ));
        out.push_str(running);
        out.push_str(",\n  \"sources\": [],\n  \"started_at\": ");
        crate::render::py_string(&state.started_at, &mut out);
        out.push_str("\n}\n");
        out
    } else {
        format!(
            concat!(
                "live feed: {}; {} cycle(s)\n",
                "source health: missing; 0 of 0 required source(s); none coverage\n",
                "  fix: {fix}\n",
                "no live sources: {fix} (or a plugin with live sources)\n",
            ),
            if state.running { "running" } else { "stopped" },
            state.cycles,
            fix = START_FIX,
        )
    };
    // `live._dir()` makes the state folder on every call.
    std::fs::create_dir_all(root.join("work/live")).ok()?;
    Some(Outcome {
        stdout,
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}

/// `notify check` with notifications off: `notify.process` returns before
/// it reads the feed or writes the list; `run` logs one `notify.check` row.
fn notify_check(ctx: &Ctx) -> Option<Outcome> {
    if !event_version_ok() || !notifier_path_ok() {
        return None;
    }
    let root = &ctx.root;
    let settings = read_yaml(root, "settings.yaml")?;
    if !notify_settings_ok(&settings) {
        return None;
    }
    if !matches!(
        get(&settings, "notify_events"),
        None | Some(Canon::Bool(false))
    ) {
        return None;
    }
    let (kind, _, _) = careerbrew_live::notify::backend(root).ok()?;
    if !["none", "osascript", "alerter", "terminal-notifier"].contains(&kind.as_str()) {
        return None;
    }
    let zero = || Canon::Int(0.into());
    careerbrew_feed::emit(
        root,
        "notify.check",
        vec![
            ("status", Canon::Str("off".into())),
            ("source_kind", Canon::Str(kind)),
            ("new", zero()),
            ("sent", zero()),
            ("batched", zero()),
            ("held", zero()),
            ("capped", zero()),
        ],
    )
    .ok()?;
    Some(Outcome {
        stdout: "notifications are off: turn them on with careerbrew config set notify_events true (careerbrew notify status shows the rules)\n".to_owned(),
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}

// ---------- work session list ----------

/// `date.fromisoformat(day).isoformat() == day`.
fn plain_day(day: &str) -> bool {
    day.len() == 10 && iso(&format!("{day}T00:00:00")).is_some()
}

/// Days since 1970-01-01 for a `YYYY-MM-DD` day (Howard Hinnant).
fn day_number(day: &str) -> Option<i64> {
    let y: i64 = day.get(0..4)?.parse().ok()?;
    let m: i64 = day.get(5..7)?.parse().ok()?;
    let d: i64 = day.get(8..10)?.parse().ok()?;
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// The `YYYY-MM-DD` day `n` days after 1970-01-01.
fn day_text(n: i64) -> Option<String> {
    let secs = u64::try_from(n.checked_mul(86_400)?).ok()?;
    crate::events::stamp(secs).get(0..10).map(str::to_owned)
}

/// One saved session as Python's `sessions._load` accepts it, in the forms
/// the listing below reads the same way.
fn session_ok(session: &Canon) -> bool {
    if !matches!(session, Canon::Map(_))
        || ["id", "project", "day", "started", "ended"]
            .iter()
            .any(|key| string(session, key).is_none())
    {
        return false;
    }
    let id = string(session, "id").unwrap_or("");
    let project = string(session, "project").unwrap_or("");
    let started = string(session, "started").unwrap_or("");
    let ended = string(session, "ended").unwrap_or("");
    let paired =
        get(session, "record_id").is_some() == get(session, "project_incarnation").is_some();
    let record_ok = match get(session, "record_id") {
        None => true,
        Some(Canon::Str(value)) => {
            value.len() == 32
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }
        Some(_) => false,
    };
    printable(id)
        && !project.is_empty()
        && paired
        && record_ok
        && matches!(iso(started), Some(Some(_)))
        && plain_day(string(session, "day").unwrap_or(""))
        && (ended.is_empty() || iso(ended).is_some())
        && ["handoff", "wrap_requested_at", "handoff_at"]
            .iter()
            .all(|key| matches!(get(session, key), None | Some(Canon::Str(_))))
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

/// Python's `_fresh_handoff`; None when Python could read a stamp this
/// reader does not.
fn fresh_handoff(session: &Canon) -> Option<bool> {
    if string(session, "handoff").unwrap_or("").is_empty() {
        return Some(false);
    }
    let wrap = string(session, "wrap_requested_at").unwrap_or("");
    if wrap.is_empty() {
        return Some(true);
    }
    let written = string(session, "handoff_at").unwrap_or("");
    if written.is_empty() {
        return Some(false);
    }
    let (Some(_), Some(_)) = (iso(written)?, iso(wrap)?) else {
        return Some(false);
    };
    let written = careerbrew_live::parse_time(written)?;
    let wrap = careerbrew_live::parse_time(wrap)?;
    Some(written > wrap)
}

/// A setting Python reads with `int(config.get(key))`: absent or an int.
fn int_setting(settings: &Canon, key: &str, default: i64) -> Option<i64> {
    match get(settings, key) {
        None => Some(default),
        value => int(value),
    }
}

/// True when a day file `day/work-<day>.json` (the work-day questions) may
/// exist: the listing counts their open questions, which stay with Python.
fn work_day_files(root: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(root.join("day")) else {
        return root.join("day").exists();
    };
    entries.into_iter().any(|entry| match entry {
        Ok(entry) => {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("work-") || name.starts_with(".work-")
        }
        Err(_) => true,
    })
}

struct Row<'a> {
    session: &'a Canon,
    state: &'static str,
    claims: usize,
    fresh: bool,
}

fn session_list(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let root = &ctx.root;
    // Projects and handoffs show only on a terminal (a plugin policy is
    // refused with the redaction inputs).
    if ctx.env.stdout_is_terminal {
        return None;
    }
    // Agent activity and open questions stay with Python.
    if root.join("work/agents/events.jsonl").exists() || work_day_files(root) {
        return None;
    }
    let settings = read_yaml(root, "settings.yaml")?;
    let cap = int_setting(&settings, "work_session_cap", 3)?;
    let keep_days = int_setting(&settings, "agent_keep_days", 7)?;
    int_setting(&settings, "agent_quiet_minutes", 0)?;
    let store = if root.join("work/sessions.json").exists() {
        read_json(root, "work/sessions.json")?
    } else {
        Canon::Map(Vec::new())
    };
    if !matches!(store, Canon::Map(_)) {
        return None;
    }
    let sessions = match get(&store, "sessions") {
        None => &[][..],
        value => list(value)?,
    };
    let claims = match get(&store, "claims") {
        None => &[][..],
        value => list(value)?,
    };
    if !sessions.iter().all(session_ok) || !claims.iter().all(claim_ok) {
        return None;
    }
    let mut records: Vec<&str> = sessions
        .iter()
        .filter_map(|s| string(s, "record_id"))
        .collect();
    let count = records.len();
    records.sort_unstable();
    records.dedup();
    if records.len() != count {
        return None;
    }
    let now = careerbrew_feed::now();
    let today = now.date_naive().to_string();
    let keep = day_text(day_number(&today)?.checked_sub(keep_days)?)?;
    let open_ids: Vec<&str> = sessions
        .iter()
        .filter(|s| string(s, "ended") == Some(""))
        .filter_map(|s| string(s, "id"))
        .collect();
    let held: Vec<&Canon> = claims
        .iter()
        .filter(|c| {
            string(c, "until")
                .and_then(careerbrew_live::parse_time)
                .is_some_and(|until| until > now)
                && string(c, "session").is_some_and(|s| open_ids.contains(&s))
        })
        .collect();
    let all = args.flag("all");
    let mut rows = Vec::new();
    for session in sessions.iter().rev() {
        let ended = !string(session, "ended")?.is_empty();
        let day = string(session, "day")?;
        if ended && (!all || day < keep.as_str()) {
            continue;
        }
        let id = string(session, "id")?;
        rows.push(Row {
            session,
            state: if ended {
                "ended"
            } else if day == today {
                "live"
            } else {
                "stale"
            },
            claims: held
                .iter()
                .filter(|c| string(c, "session") == Some(id))
                .count(),
            fresh: fresh_handoff(session)?,
        });
    }
    let stdout = if args.flag("json") {
        session_json(cap, &rows)?
    } else {
        session_text(cap, &rows)?
    };
    Some(Outcome {
        stdout,
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}

/// `json.dumps({"cap": ..., "sessions": [public(row), ...]})`.
fn session_json(cap: i64, rows: &[Row]) -> Option<String> {
    use crate::render::py_string;
    let mut out = format!("{{\"cap\": {cap}, \"sessions\": [");
    for (n, row) in rows.iter().enumerate() {
        let s = row.session;
        if n > 0 {
            out.push_str(", ");
        }
        out.push_str("{\"id\": ");
        py_string(string(s, "id")?, &mut out);
        out.push_str(", \"project\": ");
        let hash = careerbrew_sessions::digest(string(s, "project")?);
        py_string(hash.get(..8)?, &mut out);
        for key in ["day", "started", "ended"] {
            out.push_str(&format!(", \"{key}\": "));
            py_string(string(s, key)?, &mut out);
        }
        out.push_str(", \"state\": ");
        py_string(row.state, &mut out);
        out.push_str(", \"questions\": 0, \"has_handoff\": ");
        out.push_str(if row.fresh { "true" } else { "false" });
        for key in ["wrap_requested_at", "handoff_at"] {
            out.push_str(&format!(", \"{key}\": "));
            py_string(string(s, key).unwrap_or(""), &mut out);
        }
        out.push_str(
            ", \"activity\": {\"status\": \"none\", \"at\": \"\", \"events\": 0}, \"claims\": ",
        );
        out.push_str(&row.claims.to_string());
        out.push('}');
    }
    out.push_str("]}\n");
    Some(out)
}

/// `cmd_work_session`'s listing with business data hidden.
fn session_text(cap: i64, rows: &[Row]) -> Option<String> {
    if rows.is_empty() {
        return Some("no project sessions: the /work agent opens one per pressing project with careerbrew work session open <project> --id <session id>\n".to_owned());
    }
    let mut out = String::new();
    for row in rows {
        let s = row.session;
        let handoff = if row.fresh {
            "written"
        } else if !string(s, "handoff").unwrap_or("").is_empty() {
            "stale or unverified: write a fresh handoff"
        } else {
            "not yet"
        };
        out.push_str(&format!(
            "  {:<6} {}  no event yet, {} claim(s), 0 open question(s), handoff {handoff}\n",
            row.state,
            string(s, "id")?,
            row.claims
        ));
    }
    let live = rows.iter().filter(|r| r.state == "live").count();
    let stale = rows.iter().filter(|r| r.state == "stale").count();
    out.push_str(&format!("sessions: {live} of {cap} live"));
    if stale > 0 {
        out.push_str(&format!(", {stale} left from an earlier day"));
    }
    out.push('\n');
    out.push_str("projects and handoffs hidden: stdout is not a terminal and no plugin policy allows agents to read business data\n");
    Some(out)
}

// ---------- work session end ----------

/// Only the argument error (no id and no `--all`, or both): Python raises
/// it before it reads the session store. Every real end stays with Python.
fn session_end(args: &Parsed) -> Option<Outcome> {
    let id = args.text("id")?;
    // Python raises when `bool(id) == bool(all)`.
    if id.is_empty() == args.flag("all") {
        return None;
    }
    Some(Outcome {
        stdout: "error: end needs a session id (or its project) or --all (breaks US-107)\n"
            .to_owned(),
        stderr: String::new(),
        code: 2,
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
