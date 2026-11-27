//! Native CLI handlers owned by `rust-16` (see `steps/mod.rs`): the work
//! day's wrap-up control (`day wrap`) and its question records (`day
//! question list`, `day question close`).
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
pub const COMMANDS: &[&str] = &["day.wrap", "day.question.list", "day.question.close"];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let started = std::time::Instant::now();
    match name {
        "day.wrap" => wrap(args, ctx),
        "day.question.list" => logged("day.question", question_list(args, ctx)?, ctx, started),
        "day.question.close" => logged("day.question", question_close(args, ctx)?, ctx, started),
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
