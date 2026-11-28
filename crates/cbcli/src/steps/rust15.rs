//! Native CLI handlers owned by `rust-15` (see `steps/mod.rs`): the work
//! day's `work session handoff`, over `careerbrew_sessions`, and (wave 4)
//! the unknown-project error of `work session open`.
//!
//! The handler checks every input Python's `sessions.write_handoff` and
//! `day/handoff.add` read before it writes anything: the text and link, the
//! session store (Python's validation is wider than the crate's), the day's
//! handoff file and the clock. It serves only a UTC clock: the crate stamps
//! `handoff_at` and picks the handoff day in UTC, Python in the local zone.
//! The `Canon` and clock helpers are shared with `rust14`.
//!
//! Wave 6 adds `work session ask` for a new question: the session errors
//! (no open session, a session from an earlier day), the record's
//! `ValueError`s, and a first save of a question id in a plain-text data
//! folder. Every other case (an id already saved, a command recheck, a
//! quiet project) stays with Python.
use super::rust14::{
    cli_error, event_version_ok, get, inputs_ok, int, iso, list, printable, read_json, read_yaml,
    string, written,
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
pub const COMMANDS: &[&str] = &[
    "work.session.handoff",
    "work.session.open",
    "work.session.ask",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let started = Instant::now();
    if !inputs_ok(ctx) || !event_version_ok() {
        return None;
    }
    match name {
        "work.session.handoff" => handoff(args, ctx, started),
        "work.session.open" => session_open(args, ctx),
        "work.session.ask" => session_ask(args, ctx, started),
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

// ---------- work session open ----------

/// `work session open` for a project `projects.records()` does not hold:
/// `sessions._project` raises `SessionError` before any lock or write (only
/// `paths.sub` makes the `work` folder), and `cmd_work_session` prints it
/// with exit 2. Every other open (a known project) stays with Python: a new
/// session takes a random record id.
fn session_open(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let root = &ctx.root;
    let project = args.text("project")?;
    // `str(name).strip()`: a name with no outer space and only plain
    // characters reads the same way on both sides.
    if !printable(project) || project.trim() != project {
        return None;
    }
    // `(read_yaml(path) or {}).get("projects", {}) or {}`; any other shape
    // (a list or a string answers `in` differently) stays with Python.
    let file = read_yaml(root, "work/projects.yaml")?;
    let known = match get(&file, "projects") {
        None | Some(Canon::Null) => false,
        Some(Canon::Map(pairs)) => pairs
            .iter()
            .any(|(key, _)| matches!(key, Canon::Str(name) if name == project)),
        Some(_) => return None,
    };
    if known {
        return None;
    }
    std::fs::create_dir_all(root.join("work")).ok()?;
    Some(Outcome {
        stdout: "error: unknown project: add it first with `careerbrew work project add <name>` (careerbrew work project list shows them) (breaks US-107)\n".to_owned(),
        stderr: String::new(),
        code: 2,
        emit_event: true,
    })
}

// ---------- wave 6: work session ask ----------

/// `MAX_TEXT`, `MAX_OPTION` and `MAX_OPTIONS` in `workday/questions.py`.
const MAX_QUESTION: usize = 200;
const MAX_OPTION: usize = 80;
const MAX_OPTIONS: usize = 6;
/// `CARRY_DAYS` in `workday/questions.py`: `_find` looks this far back.
const CARRY_DAYS: i64 = 14;

/// `calendar.now()` as `isoformat(timespec="seconds")` and its date. A
/// pinned clock serves only an aware stamp with whole seconds (Python
/// prints it back as typed); `-00:00` prints as `+00:00`, so it refuses.
fn ask_clock() -> Option<(String, String)> {
    match std::env::var("CAREERBREW_NOW") {
        Ok(value) => {
            if value.len() != 25
                || !matches!(iso(&value), Some(Some(_)))
                || value.ends_with("-00:00")
            {
                return None;
            }
            let day = value.get(0..10)?.to_owned();
            Some((value, day))
        }
        Err(std::env::VarError::NotPresent) => {
            let now = careerbrew_feed::now();
            let stamp = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();
            let day = stamp.get(0..10)?.to_owned();
            Some((stamp, day))
        }
        Err(_) => None,
    }
}

/// Days since 1970-01-01 for a `YYYY-MM-DD` day (Howard Hinnant), as
/// `rust14` counts them.
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

/// A value `json.dumps(sort_keys=True)` writes the way `dump_sorted` does:
/// no float, string keys only, and no repeated key (Python keeps the last).
fn sortable(value: &Canon) -> bool {
    match value {
        Canon::Null | Canon::Bool(_) | Canon::Int(_) | Canon::Str(_) => true,
        Canon::List(items) => items.iter().all(sortable),
        Canon::Map(pairs) => {
            let mut keys: Vec<&str> = Vec::new();
            for (key, item) in pairs {
                let Canon::Str(key) = key else {
                    return false;
                };
                if keys.contains(&key.as_str()) || !sortable(item) {
                    return false;
                }
                keys.push(key);
            }
            true
        }
        _ => false,
    }
}

/// `json.dumps(value, indent=2, sort_keys=True)` (`ensure_ascii`) for a
/// `sortable` value: `store.write_json`.
fn dump_sorted(value: &Canon, level: usize, out: &mut String) {
    let pad = |level: usize| "  ".repeat(level);
    match value {
        Canon::Null => out.push_str("null"),
        Canon::Bool(true) => out.push_str("true"),
        Canon::Bool(false) => out.push_str("false"),
        Canon::Int(value) => out.push_str(&value.to_string()),
        Canon::Str(text) => crate::render::py_string(text, out),
        Canon::List(items) if items.is_empty() => out.push_str("[]"),
        Canon::List(items) => {
            out.push_str("[\n");
            for (at, item) in items.iter().enumerate() {
                out.push_str(&pad(level + 1));
                dump_sorted(item, level + 1, out);
                out.push_str(if at + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(level));
            out.push(']');
        }
        Canon::Map(pairs) if pairs.is_empty() => out.push_str("{}"),
        Canon::Map(pairs) => {
            let mut sorted: Vec<(&str, &Canon)> = pairs
                .iter()
                .filter_map(|(key, item)| match key {
                    Canon::Str(key) => Some((key.as_str(), item)),
                    _ => None,
                })
                .collect();
            // Rust orders strings by UTF-8 bytes, which is code point order.
            sorted.sort_by(|a, b| a.0.cmp(b.0));
            out.push_str("{\n");
            for (at, (key, item)) in sorted.iter().enumerate() {
                out.push_str(&pad(level + 1));
                crate::render::py_string(key, out);
                out.push_str(": ");
                dump_sorted(item, level + 1, out);
                out.push_str(if at + 1 < sorted.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(level));
            out.push('}');
        }
        _ => {}
    }
}

/// Opens `<dir>/.<name>.lock` in append mode and locks it, as
/// `projects.locked()` and `questions._change` do.
fn lock_beside(root: &Path, dir: &str, name: &str) -> Option<std::fs::File> {
    let dir = root.join(dir);
    std::fs::create_dir_all(&dir).ok()?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!(".{name}.lock")))
        .ok()?;
    file.lock().ok()?;
    Some(file)
}

/// `cmd_work_session` on a `SessionError`: `error: <msg> (breaks US-107)`,
/// exit 2, after `projects.locked()` made its lock file.
fn session_error(root: &Path, message: String) -> Option<Outcome> {
    let _lock = lock_beside(root, "work", "projects.yaml")?;
    Some(Outcome {
        stdout: format!("error: {message} (breaks US-107)\n"),
        stderr: String::new(),
        code: 2,
        emit_event: true,
    })
}

/// The saved sessions as `sessions._load` accepts them (none when the file
/// is missing); None for anything Python could read differently.
fn ask_sessions(root: &Path) -> Option<Vec<Canon>> {
    if !root.join("work/sessions.json").exists() {
        return Some(Vec::new());
    }
    careerbrew_sessions::sessions::load(root).ok()?;
    let store = read_json(root, "work/sessions.json")?;
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
    let named = |s: &Canon| {
        string(s, "id").is_some_and(|t| !t.is_empty())
            && string(s, "project").is_some_and(|t| !t.is_empty())
    };
    if !sessions.iter().all(|s| session_ok(s) && named(s)) || !claims.iter().all(claim_ok) {
        return None;
    }
    Some(sessions.to_vec())
}

/// `projects.quiet(project)` is False: the project's record exists, is a
/// mapping, and has no quiet date. Anything else stays with Python.
fn not_quiet(root: &Path, project: &str) -> Option<()> {
    let file = read_yaml(root, "work/projects.yaml")?;
    let Some(Canon::Map(pairs)) = get(&file, "projects") else {
        return None;
    };
    let record = pairs
        .iter()
        .find(|(key, _)| matches!(key, Canon::Str(name) if name == project))
        .map(|(_, value)| value)?;
    if !matches!(record, Canon::Map(_)) {
        return None;
    }
    match get(record, "quiet_until") {
        None | Some(Canon::Null) | Some(Canon::Bool(false)) => Some(()),
        Some(Canon::Str(text)) if text.is_empty() => Some(()),
        Some(_) => None,
    }
}

type Asked = (String, Vec<String>, String);

/// `questions.record`'s checks up to the recheck, in its order: the
/// question, the options, the pick. Err is the `ValueError` message.
fn ask_record(raw: &str, options: &[String], pick: &str) -> Result<Asked, String> {
    let line = |field: &str, value: &str, limit: usize| {
        let value = one_line(value);
        let n = value.chars().count();
        if n > limit {
            return Err(format!(
                "keep {field} short: {n} characters, at most {limit}"
            ));
        }
        Ok(value)
    };
    let text = line("the question", raw, MAX_QUESTION)?;
    if text.is_empty() {
        return Err("the question text is empty".into());
    }
    let mut lines = Vec::new();
    for option in options {
        lines.push(line("an --option", option, MAX_OPTION)?);
    }
    let mut opts: Vec<String> = Vec::new();
    for option in lines {
        if !option.is_empty() && !opts.contains(&option) {
            opts.push(option);
        }
    }
    if opts.len() > MAX_OPTIONS {
        return Err(format!("at most {MAX_OPTIONS} --option values"));
    }
    let pick = line("--pick", pick, MAX_OPTION)?;
    if !pick.is_empty() && !opts.is_empty() && !opts.contains(&pick) {
        return Err("--pick must be one of the --option values".into());
    }
    Ok((text, opts, pick))
}

/// `repr(text)` for text Python quotes with single quotes and no escapes.
fn plain_repr(text: &str) -> Option<String> {
    (printable(text) && !text.contains('\'') && !text.contains('\\')).then(|| format!("'{text}'"))
}

/// `work session ask`: `sessions.ask` under `projects.locked()`, then
/// `questions.add` for a question id no carried day holds yet.
fn session_ask(args: &Parsed, ctx: &Ctx, started: Instant) -> Option<Outcome> {
    let root = &ctx.root;
    let target = args.text("id")?;
    let raw = args.text("text")?;
    let options = args.list("option");
    let pick = args.text("pick").unwrap_or("");
    let recheck = args.text("recheck").unwrap_or("");
    if [target, raw, pick, recheck]
        .iter()
        .any(|text| odd_space(text))
        || options.iter().any(|text| odd_space(text))
    {
        return None;
    }
    let (stamp, today) = ask_clock()?;
    let sessions = ask_sessions(root)?;
    // `live`: `_must`, then `state`, then `projects.quiet`.
    let Some(session) = find(&sessions, target) else {
        let shown: String = target.chars().take(64).collect();
        let shown = plain_repr(&shown)?;
        return session_error(
            root,
            format!("no open session {shown}: open one with `careerbrew work session open <project> --id <session id>` (careerbrew work session list shows them)"),
        );
    };
    let sid = string(session, "id")?;
    let day = string(session, "day")?;
    if !printable(sid) {
        return None;
    }
    if day != today {
        return session_error(
            root,
            format!("session {sid} was opened on {day}: end it (careerbrew work session end {sid}) and open today's"),
        );
    }
    not_quiet(root, string(session, "project")?)?;
    // `questions.record`: its `ValueError` reaches `cli.main`, which prints
    // it plainly (no story suffix) with one `cli.error` row.
    let (text, opts, pick) = match ask_record(raw, options, pick) {
        Ok(parts) => parts,
        Err(message) => {
            let _lock = lock_beside(root, "work", "projects.yaml")?;
            return Some(cli_error(ctx, "work.session", "valueerror", &message));
        }
    };
    // `check_recheck`: only an empty recheck, the sign-in probe, or a link
    // with no space in it; a command (shlex) or a PR recheck stays with Python.
    let kind = careerbrew_workday::questions::kind(recheck);
    if !(recheck.is_empty()
        || (matches!(kind.as_str(), "url" | "signin") && !recheck.chars().any(char::is_whitespace)))
    {
        return None;
    }
    // A sealed write differs on every run.
    let context = careerbrew_vault::transfer::Context::from_env().ok()?;
    if context.store().encryption_enabled() {
        return None;
    }
    let step = format!("session:{sid}");
    let qid = careerbrew_workday::questions::qid(&step, &text);
    // `save` -> `_find`: a record with this id in a carried day takes
    // another branch; it stays with Python.
    let base = day_number(&today)?;
    let mut data = Canon::Map(Vec::new());
    for n in 0..=CARRY_DAYS {
        let d = day_text(base - n)?;
        let rel = format!("day/work-{d}.json");
        if !root.join(&rel).exists() {
            continue;
        }
        let file = read_json(root, &rel)?;
        let saved = match get(&file, "questions") {
            Some(Canon::List(items)) => &items[..],
            _ => &[][..],
        };
        if saved.iter().any(|q| string(q, "id") == Some(qid.as_str())) {
            return None;
        }
        if n == 0 {
            data = file;
        }
    }
    let Canon::Map(mut pairs) = data else {
        return None;
    };
    if !sortable(&Canon::Map(pairs.clone())) {
        return None;
    }
    let mut have = match get(&Canon::Map(pairs.clone()), "questions") {
        None => Vec::new(),
        Some(Canon::List(items)) => items.clone(),
        Some(_) => return None,
    };
    // `records` keeps only mappings with an id; `_change` writes that list.
    if !have
        .iter()
        .all(|q| matches!(q, Canon::Map(_)) && string(q, "id").is_some_and(|id| !id.is_empty()))
    {
        return None;
    }
    let s = |text: &str| Canon::Str(text.to_owned());
    have.push(Canon::Map(vec![
        (s("id"), s(&qid)),
        (s("step"), s(&step)),
        (s("text"), s(&text)),
        (
            s("options"),
            Canon::List(opts.iter().map(|o| s(o)).collect()),
        ),
        (s("pick"), s(&pick)),
        (s("createdAt"), s(&stamp)),
        (s("recheck"), s(recheck)),
        (s("source"), s("agent")),
        (s("closed"), s("")),
        (s("closedAt"), s("")),
    ]));
    // `{"rows": [], "handled": {}, **data, "day": ..., "questions": have}`.
    let mut set = |key: &str, value: Canon, keep: bool| match pairs
        .iter_mut()
        .find(|(k, _)| matches!(k, Canon::Str(name) if name == key))
    {
        Some((_, old)) if !keep => *old = value,
        Some(_) => {}
        None => pairs.push((s(key), value)),
    };
    set("rows", Canon::List(Vec::new()), true);
    set("handled", Canon::Map(Vec::new()), true);
    set("day", s(&today), false);
    set("questions", Canon::List(have), false);
    let mut body = String::new();
    dump_sorted(&Canon::Map(pairs), 0, &mut body);
    let options_count = Canon::Int((opts.len() as u64).into());
    let _projects = lock_beside(root, "work", "projects.yaml")?;
    let name = format!("work-{today}.json");
    let _day = lock_beside(root, "day", &name)?;
    context
        .store()
        .write_text(&root.join("day").join(&name), &body)
        .ok()?;
    let _ = careerbrew_feed::emit(
        root,
        "workday.question",
        vec![
            ("action", s("add")),
            ("source", s("agent")),
            ("recheck", s(&kind)),
            ("options", options_count),
        ],
    );
    let _ = careerbrew_feed::emit(
        root,
        "work.session",
        vec![("action", s("ask")), ("status", s("ok"))],
    );
    let line = format!(
        "question {qid} saved for {step} ({kind} recheck); the main session asks it in the one batched ask (careerbrew day question recheck)\n"
    );
    Some(written(ctx, "work.session", started, line))
}
