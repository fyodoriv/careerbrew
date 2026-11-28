//! Native CLI handlers owned by `rust-16` (see `steps/mod.rs`): the work
//! day's wrap-up control (`day wrap`), its question records (`day
//! question list`, `day question close`), the watch loop's records (`day
//! loop status`, `add` and `stop`; every other action goes to Python) and
//! the step history (`day history`, only in a home that holds nothing else
//! its plan could read), and closing a promise or recording one of its
//! steps (`work promise done`, `dropped` and `step`, only in a plaintext
//! data folder whose `work/projects.yaml` is exactly what PyYAML's
//! `safe_dump` writes for the value read back).
//!
//! Each handler calls the `careerbrew_workexec` / `careerbrew_workday`
//! component and prints what `cli.py` prints. It refuses (exit 64, Python
//! runs it) before any write when:
//! - Python's `may_show_business()` could be true where the output depends
//!   on it (stdout is a terminal, or a plugin lock exists), or
//!   `privacy.redact` would read an input,
//! - the clock or time zone is a form the components would read
//!   differently from `calendar.now()` (see `pin_clock`),
//! - a stored value has a type Python would treat differently, or
//! - a component returns an error Python does not print as `error: ...`.
//!
//! The helpers marked `pub(crate)` are shared with `rust17`.
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;
use std::path::Path;

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust16.rs`.
pub const COMMANDS: &[&str] = &[
    "day.wrap",
    "day.question.list",
    "day.question.close",
    "day.loop",
    "day.history",
    "work.promise.done",
    "work.promise.dropped",
    "work.promise.step",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let started = std::time::Instant::now();
    match name {
        "day.wrap" => wrap(args, ctx),
        "day.question.list" => logged("day.question", question_list(args, ctx)?, ctx, started),
        "day.question.close" => logged("day.question", question_close(args, ctx)?, ctx, started),
        "day.loop" => day_loop(args, ctx),
        "day.history" => history(args, ctx),
        "work.promise.done" => logged(
            "work.promise",
            promise_close(args, ctx, "done")?,
            ctx,
            started,
        ),
        "work.promise.dropped" => logged(
            "work.promise",
            promise_close(args, ctx, "dropped")?,
            ctx,
            started,
        ),
        "work.promise.step" => logged("work.promise", promise_step(args, ctx)?, ctx, started),
        _ => None,
    }
}

/// Appends the `cli.run` row under Python's command name and turns off the
/// one `main` would append. `cli.main` names a nested command by its first
/// two words (`day.question`, `work.promise`), not by the route key.
pub(crate) fn logged(
    command: &str,
    outcome: Outcome,
    ctx: &Ctx,
    started: std::time::Instant,
) -> Option<Outcome> {
    let ms = started.elapsed().as_millis() as u64;
    crate::events::emit_command(&ctx.env.caps, command, outcome.code == 0, ms);
    Some(Outcome {
        emit_event: false,
        ..outcome
    })
}

// ---------- shared helpers (also used by rust17) ----------

/// `serde_json::Value`, named through a public alias of a domain crate:
/// this crate does not depend on `serde_json` directly.
pub(crate) type Value =
    <<careerbrew_sources::connectors::Fetch<'static> as ReturnOf>::R as OkOf>::T;

pub(crate) trait ReturnOf {
    type R;
}

impl<R> ReturnOf for &'static mut dyn FnMut(&str) -> R {
    type R = R;
}

pub(crate) trait OkOf {
    type T;
}

impl<T, E> OkOf for Result<T, E> {
    type T = T;
}

pub(crate) fn out(stdout: String, code: i32) -> Outcome {
    Outcome {
        stdout,
        stderr: String::new(),
        code,
        emit_event: true,
    }
}

/// True when Python's `may_show_business()` is false for sure and
/// `privacy.redact` reads nothing: no terminal stdout, no plugin lock, no
/// sensitive-terms or profile file.
pub(crate) fn business_hidden(ctx: &Ctx) -> bool {
    !ctx.env.stdout_is_terminal && !ctx.redact_inputs_present()
}

fn digits(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit)
}

/// `YYYY-MM-DD` with digits only (a valid date is checked separately).
pub(crate) fn day_form(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 10
        && digits(&b[0..4])
        && b[4] == b'-'
        && digits(&b[5..7])
        && b[7] == b'-'
        && digits(&b[8..10])
}

/// `HH:MM` from 00:00 to 23:59.
fn clock_form(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 5
        && digits(&b[0..2])
        && b[2] == b':'
        && digits(&b[3..5])
        && (b[0] < b'2' || (b[0] == b'2' && b[1] <= b'3'))
        && b[3] <= b'5'
}

/// The stamp forms both `datetime.fromisoformat` and the components read
/// the same way: `YYYY-MM-DDTHH:MM`, optional `:SS` and `.ffffff`, then
/// nothing (naive) or, after seconds, `Z` or `+HH:MM`. Some(true) when it
/// has an offset.
fn stamp_form(text: &str) -> Option<bool> {
    let b = text.as_bytes();
    if b.len() < 16 || !day_form(text.get(..10)?) || b[10] != b'T' {
        return None;
    }
    if !(digits(&b[11..13]) && b[13] == b':' && digits(&b[14..16])) {
        return None;
    }
    let mut rest = &b[16..];
    let mut seconds = false;
    if rest.first() == Some(&b':') {
        if rest.len() < 3 || !digits(&rest[1..3]) {
            return None;
        }
        rest = &rest[3..];
        seconds = true;
        if rest.first() == Some(&b'.') {
            if rest.len() < 7 || !digits(&rest[1..7]) {
                return None;
            }
            rest = &rest[7..];
        }
    }
    match rest {
        [] => Some(false),
        b"Z" if seconds => Some(true),
        [sign, h1, h2, b':', m1, m2]
            if seconds && (*sign == b'+' || *sign == b'-') && digits(&[*h1, *h2, *m1, *m2]) =>
        {
            Some(true)
        }
        _ => None,
    }
}

/// `calendar.now()` for the current `CAREERBREW_NOW`, as (RFC 3339 text,
/// the local day, the UTC offset in seconds), or None when it panics (a
/// date or local time that does not exist).
fn local_now(root: &Path) -> Option<(String, String, i32)> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let got = std::panic::catch_unwind(|| {
        let at = careerbrew_teacher::now_local(root);
        (
            at.to_rfc3339(),
            at.date_naive().to_string(),
            at.offset().local_minus_utc(),
        )
    })
    .ok();
    std::panic::set_hook(hook);
    got
}

fn restore_now(pinned: Option<&str>) {
    match pinned {
        Some(text) => std::env::set_var("CAREERBREW_NOW", text),
        None => std::env::remove_var("CAREERBREW_NOW"),
    }
}

/// Pins the components' clock to `calendar.now()`, and returns the
/// settings (`settings.yaml`, Null when missing).
///
/// The components read `CAREERBREW_NOW` only as RFC 3339 (else the UTC wall
/// clock), while Python reads a naive pin, and the wall clock, in the
/// `timezone` setting or the machine's zone. This resolves the local time
/// as Python does (`careerbrew_teacher::now_local`) and exports it as RFC
/// 3339, so every component call in this run sees that instant and offset.
/// It refuses (None) on a pin form, a zone, or a day with a UTC offset
/// change that the two sides could read differently. It also exports the
/// package version the components stamp on their event rows.
pub(crate) fn pin_clock(root: &Path) -> Option<Value> {
    let settings: Value =
        careerbrew_projects::read_yaml(root, "settings.yaml", Default::default()).ok()?;
    if !(settings.is_null() || settings.is_object()) {
        return None;
    }
    if let Some(zone) = settings.get("timezone") {
        if !zone.is_null() {
            let name = zone.as_str()?;
            if !name.is_empty() && careerbrew_tutor::local_zone(root).name() != name {
                return None;
            }
        }
    }
    let pinned = std::env::var("CAREERBREW_NOW").ok();
    let aware = match &pinned {
        Some(text) => stamp_form(text)?,
        None => false,
    };
    let (now, day, offset) = local_now(root)?;
    if !aware {
        // Python combines a naive clock time with the zone at that time;
        // the components use the current offset. They agree unless the
        // zone's offset changes during this day.
        for clock in ["00:00:00", "23:59:59"] {
            std::env::set_var("CAREERBREW_NOW", format!("{day}T{clock}"));
            let edge = local_now(root).map(|(_, _, edge)| edge);
            if edge != Some(offset) {
                restore_now(pinned.as_deref());
                return None;
            }
        }
    }
    std::env::set_var("CAREERBREW_NOW", now);
    std::env::set_var("CAREERBREW_VERSION", crate::events::version());
    Some(settings)
}

/// Python's `json.dumps` (default separators, `ensure_ascii`) of a value
/// read from a JSON file. None for a value whose spelling could differ (a
/// float, or an object with more than one key).
pub(crate) fn py_json(value: &Value, out: &mut String) -> Option<()> {
    if value.is_null() {
        out.push_str("null");
    } else if let Some(b) = value.as_bool() {
        out.push_str(if b { "true" } else { "false" });
    } else if let Some(s) = value.as_str() {
        crate::render::py_string(s, out);
    } else if value.is_i64() || value.is_u64() {
        out.push_str(&value.to_string());
    } else if let Some(items) = value.as_array() {
        out.push('[');
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            py_json(item, out)?;
        }
        out.push(']');
    } else {
        // Anything else but an object is a float.
        let map = value.as_object()?;
        // This map's keys come back sorted; `json.loads` keeps the file's
        // order, so only an object with one key is spelled the same.
        if map.len() > 1 {
            return None;
        }
        out.push('{');
        for (i, (key, item)) in map.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            crate::render::py_string(key, out);
            out.push_str(": ");
            py_json(item, out)?;
        }
        out.push('}');
    }
    Some(())
}

/// The `error: <msg> (breaks <story>)` line and exit 2 of `_workday_fail`
/// (and `cmd_work_promise`).
pub(crate) fn fail(message: &str, story: &str) -> Outcome {
    out(format!("error: {message} (breaks {story})\n"), 2)
}

// ---------- day wrap ----------

fn wrap(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let at = args.text("at");
    let hold = args.flag("hold");
    // argparse's mutually exclusive group prints usage and exits 2.
    if at.is_some() && hold {
        return None;
    }
    // The lines hold no business text, but `say` redacts them.
    if ctx.redact_inputs_present() {
        return None;
    }
    let root = ctx.root.as_path();
    let settings = pin_clock(root)?;
    // `int(config.get("work_wrap_minutes"))` and
    // `time.fromisoformat(str(config.get("work_end")))`: only the forms the
    // component reads the same way.
    // A value far from a day's length overflows Python's `timedelta` or
    // `datetime` (a traceback) where chrono still computes a stamp.
    if let Some(minutes) = settings.get("work_wrap_minutes") {
        if minutes.as_i64()?.abs() > 1_000_000 {
            return None;
        }
    }
    if let Some(end) = settings.get("work_end") {
        if !clock_form(end.as_str()?) {
            return None;
        }
    }
    let day = careerbrew_workday::today();
    let data = careerbrew_workexec::day_data(root, day).ok()?;
    // The day file is written back whole: a float could be spelled differently.
    if (at.is_some() || hold) && has_float(&careerbrew_setup::value::to_canon(&data)) {
        return None;
    }
    if let Some(control) = data.get("wrap_control") {
        if careerbrew_projects::truthy(Some(control)) {
            if !control.is_object() {
                return None;
            }
            if let Some(chosen) = control.get("at") {
                let chosen = chosen.as_str()?;
                if !chosen.is_empty() && !clock_form(chosen) {
                    return None;
                }
            }
        }
    }
    // Read-only first, so a refusal never follows the write.
    careerbrew_workexec::end::view(root, careerbrew_workday::now()).ok()?;
    let got = match careerbrew_workexec::end::control(root, at.unwrap_or(""), hold) {
        Ok(got) => got,
        Err(error) if error.class == "ValueError" => return Some(fail(&error.message, "US-106")),
        Err(_) => return None,
    };
    let text = |key: &str| got.get(key).and_then(Value::as_str).map(str::to_owned);
    let (wrap_at, phase, end_at) = (text("at")?, text("phase")?, text("end_at")?);
    let held = got.get("hold").and_then(Value::as_bool)?;
    let mut stdout = String::new();
    if args.flag("json") {
        stdout.push_str("{\"at\": ");
        crate::render::py_string(&wrap_at, &mut stdout);
        stdout.push_str(", \"phase\": ");
        crate::render::py_string(&phase, &mut stdout);
        stdout.push_str(", \"command\": ");
        crate::render::py_string(careerbrew_workexec::end::COMMAND, &mut stdout);
        stdout.push_str(if held {
            ", \"hold\": true, \"end_at\": "
        } else {
            ", \"hold\": false, \"end_at\": "
        });
        crate::render::py_string(&end_at, &mut stdout);
        stdout.push_str("}\n");
    } else if held {
        stdout.push_str("wrap-up: held; automatic wrap-up and clock ending are paused. Resume with careerbrew day wrap --at HH:MM, or explicitly end now with careerbrew day end.\n");
    } else {
        stdout.push_str(&format!(
            "wrap-up: {}; end {}. Explicit careerbrew day end ends now, including while held.\n",
            wrap_at.get(11..16)?,
            end_at.get(11..16)?
        ));
    }
    Some(out(stdout, 0))
}

pub(crate) use careerbrew_vault::canon::Canon;

/// True when a value holds a float anywhere: Python and the components may
/// spell one differently when a file is written back.
pub(crate) fn has_float(value: &Canon) -> bool {
    match value {
        Canon::Float(_) => true,
        Canon::List(items) => items.iter().any(has_float),
        Canon::Map(pairs) => pairs.iter().any(|(k, v)| has_float(k) || has_float(v)),
        _ => false,
    }
}

// ---------- day question ----------

/// None unless Python and the component read the carried days' records the
/// same way.
///
/// Each record Python counts (a dict with a truthy `id`) must have a string
/// `id` and `step`; a string or null `closed`, `recheck` and `source`; a
/// list `options` when present; a `createdAt` that is empty or an aware
/// stamp; and no `state` or `why` key (only a recheck sets those). No day
/// file may hold a float, since `close` writes one back.
fn checked_days(root: &Path) -> Option<()> {
    let mut day = careerbrew_workday::today();
    for n in 0..=careerbrew_workday::questions::CARRY_DAYS {
        let rel = format!("day/work-{day}.json");
        let data = careerbrew_workday::read_json(root, &rel, Default::default()).ok()?;
        if has_float(&careerbrew_setup::value::to_canon(&data)) {
            return None;
        }
        for q in data
            .get("questions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if !q.is_object() || !careerbrew_projects::truthy(q.get("id")) {
                continue;
            }
            q.get("id")?.as_str()?;
            q.get("step")?.as_str()?;
            for key in ["closed", "recheck", "source"] {
                if q.get(key).is_some_and(|v| !v.is_null() && !v.is_string()) {
                    return None;
                }
            }
            if let Some(options) = q.get("options") {
                options.as_array()?;
            }
            if let Some(created) = q.get("createdAt") {
                let created = created.as_str()?;
                if !created.is_empty() && stamp_form(created) != Some(true) {
                    return None;
                }
            }
            if q.get("state").is_some() || q.get("why").is_some() {
                return None;
            }
        }
        if n < careerbrew_workday::questions::CARRY_DAYS {
            day = day.pred_opt()?;
        }
    }
    Some(())
}

/// `json.dumps(workday_questions.public(q))`, keys in Python's order.
fn public_json(q: &Value, out: &mut String) -> Option<()> {
    fn key(name: &str, first: &mut bool, out: &mut String) {
        if !*first {
            out.push_str(", ");
        }
        *first = false;
        crate::render::py_string(name, out);
        out.push_str(": ");
    }
    out.push('{');
    let mut first = true;
    for name in [
        "id",
        "step",
        "steps",
        "source",
        "createdAt",
        "day",
        "closed",
        "closedAt",
        "age",
    ] {
        if let Some(value) = q.get(name) {
            key(name, &mut first, out);
            match value.as_str() {
                Some(text) if name == "source" && text.starts_with("plugin:") => {
                    crate::render::py_string("plugin", out)
                }
                _ => py_json(value, out)?,
            }
        }
    }
    key("recheck", &mut first, out);
    let recheck = q.get("recheck").and_then(Value::as_str).unwrap_or("");
    crate::render::py_string(&careerbrew_workday::questions::kind(recheck), out);
    key("options", &mut first, out);
    let options = q
        .get("options")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    out.push_str(&options.to_string());
    out.push('}');
    Some(())
}

fn question_list(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if !business_hidden(ctx) {
        return None;
    }
    let root = ctx.root.as_path();
    pin_clock(root)?;
    checked_days(root)?;
    let rows =
        careerbrew_workday::questions::open_records(root, careerbrew_workday::today()).ok()?;
    let mut stdout = String::new();
    if args.flag("json") {
        stdout.push_str("{\"questions\": [");
        for (i, q) in rows.iter().enumerate() {
            if i > 0 {
                stdout.push_str(", ");
            }
            public_json(q, &mut stdout)?;
        }
        stdout.push_str("]}\n");
        return Some(out(stdout, 0));
    }
    for q in &rows {
        let text = |key: &str| q.get(key).and_then(Value::as_str).map(str::to_owned);
        stdout.push_str(&format!(
            "  {}  {}  {} old  (text hidden)\n",
            text("id")?,
            text("step")?,
            text("age")?
        ));
    }
    stdout.push_str(&format!("questions: {} open", rows.len()));
    if !rows.is_empty() {
        stdout.push_str("; before the batched ask: careerbrew day question recheck");
    }
    stdout.push('\n');
    Some(out(stdout, 0))
}

fn question_close(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if !business_hidden(ctx) {
        return None;
    }
    let ident = args.text("id")?;
    let root = ctx.root.as_path();
    pin_clock(root)?;
    checked_days(root)?;
    let day = careerbrew_workday::today();
    match careerbrew_workday::questions::close(root, ident, "answered", day, None) {
        Ok(true) => Some(out(format!("question {ident} closed\n"), 0)),
        Ok(false) => Some(fail(
            &format!("no open question {ident} (see: careerbrew day question list)"),
            "US-104",
        )),
        Err(_) => None,
    }
}

// ---------- day loop: Canon checks and Python spellings ----------

use super::rust14::{get, list};

/// True when `value` reads the same through `json.loads` and through the
/// components' serde round trip, and `canon_json` spells it as Python
/// does: text keys that never repeat, integers within i64, finite floats,
/// and no YAML dates.
fn plain_canon(value: &Canon) -> bool {
    match value {
        Canon::Null | Canon::Bool(_) | Canon::Str(_) => true,
        Canon::Int(n) => n.to_string().parse::<i64>().is_ok(),
        Canon::Float(f) => f.is_finite(),
        Canon::Date(_) | Canon::DateTime(_) => false,
        Canon::List(items) => items.iter().all(plain_canon),
        Canon::Map(pairs) => {
            let mut seen: Vec<&str> = Vec::new();
            pairs.iter().all(|(k, v)| match k {
                Canon::Str(key) if !seen.contains(&key.as_str()) => {
                    seen.push(key);
                    plain_canon(v)
                }
                _ => false,
            })
        }
    }
}

/// `json.dumps(value)` (default separators, `ensure_ascii`) of a value
/// `plain_canon` accepts, with each map's keys in stored order, as
/// `json.loads` keeps them.
fn canon_json(value: &Canon, out: &mut String) -> Option<()> {
    match value {
        Canon::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                canon_json(item, out)?;
            }
            out.push(']');
        }
        Canon::Map(pairs) => {
            out.push('{');
            for (i, (key, item)) in pairs.iter().enumerate() {
                let Canon::Str(key) = key else {
                    return None;
                };
                if i > 0 {
                    out.push_str(", ");
                }
                crate::render::py_string(key, out);
                out.push_str(": ");
                canon_json(item, out)?;
            }
            out.push('}');
        }
        Canon::Str(text) => crate::render::py_string(text, out),
        Canon::Date(_) | Canon::DateTime(_) => return None,
        scalar => out.push_str(&careerbrew_vault::json_yaml::json_compact_python(scalar).ok()?),
    }
    Some(())
}

/// Python's `repr` of a text of printable ASCII with no quote or backslash
/// (`'text'`); None for any other text.
fn simple_repr(text: &str) -> Option<String> {
    text.bytes()
        .all(|b| (0x20..0x7f).contains(&b) && b != b'\'' && b != b'\\')
        .then(|| format!("'{text}'"))
}

fn first_chars(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

/// A `YYYY-MM-DD` day that `date.fromisoformat` and the components accept.
fn real_day(text: &str) -> bool {
    day_form(text)
        && text.get(..4) != Some("0000")
        && careerbrew_workexec::parse_date(text).is_some()
}

/// A loop stamp that `datetime.fromisoformat` and the components' parser
/// read the same way: `YYYY-MM-DDTHH:MM`, optional `:SS` then optional
/// `.ffffff`, then nothing or `+HH:MM` / `-HH:MM`.
fn loop_stamp(text: &str) -> bool {
    let b = text.as_bytes();
    if !text.is_ascii() || b.len() < 16 || b[10] != b'T' || !real_day(&text[..10]) {
        return false;
    }
    let two = |i: usize, max: u8| {
        b.len() >= i + 2
            && b[i].is_ascii_digit()
            && b[i + 1].is_ascii_digit()
            && (b[i] - b'0') * 10 + (b[i + 1] - b'0') <= max
    };
    if !(two(11, 23) && b[13] == b':' && two(14, 59)) {
        return false;
    }
    let mut at = 16;
    if b.get(at) == Some(&b':') {
        if !two(at + 1, 59) {
            return false;
        }
        at += 3;
        if b.get(at) == Some(&b'.') {
            if b.len() < at + 7 || !b[at + 1..at + 7].iter().all(u8::is_ascii_digit) {
                return false;
            }
            at += 7;
        }
    }
    match &b[at..] {
        [] => true,
        [sign, ..] => {
            (*sign == b'+' || *sign == b'-')
                && b.len() == at + 6
                && two(at + 1, 23)
                && b[at + 3] == b':'
                && two(at + 4, 59)
        }
    }
}

/// The loop store (`day/loops.json`, an empty map when missing) when
/// Python's `watch._load` accepts it and the component reads it the same
/// way; None for anything else (Python then prints its own error).
fn loop_store(root: &Path) -> Option<Canon> {
    // `paths.sub` makes the folder before Python reads the file.
    std::fs::create_dir_all(root.join("day")).ok()?;
    let state = super::rust14::read_json(root, "day/loops.json")?;
    if !plain_canon(&state) || !matches!(state, Canon::Map(_)) {
        return None;
    }
    if !matches!(get(&state, "passes"), None | Some(Canon::List(_)))
        || !matches!(get(&state, "inputs"), None | Some(Canon::Map(_)))
        || !matches!(get(&state, "loops"), None | Some(Canon::List(_)))
    {
        return None;
    }
    for lp in list(get(&state, "loops")).unwrap_or(&[]) {
        let text = |key: &str| match get(lp, key) {
            Some(Canon::Str(text)) => Some(text.as_str()),
            _ => None,
        };
        let kind = text("kind")?;
        let stopped = text("stopped")?;
        if !careerbrew_workexec::watch::KINDS.contains(&kind)
            || text("id")?.is_empty()
            || text("step").is_none()
            || !loop_stamp(text("started")?)
            || !real_day(text("day")?)
            || !(stopped.is_empty() || loop_stamp(stopped))
        {
            return None;
        }
    }
    Some(state)
}

fn loop_text<'a>(lp: &'a Canon, key: &str) -> &'a str {
    match get(lp, key) {
        Some(Canon::Str(text)) => text,
        _ => "",
    }
}

/// `cli._loop_line`.
fn loop_line(lp: &Canon) -> String {
    let step = loop_text(lp, "step");
    let step = if step.is_empty() {
        String::new()
    } else {
        format!(" for {step}")
    };
    format!(
        "{} {}{step} (since {} on {})",
        loop_text(lp, "kind"),
        loop_text(lp, "id"),
        loop_text(lp, "started").get(11..16).unwrap_or(""),
        loop_text(lp, "day")
    )
}

/// The `cmd_day_loop` actions served here: `status`, `add` and `stop`.
fn day_loop(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    // `say` redacts every line it prints.
    if ctx.redact_inputs_present() {
        return None;
    }
    let root = ctx.root.as_path();
    let action = args.text("action")?;
    if !matches!(action, "status" | "add" | "stop") {
        return None;
    }
    pin_clock(root)?;
    match action {
        "status" => loop_status(args, root),
        "add" => loop_add(args, root),
        _ => loop_stop(args, root),
    }
}

/// Python's truth value of a stored value.
fn truthy(value: &Canon) -> bool {
    match value {
        Canon::Null | Canon::Bool(false) => false,
        Canon::Str(text) => !text.is_empty(),
        Canon::List(items) => !items.is_empty(),
        Canon::Map(pairs) => !pairs.is_empty(),
        Canon::Int(n) => n.to_string() != "0",
        Canon::Float(f) => *f != 0.0,
        _ => true,
    }
}

fn loop_status(args: &Parsed, root: &Path) -> Option<Outcome> {
    let state = loop_store(root)?;
    let passes = list(get(&state, "passes")).unwrap_or(&[]);
    let mut seconds = Vec::new();
    for pass in passes {
        match pass {
            Canon::Int(n) => {
                let value: i64 = n.to_string().parse().ok()?;
                if value.abs() > 1 << 40 {
                    return None;
                }
                seconds.push(value as f64);
            }
            Canon::Float(f) if f.abs() < 1e12 => seconds.push(*f),
            _ => return None,
        }
    }
    let interval = careerbrew_workexec::watch::interval(&seconds);
    let next_at = match get(&state, "next_at") {
        None => "",
        Some(Canon::Str(text)) => text.as_str(),
        Some(_) => return None,
    };
    let today = careerbrew_workday::today().to_string();
    let running: Vec<&Canon> = list(get(&state, "loops"))
        .unwrap_or(&[])
        .iter()
        .filter(|lp| loop_text(lp, "stopped").is_empty())
        .collect();
    let stale = |lp: &Canon| loop_text(lp, "day") < today.as_str();
    let mut stdout = String::new();
    if args.flag("json") {
        let empty = Canon::Map(Vec::new());
        let last = match get(&state, "last") {
            Some(value) if truthy(value) => value,
            _ => &empty,
        };
        let no_end = Canon::Str(String::new());
        stdout.push_str("{\"passes\": ");
        canon_json(&Canon::List(passes.to_vec()), &mut stdout)?;
        stdout.push_str(&format!(", \"interval\": {interval}, \"next_at\": "));
        crate::render::py_string(next_at, &mut stdout);
        stdout.push_str(", \"last\": ");
        canon_json(last, &mut stdout)?;
        stdout.push_str(", \"ended\": ");
        canon_json(get(&state, "ended").unwrap_or(&no_end), &mut stdout)?;
        stdout.push_str(", \"running\": [");
        for (i, lp) in running.iter().enumerate() {
            if i > 0 {
                stdout.push_str(", ");
            }
            for (j, key) in ["kind", "id", "step", "day"].iter().enumerate() {
                stdout.push_str(if j == 0 { "{" } else { ", " });
                crate::render::py_string(key, &mut stdout);
                stdout.push_str(": ");
                crate::render::py_string(loop_text(lp, key), &mut stdout);
            }
            stdout.push('}');
        }
        stdout.push_str("], \"stale\": [");
        for (i, lp) in running.iter().filter(|lp| stale(lp)).enumerate() {
            if i > 0 {
                stdout.push_str(", ");
            }
            crate::render::py_string(loop_text(lp, "id"), &mut stdout);
        }
        stdout.push_str("]}\n");
        return Some(out(stdout, 0));
    }
    let next: String = next_at.chars().skip(11).take(5).collect();
    stdout.push_str(&format!(
        "watch loop: {} measured pass(es), every {interval} s; next pass at {}; re-arm: careerbrew day loop pass\n",
        passes.len(),
        if next.is_empty() { "not armed" } else { &next }
    ));
    for lp in &running {
        let mark = if stale(lp) {
            "  [survived an earlier day]"
        } else {
            ""
        };
        stdout.push_str(&format!("  running {}{mark}\n", loop_line(lp)));
    }
    stdout.push_str(&format!(
        "{} loop(s) running; end of day: careerbrew day loop end\n",
        running.len()
    ));
    Some(out(stdout, 0))
}

/// `watch.LOOP_ID`, without the trailing newline Python's `$` lets
/// through (a caller refuses an id with a newline first).
fn loop_id_ok(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'-'))
}

fn loop_add(args: &Parsed, root: &Path) -> Option<Outcome> {
    let kind = args.text("kind").unwrap_or("");
    let id = args.text("id").unwrap_or("");
    if kind.is_empty() || id.is_empty() {
        return Some(fail(
            "add needs --kind and --id: the loop's kind (cron, monitor, agent, session) and its tool's id",
            "US-103",
        ));
    }
    if id.contains('\n') {
        return None;
    }
    if !loop_id_ok(id) {
        let shown = simple_repr(&first_chars(id, 40))?;
        return Some(fail(
            &format!(
                "loop id {shown}: use the tool's own id, 1 to 64 letters, digits, . _ : - (no spaces)"
            ),
            "US-103",
        ));
    }
    let step = args.text("step").unwrap_or("");
    if !step.is_empty() {
        // Python strips with `str.strip()`; for printable ASCII that is spaces.
        if !step.bytes().all(|b| (0x20..0x7f).contains(&b)) {
            return None;
        }
        let name = step.trim_matches(' ');
        let bare = name.strip_prefix('/').unwrap_or(name);
        if !loop_id_ok(bare) || name.contains("..") {
            let shown = simple_repr(&first_chars(name, 40))?;
            return Some(fail(
                &format!(
                    "step {shown}: use a command or skill name such as /standup (letters, digits, . _ : -)"
                ),
                "US-103",
            ));
        }
    }
    loop_store(root)?;
    careerbrew_workexec::watch::add(root, kind, id, step, "").ok()?;
    Some(out(
        format!("recorded {kind} {id}: end of day checks it stopped (careerbrew day loop end)\n"),
        0,
    ))
}

fn loop_stop(args: &Parsed, root: &Path) -> Option<Outcome> {
    let id = args.text("id").unwrap_or("");
    if id.is_empty() {
        return Some(fail(
            "stop needs --id: the loop's id from careerbrew day loop status",
            "US-103",
        ));
    }
    let state = loop_store(root)?;
    let running = list(get(&state, "loops"))
        .unwrap_or(&[])
        .iter()
        .filter(|lp| loop_text(lp, "id") == id && loop_text(lp, "stopped").is_empty())
        .count();
    let shown = simple_repr(&first_chars(id, 40));
    if running == 0 && shown.is_none() {
        return None;
    }
    match careerbrew_workexec::watch::stop(root, id, "", false) {
        Ok(_) => Some(out(format!("stopped {id}\n"), 0)),
        Err(error) if error.class == "WatchError" && running == 0 => Some(fail(
            &format!(
                "no running loop has id {} (careerbrew day loop status lists them)",
                shown?
            ),
            "US-103",
        )),
        Err(_) => None,
    }
}

// ---------- day history ----------

/// True when every data-root file is one `day history` may meet here.
/// With nothing else, Python's `plan.build()` declares only careerbrew's
/// own step, raises nothing, and writes only two empty lock files.
fn history_home_ok(root: &Path, dir: &Path) -> Option<()> {
    for entry in std::fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        let kind = std::fs::symlink_metadata(&path).ok()?.file_type();
        let rel = path.strip_prefix(root).ok()?.to_str()?.to_owned();
        if kind.is_symlink() {
            return None;
        }
        if kind.is_dir() {
            history_home_ok(root, &path)?;
            continue;
        }
        let known = rel.starts_with("logs/")
            || matches!(
                rel.as_str(),
                ".plaintext"
                    | "machine.yaml"
                    | "settings.yaml"
                    | "day/work-history.json"
                    | "day/.work-history.json.lock"
                    | "work/feed/.lock"
            )
            || (rel.starts_with("day/.pm-sent-") && rel.ends_with(".lock"));
        if !known {
            return None;
        }
    }
    Some(())
}

/// Monday = 0, for a `YYYY-MM-DD` day.
fn weekday(day: &str) -> Option<i64> {
    let y: i64 = day.get(0..4)?.parse().ok()?;
    let m: i64 = day.get(5..7)?.parse().ok()?;
    let d: i64 = day.get(8..10)?.parse().ok()?;
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    // 1970-01-01, day 0, was a Thursday.
    Some((era * 146_097 + doe - 719_468 + 3).rem_euclid(7))
}

/// `timing.shown` for a duration that is present.
fn shown(seconds: i64) -> String {
    if seconds < 60 {
        return format!("{seconds} s");
    }
    let minutes = careerbrew_workexec::round_half_even(seconds, 60);
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{} h {} min", minutes / 60, minutes % 60)
    }
}

#[derive(Default)]
struct StepRuns {
    name: String,
    runs: i64,
    ends: [i64; 3],
    seconds: i64,
    times: Vec<i64>,
}

const ENDS: [&str; 3] = ["done", "blocked", "failed"];

/// An entry's `seconds` as `isinstance(seconds, int)` reads it, or Err for
/// a value this handler does not serve (a bool, or a huge number).
fn entry_seconds(entry: &Canon) -> Result<Option<i64>, ()> {
    match get(entry, "seconds") {
        Some(Canon::Bool(_)) => Err(()),
        Some(Canon::Int(n)) => match n.to_string().parse::<i64>() {
            Ok(v) if v.abs() < 1 << 40 => Ok(Some(v)),
            _ => Err(()),
        },
        _ => Ok(None),
    }
}

type DayRow<'a> = (&'a str, usize, [i64; 3], i64);

fn history(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if !business_hidden(ctx) {
        return None;
    }
    let n_days = match args.get("days") {
        crate::args::Value::Number(t) | crate::args::Value::Text(t)
            if (1..=3).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_digit()) =>
        {
            t.parse::<i64>().ok()?
        }
        _ => return None,
    };
    if !(1..=120).contains(&n_days) {
        return None;
    }
    let root = ctx.root.as_path();
    let settings = pin_clock(root)?;
    if settings
        .as_object()
        .is_some_and(|map| map.keys().any(|key| key != "timezone"))
    {
        return None;
    }
    history_home_ok(root, root)?;
    if root.join("machine.yaml").exists() {
        super::rust14::read_yaml(root, "machine.yaml")?;
    }
    let data = super::rust14::read_json(root, "day/work-history.json")?;
    if !plain_canon(&data) {
        return None;
    }
    let today = careerbrew_workday::today();
    let mut since = today;
    for _ in 1..n_days {
        since = since.pred_opt()?;
    }
    let (since, until) = (since.to_string(), today.to_string());
    let stored: &[(Canon, Canon)] = match get(&data, "days") {
        Some(Canon::Map(pairs)) => pairs,
        _ => &[],
    };
    let mut days: Vec<(&str, &[(Canon, Canon)])> = Vec::new();
    for (day, entries) in stored {
        if let (Canon::Str(day), Canon::Map(entries)) = (day, entries) {
            if since.as_str() <= day.as_str() && day.as_str() <= until.as_str() {
                days.push((day, entries));
            }
        }
    }
    days.sort_by(|a, b| b.0.cmp(a.0));
    let mut per: Vec<StepRuns> = Vec::new();
    let mut day_rows: Vec<DayRow> = Vec::new();
    for (day, pairs) in &days {
        let entries: Vec<(&str, &Canon)> = pairs
            .iter()
            .filter_map(|(k, v)| match (k, v) {
                (Canon::Str(k), Canon::Map(_)) => Some((k.as_str(), v)),
                _ => None,
            })
            .collect();
        let mut counts = [0; 3];
        let mut took = 0;
        for (step, entry) in &entries {
            let status = match get(entry, "status") {
                Some(Canon::Str(s)) => ENDS.iter().position(|end| end == s),
                _ => None,
            };
            let secs = entry_seconds(entry).ok()?;
            if let Some(i) = status {
                counts[i] += 1;
            }
            took += secs.unwrap_or(0);
            let at = match per.iter().position(|p| p.name == *step) {
                Some(at) => at,
                None => {
                    per.push(StepRuns {
                        name: (*step).to_owned(),
                        ..StepRuns::default()
                    });
                    per.len() - 1
                }
            };
            let p = &mut per[at];
            p.runs += 1;
            if let Some(i) = status {
                p.ends[i] += 1;
            }
            if let Some(s) = secs {
                p.seconds += s;
                if status == Some(0) {
                    p.times.push(s);
                }
            }
        }
        day_rows.push((day, entries.len(), counts, took));
    }
    // `timing.labels` with only careerbrew's own step declared: every other
    // name becomes `other step <n>`, numbered in name order.
    let mut names: Vec<&str> = per.iter().map(|p| p.name.as_str()).collect();
    names.sort_unstable();
    let mut n = 0;
    let labels: Vec<(&str, String)> = names
        .iter()
        .map(|name| {
            if *name == "/careerbrew" {
                (*name, (*name).to_owned())
            } else {
                n += 1;
                (*name, format!("other step {n}"))
            }
        })
        .collect();
    let mut steps: Vec<(String, &StepRuns, Option<i64>)> = per
        .iter()
        .map(|p| {
            let label = labels
                .iter()
                .find(|(name, _)| *name == p.name)
                .map_or_else(String::new, |(_, label)| label.clone());
            let mut times = p.times.clone();
            times.sort_unstable();
            let k = times.len();
            let median = match k {
                0 => None,
                _ if k % 2 == 1 => Some(times[k / 2]),
                _ => Some(((times[k / 2 - 1] + times[k / 2]) as f64 / 2.0).trunc() as i64),
            };
            (label, p, median)
        })
        .collect();
    steps.sort_by(|a, b| b.1.ends[2].cmp(&a.1.ends[2]).then_with(|| a.0.cmp(&b.0)));
    let mut stdout = String::new();
    if args.flag("json") {
        stdout.push_str("{\"since\": ");
        crate::render::py_string(&since, &mut stdout);
        stdout.push_str(", \"until\": ");
        crate::render::py_string(&until, &mut stdout);
        stdout.push_str(", \"days\": [");
        for (i, (day, count, ends, took)) in day_rows.iter().enumerate() {
            if i > 0 {
                stdout.push_str(", ");
            }
            stdout.push_str("{\"day\": ");
            crate::render::py_string(day, &mut stdout);
            stdout.push_str(&format!(
                ", \"steps\": {count}, \"done\": {}, \"blocked\": {}, \"failed\": {}, \"seconds\": {took}}}",
                ends[0], ends[1], ends[2]
            ));
        }
        stdout.push_str("], \"steps\": [");
        for (i, (label, p, median)) in steps.iter().enumerate() {
            if i > 0 {
                stdout.push_str(", ");
            }
            stdout.push_str("{\"step\": ");
            crate::render::py_string(label, &mut stdout);
            let median = median.map_or_else(|| "null".to_owned(), |m| m.to_string());
            stdout.push_str(&format!(
                ", \"runs\": {}, \"done\": {}, \"blocked\": {}, \"failed\": {}, \"seconds\": {}, \"median\": {median}}}",
                p.runs, p.ends[0], p.ends[1], p.ends[2], p.seconds
            ));
        }
        stdout.push_str("]}\n");
    } else {
        for (label, p, median) in &steps {
            let mid = median.map_or_else(String::new, |m| format!(", median {}", shown(m)));
            stdout.push_str(&format!(
                "  {label:<24} {} run(s): {} done, {} blocked, {} failed{mid}\n",
                p.runs, p.ends[0], p.ends[1], p.ends[2]
            ));
        }
        for (day, count, ends, took) in &day_rows {
            stdout.push_str(&format!(
                "  {day}  {count} step(s): {} done, {} blocked, {} failed, {} of steps\n",
                ends[0],
                ends[1],
                ends[2],
                shown(*took)
            ));
        }
        let most = steps
            .iter()
            .find(|(_, p, _)| p.ends[2] != 0)
            .map_or_else(String::new, |(label, p, _)| {
                format!("; fails most: {label} ({})", p.ends[2])
            });
        stdout.push_str(&format!(
            "work day history {since} to {until}: {} day(s), {} step(s){most}\n",
            day_rows.len(),
            steps.len()
        ));
    }
    // `plan.build()` holds the previous workday's sent-PM lock and the feed
    // lock while it reads; both files stay behind, empty.
    let mut prior = today;
    for _ in 0..7 {
        prior = prior.pred_opt()?;
        if weekday(&prior.to_string())? < 5 {
            break;
        }
    }
    for rel in [
        format!("day/.pm-sent-{prior}.lock"),
        "work/feed/.lock".to_owned(),
    ] {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
    }
    Some(out(stdout, 0))
}

// ---------- work promise done, dropped and step (wave 4) ----------

const PROMISE_STEPS: [&str; 3] = ["research", "draft", "status"];

/// `projects.UPDATES_KEPT` and `projects.UPDATE_MAX`.
const UPDATES_KEPT: usize = 20;
const UPDATE_MAX: usize = 200;

fn ckey(name: &str) -> Canon {
    Canon::Str(name.to_owned())
}

/// The value under a text key; Err for a map whose keys are not distinct
/// text (Python and this module could then pick another entry).
fn pget<'a>(pairs: &'a [(Canon, Canon)], key: &str) -> Result<Option<&'a Canon>, ()> {
    let mut found = None;
    for (k, v) in pairs {
        let Canon::Str(name) = k else {
            return Err(());
        };
        if name == key {
            if found.is_some() {
                return Err(());
            }
            found = Some(v);
        }
    }
    Ok(found)
}

/// `d[key] = value`: an existing key keeps its place, a new one goes last.
fn pset(pairs: &mut Vec<(Canon, Canon)>, key: &str, value: Canon) {
    match pairs
        .iter_mut()
        .find(|(k, _)| matches!(k, Canon::Str(k) if k == key))
    {
        Some((_, slot)) => *slot = value,
        None => pairs.push((ckey(key), value)),
    }
}

/// Python's truth value, or None for a float (not modelled here).
fn ptruthy(value: Option<&Canon>) -> Option<bool> {
    Some(match value {
        None | Some(Canon::Null) => false,
        Some(Canon::Bool(b)) => *b,
        Some(Canon::Int(n)) => n.to_string() != "0",
        Some(Canon::Float(_)) => return None,
        Some(Canon::Str(s) | Canon::Date(s) | Canon::DateTime(s)) => !s.is_empty(),
        Some(Canon::List(items)) => !items.is_empty(),
        Some(Canon::Map(pairs)) => !pairs.is_empty(),
    })
}

/// `value or {}` for a value Python then reads as a dict: the pairs, or
/// None when a truthy value is not a map (Python would raise), or when a
/// key is not text.
fn map_or_empty(value: Option<&Canon>) -> Option<Vec<(Canon, Canon)>> {
    match value {
        Some(Canon::Map(pairs)) => {
            if pairs.iter().any(|(k, _)| !matches!(k, Canon::Str(_))) {
                return None;
            }
            Some(pairs.clone())
        }
        other if !ptruthy(other)? => Some(Vec::new()),
        _ => None,
    }
}

/// True when PyYAML could read `text` in a way this module does not model:
/// anchors, aliases, tags, merge keys, directives, complex keys, a BOM or a
/// carriage return.
fn odd_projects_yaml(text: &str) -> bool {
    text.contains(['&', '*', '!', '\r', '\u{feff}'])
        || text.contains("<<")
        || text.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with('%') || line.starts_with('?')
        })
}

/// What `projects._load()` returns, as pairs, read from a plaintext data
/// folder; None (refuse) for an encrypted folder, a file that is not
/// exactly `safe_dump` of what it parses to, or any shape Python would read
/// differently or fail on.
fn projects_load(
    store: &careerbrew_vault::store::Store,
    path: &Path,
) -> Option<Vec<(Canon, Canon)>> {
    if store.encryption_enabled() {
        return None;
    }
    if std::fs::symlink_metadata(path).is_ok_and(|meta| !meta.is_file()) {
        return None;
    }
    let Some(text) = store.read_text(path, None, true).ok()? else {
        return Some(Vec::new());
    };
    if odd_projects_yaml(&text) {
        return None;
    }
    let data = careerbrew_vault::json_yaml::yaml_read(&text).ok()?;
    // The parse is trusted only when it dumps back to the same bytes.
    if crate::pyyaml::safe_dump(&data)? != text {
        return None;
    }
    let top = map_or_empty(Some(&data))?;
    let projects = map_or_empty(pget(&top, "projects").ok()?)?;
    // Distinct text names: `records().get(name)` then finds the one entry.
    pget(&projects, "").ok()?;
    let mut seen: Vec<&Canon> = Vec::new();
    for (name, _) in &projects {
        if seen.contains(&name) {
            return None;
        }
        seen.push(name);
    }
    Some(projects)
}

/// `promises._get(name)`: Ok(the record's pairs), Err(Some(outcome)) for
/// Python's "no promise" error, Err(None) to refuse.
fn promise_record(
    projects: &[(Canon, Canon)],
    name: &str,
) -> Result<Vec<(Canon, Canon)>, Option<Outcome>> {
    let missing = || {
        Err(Some(fail(
            &format!("no promise {name} (see: careerbrew work promise list)"),
            "US-99",
        )))
    };
    match pget(projects, name).map_err(|_| None)? {
        None => missing(),
        Some(Canon::Map(pairs)) => {
            if pairs.is_empty() {
                return missing();
            }
            if pairs.iter().any(|(k, _)| !matches!(k, Canon::Str(_))) {
                return Err(None);
            }
            match pget(pairs, "kind").map_err(|_| None)? {
                Some(Canon::Str(kind)) if kind == "promise" => Ok(pairs.clone()),
                // Only plain values are sure to compare unequal to "promise".
                Some(Canon::Str(_) | Canon::Null | Canon::Bool(_) | Canon::Int(_)) | None => {
                    missing()
                }
                Some(_) => Err(None),
            }
        }
        Some(other) => match ptruthy(Some(other)) {
            Some(false) => missing(),
            _ => Err(None),
        },
    }
}

/// `projects._now().isoformat(timespec="seconds")`: the instant in UTC.
fn utc_stamp() -> String {
    let now = careerbrew_workday::now();
    format!("{}+00:00", now.naive_utc().format("%Y-%m-%dT%H:%M:%S"))
}

/// Holds `.projects.yaml.lock` as `projects.locked()` does (created in
/// append mode, then an exclusive lock), so a Python writer waits for us.
fn projects_lock(path: &Path) -> Option<std::fs::File> {
    let dir = path.parent()?;
    std::fs::create_dir_all(dir).ok()?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(".projects.yaml.lock"))
        .ok()?;
    file.lock().ok()?;
    Some(file)
}

/// Appends one row as `events.emit("work.promise", status="ok",
/// action=<action>, count=1)` writes it (keys sorted).
fn promise_event(ctx: &Ctx, action: &str) {
    use std::io::Write;
    if !ctx.env.caps.events_on {
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut line = String::from("{\"action\": ");
    crate::render::py_string(action, &mut line);
    line.push_str(", \"count\": 1, \"event\": \"work.promise\", \"status\": \"ok\", \"ts\": ");
    crate::render::py_string(&crate::events::stamp(secs), &mut line);
    line.push_str(", \"v\": ");
    crate::render::py_string(crate::events::version(), &mut line);
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

/// What a promise writer reads before it may write: the store, the file's
/// path and its projects.
struct PromiseFiles {
    context: careerbrew_vault::transfer::Context,
    path: std::path::PathBuf,
    projects: Vec<(Canon, Canon)>,
}

fn promise_files(ctx: &Ctx) -> Option<PromiseFiles> {
    // The lines name the promise the caller typed; `say` redacts them.
    if !business_hidden(ctx) {
        return None;
    }
    let root = ctx.root.as_path();
    pin_clock(root)?;
    let context = careerbrew_vault::transfer::Context::from_env().ok()?;
    let path = root.join("work/projects.yaml");
    let projects = projects_load(&context.store(), &path)?;
    Some(PromiseFiles {
        context,
        path,
        projects,
    })
}

/// Saves `{"projects": projects}` as `projects._save` does. The text is
/// built before the lock is taken, so a refusal never follows a write.
fn promise_save(files: &PromiseFiles, projects: Vec<(Canon, Canon)>) -> Option<()> {
    let text =
        crate::pyyaml::safe_dump(&Canon::Map(vec![(ckey("projects"), Canon::Map(projects))]))?;
    let _lock = projects_lock(&files.path)?;
    files.context.store().write_text(&files.path, &text).ok()
}

fn with_project(
    projects: &[(Canon, Canon)],
    name: &str,
    record: Vec<(Canon, Canon)>,
) -> Vec<(Canon, Canon)> {
    let mut projects = projects.to_vec();
    if let Some((_, slot)) = projects
        .iter_mut()
        .find(|(k, _)| matches!(k, Canon::Str(k) if k == name))
    {
        *slot = Canon::Map(record);
    }
    projects
}

fn promise_close(args: &Parsed, ctx: &Ctx, how: &str) -> Option<Outcome> {
    let name = args.text("name")?;
    let files = promise_files(ctx)?;
    let mut p = match promise_record(&files.projects, name) {
        Ok(p) => p,
        Err(outcome) => return outcome,
    };
    // `{**{k: v for k, v in promise.items() if k not in (...)}, "closed": how}`.
    let mut promise = map_or_empty(pget(&p, "promise").ok()?)?;
    promise.retain(
        |(k, _)| !matches!(k, Canon::Str(k) if k == "pm_removed" || k == "manual_kept_day"),
    );
    pset(&mut promise, "closed", ckey(how));
    pset(&mut p, "promise", Canon::Map(promise));
    pset(&mut p, "archived_at", Canon::Str(utc_stamp()));
    promise_save(&files, with_project(&files.projects, name, p))?;
    promise_event(ctx, how);
    Some(out(
        format!("promise {name} {how}: it left Commitments and the handoff\n"),
        0,
    ))
}

/// The characters `str.split()` and `str.strip()` treat as whitespace.
fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

fn promise_step(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let name = args.text("name")?;
    let which = args.text("step")?;
    let text = args.text("text")?;
    // argparse's choices print usage and exit 2.
    if !PROMISE_STEPS.contains(&which) {
        return None;
    }
    let files = promise_files(ctx)?;
    let mut p = match promise_record(&files.projects, name) {
        Ok(p) => p,
        Err(outcome) => return outcome,
    };
    if ptruthy(pget(&p, "archived_at").ok()?)? {
        return Some(fail(
            &format!("{name} is closed (open it again: careerbrew work project unarchive {name})"),
            "US-99",
        ));
    }
    // `projects.label(text, "a step note", UPDATE_MAX)`, then the empty
    // check: both raise inside `locked()`, which has made the lock file.
    let note = text.trim_matches(py_space);
    let problem = if note.contains('\n') || note.chars().count() > UPDATE_MAX {
        Some(format!(
            "a step note is one line of at most {UPDATE_MAX} characters"
        ))
    } else if note.is_empty() {
        Some("a step needs one line of text: what was done".to_owned())
    } else {
        None
    };
    let at = utc_stamp();
    let mut promise = map_or_empty(pget(&p, "promise").ok()?)?;
    let mut steps = map_or_empty(pget(&promise, "steps").ok()?)?;
    let mut updates = match pget(&p, "updates").ok()? {
        Some(Canon::List(items)) => items.clone(),
        other if !ptruthy(other)? => Vec::new(),
        _ => return None,
    };
    if let Some(problem) = problem {
        projects_lock(&files.path)?;
        return Some(fail(&problem, "US-99"));
    }
    pset(&mut steps, which, Canon::Str(at.clone()));
    let next = PROMISE_STEPS
        .into_iter()
        .find(|s| {
            !steps
                .iter()
                .any(|(k, _)| matches!(k, Canon::Str(k) if k == s))
        })
        .unwrap_or("status");
    pset(&mut promise, "steps", Canon::Map(steps));
    pset(&mut p, "promise", Canon::Map(promise));
    updates.push(Canon::Map(vec![
        (ckey("at"), Canon::Str(at)),
        (ckey("kind"), ckey("project")),
        (ckey("text"), Canon::Str(format!("{which}: {note}"))),
    ]));
    let keep = updates.len().saturating_sub(UPDATES_KEPT);
    updates.drain(..keep);
    pset(&mut p, "updates", Canon::List(updates));
    promise_save(&files, with_project(&files.projects, name, p))?;
    promise_event(ctx, which);
    Some(out(
        format!("promise {name}: {which} recorded; next: {next}\n"),
        0,
    ))
}
