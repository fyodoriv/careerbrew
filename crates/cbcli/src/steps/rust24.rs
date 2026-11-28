//! Native CLI handlers owned by `rust-24` (see `steps/mod.rs`): the small,
//! certain cases of `time baseline`, `checklist` and `work project`.
//!
//! Python's `cmd_time`, `cmd_checklist` and `cmd_work_project`
//! (`src/careerbrew/cli.py`) and `work/projects.py` are the reference. Each
//! handler serves only these cases:
//! - `time baseline <action> <minutes>`: `config.set_value` through the
//!   rust-02 settings rows, as `config set` does (`rust05`), with its
//!   `ValueError` line;
//! - `checklist` when none of the files its steps read exists (only the
//!   settings may be set);
//! - `work project list` (text and `--json`) with no project at all;
//! - `work project rules`: no rules, hidden rules (exit 3), or an unknown
//!   project;
//! - the confirmation lines of `rules-set` and `remove` without `--yes`;
//! - the `ProjectError` lines a writer raises before it changes anything
//!   (an unknown project, a project that is not a prep project, a bad kind
//!   or label), after taking the same `work/.projects.yaml.lock` Python's
//!   `@_writes` takes;
//! - `work project check --since --status`, and `work project prd --slot`
//!   out of range (Python's `SystemExit`).
//!
//! Every other form refuses (exit 64, Python runs it) before any write: a
//! redaction input, any read error, a write of project data, any text that
//! is not printable ASCII, a clock or zone Python could read differently,
//! and anything a terminal would show differently.
//!
//! The settings helpers (`settings_op` to `written_yaml`) are private copies
//! of the ones in `rust05.rs`, which keeps them private.
use super::Ctx;
use crate::args::{Parsed, Value};
use crate::{events, Outcome};
use careerbrew_vault::canon::Canon;
use careerbrew_vault::store::Store;
use careerbrew_vault::transfer::Context;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust24.rs`.
pub const COMMANDS: &[&str] = &[
    "time.baseline",
    "checklist",
    "work.project.list",
    "work.project.rules",
    "work.project.rules-set",
    "work.project.remove",
    "work.project.archive",
    "work.project.unarchive",
    "work.project.quiz",
    "work.project.gate",
    "work.project.update",
    "work.project.source",
    "work.project.check",
    "work.project.prd",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    // Every line goes through `privacy.redact`, and with no plugin lock
    // (a redaction input) no plugin policy can allow business reading.
    if ctx.redact_inputs_present() || !ctx.root.is_absolute() {
        return None;
    }
    match name {
        "time.baseline" => time_baseline(args, ctx),
        "checklist" => checklist(ctx),
        "work.project.list" => project_list(args, ctx),
        "work.project.rules" => project_rules(args, ctx),
        "work.project.rules-set" => project_rules_set(args, ctx),
        "work.project.remove" => project_remove(args, ctx),
        "work.project.archive" | "work.project.unarchive" => {
            let name = printable_name(args)?;
            writer_error(ctx, |every| unknown(every, name))
        }
        "work.project.quiz" | "work.project.gate" => {
            let name = printable_name(args)?;
            writer_error(ctx, |every| not_prep(every, name))
        }
        "work.project.update" => project_update(args, ctx),
        "work.project.source" => project_source(args, ctx),
        "work.project.check" => project_check(args, ctx),
        "work.project.prd" => project_prd(args),
        _ => None,
    }
}

// ---------- outcomes and event rows ----------

fn said(stdout: String, code: i32) -> Option<Outcome> {
    Some(Outcome {
        stdout,
        stderr: String::new(),
        code,
        emit_event: true,
    })
}

/// Printable ASCII only: it prints the same under every locale, and
/// Python's `str.strip` and `len` read it as Rust does.
fn printable(text: &str) -> bool {
    text.chars().all(|c| (' '..='~').contains(&c))
}

fn printable_name(args: &Parsed) -> Option<&str> {
    args.text("name")
        .filter(|name| !name.is_empty() && printable(name))
}

/// Appends one row as `events.emit` writes it: `ts`, `v` and the id
/// fields, sorted by key, in `json.dumps` form.
fn emit(ctx: &Ctx, event: &str, fields: &[(&str, &str)]) {
    if !ctx.env.caps.events_on {
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let stamp = events::stamp(secs);
    let mut pairs: Vec<(&str, &str)> =
        vec![("event", event), ("ts", &stamp), ("v", events::version())];
    pairs.extend_from_slice(fields);
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    let mut line = String::from("{");
    for (i, (key, value)) in pairs.iter().enumerate() {
        if i > 0 {
            line.push_str(", ");
        }
        crate::render::py_string(key, &mut line);
        line.push_str(": ");
        crate::render::py_string(value, &mut line);
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

/// `cli.main` on a caught error: one `cli.error` row, `error: <message>`,
/// exit 2, and no `cli.run` row. `class` is the lowercase class name.
fn cli_error(ctx: &Ctx, command: &str, class: &str, message: &str) -> Option<Outcome> {
    if !printable(message) {
        return None;
    }
    emit(
        ctx,
        "cli.error",
        &[
            ("command", command),
            ("error", class),
            ("via", ctx.env.caps.via),
        ],
    );
    Some(Outcome {
        stdout: format!("error: {message}\n"),
        stderr: String::new(),
        code: 2,
        emit_event: false,
    })
}

fn project_error(ctx: &Ctx, message: &str) -> Option<Outcome> {
    cli_error(ctx, "work.project", "projecterror", message)
}

// ---------- the vault and plain values ----------

/// The vault context from the environment, when it resolves the key path
/// as Python's `keyfile.key_path` does (copied from `rust05`).
fn vault_context() -> Option<Context> {
    if std::env::var("HOME").map_or(true, |home| home.is_empty()) {
        return None;
    }
    if let Ok(key) = std::env::var("CAREERBREW_KEY_FILE") {
        if key.starts_with('~') && key != "~" && !key.starts_with("~/") {
            return None;
        }
    }
    Context::from_env().ok()
}

/// Python's truth value of a loaded value.
fn truthy(value: &Canon) -> bool {
    match value {
        Canon::Null => false,
        Canon::Bool(b) => *b,
        Canon::Int(n) => n.to_string() != "0",
        Canon::Float(f) => *f != 0.0,
        Canon::Str(s) => !s.is_empty(),
        Canon::List(items) => !items.is_empty(),
        Canon::Map(pairs) => !pairs.is_empty(),
        Canon::Date(_) | Canon::DateTime(_) => true,
    }
}

type Pairs = Vec<(Canon, Canon)>;

/// A map whose keys are distinct strings (PyYAML keeps the last of a
/// repeated key; such a file goes to Python).
fn plain_map(value: &Canon) -> Option<&Pairs> {
    match value {
        Canon::Map(pairs)
            if pairs.iter().enumerate().all(|(i, (key, _))| {
                matches!(key, Canon::Str(_)) && pairs[..i].iter().all(|(other, _)| other != key)
            }) =>
        {
            Some(pairs)
        }
        _ => None,
    }
}

fn get<'a>(pairs: &'a Pairs, key: &str) -> Option<&'a Canon> {
    pairs
        .iter()
        .find(|(k, _)| matches!(k, Canon::Str(k) if k == key))
        .map(|(_, v)| v)
}

/// True when nothing (not even a dangling link) is at `path`.
fn absent(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_err()
}

/// `store.read_yaml(path, {}) or {}` as a map of distinct string keys;
/// None on a read error or another shape.
fn read_map(store: &Store, path: &Path) -> Option<Pairs> {
    if absent(path) {
        return Some(Vec::new());
    }
    match store.read_yaml(path, Canon::Map(Vec::new()), false).ok()? {
        value @ Canon::Map(_) => plain_map(&value).cloned(),
        other if !truthy(&other) => Some(Vec::new()),
        _ => None,
    }
}

/// `paths.sub(...)`: Python makes the parent folders. None when a part
/// exists as anything but a folder.
fn make_dirs(root: &Path, parts: &[&str]) -> Option<()> {
    let mut at: PathBuf = root.to_path_buf();
    for part in parts {
        at = at.join(part);
        if let Ok(meta) = std::fs::symlink_metadata(&at) {
            if !meta.is_dir() {
                return None;
            }
        }
    }
    std::fs::create_dir_all(&at).ok()
}

// ---------- the clock Python reads first ----------

fn digits(text: &str, lo: u32, hi: u32) -> bool {
    text.bytes().all(|b| b.is_ascii_digit())
        && text.parse::<u32>().is_ok_and(|n| (lo..=hi).contains(&n))
}

/// A `CAREERBREW_NOW` that `datetime.fromisoformat` reads for sure:
/// `YYYY-MM-DDTHH:MM[:SS][+HH:MM]` (days up to 28, so every month has it).
fn simple_now(text: &str) -> bool {
    if !text.is_ascii() {
        return false;
    }
    let (stamp, zone) = match text.get(19..) {
        Some(rest) if !rest.is_empty() => (&text[..19], Some(rest)),
        _ => (text, None),
    };
    let zone_ok = zone.is_none_or(|zone| {
        let b = zone.as_bytes();
        b.len() == 6
            && (b[0] == b'+' || b[0] == b'-')
            && digits(&zone[1..3], 0, 23)
            && b[3] == b':'
            && digits(&zone[4..6], 0, 59)
    });
    let b = stamp.as_bytes();
    let long = b.len() == 19 && b[16] == b':' && digits(&stamp[17..19], 0, 59);
    zone_ok
        && (b.len() == 16 || long)
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && digits(&stamp[..4], 1, 9999)
        && digits(&stamp[5..7], 1, 12)
        && digits(&stamp[8..10], 1, 28)
        && digits(&stamp[11..13], 0, 23)
        && digits(&stamp[14..16], 0, 59)
}

/// True when `calendar.now()` cannot raise: a pinned clock in a form
/// `fromisoformat` reads, and a `timezone` setting that is empty or names a
/// zone file `ZoneInfo` loads from the system zone folder.
fn clock_safe(settings: &Pairs) -> bool {
    if let Ok(pinned) = std::env::var("CAREERBREW_NOW") {
        if !pinned.is_empty() && !simple_now(&pinned) {
            return false;
        }
    }
    match get(settings, "timezone") {
        Some(value) if truthy(value) => {
            let Canon::Str(zone) = value else {
                return false;
            };
            let ok_chars = zone
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "/_-+".contains(c));
            ok_chars
                && !zone.starts_with('/')
                && !zone.split('/').any(|part| part.is_empty() || part == "..")
                && Path::new("/usr/share/zoneinfo").join(zone).is_file()
        }
        _ => true,
    }
}

// ---------- work project ----------

/// `projects._load()`: `(store.read_yaml(path(), {}) or {}).get("projects",
/// {}) or {}` as (name, record) pairs; None for any other shape or error.
fn load_projects(store: &Store, root: &Path) -> Option<Vec<(String, Canon)>> {
    let top = read_map(store, &root.join("work").join("projects.yaml"))?;
    let projects = match get(&top, "projects") {
        None => return Some(Vec::new()),
        Some(value) if !truthy(value) => return Some(Vec::new()),
        Some(value) => plain_map(value)?,
    };
    projects
        .iter()
        .map(|(key, record)| match key {
            Canon::Str(key) => Some((key.clone(), record.clone())),
            _ => None,
        })
        .collect()
}

/// `projects._get`'s `ProjectError` text, or None when the project exists.
/// The names it lists must print the same: printable ASCII.
fn unknown_text(every: &[(String, Canon)], name: &str) -> Option<Option<String>> {
    if every.iter().any(|(n, _)| n == name) {
        return Some(None);
    }
    let mut names: Vec<&str> = every.iter().map(|(n, _)| n.as_str()).collect();
    if !names.iter().all(|n| printable(n)) {
        return None;
    }
    names.sort_unstable();
    let have = if names.is_empty() {
        "none yet".to_owned()
    } else {
        names.join(", ")
    };
    Some(Some(format!(
        "unknown project {name} (fix: `careerbrew work project add {name}`; have: {have})"
    )))
}

/// A writer's error for an unknown project; None (refuse) for a known one,
/// whose change only Python makes.
fn unknown(every: &[(String, Canon)], name: &str) -> Option<String> {
    unknown_text(every, name)?
}

/// `quiz` and `gate`: unknown, then not a prep project.
fn not_prep(every: &[(String, Canon)], name: &str) -> Option<String> {
    if let Some(message) = unknown_text(every, name)? {
        return Some(message);
    }
    let record = &every.iter().find(|(n, _)| n == name)?.1;
    let pairs = plain_map(record)?;
    match get(pairs, "kind") {
        Some(Canon::Str(kind)) if kind == "prep" => None,
        _ => Some(format!(
            "{name} is not a prep project (fix: careerbrew work project set {name} --kind prep)"
        )),
    }
}

/// Takes `work/.projects.yaml.lock` as `projects.locked()` does (making
/// `work/` first), and runs `f` under it.
fn under_lock<T>(ctx: &Ctx, f: impl FnOnce() -> T) -> Option<T> {
    make_dirs(&ctx.root, &["work"])?;
    let lock = ctx.root.join("work").join(".projects.yaml.lock");
    careerbrew_vault::lock::with_exact_lock(&lock, f).ok()
}

/// A `@_writes` writer that raises `ProjectError` after `_load()`: decide
/// from the records before any write, then again under the lock.
fn writer_error(
    ctx: &Ctx,
    decide: impl Fn(&[(String, Canon)]) -> Option<String>,
) -> Option<Outcome> {
    let vault = vault_context()?;
    let store = vault.store();
    if !(ctx.root.join("work").is_dir() || absent(&ctx.root.join("work"))) {
        return None;
    }
    let first = decide(&load_projects(&store, &ctx.root)?)?;
    let again = under_lock(ctx, || {
        load_projects(&store, &ctx.root).and_then(|every| decide(&every))
    })??;
    if again != first {
        return None;
    }
    project_error(ctx, &first)
}

/// A `@_writes` writer that raises `ProjectError` before `_load()`.
fn writer_early_error(ctx: &Ctx, message: &str) -> Option<Outcome> {
    if !printable(message) {
        return None;
    }
    under_lock(ctx, || ())?;
    project_error(ctx, message)
}

/// `projects.label`'s check of printable ASCII text: the stripped text, or
/// the error when it is longer than `limit`.
fn label(text: &str, what: &str, limit: usize) -> Option<Result<String, String>> {
    if !printable(text) {
        return None;
    }
    let value = text.trim_matches(' ');
    Some(if value.len() > limit {
        Err(format!("{what} is one line of at most {limit} characters"))
    } else {
        Ok(value.to_owned())
    })
}

fn project_list(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if args.flag("active") && args.flag("all") {
        return None;
    }
    let vault = vault_context()?;
    let store = vault.store();
    let settings = read_map(&store, &ctx.root.join("settings.yaml"))?;
    if !clock_safe(&settings) {
        return None;
    }
    if !(ctx.root.join("work").is_dir() || absent(&ctx.root.join("work"))) {
        return None;
    }
    if !load_projects(&store, &ctx.root)?.is_empty() {
        return None;
    }
    make_dirs(&ctx.root, &["work"])?;
    if args.flag("json") {
        return said("[]\n".to_owned(), 0);
    }
    said(
        "no projects yet: careerbrew work project add <name>\n".to_owned(),
        0,
    )
}

fn project_rules(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let name = printable_name(args)?;
    let vault = vault_context()?;
    let store = vault.store();
    if !(ctx.root.join("work").is_dir() || absent(&ctx.root.join("work"))) {
        return None;
    }
    let every = load_projects(&store, &ctx.root)?;
    make_dirs(&ctx.root, &["work"])?;
    if let Some(message) = unknown_text(&every, name)? {
        return project_error(ctx, &message);
    }
    let record = plain_map(&every.iter().find(|(n, _)| n == name)?.1)?;
    let rules = match get(record, "standing_rules") {
        None => Some(""),
        Some(Canon::Str(text)) => Some(text.as_str()),
        Some(Canon::Null) => None,
        Some(_) => return None,
    };
    if rules.is_some_and(|text| !text.is_empty()) {
        // `may_show_business()`: a terminal shows them; Python prints that.
        if ctx.env.stdout_is_terminal {
            return None;
        }
        return said(
            "standing rules are business data; reading them requires a terminal, owner UI, or plugin read permission\n".to_owned(),
            3,
        );
    }
    if args.flag("json") {
        let shown = if rules.is_some() { "\"\"" } else { "null" };
        return said(format!("{{\"rules\": {shown}}}\n"), 0);
    }
    said("no standing rules\n".to_owned(), 0)
}

fn project_rules_set(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    // The required mutually exclusive `--text` / `--clear`: argparse
    // rejects none or both.
    let text = args.text("text");
    if text.is_some() == args.flag("clear") {
        return None;
    }
    let name = printable_name(args)?;
    if !args.flag("yes") {
        return said(
            "standing rules were not changed; rerun with --yes to confirm replacement\n".to_owned(),
            1,
        );
    }
    let text = text.unwrap_or("");
    // `set_rules` loads first, then evaluates `label(...)`, then `_get`.
    let labelled = label(text, "standing rules", 1000)?;
    writer_error(ctx, |every| {
        if let Err(message) = &labelled {
            return Some(message.clone());
        }
        unknown(every, name)
    })
}

fn project_remove(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let name = printable_name(args)?;
    if !args.flag("yes") {
        return said(
            format!("not removed: this deletes the record of {name} (its memory file and tasks stay); rerun with --yes to confirm, or keep it with: careerbrew work project archive {name}\n"),
            2,
        );
    }
    writer_error(ctx, |every| unknown(every, name))
}

const UPDATE_KINDS: &[&str] = &["company", "project", "team", "other", "approval"];

fn project_update(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let name = printable_name(args)?;
    let kind = args.text("kind")?;
    let text = args.text("text")?;
    if !printable(kind) {
        return None;
    }
    if !UPDATE_KINDS.contains(&kind) {
        return writer_early_error(
            ctx,
            &format!(
                "unknown update kind {kind}; use one of {}",
                UPDATE_KINDS.join(", ")
            ),
        );
    }
    match label(text, "an update", 200)? {
        Err(message) => writer_early_error(ctx, &message),
        Ok(value) if value.is_empty() => {
            writer_early_error(ctx, "an update needs one line of text")
        }
        Ok(_) => writer_error(ctx, |every| unknown(every, name)),
    }
}

const SOURCE_KINDS: &[&str] = &["doc", "tracker", "repo", "chat", "memory", "url"];

fn project_source(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    printable_name(args)?;
    let kind = args.text("kind")?;
    args.text("ref")?;
    if !printable(kind) || SOURCE_KINDS.contains(&kind) {
        return None;
    }
    writer_early_error(
        ctx,
        &format!(
            "unknown source kind {kind}; use one of {}",
            SOURCE_KINDS.join(", ")
        ),
    )
}

fn project_check(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let name = printable_name(args)?;
    if !args.flag("since") || !args.text("status").is_some_and(|s| !s.is_empty()) {
        return None;
    }
    project_error(ctx, &format!("--since stamps nothing, so it takes no --status (fix: read what changed, then `careerbrew work project check {name} <ref> --status ...`)"))
}

/// Python's `format(x, "g")` for a plain decimal, when Rust's shortest form
/// is the same text: at most 6 significant digits, no exponent form.
fn fmt_g(text: &str) -> Option<String> {
    let value: f64 = text.parse().ok()?;
    if value == 0.0 {
        return Some("0".to_owned());
    }
    if !(1e-4..1e6).contains(&value.abs()) {
        return None;
    }
    let shown = value.to_string();
    let significant = shown
        .chars()
        .filter(char::is_ascii_digit)
        .skip_while(|c| *c == '0')
        .count();
    (significant <= 6).then_some(shown)
}

fn project_prd(args: &Parsed) -> Option<Outcome> {
    printable_name(args)?;
    if args.text("check").is_some_and(|c| !c.is_empty()) {
        return None;
    }
    let Value::Number(slot) = args.get("slot") else {
        return None;
    };
    let value: f64 = slot.parse().ok()?;
    if 0.0 < value && value <= 520.0 {
        return None;
    }
    // An uncaught `SystemExit(text)`: the text on stderr, exit 1, no row.
    Some(Outcome {
        stdout: String::new(),
        stderr: format!(
            "error: --slot is the plan slot in dev weeks, more than 0 and at most 520 (got {})\n",
            fmt_g(slot)?
        ),
        code: 1,
        emit_event: false,
    })
}

// ---------- checklist ----------

/// id-free copy of `checklist.STEPS`: title, why, view, command.
const STEPS: &[(&str, &str, &str, &str)] = &[
    ("Connect a work tool", "careerbrew reads your commits, PRs, tickets, and messages from it, so the log and the suggestions fill in", "#/setup", "careerbrew connect"),
    ("Turn on the day watcher", "it keeps Today, the day log, and the suggestions fresh while you work", "#/setup", "careerbrew day watch on"),
    ("Add your first task", "the planner fits it into your day and Next step names it", "#/week", "careerbrew work task \"<title>\" --hours 1"),
    ("Set a goal", "Teach me and the plan work toward it", "#/goals", "careerbrew goals set \"<title>\" --date <YYYY-MM-DD>"),
    ("Accept or dismiss one suggestion", "a suggestion changes nothing until you decide; Today explains each one", "#/today", "careerbrew day inbox"),
    ("Say where your PM update goes", "Copy update and Send use it", "#/setup", "careerbrew config set pm_slack <#channel>"),
];

/// `config.get(key)` truthiness, from the stored settings or the default.
fn setting_truthy(settings: &Pairs, key: &str, default: bool) -> bool {
    get(settings, key).map_or(default, truthy)
}

fn checklist(ctx: &Ctx) -> Option<Outcome> {
    // The first five steps are done only when their files say so; served
    // only when none of those files exists, so each reads as not done.
    for rel in [
        "work/sources.yaml",
        "work/connectors.yaml",
        "day/watch.json",
        "work/tasks.yaml",
        "goals.yaml",
        "day/inbox.json",
    ] {
        if !absent(&ctx.root.join(rel)) {
            return None;
        }
    }
    // `evidence.sources_list` also reads the repo's base sources folder.
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../knowledge/evidence_sources");
    if !super::rust18::same_repo() || !absent(&base) {
        return None;
    }
    for dir in ["work", "day"] {
        let path = ctx.root.join(dir);
        if !(path.is_dir() || absent(&path)) {
            return None;
        }
    }
    let vault = vault_context()?;
    let store = vault.store();
    let settings = read_map(&store, &ctx.root.join("settings.yaml"))?;
    // `goals.load` reads `calendar.now()`.
    if !clock_safe(&settings) {
        return None;
    }
    make_dirs(&ctx.root, &["work"])?;
    make_dirs(&ctx.root, &["day"])?;
    let pm = setting_truthy(&settings, "pm_slack", false)
        || setting_truthy(&settings, "pm_email", false);
    let hidden = !setting_truthy(&settings, "first_run_checklist", true);
    let done: Vec<bool> = (0..STEPS.len()).map(|i| i == 5 && pm).collect();
    let count = done.iter().filter(|d| **d).count();
    let mut out = format!(
        "getting started: {count} of {} done{}\n",
        STEPS.len(),
        if hidden {
            " (hidden on Dashboard: first_run_checklist is false)"
        } else {
            ""
        }
    );
    for ((title, why, view, command), done) in STEPS.iter().zip(&done) {
        if *done {
            out.push_str(&format!("[x] {title}\n"));
        } else {
            let view = view.strip_prefix("#/").unwrap_or(view);
            out.push_str(&format!(
                "[ ] {title}: {command} (or {view} in the web app); {why}\n"
            ));
        }
    }
    if !hidden && count < STEPS.len() {
        out.push_str("hide it on Dashboard: careerbrew config set first_run_checklist false\n");
    }
    said(out, 0)
}

// ---------- time baseline (settings helpers copied from rust05) ----------

fn time_baseline(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let action = args.text("action")?;
    let raw = args.text("minutes")?;
    if !printable(action) || !printable(raw) {
        return None;
    }
    let key = format!("time_baseline_{action}");
    let vault = vault_context()?;
    let store = vault.store();
    // Python validates before it takes the lock or reads the file.
    let empty = Canon::Map(Vec::new());
    match set_plan(&empty, &key, raw)? {
        SetPlan::Raised("ValueError", message) => {
            return cli_error(ctx, "time.baseline", "valueerror", &message)
        }
        SetPlan::Raised(..) => return None,
        SetPlan::Write(..) => {}
    }
    // A sealed write differs on every run; leave it to Python.
    if store.encryption_enabled() {
        return None;
    }
    let plan = |store: &Store| -> Option<(Canon, String)> {
        let settings = stored_settings(store)?;
        match set_plan(&settings, &key, raw)? {
            SetPlan::Write(value, written) => Some((value, written_yaml(&written)?)),
            SetPlan::Raised(..) => None,
        }
    };
    let (value, _) = plan(&store)?;
    let Canon::Int(minutes) = &value else {
        return None;
    };
    let minutes = minutes.to_string();
    let line = if minutes == "0" {
        format!("baseline {action}: back to the labeled estimate\n")
    } else {
        format!("baseline {action}: {minutes} min (measured)\n")
    };
    let lock = ctx.root.join(".settings.yaml.lock");
    careerbrew_vault::lock::with_exact_lock(&lock, || -> Option<()> {
        let (_, text) = plan(&store)?;
        store
            .write_text(&ctx.root.join("settings.yaml"), &text)
            .ok()
    })
    .ok()??;
    emit(ctx, "config.set", &[("status", "ok")]);
    said(line, 0)
}

/// A plain scalar PyYAML writes unquoted and reads back as the same string.
fn plain(text: &str) -> bool {
    const WORDS: &[&str] = &["y", "n", "yes", "no", "true", "false", "on", "off", "null"];
    let mut chars = text.chars();
    text.len() <= 60
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !WORDS.contains(&text.to_ascii_lowercase().as_str())
}

fn int_text(value: &Canon) -> Option<String> {
    let Canon::Int(n) = value else {
        return None;
    };
    let text = n.to_string();
    let digits = text.strip_prefix('-').unwrap_or(&text);
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some(text)
}

/// `HH:MM` in ASCII digits, the form the `_hhmm` validator accepts.
fn hhmm(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 5
        && match b[0] {
            b'0' | b'1' => b[1].is_ascii_digit(),
            b'2' => (b'0'..=b'3').contains(&b[1]),
            _ => false,
        }
        && b[2] == b':'
        && (b'0'..=b'5').contains(&b[3])
        && b[4].is_ascii_digit()
}

/// The exact spellings PyYAML's YAML 1.1 bool and null resolvers match.
const RESOLVED_WORDS: &[&str] = &[
    "yes", "Yes", "YES", "no", "No", "NO", "true", "True", "TRUE", "false", "False", "FALSE", "on",
    "On", "ON", "off", "Off", "OFF", "null", "Null", "NULL",
];

fn scalar(value: &Canon) -> Option<String> {
    match value {
        Canon::Str(s) if hhmm(s) && s.starts_with(['1', '2']) => Some(format!("'{s}'")),
        Canon::Str(s) if hhmm(s) => Some(s.clone()),
        Canon::Str(s) if RESOLVED_WORDS.contains(&s.as_str()) => Some(format!("'{s}'")),
        Canon::Null => Some("null".into()),
        Canon::Bool(b) => Some(if *b { "true" } else { "false" }.into()),
        Canon::Int(_) => int_text(value),
        Canon::Str(s) if s.is_empty() => Some("''".into()),
        Canon::Str(s) if plain(s) => Some(s.clone()),
        _ => None,
    }
}

/// `yaml.safe_dump(data, sort_keys=False, allow_unicode=True)` for a map of
/// identifier keys whose values are null, booleans, integers, empty or
/// plain strings, or lists of those. Anything else returns None.
fn settings_yaml(items: &[(&str, &Canon)]) -> Option<String> {
    if items.is_empty() {
        return Some("{}\n".into());
    }
    let mut out = String::new();
    for (key, value) in items {
        let mut chars = key.chars();
        let key_ok = key.len() <= 40
            && chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && plain(key);
        if !key_ok {
            return None;
        }
        match value {
            Canon::List(list) if list.is_empty() => out.push_str(&format!("{key}: []\n")),
            Canon::List(list) => {
                out.push_str(&format!("{key}:\n"));
                for item in list {
                    if matches!(item, Canon::Str(s) if s.is_empty()) {
                        return None;
                    }
                    out.push_str(&format!("- {}\n", scalar(item)?));
                }
            }
            _ => out.push_str(&format!("{key}: {}\n", scalar(value)?)),
        }
    }
    Some(out)
}

/// One answer of the rust-02 `settings` vector set for `op` over the stored
/// settings, in the typed `{t, v}` form.
fn settings_op(op: &str, settings: &Canon, extra: &str) -> Option<Canon> {
    let mut text = String::from("{\"vectors\": [{\"id\": \"native\", \"op\": ");
    crate::render::py_string(op, &mut text);
    text.push_str(", \"input\": {\"settings\": ");
    text.push_str(&settings.to_json().to_string());
    text.push_str(extra);
    text.push_str("}}]}");
    let fixture = text.parse().ok()?;
    let answer = careerbrew_purevec::run("settings", &fixture).ok()?;
    let output = &answer["results"][0]["output"]["ok"];
    let parsed = crate::registry::parse(&output.to_string()).ok()?;
    from_answer(&parsed)
}

fn from_answer(value: &crate::registry::Json) -> Option<Canon> {
    use crate::registry::Json;
    if let Some(Json::Str(kind)) = value.get("t") {
        let text = value.get("v");
        return Some(match (kind.as_str(), text) {
            ("null", _) => Canon::Null,
            ("bool", Some(Json::Bool(b))) => Canon::Bool(*b),
            ("int", Some(Json::Str(n))) => Canon::Int(n.parse().ok()?),
            ("str", Some(Json::Str(s))) => Canon::Str(s.clone()),
            ("list", Some(Json::Array(items))) => {
                Canon::List(items.iter().map(from_answer).collect::<Option<_>>()?)
            }
            ("map", Some(Json::Array(pairs))) => Canon::Map(
                pairs
                    .iter()
                    .map(|pair| match pair.as_array()? {
                        [k, v] => Some((from_answer(k)?, from_answer(v)?)),
                        _ => None,
                    })
                    .collect::<Option<_>>()?,
            ),
            _ => return None,
        });
    }
    match value {
        Json::Array(items) => Some(Canon::List(
            items.iter().map(from_answer).collect::<Option<_>>()?,
        )),
        Json::Object(pairs) => Some(Canon::Map(
            pairs
                .iter()
                .map(|(k, v)| Some((Canon::Str(k.clone()), from_answer(v)?)))
                .collect::<Option<_>>()?,
        )),
        Json::Str(s) => Some(Canon::Str(s.clone())),
        Json::Bool(b) => Some(Canon::Bool(*b)),
        Json::Null => Some(Canon::Null),
        Json::Number(_) => None,
    }
}

fn answer_text<'a>(map: &'a Canon, key: &str) -> Option<&'a str> {
    match map {
        Canon::Map(items) => match items
            .iter()
            .find(|(k, _)| matches!(k, Canon::Str(k) if k == key))
            .map(|(_, v)| v)?
        {
            Canon::Str(s) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

fn answer_get<'a>(map: &'a Canon, key: &str) -> Option<&'a Canon> {
    match map {
        Canon::Map(items) => items
            .iter()
            .find(|(k, _)| matches!(k, Canon::Str(k) if k == key))
            .map(|(_, v)| v),
        _ => None,
    }
}

