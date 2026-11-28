//! Native CLI handlers owned by `rust-25` (see `steps/mod.rs`): the read
//! views of independent mode (US-112), the trial list of peer improvement
//! (US-113), and the error and confirm lines these commands print before
//! any write.
//!
//! What is served, per command:
//! - `indie.goal`, `indie.income`: the progress line or JSON (no write), and
//!   the argument errors Python raises before it writes. A form that would
//!   write `goal.yaml` or `income.jsonl` refuses.
//! - `indie.bets`: the whole view (read only).
//! - `indie.bet`, `indie.decide`, `indie.measure`: the error lines raised
//!   before a write (an unknown bet, no `--yes`, a bet that cannot be
//!   active yet, no measure command). Every other form refuses: showing a
//!   bet depends on `may_show_business`, and the rest writes or runs a
//!   command.
//! - `indie.scan`: the run that finds no repo to read (no folder with a
//!   `.git` folder under `indie_roots`, two levels deep): it counts the
//!   bets and their missing folders, writes nothing and logs `indie.scan`.
//!   A run that finds a repo asks `git` for its origin and may write
//!   `bets.yaml`: Python runs it.
//! - `indie.end`: the `--hours` error, when a value is certainly not a
//!   float. Every other form writes and refuses.
//! - `peer.experiment`: the trial list, and the errors `--start` and
//!   `--close` raise before they measure or write.
//! - `web.rotate-token`: the confirm line without `--yes` (exit 1).
//! - `changes`: the run that finds no change (`changes.check`): it hashes
//!   the ladders and company files as Python does, writes the baseline
//!   snapshot on a first run (plaintext data folder only) and prints the
//!   no-change line. `--fetch` (network), an installed plugin, a found
//!   change (the pivot re-plans) and a sealed write all refuse.
//!
//! Every handler checks the files it reads first, in the vault's typed form
//! (`Canon`), and refuses on any shape Python would read differently or
//! crash on, on a corporate role for `indie`, and on a redaction input. A
//! refusal always comes before a write: the only write here is the empty
//! folder Python's `paths.sub` makes when it reads.
//!
//! Left in Python, with the reason:
//! - `start`: the whole start-up (watcher, web server, refresh, run page).
//! - `indie.run`, `indie.review`, `peer.status`: git subprocesses (every
//!   form) and run pages.
use super::Ctx;
use crate::args::{Parsed, Value};
use crate::Outcome;
use careerbrew_vault::canon::Canon;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust25.rs`.
pub const COMMANDS: &[&str] = &[
    "web.rotate-token",
    "indie.goal",
    "indie.income",
    "indie.bets",
    "indie.bet",
    "indie.decide",
    "indie.measure",
    "indie.end",
    "indie.scan",
    "peer.experiment",
    "changes",
];

const INDIE_STORY: &str = "US-112";
const PEER_STORY: &str = "US-113";

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    // The domain crates panic on a pinned clock they cannot parse; that is a
    // refusal here, and every handler reads its clock before any write.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| serve(name, args, ctx)))
        .ok()
        .flatten()
}

fn serve(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.redact_inputs_present() {
        return None;
    }
    if name == "web.rotate-token" {
        return rotate_token(args);
    }
    let vault = careerbrew_vault::transfer::Context::from_env().ok()?;
    if vault.root != ctx.root {
        return None;
    }
    let files = Files {
        root: ctx.root.clone(),
        vault: &vault,
    };
    match name {
        "peer.experiment" => peer_experiment(args, &files),
        "changes" => changes(args, &files),
        _ => {
            files.personal()?;
            match name {
                "indie.goal" => indie_goal(args, &files),
                "indie.income" => indie_income(args, &files),
                "indie.bets" => indie_bets(args, &files),
                "indie.bet" => indie_bet(args, &files),
                "indie.decide" => indie_decide(args, &files),
                "indie.measure" => indie_measure(args, &files),
                "indie.end" => indie_end(args),
                "indie.scan" => indie_scan(args, ctx, &files),
                _ => None,
            }
        }
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

/// `say(f"error: {e} (breaks {STORY})")`, exit 2.
fn indie_error(message: &str) -> Option<Outcome> {
    ok(format!("error: {message} (breaks {INDIE_STORY})\n"), 2)
}

fn peer_error(message: &str) -> Option<Outcome> {
    ok(format!("error: {message} (breaks {PEER_STORY})\n"), 2)
}

// ---------- web ----------

fn rotate_token(args: &Parsed) -> Option<Outcome> {
    if args.flag("yes") {
        // A new token is random and written to the store: Python does it.
        return None;
    }
    ok(
        "this signs out every open web app tab and bookmark (they need a new link from `careerbrew open`); rerun with --yes to confirm\n".to_owned(),
        1,
    )
}

// ---------- typed value helpers ----------

/// The value of `key` in a map, as Python's dict keeps it (the last one).
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

fn text<'a>(value: &'a Canon, key: &str) -> Option<&'a str> {
    match get(value, key) {
        Some(Canon::Str(text)) => Some(text),
        _ => None,
    }
}

/// A map with text keys and no key twice: the only maps read here.
fn plain_map(value: &Canon) -> bool {
    let Canon::Map(pairs) = value else {
        return false;
    };
    let mut seen = std::collections::BTreeSet::new();
    pairs
        .iter()
        .all(|(k, _)| matches!(k, Canon::Str(k) if seen.insert(k.clone())))
}

/// Python truthiness.
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

/// A Python `float()` of a stored number, for ints and finite floats that
/// convert exactly the same way; None for anything else.
fn number(value: &Canon) -> Option<f64> {
    match value {
        Canon::Float(value) if value.is_finite() => Some(*value),
        Canon::Int(value) => {
            let n: i64 = value.to_string().parse().ok()?;
            (n.unsigned_abs() <= 1 << 53).then_some(n as f64)
        }
        _ => None,
    }
}

/// The parsed `type=float` argument, or None when it was not given.
fn float_arg(args: &Parsed, name: &str) -> Option<Option<f64>> {
    match args.get(name) {
        Value::Missing => Some(None),
        Value::Number(text) => text.parse::<f64>().ok().map(Some),
        _ => None,
    }
}

/// Python `json.dumps` (default separators, `ensure_ascii`, insertion
/// order) for a value read from JSON; None for a form it would write
/// differently (a non-finite float, a map key that is not text).
fn dump(value: &Canon, out: &mut String) -> Option<()> {
    match value {
        Canon::Null => out.push_str("null"),
        Canon::Bool(true) => out.push_str("true"),
        Canon::Bool(false) => out.push_str("false"),
        Canon::Int(value) => out.push_str(&value.to_string()),
        Canon::Float(value) if value.is_finite() => {
            out.push_str(&careerbrew_reports::py_repr(*value));
        }
        Canon::Str(text) => crate::render::py_string(text, out),
        Canon::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                dump(item, out)?;
            }
            out.push(']');
        }
        Canon::Map(pairs) if plain_map(value) => {
            out.push('{');
            for (i, (key, item)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                dump(key, out)?;
                out.push_str(": ");
                dump(item, out)?;
            }
            out.push('}');
        }
        _ => return None,
    }
    Some(())
}

fn json_str(text: &str) -> String {
    let mut out = String::new();
    crate::render::py_string(text, &mut out);
    out
}

fn json_list(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|item| json_str(item)).collect();
    format!("[{}]", inner.join(", "))
}

// ---------- the clock ----------

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

/// A zone name the system's database holds, in a form Python's `ZoneInfo`
/// and the domain crate both resolve to the same zone.
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

/// `calendar.now()` reads without an error: a pinned clock in a form both
/// sides read, and a zone setting Python's `ZoneInfo` loads. None refuses.
fn clock_ok(files: &Files) -> Option<()> {
    let zone_read = match std::env::var("CAREERBREW_NOW") {
        Err(std::env::VarError::NotPresent) => true,
        Ok(pin) => !pin_shape(&pin)?,
        Err(_) => return None,
    };
    if zone_read {
        let settings = files.yaml("settings.yaml")?;
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
                if !zone_ok(&name, careerbrew_tutor::local_zone(&files.root).name()) {
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
    let _ = careerbrew_teacher::now_local(&files.root);
    Some(())
}

/// `calendar.now().date()` as `(year, month)`.
fn this_month(files: &Files) -> Option<(i64, i64)> {
    clock_ok(files)?;
    let day = careerbrew_teacher::today(&files.root).to_string();
    let b = day.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    Some((day[..4].parse().ok()?, day[5..7].parse().ok()?))
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Microseconds since the epoch for `YYYY-MM-DDTHH:MM:SS+HH:MM` (the stamp
/// `isoformat(timespec="seconds")` writes for an aware time), else None.
fn stamp_micros(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| b[range].iter().all(u8::is_ascii_digit);
    let shape = b.len() == 25
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19)
        && matches!(b[19], b'+' | b'-')
        && digits(20..22)
        && b[22] == b':'
        && digits(23..25);
    if !shape {
        return None;
    }
    let n = |range: std::ops::Range<usize>| text[range].parse::<i64>().ok();
    let (year, month, day) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hour, minute, second) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let (oh, om) = (n(20..22)?, n(23..25)?);
    let month_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = if month == 2 && leap {
        29
    } else {
        *month_days.get(usize::try_from(month - 1).ok()?)?
    };
    if year < 1
        || !(1..=max_day).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
        || oh > 23
        || om > 59
    {
        return None;
    }
    let offset = (oh * 3600 + om * 60) * if b[19] == b'-' { -1 } else { 1 };
    let secs =
        days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset;
    Some(secs * 1_000_000)
}

// ---------- files ----------

struct Files<'a> {
    root: PathBuf,
    vault: &'a careerbrew_vault::transfer::Context,
}

impl Files<'_> {
    /// A data-root file through the vault; `Null` when it is missing.
    fn yaml(&self, relative: &str) -> Option<Canon> {
        self.vault
            .store()
            .read_yaml(&self.root.join(relative), Canon::Null, false)
            .ok()
    }

    /// `store.read_jsonl`, refused on a line break or space Python's
    /// `splitlines`/`strip` read differently from this reader.
    fn jsonl(&self, relative: &str) -> Option<Vec<Canon>> {
        let text = self
            .vault
            .store()
            .read_text(&self.root.join(relative), None, false)
            .ok()?
            .unwrap_or_default();
        if text.chars().any(|ch| {
            matches!(
                ch,
                '\r' | '\u{b}' | '\u{c}' | '\u{1c}'..='\u{1f}' | '\u{85}' | '\u{2028}' | '\u{2029}'
            )
        }) {
            return None;
        }
        text.split('\n')
            .filter(|line| !line.trim().is_empty())
            .map(|line| careerbrew_vault::json_yaml::json_read(line).ok())
            .collect()
    }

    /// `paths.sub(folder, ...)` makes the folder when Python reads a file
    /// in it; made only once a run is served.
    fn touch(&self, folder: &str) -> Option<()> {
        std::fs::create_dir_all(self.root.join(folder)).ok()
    }

    /// `machine.guard("indie")`: a corporate role (or a machine file Python
    /// could not read the same way) refuses natively.
    fn personal(&self) -> Option<()> {
        let info = self.yaml("machine.yaml")?;
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

    /// `indie.bets()`: the bets by id, in file order, each a text-keyed map.
    fn bets(&self) -> Option<Vec<(String, Canon)>> {
        let data = self.yaml("indie/bets.yaml")?;
        if falsy(&data) {
            return Some(Vec::new());
        }
        if !plain_map(&data) {
            return None;
        }
        let Some(all) = get(&data, "bets") else {
            return Some(Vec::new());
        };
        if !plain_map(all) {
            return None;
        }
        let Canon::Map(pairs) = all else {
            return None;
        };
        pairs
            .iter()
            .map(|(k, v)| match k {
                Canon::Str(k) => Some((k.clone(), v.clone())),
                _ => None,
            })
            .collect()
    }

    fn bet(&self, ident: &str) -> Option<Option<Canon>> {
        Some(
            self.bets()?
                .into_iter()
                .find(|(k, _)| k == ident)
                .map(|(_, v)| v),
        )
    }

    /// `indie.readings()`: every row a text-keyed map with a `bet` key.
    fn readings(&self) -> Option<Vec<Canon>> {
        let rows = self.jsonl("indie/measures.jsonl")?;
        rows.iter()
            .all(|row| plain_map(row) && get(row, "bet").is_some())
            .then_some(rows)
    }
}

// ---------- indie: scan ----------

/// Python's `Path.is_dir` (or `Path.exists`) on this path: Some(false) for
/// the errors Python reads as "no", None for one it may raise on.
fn py_stat(path: &Path, want_dir: bool) -> Option<bool> {
    match std::fs::metadata(path) {
        Ok(meta) => Some(!want_dir || meta.is_dir()),
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Some(false)
        }
        Err(_) => None,
    }
}

/// The folders of `dir` whose names do not start with a dot, as
/// `d.iterdir()` filtered on `is_dir()`; None on any read error.
fn sub_dirs(dir: &Path) -> Option<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        let hidden = path
            .file_name()
            .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."));
        if !hidden && py_stat(&path, true)? {
            out.push(path);
        }
    }
    Some(out)
}

/// `config.get("indie_roots")` split as `indie._roots` does, in printable
/// ASCII only (so `str.strip` and this trim agree); None refuses.
fn indie_roots(files: &Files, home: Option<String>) -> Option<Vec<PathBuf>> {
    let settings = files.yaml("settings.yaml")?;
    let value = if falsy(&settings) {
        None
    } else if plain_map(&settings) {
        get(&settings, "indie_roots")
    } else {
        return None;
    };
    let raw = match value {
        None => "~/apps".to_owned(),
        Some(Canon::Str(text)) => text.clone(),
        Some(_) => return None,
    };
    if !raw.chars().all(|c| (' '..='~').contains(&c)) {
        return None;
    }
    raw.split(',')
        .map(|part| part.trim_matches(' '))
        .filter(|part| !part.is_empty())
        .map(|part| crate::events::expand_user(part, home.clone()).map(PathBuf::from))
        .collect()
}

/// Some(true) when `indie.found_repos` finds no repo, so it runs no `git`;
/// Some(false) when it finds one; None on a read Python might fail on.
fn no_repo_found(roots: &[PathBuf]) -> Option<bool> {
    for root in roots {
        if !py_stat(root, true)? {
            continue;
        }
        let level1 = sub_dirs(root)?;
        let mut all = level1.clone();
        for d in &level1 {
            let noindex = d
                .file_name()
                .is_some_and(|name| name.as_encoded_bytes().ends_with(b".noindex"));
            if !py_stat(&d.join(".git"), false)? && !noindex {
                all.extend(sub_dirs(d)?);
            }
        }
        for d in &all {
            if py_stat(&d.join(".git"), true)? {
                // A repo: Python asks `git` for its origin.
                return Some(false);
            }
        }
    }
    Some(true)
}

/// Appends one `events.emit` row whose fields are all ints, keys sorted.
fn emit_counts(ctx: &Ctx, event: &str, fields: &[(&str, usize)]) {
    use std::io::Write;
    if !ctx.env.caps.events_on {
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let stamp = crate::events::stamp(secs);
    let mut pairs: Vec<(&str, String)> = vec![
        ("event", json_str(event)),
        ("ts", json_str(&stamp)),
        ("v", json_str(crate::events::version())),
    ];
    pairs.extend(fields.iter().map(|(k, n)| (*k, n.to_string())));
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    let body: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("{}: {v}", json_str(k)))
        .collect();
    let line = format!("{{{}}}\n", body.join(", "));
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

/// `indie scan` when it finds no repo: every bet stays as it is, so it
/// writes nothing but the `indie` folder `paths.sub` makes.
fn indie_scan(args: &Parsed, ctx: &Ctx, files: &Files) -> Option<Outcome> {
    let mut bets = files.bets()?;
    let home = match std::env::var("HOME") {
        Ok(home) => Some(home),
        Err(std::env::VarError::NotPresent) => None,
        Err(_) => return None,
    };
    let roots = indie_roots(files, home)?;
    if !no_repo_found(&roots)? {
        return None;
    }
    let mut missing = Vec::new();
    for (ident, b) in &bets {
        if !plain_map(b) {
            return None;
        }
        let path = text(b, "path")?;
        if path.contains('\0') {
            return None;
        }
        // `Path("")` is the current folder.
        let path = if path.is_empty() { "." } else { path };
        if !py_stat(Path::new(path), true)? {
            missing.push(ident.clone());
        }
    }
    missing.sort();
    bets.sort_by(|a, b| a.0.cmp(&b.0));
    let ids: Vec<String> = bets.into_iter().map(|(k, _)| k).collect();
    files.touch("indie")?;
    emit_counts(
        ctx,
        "indie.scan",
        &[("found", 0), ("added", 0), ("missing", missing.len())],
    );
    if args.flag("json") {
        return ok(
            format!(
                "{{\"added\": [], \"missing\": {}, \"bets\": {}}}\n",
                json_list(&missing),
                json_list(&ids)
            ),
            0,
        );
    }
    ok(
        format!(
            "bets: {} (0 new, {} with no folder)\n",
            ids.len(),
            missing.len()
        ),
        0,
    )
}

// ---------- indie: bets ----------

fn no_bet(ident: &str) -> String {
    format!("no bet {ident} (careerbrew indie bets; careerbrew indie scan finds new repos)")
}

/// `indie.ready(b)`: what a bet lacks before it may get agent time.
fn ready(b: &Canon) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["kind", "hypothesis", "metric"] {
        if get(b, key).is_none_or(falsy) {
            out.push(key.to_owned());
        }
    }
    for key in ["success", "pivot"] {
        if matches!(get(b, key), None | Some(Canon::Null)) {
            out.push(key.to_owned());
        }
    }
    out
}

fn bet_is(row: &Canon, ident: &str) -> bool {
    matches!(get(row, "bet"), Some(Canon::Str(bet)) if bet == ident)
}

/// `indie.held`: two PIVOT readings in a row since the last decision.
fn held(ident: &str, b: &Canon, rows: &[Canon]) -> Option<bool> {
    let skip = match get(b, "decided_readings") {
        None => 0,
        Some(value) if falsy(value) && !matches!(value, Canon::Str(_)) => 0,
        Some(Canon::Int(n)) => usize::try_from(i64::try_from(n.clone()).ok()?).ok()?,
        Some(_) => return None,
    };
    let mine: Vec<&Canon> = rows
        .iter()
        .filter(|r| bet_is(r, ident))
        .skip(skip)
        .collect();
    Some(
        mine.len() >= 2
            && mine[mine.len() - 2..]
                .iter()
                .all(|r| text(r, "status") == Some("PIVOT")),
    )
}

struct BetRow {
    bet: String,
    state: String,
    kind: String,
    lacks: Vec<String>,
    held: bool,
    status: String,
}

fn indie_bets(args: &Parsed, files: &Files) -> Option<Outcome> {
    let rows = files.readings()?;
    let mut bets = files.bets()?;
    bets.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = Vec::new();
    for (ident, b) in &bets {
        if !plain_map(b) {
            return None;
        }
        let state = text(b, "state")?.to_owned();
        let kind = match get(b, "kind") {
            None => String::new(),
            Some(Canon::Str(kind)) => kind.clone(),
            Some(_) => return None,
        };
        let mut status = "no-data".to_owned();
        for row in rows.iter().filter(|r| bet_is(r, ident)) {
            status = text(row, "status")?.to_owned();
        }
        out.push(BetRow {
            bet: ident.clone(),
            state,
            kind,
            lacks: ready(b),
            held: held(ident, b, &rows)?,
            status,
        });
    }
    files.touch("indie")?;
    if args.flag("json") {
        let items: Vec<String> = out
            .iter()
            .map(|r| {
                format!(
                    "{{\"bet\": {}, \"state\": {}, \"kind\": {}, \"lacks\": {}, \"held\": {}, \"metric_status\": {}}}",
                    json_str(&r.bet),
                    json_str(&r.state),
                    json_str(&r.kind),
                    json_list(&r.lacks),
                    r.held,
                    json_str(&r.status)
                )
            })
            .collect();
        return ok(format!("{{\"bets\": [{}]}}\n", items.join(", ")), 0);
    }
    let mut stdout = String::new();
    for r in &out {
        let kind = if r.kind.is_empty() { "-" } else { &r.kind };
        stdout.push_str(&format!(
            "  {:<24} {:<9} {:<10} {:<8}",
            r.bet, r.state, kind, r.status
        ));
        if r.held {
            stdout.push_str(" HELD");
        }
        if !r.lacks.is_empty() {
            stdout.push_str(&format!("  lacks: {}", r.lacks.join(", ")));
        }
        stdout.push('\n');
    }
    stdout.push_str(&format!(
        "bets: {}, {} active, {} waiting for a hypothesis\n",
        out.len(),
        out.iter().filter(|r| r.state == "active").count(),
        out.iter().filter(|r| r.state == "proposed").count()
    ));
    ok(stdout, 0)
}

/// `indie bet <id>`: served only for an unknown id (with or without
/// fields, `set_bet` looks the bet up first).
fn indie_bet(args: &Parsed, files: &Files) -> Option<Outcome> {
    let ident = args.text("id")?;
    if files.bet(ident)?.is_some() {
        return None;
    }
    files.touch("indie")?;
    indie_error(&no_bet(ident))
}

fn indie_decide(args: &Parsed, files: &Files) -> Option<Outcome> {
    let ident = args.text("id")?;
    let decision = args.text("decision")?;
    if !args.flag("yes") {
        return indie_error(&format!("a decision on a bet is the owner's: add --yes after the owner says so (careerbrew indie decide {ident} {decision} --yes)"));
    }
    let active = matches!(decision, "activate" | "persevere");
    if !active && !matches!(decision, "pause" | "pivot" | "kill") {
        return None;
    }
    let Some(b) = files.bet(ident)? else {
        files.touch("indie")?;
        return indie_error(&no_bet(ident));
    };
    if !plain_map(&b) {
        return None;
    }
    let lack = ready(&b);
    if active && !lack.is_empty() {
        files.touch("indie")?;
        return indie_error(&format!("bet {ident} cannot be active yet: it lacks {} (careerbrew indie bet {ident} --kind ... --hypothesis ... --metric ... --success N --pivot N)", lack.join(", ")));
    }
    None
}

fn indie_measure(args: &Parsed, files: &Files) -> Option<Outcome> {
    let ident = args.text("id")?;
    let value = float_arg(args, "value")?;
    let Some(b) = files.bet(ident)? else {
        files.touch("indie")?;
        return indie_error(&no_bet(ident));
    };
    if value.is_none() && plain_map(&b) && get(&b, "measure").is_none_or(falsy) {
        files.touch("indie")?;
        return indie_error(&format!("bet {ident} has no measure command: pass --value, or set one with careerbrew indie bet {ident} --measure '<command>'"));
    }
    None
}

/// True when Python's `float(text)` certainly raises: an empty value, or
/// letters and digits only with no digit, that do not spell nan or inf.
fn not_a_float(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    if !text.chars().all(|ch| ch.is_ascii_alphanumeric())
        || text.chars().any(|ch| ch.is_ascii_digit())
    {
        return false;
    }
    !matches!(
        text.to_ascii_lowercase().as_str(),
        "nan" | "inf" | "infinity"
    )
}

fn indie_end(args: &Parsed) -> Option<Outcome> {
    let bad = args.list("hours").iter().any(|item| {
        let value = item.split_once('=').map_or("", |(_, value)| value);
        not_a_float(value)
    });
    if bad {
        return indie_error("--hours looks like <bet>=<hours>, for example sidequest=1.5");
    }
    None
}

// ---------- indie: goal and income ----------

struct Progress {
    set: bool,
    by: Canon,
    month: String,
    percent: Option<f64>,
    previous: Option<f64>,
    months: usize,
}

/// `round(value, 1)`, or None at an exact tie (`n.25`, `n.75`), where a
/// half-even rule could round differently.
fn round1(value: f64) -> Option<f64> {
    let quarter = value * 4.0;
    if quarter.fract() == 0.0 && quarter.abs() < 9.0e15 && (quarter as i64) % 2 != 0 {
        return None;
    }
    let out = careerbrew_reports::py_round(value, 1);
    out.is_finite().then_some(out)
}

/// `indie.progress()`.
fn progress(files: &Files) -> Option<Progress> {
    let goal = files.yaml("indie/goal.yaml")?;
    let goal = if falsy(&goal) {
        Canon::Map(Vec::new())
    } else if plain_map(&goal) {
        goal
    } else {
        return None;
    };
    let target = match get(&goal, "monthly") {
        None => 0.0,
        Some(Canon::Bool(_)) => return None,
        Some(value) if falsy(value) => 0.0,
        Some(value) => number(value)?,
    };
    let by = match get(&goal, "by") {
        None => Canon::Str(String::new()),
        Some(value @ (Canon::Str(_) | Canon::Null)) => value.clone(),
        Some(_) => return None,
    };
    let (year, month) = this_month(files)?;
    let back = |(y, m): (i64, i64)| if m == 1 { (y - 1, 12) } else { (y, m - 1) };
    let last = back((year, month));
    let prev = back(last);
    let label = |(y, m): (i64, i64)| format!("{y:04}-{m:02}");
    if !(1..=9999).contains(&prev.0) {
        return None;
    }
    let mut by_month: Vec<(String, f64)> = Vec::new();
    for row in files.jsonl("indie/income.jsonl")? {
        if !plain_map(&row) {
            return None;
        }
        let month = text(&row, "month")?.to_owned();
        let amount = number(get(&row, "amount")?)?;
        match by_month.iter_mut().find(|(m, _)| *m == month) {
            Some((_, sum)) => *sum += amount,
            None => by_month.push((month, 0.0 + amount)),
        }
    }
    let pct = |month: &str| -> Option<Option<f64>> {
        if target == 0.0 {
            return Some(None);
        }
        match by_month.iter().find(|(m, _)| m == month) {
            None => Some(None),
            Some((_, sum)) => round1(100.0 * sum / target).map(Some),
        }
    };
    Some(Progress {
        set: target != 0.0,
        by,
        percent: pct(&label(last))?,
        previous: pct(&label(prev))?,
        month: label(last),
        months: by_month.len(),
    })
}

fn opt_float(value: Option<f64>) -> String {
    value.map_or("null".to_owned(), careerbrew_reports::py_repr)
}

fn progress_json(p: &Progress) -> Option<String> {
    let mut by = String::new();
    dump(&p.by, &mut by)?;
    Some(format!(
        "{{\"set\": {}, \"by\": {by}, \"month\": {}, \"percent\": {}, \"previous_percent\": {}, \"months_recorded\": {}}}\n",
        p.set,
        json_str(&p.month),
        opt_float(p.percent),
        opt_float(p.previous),
        p.months
    ))
}

fn indie_goal(args: &Parsed, files: &Files) -> Option<Outcome> {
    if let Some(monthly) = float_arg(args, "monthly")? {
        if !monthly.is_finite() || monthly <= 0.0 {
            return indie_error("the monthly income target is a number above 0");
        }
        // A valid target writes `goal.yaml` (PyYAML): Python does it.
        return None;
    }
    let p = progress(files)?;
    files.touch("indie")?;
    if args.flag("json") {
        return ok(progress_json(&p)?, 0);
    }
    if !p.set {
        return ok("goal: not set. You type it (it stays encrypted, and is never printed): careerbrew indie goal --monthly <amount> --by YYYY-MM-DD\n".to_owned(), 0);
    }
    let got = match p.percent {
        None => "no income recorded".to_owned(),
        Some(value) => format!("{}% of the target", careerbrew_reports::py_repr(value)),
    };
    let by = match &p.by {
        Canon::Str(by) if !by.is_empty() => format!(" for {by}"),
        _ => String::new(),
    };
    ok(
        format!(
            "goal: set{by}; {}: {got} ({} month(s) recorded)\n",
            p.month, p.months
        ),
        0,
    )
}

/// `re.fullmatch(r"\d{4}-(0[1-9]|1[0-2])", month)` for ASCII text; None
/// for text with other digits, which `\d` could also read.
fn month_ok(month: &str) -> Option<bool> {
    if month
        .chars()
        .any(|ch| ch.is_numeric() && !ch.is_ascii_digit())
    {
        return None;
    }
    let b = month.as_bytes();
    Some(
        b.len() == 7
            && b[..4].iter().all(u8::is_ascii_digit)
            && b[4] == b'-'
            && matches!((b[5], b[6]), (b'0', b'1'..=b'9') | (b'1', b'0'..=b'2')),
    )
}

fn indie_income(args: &Parsed, files: &Files) -> Option<Outcome> {
    if let Some(amount) = float_arg(args, "amount")? {
        let month = args.text("month").unwrap_or("");
        if month.is_empty() {
            return indie_error("pass --month YYYY-MM with --amount");
        }
        if !month_ok(month)? {
            return indie_error("the month looks like 2026-11");
        }
        if !amount.is_finite() || amount < 0.0 {
            return indie_error("the amount is a number, 0 or more");
        }
        let bet = args.text("bet").unwrap_or("");
        if !bet.is_empty() && files.bet(bet)?.is_none() {
            files.touch("indie")?;
            return indie_error(&format!("no bet {bet} (careerbrew indie bets)"));
        }
        // A valid row appends to `income.jsonl`: Python does it.
        return None;
    }
    let p = progress(files)?;
    files.touch("indie")?;
    if args.flag("json") {
        return ok(progress_json(&p)?, 0);
    }
    let got = match p.percent {
        None => "no record".to_owned(),
        Some(value) => format!("{}% of the target", careerbrew_reports::py_repr(value)),
    };
    ok(
        format!(
            "income: {} month(s) recorded; {}: {got}\n",
            p.months, p.month
        ),
        0,
    )
}

// ---------- peer ----------

const SIDES: &[(&str, &[&str])] = &[
    (
        "indie",
        &["hours_per_outcome", "asks_per_outcome", "outcomes_per_week"],
    ),
    (
        "work",
        &["step_fail_rate", "step_blocked_rate", "step_minutes"],
    ),
];

fn peer_of(side: &str) -> &'static str {
    if side == "indie" {
        "work"
    } else {
        "indie"
    }
}

/// `peers.experiments()`: every row a text-keyed map with text `id`,
/// `side`, `metric`, `verdict` and `change`, and a `closed` key.
fn experiments(files: &Files) -> Option<Vec<Canon>> {
    let rows = files.jsonl("peer/experiments.jsonl")?;
    rows.iter()
        .all(|row| {
            plain_map(row)
                && ["id", "side", "metric", "verdict", "change"]
                    .iter()
                    .all(|key| text(row, key).is_some())
                && get(row, "closed").is_some()
        })
        .then_some(rows)
}

fn open_experiment<'a>(rows: &'a [Canon], side: &str) -> Option<&'a Canon> {
    rows.iter()
        .find(|row| text(row, "side") == Some(side) && get(row, "closed").is_none_or(falsy))
}

/// `--lesson`: Some(true) when it names a lesson of `peer`, Some(false)
/// when it certainly does not, None when the regex could read it either
/// way (any character but ASCII letters, digits and `-`).
fn lesson_ok(lesson: &str, peer: &str) -> Option<bool> {
    if !lesson
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return None;
    }
    let Some(rest) = lesson.strip_prefix("L-") else {
        return Some(false);
    };
    let Some((side, number)) = rest.split_once('-') else {
        return Some(false);
    };
    let digits = !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit());
    Some(digits && matches!(side, "indie" | "work") && side == peer)
}

fn peer_experiment(args: &Parsed, files: &Files) -> Option<Outcome> {
    let side = args.text("side");
    if let Some(change) = args.text("start").filter(|start| !start.is_empty()) {
        let (Some(side), Some(metric)) = (side, args.text("metric").filter(|m| !m.is_empty()))
        else {
            return peer_error("--start needs --side and --metric");
        };
        let metrics = SIDES.iter().find(|(name, _)| *name == side)?.1;
        if change.chars().any(|ch| ('\u{1c}'..='\u{1f}').contains(&ch)) {
            return None;
        }
        let change = change.split_whitespace().collect::<Vec<_>>().join(" ");
        if !(8..=300).contains(&change.chars().count()) {
            return peer_error("describe the change in one line of 8 to 300 characters");
        }
        let peer = peer_of(side);
        let lesson = args.text("lesson").unwrap_or("");
        if !lesson.is_empty() && !lesson_ok(lesson, peer)? {
            return peer_error(&format!("--lesson names a lesson of {peer}, like L-{peer}-1 (careerbrew peer status --for {side})"));
        }
        let rows = experiments(files)?;
        if let Some(cur) = open_experiment(&rows, side) {
            files.touch("peer")?;
            return peer_error(&format!("{side} already trials {}: one change at a time, or the number cannot say which change moved it (careerbrew peer experiment --side {side} --close)", text(cur, "id")?));
        }
        if !metrics.contains(&metric) {
            files.touch("peer")?;
            return peer_error(&format!("{side} metrics: {}", metrics.join(", ")));
        }
        // A known metric measures and appends the trial: Python does it.
        return None;
    }
    if args.flag("close") {
        let Some(side) = side else {
            return peer_error("--close needs --side");
        };
        let rows = experiments(files)?;
        let Some(cur) = open_experiment(&rows, side) else {
            files.touch("peer")?;
            return peer_error(&format!(
                "{side} has no open trial (careerbrew peer experiment --side {side} --start ...)"
            ));
        };
        let started = stamp_micros(text(cur, "started")?)?;
        clock_ok(files)?;
        let now = careerbrew_teacher::now_local(&files.root);
        let now_us = now.timestamp() * 1_000_000 + i64::from(now.timestamp_subsec_micros());
        let ran = (now_us - started).div_euclid(86_400_000_000);
        if ran < 3 {
            files.touch("peer")?;
            return peer_error(&format!(
                "{} ran {ran} day(s): a trial needs at least 3 before its number means anything",
                text(cur, "id")?
            ));
        }
        // A trial that ran long enough is measured and closed: Python does it.
        return None;
    }
    let rows: Vec<Canon> = experiments(files)?
        .into_iter()
        .filter(|row| side.is_none_or(|side| text(row, "side") == Some(side)))
        .collect();
    files.touch("peer")?;
    if args.flag("json") {
        let mut out = String::from("{\"experiments\": ");
        dump(&Canon::List(rows), &mut out)?;
        out.push_str("}\n");
        return ok(out, 0);
    }
    let mut stdout = String::new();
    for row in &rows {
        let verdict = text(row, "verdict")?;
        stdout.push_str(&format!(
            "  {:<12} {:<20} {:<8} {}\n",
            text(row, "id")?,
            text(row, "metric")?,
            if verdict.is_empty() { "open" } else { verdict },
            text(row, "change")?
        ));
    }
    stdout.push_str(&format!(
        "trials: {}, {} open, {} kept\n",
        rows.len(),
        rows.iter()
            .filter(|row| get(row, "closed").is_none_or(falsy))
            .count(),
        rows.iter()
            .filter(|row| text(row, "verdict") == Some("KEEP"))
            .count()
    ));
    ok(stdout, 0)
}

// ---------- changes (changes.py) ----------

/// `paths.REPO_ROOT / "knowledge"` of the checkout this binary was built from.
fn knowledge() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../knowledge")
}

/// `paths.extra_knowledge()`.
fn extra_knowledge() -> bool {
    std::env::var("CAREERBREW_EXTRA_KNOWLEDGE").map_or(true, |value| value != "0")
}

/// The regular files of `dir` (`*` or `*.yaml`), in `Path` order; None on
/// a name Python's `glob` or `Path.stem` could read differently.
fn listed(dir: &Path, yaml_only: bool) -> Option<Vec<PathBuf>> {
    match std::fs::metadata(dir) {
        Ok(meta) if meta.is_dir() => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(Vec::new()),
        _ => return None,
    }
    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(dir).ok()? {
        let name = entry.ok()?.file_name().into_string().ok()?;
        if yaml_only && !name.ends_with(".yaml") {
            continue;
        }
        if name.starts_with('.') {
            return None;
        }
        match std::fs::metadata(dir.join(&name)) {
            Ok(meta) if meta.is_file() => names.push(name),
            // `glob("*")` then `is_file()` skips a folder; a `*.yaml` folder
            // or a broken link is read by Python and fails.
            Ok(meta) if meta.is_dir() && !yaml_only => {}
            _ => return None,
        }
    }
    names.sort();
    Some(names.into_iter().map(|name| dir.join(name)).collect())
}

/// `store.read_yaml(path, {})` as Canon (a missing or empty file is `{}`).
fn read_yaml_or_empty(files: &Files, path: &Path) -> Option<Canon> {
    files
        .vault
        .store()
        .read_yaml(path, Canon::Map(Vec::new()), false)
        .ok()
}

/// The keys of a text-keyed map hold `key` (Python's `key in d`).
fn has(value: &Canon, key: &str) -> bool {
    get(value, key).is_some()
}

/// `promo.validate_ladder(data)` passes; None for a ladder Python refuses
/// or reads in a way not checked here.
fn ladder_ok(data: &Canon) -> Option<()> {
    if !plain_map(data) {
        return None;
    }
    for key in ["id", "levels", "competencies"] {
        if falsy(get(data, key)?) {
            return None;
        }
    }
    let Some(Canon::List(levels)) = get(data, "levels") else {
        return None;
    };
    let Some(Canon::List(comps)) = get(data, "competencies") else {
        return None;
    };
    for c in comps {
        if !plain_map(c) {
            return None;
        }
        for key in ["id", "name", "weight", "signals", "expect"] {
            if !has(c, key) {
                return None;
            }
        }
        let expect = get(c, "expect")?;
        if !plain_map(expect) {
            return None;
        }
        for level in levels {
            let Canon::Str(level) = level else {
                return None;
            };
            if !has(expect, level) {
                return None;
            }
        }
        for key in ["descriptions", "examples"] {
            if let Some(value) = get(c, key) {
                if !matches!(value, Canon::Map(_)) {
                    return None;
                }
            }
        }
    }
    Some(())
}

/// `changes._h(obj)`.
fn input_hash(value: &Canon) -> Option<String> {
    let typed = careerbrew_purevec::hash::typed(&value.to_json()).ok()?;
    careerbrew_purevec::hash::input_hash(&typed).ok()
}

/// Sets `key` in an ordered id map as a dict assignment does.
fn assign(out: &mut Vec<(String, String)>, key: String, value: String) {
    match out.iter_mut().find(|(have, _)| *have == key) {
        Some(slot) => slot.1 = value,
        None => out.push((key, value)),
    }
}

/// `{lid: _h(lad without origin)}` over `promo.ladders()` with no plugin:
/// the base ladders, then the user's.
fn ladder_hashes(files: &Files) -> Option<Vec<(String, String)>> {
    let mut found = listed(&knowledge().join("ladders"), false)?;
    found.extend(listed(&files.root.join("ladders"), true)?);
    let mut out = Vec::new();
    for path in found {
        let data = read_yaml_or_empty(files, &path)?;
        ladder_ok(&data)?;
        let Some(Canon::Str(id)) = get(&data, "id") else {
            return None;
        };
        let Canon::Map(pairs) = &data else {
            return None;
        };
        let kept: Vec<(Canon, Canon)> = pairs
            .iter()
            .filter(|(k, _)| !matches!(k, Canon::Str(k) if k == "origin"))
            .cloned()
            .collect();
        assign(&mut out, id.clone(), input_hash(&Canon::Map(kept))?);
    }
    Some(out)
}

/// `{cid: _h(...)}` over `interview.companies()` with no plugin.
fn company_hashes() -> Option<Vec<(String, String)>> {
    let base = knowledge();
    // `plugins.resources("knowledge")` also lists this base folder.
    if base.join("knowledge").symlink_metadata().is_ok() {
        return None;
    }
    let dirs: &[&str] = if extra_knowledge() {
        &["companies-extra", "companies"]
    } else {
        &["companies"]
    };
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for dir in dirs {
        for path in listed(&base.join(dir), true)? {
            found.push(((*dir).to_owned(), path));
        }
    }
    // `sorted(...)` over the paths of both folders: by folder, then name.
    found.sort();
    let mut out = Vec::new();
    for (_, path) in found {
        let raw = std::fs::read(&path).ok()?;
        if raw.starts_with(b"CBV1\n") || raw.starts_with(b"gAAAAA") {
            return None;
        }
        let text = String::from_utf8(raw).ok()?;
        let data = careerbrew_vault::json_yaml::yaml_read(&text).ok()?;
        let data = if falsy(&data) {
            Canon::Map(Vec::new())
        } else {
            data
        };
        if !plain_map(&data) {
            return None;
        }
        if get(&data, "loop").is_none_or(falsy) {
            continue;
        }
        let id = match get(&data, "id") {
            None => path
                .file_name()?
                .to_str()?
                .strip_suffix(".yaml")?
                .to_owned(),
            Some(Canon::Str(id)) => id.clone(),
            Some(Canon::Int(id)) => id.to_string(),
            _ => return None,
        };
        if !id.is_ascii() {
            return None;
        }
        let picked: Vec<(Canon, Canon)> = ["loop", "remote_policy", "ai_policy", "company_tips"]
            .iter()
            .map(|key| {
                (
                    Canon::Str((*key).to_owned()),
                    get(&data, key).cloned().unwrap_or(Canon::Null),
                )
            })
            .collect();
        assign(
            &mut out,
            id.to_ascii_lowercase(),
            input_hash(&Canon::Map(picked))?,
        );
    }
    Some(out)
}

/// True when the stored snapshot section `old` equals `items` (no change).
fn same_section(old: Option<&Canon>, items: &[(String, String)]) -> Option<bool> {
    let old = match old {
        None => return Some(items.is_empty()),
        Some(old) if plain_map(old) => old,
        Some(_) => return None,
    };
    let Canon::Map(pairs) = old else {
        return None;
    };
    if pairs.len() != items.len() {
        return Some(false);
    }
    Some(
        items
            .iter()
            .all(|(key, value)| matches!(get(old, key), Some(Canon::Str(have)) if have == value)),
    )
}

fn section(items: &[(String, String)]) -> Canon {
    Canon::Map(
        items
            .iter()
            .map(|(k, v)| (Canon::Str(k.clone()), Canon::Str(v.clone())))
            .collect(),
    )
}

/// `changes.check(fetch=False)` when nothing changed.
fn changes(args: &Parsed, files: &Files) -> Option<Outcome> {
    if args.flag("fetch") {
        return None;
    }
    // An installed plugin adds ladders, companies and a tree hash.
    match std::fs::symlink_metadata(files.root.join("plugins.lock.json")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        _ => return None,
    }
    let ladders = ladder_hashes(files)?;
    let companies = company_hashes()?;
    let none: Vec<(String, String)> = Vec::new();
    let now: [(&str, &[(String, String)]); 4] = [
        ("ladders", &ladders),
        ("plugins", &none),
        ("companies", &companies),
        ("pages", &none),
    ];
    let folder = files.root.join("changes");
    let path = folder.join("snapshot.json");
    let store = files.vault.store();
    let old_text = store.read_text(&path, None, false).ok()?;
    let prev = match &old_text {
        None => Canon::Null,
        Some(text) => careerbrew_vault::json_yaml::json_read(text).ok()?,
    };
    if prev != Canon::Null {
        if !plain_map(&prev) {
            return None;
        }
        for (kind, items) in &now {
            if !same_section(get(&prev, kind), items)? {
                // A change: Python pivots (re-assesses, re-plans).
                return None;
            }
        }
    }
    let snapshot = Canon::Map(
        now.iter()
            .map(|(kind, items)| (Canon::Str((*kind).to_owned()), section(items)))
            .collect(),
    );
    let text = careerbrew_vault::json_yaml::json_write(&snapshot).ok()?;
    if old_text.as_deref() != Some(text.as_str()) {
        if store.encryption_enabled() {
            // A sealed write differs on every run.
            return None;
        }
        std::fs::create_dir_all(&folder).ok()?;
        store.write_text(&path, &text).ok()?;
    }
    ok(
        "no changes since the last check (the first run only records a baseline)\n".to_owned(),
        0,
    )
}
