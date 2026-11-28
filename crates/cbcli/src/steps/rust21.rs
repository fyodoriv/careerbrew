//! Native CLI handlers owned by `rust-21` (see `steps/mod.rs`): the
//! teacher's written feedback, its improvement list when there is nothing to
//! propose, and the goal guard for callers without a terminal.
//!
//! Each handler refuses (Python then runs the command) on a redaction input,
//! a corporate machine role, a pinned clock it cannot read, or any input
//! where Python's text handling differs from Rust's. A refusal always comes
//! before a write.
//!
//! `goals.show` is served except for an interview goal with a company (its
//! focus line reads the interview loop, rust-19), a clock or zone Python
//! could read differently, and any goal value Python would format, compare
//! or parse differently. Python reads no machine role for `goals`.
//!
//! Left in Python, with the reason:
//! - `goals.set`: it replans the day (`day.planner.build`, rust-24).
//! - `teach.next`: it needs the interview loops (rust-19) and every step
//!   source; no parity cases in this step yet.
//!
//! Served only in part, with the reason:
//! - `goals.add-long-term`: only the refusal for a caller with no terminal
//!   (agents). The owner's own write goes through `goals.set_long_term`,
//!   which replans the day; that stays in Python.
//! - `teach.improvements`: only when no step, attempt, mistake, feedback or
//!   card file exists, so the list is empty and the output is its footer.
//!   Every proposal rule stays in Python.
//!
//! The small value helpers here are private copies of the ones in
//! `rust20.rs`: step modules may not share code outside their own file.
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;
use careerbrew_vault::canon::Canon;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust21.rs`.
pub const COMMANDS: &[&str] = &[
    "teach.feedback",
    "teach.improvements",
    "goals.add-long-term",
    "goals.show",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| serve(name, args, ctx)))
        .ok()
        .flatten()
}

fn serve(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.redact_inputs_present() {
        return None;
    }
    let vault = careerbrew_vault::transfer::Context::from_env().ok()?;
    if vault.root != ctx.root {
        return None;
    }
    // `goals` is not a personal-only group: Python reads no machine role.
    if name == "goals.show" {
        return goals_show(&vault);
    }
    personal(&vault)?;
    match name {
        "teach.feedback" => feedback(args, &vault),
        "teach.improvements" => improvements(&vault),
        "goals.add-long-term" => not_owner(),
        _ => None,
    }
}

fn ok(stdout: String, code: i32) -> Option<Outcome> {
    Some(Outcome {
        stdout,
        stderr: String::new(),
        code,
        emit_event: true,
    })
}

fn get<'a>(value: &'a Canon, key: &str) -> Option<&'a Canon> {
    let Canon::Map(pairs) = value else {
        return None;
    };
    pairs
        .iter()
        .rev()
        .find(|(k, _)| matches!(k, Canon::Str(k) if k == key))
        .map(|(_, v)| v)
}

fn plain_map(value: &Canon) -> bool {
    let Canon::Map(pairs) = value else {
        return false;
    };
    let mut seen = std::collections::BTreeSet::new();
    pairs
        .iter()
        .all(|(k, _)| matches!(k, Canon::Str(k) if seen.insert(k.clone())))
}

/// `machine.guard`: a corporate role (or a machine file this cannot read
/// the way Python does) refuses, and Python prints its own answer.
fn personal(vault: &careerbrew_vault::transfer::Context) -> Option<()> {
    let info = vault
        .store()
        .read_yaml(&vault.root.join("machine.yaml"), Canon::Null, false)
        .ok()?;
    let role = match &info {
        Canon::Null => None,
        Canon::List(items) if items.is_empty() => None,
        data if plain_map(data) => get(data, "role"),
        _ => return None,
    };
    match role {
        Some(Canon::Str(role)) if role == "corporate" => None,
        Some(Canon::Str(_)) | None => Some(()),
        Some(_) => None,
    }
}

/// Characters Python's `str.split` treats as space and Rust's
/// `split_whitespace` does not (the four information separators).
fn odd_space(text: &str) -> bool {
    text.chars().any(|ch| ('\u{1c}'..='\u{1f}').contains(&ch))
}

struct NowClock;

impl careerbrew_vault::events::Clock for NowClock {
    fn stamp(&self) -> String {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        crate::events::stamp(secs)
    }

    fn version(&self) -> String {
        crate::events::version().to_owned()
    }
}

/// `teacher.feedback(text, step)`: one row in `teach/feedback.jsonl`.
fn feedback(args: &Parsed, vault: &careerbrew_vault::transfer::Context) -> Option<Outcome> {
    let text = args.text("text")?;
    let step = args.text("step").unwrap_or("");
    if odd_space(text) || odd_space(step) {
        return None;
    }
    let at = careerbrew_teacher::stamp(&vault.root);
    let words: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(2000)
        .collect();
    let row = Canon::Map(vec![
        (Canon::Str("at".into()), Canon::Str(at)),
        (Canon::Str("text".into()), Canon::Str(words)),
        (
            Canon::Str("step".into()),
            Canon::Str(step.chars().take(30).collect()),
        ),
        (Canon::Str("rating".into()), Canon::Null),
    ]);
    let path = vault.root.join("teach/feedback.jsonl");
    vault.store().append_jsonl(&path, &[row]).ok()?;
    let fields = Canon::Map(vec![(
        Canon::Str("status".into()),
        Canon::Str("written".into()),
    )]);
    let _ = careerbrew_vault::events::emit(
        &vault.root,
        std::env::var("CAREERBREW_EVENTS").ok().as_deref(),
        "teach.feedback",
        &fields,
        &NowClock,
    );
    ok(
        "thanks: it becomes a proposal in `careerbrew teach improvements`\n".into(),
        0,
    )
}

/// The checkout whose `knowledge/teaching.yaml` Python reads.
fn repo() -> Option<PathBuf> {
    match std::env::var("CAREERBREW_NATIVE_REPO") {
        Ok(repo) if repo.starts_with('/') => Some(PathBuf::from(repo)),
        Ok(_) => None,
        Err(_) => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .ok(),
    }
}

/// `teach improvements` with nothing to propose: every source it reads is
/// missing, so only the footer prints.
fn improvements(vault: &careerbrew_vault::transfer::Context) -> Option<Outcome> {
    let sources = [
        "teach/steps.jsonl",
        "teach/feedback.jsonl",
        "tutor/exercise-attempts.jsonl",
        "tutor/mistakes.jsonl",
        "tutor/card-meta.json",
        "tutor/card-state.json",
        "tutor/card-reviews.jsonl",
        "tutor/decks",
        "tutor/private-explanations.yaml",
        "settings.yaml",
    ];
    if sources
        .iter()
        .any(|file| vault.root.join(file).symlink_metadata().is_ok())
    {
        return None;
    }
    // `cards.stats()` reads the committed public deck; another deck folder
    // goes to Python, which owns its error cases.
    if std::env::var_os("CAREERBREW_PUBLIC_CARDS").is_some_and(|dir| !dir.is_empty()) {
        return None;
    }
    // `cards.stats()` parses the pinned clock first; a bad pin panics here.
    let _ = careerbrew_tutor::now();
    let rules = std::fs::read(repo()?.join("knowledge/teaching.yaml")).ok()?;
    let rules = String::from_utf8(rules).ok()?;
    let version = &careerbrew_tutor::digest(&rules)[..8];
    ok(
        format!("0 improvement(s) proposed \u{b7} teaching rules {version}\n"),
        0,
    )
}

/// `_owner_terminal()` for a caller with no terminal: the fixed refusal.
fn not_owner() -> Option<Outcome> {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal()
        || std::env::var("CAREERBREW_TEST_OWNER").as_deref() == Ok("1")
    {
        return None;
    }
    ok(
        "only you set goals: run this in your own terminal, or use Goals in the web app\n".into(),
        1,
    )
}

// ---------- goals show ----------

/// Python's truth value for a plain value.
fn falsy(value: &Canon) -> bool {
    match value {
        Canon::Null => true,
        Canon::Bool(value) => !value,
        Canon::Int(value) => value.to_string() == "0",
        Canon::Float(value) => *value == 0.0,
        Canon::Str(text) => text.is_empty(),
        Canon::List(items) => items.is_empty(),
        Canon::Map(pairs) => pairs.is_empty(),
        Canon::Date(_) | Canon::DateTime(_) => false,
    }
}

fn text<'a>(value: &'a Canon, key: &str) -> Option<&'a str> {
    match get(value, key) {
        Some(Canon::Str(text)) => Some(text),
        _ => None,
    }
}

/// Absent, null or text: the text when it is not empty (Python's truth).
fn optional_text(value: &Canon, key: &str) -> Option<Option<String>> {
    match get(value, key) {
        None | Some(Canon::Null) => Some(None),
        Some(Canon::Str(text)) if text.is_empty() => Some(None),
        Some(Canon::Str(text)) => Some(Some(text.clone())),
        Some(_) => None,
    }
}

/// `YYYY-MM-DDTHH:MM[:SS]`, with `Z` or `+HH:MM` only after seconds: the
/// pinned clock forms Python and the domain crate read the same way.
/// Returns whether it carries an offset (then Python reads no zone).
fn pin_shape(pin: &str) -> Option<bool> {
    let b = pin.as_bytes();
    let digits = |range: std::ops::Range<usize>| b[range].iter().all(u8::is_ascii_digit);
    if b.len() < 16
        || !(digits(0..4)
            && b[4] == b'-'
            && digits(5..7)
            && b[7] == b'-'
            && digits(8..10)
            && b[10] == b'T'
            && digits(11..13)
            && b[13] == b':'
            && digits(14..16))
    {
        return None;
    }
    let seconds = b.len() >= 19 && b[16] == b':' && digits(17..19);
    let tail = &b[if seconds { 19 } else { 16 }..];
    match tail {
        [] => Some(false),
        _ if !seconds => None,
        [b'Z'] => Some(true),
        [sign, h1, h2, b':', m1, m2]
            if matches!(sign, b'+' | b'-')
                && [h1, h2, m1, m2].iter().all(|d| d.is_ascii_digit()) =>
        {
            Some(true)
        }
        _ => None,
    }
}

/// A zone name that Python's `ZoneInfo` (the system zone files) and the
/// domain crate both resolve to the same zone.
fn zone_ok(name: &str, resolved: &str) -> bool {
    let plain = !name.is_empty()
        && !name.starts_with('/')
        && name.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || "_-+".contains(ch))
        });
    plain && resolved == name && Path::new("/usr/share/zoneinfo").join(name).is_file()
}

/// `calendar.now().date()` as `YYYY-MM-DD`, or None for any clock or zone
/// Python could fail on or read as another date.
fn today(vault: &careerbrew_vault::transfer::Context) -> Option<String> {
    let zone_read = match std::env::var("CAREERBREW_NOW") {
        Err(std::env::VarError::NotPresent) => true,
        Ok(pin) => !pin_shape(&pin)?,
        Err(_) => return None,
    };
    if zone_read {
        let settings = vault
            .store()
            .read_yaml(&vault.root.join("settings.yaml"), Canon::Null, false)
            .ok()?;
        let configured = if falsy(&settings) {
            None
        } else if plain_map(&settings) {
            match get(&settings, "timezone") {
                None | Some(Canon::Null) => None,
                Some(Canon::Str(name)) if name.is_empty() => None,
                Some(Canon::Str(name)) => Some(name.clone()),
                Some(_) => return None,
            }
        } else {
            return None;
        };
        match configured {
            Some(name) => {
                if !zone_ok(&name, careerbrew_tutor::local_zone(&vault.root).name()) {
                    return None;
                }
            }
            None => {
                let tz = std::env::var("TZ").unwrap_or_default();
                let name = tz.trim_start_matches(':');
                // An empty `TZ` reads `/etc/localtime` on both sides.
                if !name.is_empty() && !zone_ok(name, careerbrew_tutor::system_zone().name()) {
                    return None;
                }
            }
        }
    }
    // A pin the domain crate cannot read panics: `run` makes it a refusal.
    Some(careerbrew_teacher::today(&vault.root).to_string())
}

/// Days since 1970-01-01 for a strict `YYYY-MM-DD` calendar date.
fn day_number(date: &str) -> Option<i64> {
    let b = date.as_bytes();
    let shape = b.len() == 10
        && b.iter().enumerate().all(|(at, ch)| {
            if at == 4 || at == 7 {
                *ch == b'-'
            } else {
                ch.is_ascii_digit()
            }
        });
    if !shape {
        return None;
    }
    let year: i64 = date[0..4].parse().ok()?;
    let month: i64 = date[5..7].parse().ok()?;
    let day: i64 = date[8..10].parse().ok()?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if year < 1 || !(1..=12).contains(&month) || day < 1 || day > days_in[month as usize - 1] {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// One queue goal, in the shapes `goals show` prints.
struct Goal {
    id: String,
    title: String,
    kind: String,
    date: Option<String>,
    raw: Canon,
}

fn goal_of(value: &Canon) -> Option<Goal> {
    if !plain_map(value) {
        return None;
    }
    Some(Goal {
        id: text(value, "id")?.to_owned(),
        title: text(value, "title")?.to_owned(),
        kind: text(value, "kind")?.to_owned(),
        date: optional_text(value, "date")?,
        raw: value.clone(),
    })
}

/// `goals.focus(goal)["why"]`, or None where Python reads more than this
/// (an interview loop) or could crash (topics that are not a list of text).
fn focus_why(goal: &Goal) -> Option<&'static str> {
    match get(&goal.raw, "topics") {
        None | Some(Canon::Null) => {}
        Some(Canon::List(items)) if items.iter().all(|t| matches!(t, Canon::Str(_))) => {}
        Some(_) => return None,
    }
    Some(match goal.kind.as_str() {
        "interview" => {
            if optional_text(&goal.raw, "company")?.is_some() {
                return None;
            }
            "the interview loop"
        }
        "learn" => "your learning topics",
        "job-search" => "the jobs you chose",
        "promotion" => "your promotion case",
        _ => "your goal",
    })
}

/// `careerbrew goals show`.
fn goals_show(vault: &careerbrew_vault::transfer::Context) -> Option<Outcome> {
    let today = today(vault)?;
    let data = vault
        .store()
        .read_yaml(&vault.root.join("goals.yaml"), Canon::Null, false)
        .ok()?;
    let data = if falsy(&data) {
        Canon::Map(Vec::new())
    } else if plain_map(&data) {
        data
    } else {
        return None;
    };
    let queue = match get(&data, "queue") {
        None | Some(Canon::Null) => match get(&data, "upcoming") {
            Some(goal) if !falsy(goal) => vec![goal_of(goal)?],
            _ => Vec::new(),
        },
        Some(Canon::List(items)) => items.iter().map(goal_of).collect::<Option<Vec<_>>>()?,
        Some(_) => return None,
    };
    let long_term = match get(&data, "long_term") {
        None => Vec::new(),
        Some(Canon::List(items)) => items.clone(),
        Some(_) => return None,
    };
    let mut out = String::new();
    for item in &long_term {
        if !plain_map(item) {
            return None;
        }
        let words = text(item, "text")?;
        out.push_str(&format!("long-term: {words}"));
        if let Some(by) = optional_text(item, "by")? {
            out.push_str(&format!(" (by {by})"));
        }
        out.push('\n');
    }
    // `_current`: the nearest dated goal still ahead (the first of a tie),
    // else the first undated one.
    let mut upcoming: Option<&Goal> = None;
    for goal in &queue {
        let Some(date) = goal.date.as_deref() else {
            continue;
        };
        if date < today.as_str() {
            continue;
        }
        if upcoming.is_none_or(|best| date < best.date.as_deref().unwrap_or_default()) {
            upcoming = Some(goal);
        }
    }
    let upcoming = upcoming.or_else(|| queue.iter().find(|goal| goal.date.is_none()));
    for goal in &queue {
        let mark = if upcoming.is_some_and(|up| up.id == goal.id) {
            "\u{2192}"
        } else {
            " "
        };
        out.push_str(&format!("{mark} {} [{}]", goal.title, goal.kind));
        if let Some(date) = &goal.date {
            out.push_str(&format!(" \u{b7} {date}"));
        }
        out.push('\n');
    }
    match upcoming {
        Some(up) => {
            let minutes = match get(&up.raw, "minutes_per_day")? {
                Canon::Int(minutes) => minutes.to_string(),
                _ => return None,
            };
            let why = focus_why(up)?;
            out.push_str(&format!("upcoming: {} [{}]", up.title, up.kind));
            if let Some(date) = &up.date {
                let left = (day_number(date)? - day_number(&today)?).max(0);
                out.push_str(&format!(" \u{b7} {left} day(s) left"));
            }
            out.push_str(&format!(" \u{b7} {minutes} min/day\n"));
            out.push_str(&format!("focus: {why}\n"));
        }
        None => {
            out.push_str("no upcoming goal: `careerbrew goals set` (or Goals in the web app)\n")
        }
    }
    ok(out, 0)
}