/// The stored settings, when they are a map of distinct string keys.
fn stored_settings(store: &Store) -> Option<Canon> {
    let data = careerbrew_vault::settings::read_settings(store).ok()?;
    plain_map(&data)?;
    Some(data)
}

/// What `config.set_value` does, from the rust-02 `set` answer.
enum SetPlan {
    /// `KeyError` or `ValueError` with its message: no lock, no read.
    Raised(&'static str, String),
    /// The parsed value, and the settings map to write.
    Write(Canon, Canon),
}

fn set_plan(settings: &Canon, key: &str, raw: &str) -> Option<SetPlan> {
    let mut extra = String::from(", \"key\": ");
    crate::render::py_string(key, &mut extra);
    extra.push_str(", \"raw\": ");
    crate::render::py_string(raw, &mut extra);
    let answer = settings_op("set", settings, &extra)?;
    if let Some(class) = answer_text(&answer, "raised") {
        let class = match class {
            "KeyError" => "KeyError",
            "ValueError" => "ValueError",
            _ => return None,
        };
        return Some(SetPlan::Raised(
            class,
            answer_text(&answer, "message")?.to_owned(),
        ));
    }
    let value = answer_get(&answer, "value")?.clone();
    let written = answer_get(&answer, "written")?.clone();
    Some(SetPlan::Write(value, written))
}

/// The settings.yaml text for a written map, in `settings_yaml`'s subset.
fn written_yaml(written: &Canon) -> Option<String> {
    let Canon::Map(items) = written else {
        return None;
    };
    let pairs: Vec<(&str, &Canon)> = items
        .iter()
        .map(|(k, v)| match k {
            Canon::Str(k) => Some((k.as_str(), v)),
            _ => None,
        })
        .collect::<Option<_>>()?;
    settings_yaml(&pairs)
}
