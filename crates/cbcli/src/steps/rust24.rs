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
//!   out of range (Python's `SystemExit`);
//! - wave 4 writers in a plaintext data folder, saved with
//!   `pyyaml::safe_dump` under the project lock: `next` (`link_task`),
//!   `reference` for a ref saved literally, `memory` for an empty path or
//!   a path that names no folder or `MEMORY.md` index, and `set` with only
//!   `--title` and `--group` when the project graph has no problem; each
//!   one's `ProjectError` lines too;
//! - `work project explain`: the outline of a project with no source, the
//!   unknown-project error, and `--check` of an ASCII file outside the data
//!   root;
//! - wave 6: `metrics time-saved` (text and `--json`) when every event row
//!   and baseline setting it reads has a shape Python reads the same way; it
//!   writes `out/time-saved.json` (a plaintext-by-design folder) only when
//!   the text changed, as `store.write_json_if_changed` does;
//! - wave 6: `cards publish --dry-run`, and the `PublishError` lines of a
//!   card that fails the bar (exit 1). Both write nothing but the `tutor/`
//!   folder Python's `paths.sub` makes. A publish that would write the
//!   public deck goes to Python: it dumps YAML at width 100, which
//!   `pyyaml::safe_dump` does not port.
//!
//! - wave 7: the `ProjectError` lines of `work project add` (a new record
//!   holds a random id, so a successful add goes to Python); `work project
//!   show` of a project that is not a prep project, whose sources have no
//!   link (`://`) and whose ages are whole tenths of a day; `work project
//!   sync` when no project has sources or a memory file; `drill progress`
//!   before any card, attempt or readiness row exists.
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
    "work.project.next",
    "work.project.reference",
    "work.project.memory",
    "work.project.set",
    "work.project.explain",
    "metrics.time-saved",
    "cards.publish",
    "work.project.add",
    "work.project.show",
    "work.project.sync",
    "drill.progress",
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
        "work.project.next" => project_next(args, ctx),
        "work.project.reference" => project_reference(args, ctx),
        "work.project.memory" => project_memory(args, ctx),
        "work.project.set" => project_set(args, ctx),
        "work.project.explain" => project_explain(args, ctx),
        "metrics.time-saved" => time_saved(args, ctx),
        "cards.publish" => cards_publish(args, ctx),
        "work.project.add" => project_add(args, ctx),
        "work.project.show" => project_show(args, ctx),
        "work.project.sync" => project_sync(ctx),
        "drill.progress" => drill_progress(ctx),
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
    let rendered: Vec<(&str, String)> = fields
        .iter()
        .map(|(key, value)| (*key, quoted(value)))
        .collect();
    emit_json(ctx, event, &rendered);
}

/// A string in `json.dumps` form.
fn quoted(text: &str) -> String {
    let mut out = String::new();
    crate::render::py_string(text, &mut out);
    out
}

/// `emit` with values already in `json.dumps` form (an int field).
fn emit_json(ctx: &Ctx, event: &str, fields: &[(&str, String)]) {
    if !ctx.env.caps.events_on {
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut pairs: Vec<(&str, String)> = vec![
        ("event", quoted(event)),
        ("ts", quoted(&events::stamp(secs))),
        ("v", quoted(events::version())),
    ];
    pairs.extend(fields.iter().map(|(key, value)| (*key, value.clone())));
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    let mut line = String::from("{");
    for (i, (key, value)) in pairs.iter().enumerate() {
        if i > 0 {
            line.push_str(", ");
        }
        crate::render::py_string(key, &mut line);
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

// ---------- project writers: next, reference, memory, set ----------

/// What one `@_writes` writer does with the records it loaded.
#[derive(PartialEq)]
enum Change {
    /// `ProjectError` with its message: nothing is saved.
    Error(String),
    /// The `projects.yaml` text `_save` writes, the lines `cmd_work_project`
    /// prints, and the fields of the `work.project` row the writer emits.
    Save {
        yaml: String,
        out: String,
        event: Option<Vec<(&'static str, String)>>,
    },
}

/// Runs a writer the way `@_writes` does: decide before any write (a
/// refusal leaves the folder alone), then take the lock, decide again from
/// the records read under it, and save only when both agree.
fn project_write(ctx: &Ctx, plan: impl Fn(&Store) -> Option<Change>) -> Option<Outcome> {
    let vault = vault_context()?;
    let store = vault.store();
    if !(ctx.root.join("work").is_dir() || absent(&ctx.root.join("work"))) {
        return None;
    }
    let first = plan(&store)?;
    match &first {
        Change::Error(message) if !printable(message) => return None,
        // A sealed write differs on every run; leave it to Python.
        Change::Save { .. } if store.encryption_enabled() => return None,
        _ => {}
    }
    let file = ctx.root.join("work").join("projects.yaml");
    let done = under_lock(ctx, || -> Option<Change> {
        let again = plan(&store)?;
        if again != first {
            return None;
        }
        if let Change::Save { yaml, .. } = &again {
            store.write_text(&file, yaml).ok()?;
        }
        Some(again)
    })??;
    match done {
        Change::Error(message) => project_error(ctx, &message),
        Change::Save { out, event, .. } => {
            if let Some(fields) = event {
                emit_json(ctx, "work.project", &fields);
            }
            said(out, 0)
        }
    }
}

/// `store.write_yaml(path(), {"projects": projects})`.
fn save_yaml(every: &[(String, Canon)]) -> Option<String> {
    let projects = every
        .iter()
        .map(|(name, record)| (Canon::Str(name.clone()), record.clone()))
        .collect();
    crate::pyyaml::safe_dump(&Canon::Map(vec![(
        Canon::Str("projects".into()),
        Canon::Map(projects),
    )]))
}

/// The named record as a map of distinct string keys, to change in place.
fn record_mut<'a>(every: &'a mut [(String, Canon)], name: &str) -> Option<&'a mut Pairs> {
    let record = &mut every.iter_mut().find(|(n, _)| n == name)?.1;
    plain_map(record)?;
    match record {
        Canon::Map(pairs) => Some(pairs),
        _ => None,
    }
}

fn get_mut<'a>(pairs: &'a mut Pairs, key: &str) -> Option<&'a mut Canon> {
    pairs
        .iter_mut()
        .find(|(k, _)| matches!(k, Canon::Str(k) if k == key))
        .map(|(_, v)| v)
}

/// `p[key] = value`: a key already there keeps its place.
fn put(pairs: &mut Pairs, key: &str, value: Canon) {
    match get_mut(pairs, key) {
        Some(slot) => *slot = value,
        None => pairs.push((Canon::Str(key.into()), value)),
    }
}

fn pop(pairs: &mut Pairs, key: &str) {
    pairs.retain(|(k, _)| !matches!(k, Canon::Str(k) if k == key));
}

/// `projects._put`: an empty value leaves the record.
fn put_opt(pairs: &mut Pairs, key: &str, value: &str) {
    if value.is_empty() {
        pop(pairs, key);
    } else {
        put(pairs, key, Canon::Str(value.into()));
    }
}

/// The task ids `priorities.load()` reads, when every task is a map with
/// a string id and no old `link` field (`_migrate` would rewrite it).
fn task_ids(store: &Store, root: &Path) -> Option<Vec<String>> {
    let top = read_map(store, &root.join("work").join("tasks.yaml"))?;
    let tasks = match get(&top, "tasks") {
        None => return Some(Vec::new()),
        Some(Canon::List(items)) => items,
        Some(_) => return None,
    };
    tasks
        .iter()
        .map(|task| {
            let pairs = plain_map(task)?;
            if get(pairs, "link").is_some() {
                return None;
            }
            match get(pairs, "id") {
                Some(Canon::Str(id)) => Some(id.clone()),
                _ => None,
            }
        })
        .collect()
}

fn nonempty_text<'a>(args: &'a Parsed, key: &str) -> Option<&'a str> {
    args.text(key).filter(|t| !t.is_empty() && printable(t))
}

/// `work project next <name> <task>`: `projects.link_task`.
fn project_next(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let name = printable_name(args)?;
    let task = nonempty_text(args, "task")?;
    project_write(ctx, |store| {
        let ids = task_ids(store, &ctx.root)?;
        if !ids.iter().any(|id| id == task) {
            return Some(Change::Error(format!(
                "unknown task {task} (fix: `careerbrew work task \"<title>\" --hours <h>` first)"
            )));
        }
        let mut every = load_projects(store, &ctx.root)?;
        if let Some(message) = unknown_text(&every, name)? {
            return Some(Change::Error(message));
        }
        let record = record_mut(&mut every, name)?;
        let Some(Canon::List(tasks)) = get_mut(record, "tasks") else {
            return None;
        };
        let id = Canon::Str(task.into());
        if !tasks.contains(&id) {
            tasks.push(id);
        }
        Some(Change::Save {
            yaml: save_yaml(&every)?,
            out: format!("next step {task} linked to {name}\n"),
            event: None,
        })
    })
}

/// `work project reference <name> <ref> [--off]`: `projects.set_reference`
/// for a ref saved literally (an alias match goes to Python).
fn project_reference(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let name = printable_name(args)?;
    let wanted = nonempty_text(args, "ref")?;
    let off = args.flag("off");
    project_write(ctx, |store| {
        let mut every = load_projects(store, &ctx.root)?;
        if let Some(message) = unknown_text(&every, name)? {
            return Some(Change::Error(message));
        }
        let record = record_mut(&mut every, name)?;
        let Some(Canon::List(sources)) = get_mut(record, "sources") else {
            return None;
        };
        let target = Canon::Str(wanted.into());
        let mut count = 0usize;
        for source in sources.iter() {
            // `s["ref"]` for every source: a missing one is a KeyError.
            if *get(plain_map(source)?, "ref")? == target {
                count += 1;
            }
        }
        if count == 0 {
            return None;
        }
        for source in sources.iter_mut() {
            if let Canon::Map(pairs) = source {
                if get(pairs, "ref") == Some(&target) {
                    put(pairs, "reference", Canon::Bool(!off));
                    pop(pairs, "stale_reason");
                }
            }
        }
        let out = if off {
            format!("{name}: {wanted} is tracked again: it goes stale after the project's limit (read it, then careerbrew work project check {name} {wanted})\n")
        } else {
            format!("{name}: {wanted} is a reference now: careerbrew never reads it and it never goes stale\n")
        };
        Some(Change::Save {
            yaml: save_yaml(&every)?,
            out,
            event: Some(vec![
                ("action", quoted("reference")),
                ("count", count.to_string()),
                ("status", quoted("ok")),
            ]),
        })
    })
}

/// What `projects.memory_path` returns for a path it keeps as typed: the
/// empty path, or one that names no folder, no `MEMORY.md` index, and that
/// `pathlib` prints unchanged. None for every other path.
fn memory_value(path: &str) -> Option<String> {
    if path.is_empty() {
        return Some(String::new());
    }
    let odd = path.contains("//")
        || path.contains("/./")
        || path.starts_with("./")
        || path.ends_with("/.")
        || path.ends_with('/')
        || path == "."
        || path.starts_with('~');
    if odd || !printable(path) {
        return None;
    }
    let last = path.rsplit('/').next()?;
    if last == "MEMORY.md" || last == ".." || Path::new(path).is_dir() {
        return None;
    }
    Some(path.to_owned())
}

/// `work project memory <name> <path>`: `projects.set_memory`.
fn project_memory(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let name = printable_name(args)?;
    let memory = memory_value(args.text("path")?)?;
    project_write(ctx, |store| {
        let mut every = load_projects(store, &ctx.root)?;
        if let Some(message) = unknown_text(&every, name)? {
            return Some(Change::Error(message));
        }
        put(
            record_mut(&mut every, name)?,
            "memory",
            Canon::Str(memory.clone()),
        );
        Some(Change::Save {
            yaml: save_yaml(&every)?,
            out: format!("memory for {name} set\n"),
            event: None,
        })
    })
}

/// A record's `parent`, as `p.get("parent") or ""`, when it is a string.
fn parent_of(record: &Canon) -> Option<String> {
    match get(plain_map(record)?, "parent") {
        None => Some(String::new()),
        Some(value) if !truthy(value) => Some(String::new()),
        Some(Canon::Str(parent)) => Some(parent.clone()),
        Some(_) => None,
    }
}

/// A record's `links`, as `p.get("links") or []`, when it is a list of
/// strings.
fn links_of(record: &Canon) -> Option<Vec<String>> {
    match get(plain_map(record)?, "links") {
        None => Some(Vec::new()),
        Some(value) if !truthy(value) => Some(Vec::new()),
        Some(Canon::List(items)) => items
            .iter()
            .map(|item| match item {
                Canon::Str(link) => Some(link.clone()),
                _ => None,
            })
            .collect(),
        Some(_) => None,
    }
}

/// True when `projects.graph_problems` finds nothing; None when a record
/// holds a parent or links Python would read differently.
fn graph_clean(every: &[(String, Canon)]) -> Option<bool> {
    let names: Vec<&str> = every.iter().map(|(n, _)| n.as_str()).collect();
    let record = |name: &str| every.iter().find(|(n, _)| n == name).map(|(_, r)| r);
    for (name, rec) in every {
        let parent = parent_of(rec)?;
        if parent == *name || (!parent.is_empty() && !names.contains(&parent.as_str())) {
            return Some(false);
        }
        for link in links_of(rec)? {
            if link == *name || !names.contains(&link.as_str()) {
                return Some(false);
            }
        }
    }
    for (name, rec) in every {
        let mut chain = vec![name.clone()];
        let mut cur = parent_of(rec)?;
        while names.contains(&cur.as_str()) && !chain.contains(&cur) {
            chain.push(cur.clone());
            cur = parent_of(record(&cur)?)?;
        }
        if chain.contains(&cur) && chain.last() != Some(&cur) {
            return Some(false);
        }
    }
    Some(true)
}

/// `work project set <name> [--title] [--group]`: `projects.edit` with no
/// parent, link, kind, event day or quiet change (those go to Python).
fn project_set(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    for key in [
        "parent",
        "link",
        "unlink",
        "kind",
        "event_on",
        "quiet_until",
    ] {
        match args.get(key) {
            Value::Missing => {}
            Value::List(items) if items.is_empty() => {}
            _ => return None,
        }
    }
    let name = printable_name(args)?;
    let title = args.text("title");
    let group = args.text("group");
    if [title, group].iter().flatten().any(|text| !printable(text)) {
        return None;
    }
    project_write(ctx, |store| {
        let mut every = load_projects(store, &ctx.root)?;
        if let Some(message) = unknown_text(&every, name)? {
            return Some(Change::Error(message));
        }
        let record = record_mut(&mut every, name)?;
        for (value, key, limit) in [(title, "title", 200), (group, "group", 60)] {
            let Some(value) = value else { continue };
            let stored = if value == "none" {
                String::new()
            } else {
                match label(value, key, limit)? {
                    Ok(text) => text,
                    Err(message) => return Some(Change::Error(message)),
                }
            };
            put_opt(record, key, &stored);
        }
        // `_put(p, "links", links)`: an empty value leaves the record.
        match get(record, "links") {
            Some(value) if !truthy(value) => pop(record, "links"),
            Some(Canon::List(items)) if items.iter().all(|i| matches!(i, Canon::Str(_))) => {}
            None => {}
            Some(_) => return None,
        }
        let prep = matches!(get(record, "kind"), Some(Canon::Str(kind)) if kind == "prep");
        if get(record, "event_on").is_some_and(truthy) && !prep {
            return Some(Change::Error(
                "only a prep project has an event day (fix: add --kind prep)".into(),
            ));
        }
        let shown = |key: &str| -> Option<String> {
            match get(record, key) {
                None => Some("none".into()),
                Some(value) if !truthy(value) => Some("none".into()),
                Some(Canon::Str(text)) if printable(text) => Some(text.clone()),
                Some(_) => None,
            }
        };
        let parent = shown("parent")?;
        let group = shown("group")?;
        let links = match get(record, "links") {
            Some(Canon::List(items)) => items
                .iter()
                .map(|item| match item {
                    Canon::Str(link) if printable(link) => Some(link.as_str()),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?
                .join(", "),
            _ => String::new(),
        };
        let links = if links.is_empty() {
            "none".into()
        } else {
            links
        };
        if !graph_clean(&every)? {
            return None;
        }
        Some(Change::Save {
            yaml: save_yaml(&every)?,
            out: format!("{name}: parent {parent}, links {links}, group {group}\n"),
            event: Some(vec![("action", quoted("set")), ("status", quoted("ok"))]),
        })
    })
}

// ---------- work project explain ----------

/// `projects.valid_name`: the shape of every project name `add` accepts.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 40
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `explain.scaffold` for a project with no source: the headings only.
fn scaffold_text(name: &str, short: bool, deep: bool) -> String {
    use careerbrew_learn::explain::{DEEP, FULL, SHORT};
    let headings = if short {
        SHORT
    } else if deep {
        DEEP
    } else {
        FULL
    };
    let mut lines = vec![
        format!("# {name}: explained"),
        String::new(),
        format!("<!-- reading list from `careerbrew work project show {name}`: re-read each source now and record the date or sha -->"),
        String::new(),
    ];
    for heading in headings {
        lines.extend([
            format!("## {heading}"),
            String::new(),
            "- TODO".into(),
            String::new(),
        ]);
    }
    lines.truncate(lines.len() - 2);
    lines.push("| Id | Kind | Locator | Read |".into());
    lines.push("|---|---|---|---|".into());
    lines.join("\n") + "\n"
}

fn project_explain(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let name = printable_name(args)?;
    let (short, deep) = (args.flag("short"), args.flag("deep"));
    if short && deep {
        return None;
    }
    match args.text("check") {
        Some(check) if !check.is_empty() => explain_check(ctx, check, short, deep),
        _ => explain_scaffold(ctx, name, short, deep),
    }
}

/// `explain --check <file>` for a file outside the data root (Python reads
/// it with `Path.read_text`): ASCII text with no carriage return only.
fn explain_check(ctx: &Ctx, check: &str, short: bool, deep: bool) -> Option<Outcome> {
    if !printable(check) {
        return None;
    }
    let file = Path::new(check);
    if !std::fs::metadata(file).ok()?.is_file() {
        return None;
    }
    let real = std::fs::canonicalize(file).ok()?;
    let root = std::fs::canonicalize(&ctx.root).ok()?;
    if real.starts_with(&root) {
        return None;
    }
    let text = String::from_utf8(std::fs::read(&real).ok()?).ok()?;
    if !text.is_ascii() || text.contains('\r') {
        return None;
    }
    let problems = careerbrew_learn::explain::check(&text, short, deep);
    if !problems.iter().all(|p| printable(p)) {
        return None;
    }
    let mut out = String::new();
    for problem in &problems {
        out.push_str(&format!("problem: {problem}\n"));
    }
    if problems.is_empty() {
        out.push_str("explain check: pass\n");
    } else {
        out.push_str(&format!("explain check: {} problem(s)\n", problems.len()));
    }
    let status = if problems.is_empty() { "ok" } else { "fail" };
    emit_json(
        ctx,
        "work.explain",
        &[
            ("problems", problems.len().to_string()),
            ("status", quoted(status)),
        ],
    );
    said(out, i32::from(!problems.is_empty()))
}

/// `explain <name>`: the outline of a project with no source yet, or the
/// unknown-project error `projects.show` raises.
fn explain_scaffold(ctx: &Ctx, name: &str, short: bool, deep: bool) -> Option<Outcome> {
    if !valid_name(name) {
        return None;
    }
    // The printed path is `paths.sub("out")`: the data root as given.
    let root_text = ctx.root.to_str()?;
    let given = std::env::var("CAREERBREW_HOME").ok()?;
    let odd = root_text.contains("//")
        || root_text.contains("/./")
        || root_text.ends_with("/.")
        || root_text.ends_with('/');
    if given != root_text || odd || !printable(root_text) {
        return None;
    }
    let vault = vault_context()?;
    let store = vault.store();
    let settings = read_map(&store, &ctx.root.join("settings.yaml"))?;
    if !clock_safe(&settings) {
        return None;
    }
    for dir in ["out", "work"] {
        let path = ctx.root.join(dir);
        if !(path.is_dir() || absent(&path)) {
            return None;
        }
    }
    let every = load_projects(&store, &ctx.root)?;
    if let Some(message) = unknown_text(&every, name)? {
        if !printable(&message) {
            return None;
        }
        make_dirs(&ctx.root, &["out"])?;
        make_dirs(&ctx.root, &["work"])?;
        return project_error(ctx, &message);
    }
    // `projects.show`: every field it reads must read as Python reads it.
    let record = plain_map(&every.iter().find(|(n, _)| n == name)?.1)?;
    if !matches!(get(record, "sources"), Some(Canon::List(items)) if items.is_empty()) {
        return None;
    }
    if !matches!(get(record, "tasks"), Some(Canon::List(items)) if items.iter().all(|t| matches!(t, Canon::Str(_))))
    {
        return None;
    }
    if !matches!(get(record, "stale_after_days"), None | Some(Canon::Int(_))) {
        return None;
    }
    if matches!(get(record, "kind"), Some(Canon::Str(kind)) if kind == "prep") {
        return None;
    }
    for (_, other) in &every {
        let pairs = plain_map(other)?;
        match get(pairs, "links") {
            None | Some(Canon::List(_) | Canon::Str(_)) => {}
            Some(value) if !truthy(value) => {}
            Some(_) => return None,
        }
    }
    task_ids(&store, &ctx.root)?;
    // A sealed write differs on every run; leave it to Python.
    if store.encryption_enabled() {
        return None;
    }
    make_dirs(&ctx.root, &["out"])?;
    let out = ctx.root.join("out").join(format!("explain-{name}.md"));
    store
        .write_text(&out, &scaffold_text(name, short, deep))
        .ok()?;
    let shown = out.display();
    let depth = if short {
        " --short"
    } else if deep {
        " --deep"
    } else {
        ""
    };
    emit_json(
        ctx,
        "work.explain",
        &[("problems", "-1".into()), ("status", quoted("ok"))],
    );
    said(
        format!("wrote {shown}: fill it from the sources, then: careerbrew work project explain {name} --check {shown}{depth}\n"),
        0,
    )
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

// ---------- metrics time-saved ----------

/// What each run's `minutes_spent` measured (`cli.TIME_SOURCE`).
fn time_source(time: &str) -> Option<&'static str> {
    Some(match time {
        "web" => "your time to copy, send, or accept in the web app",
        "cli" => "CLI run time, an upper bound of time saved",
        "web+cli" => "web app time, and CLI run time for CLI runs",
        _ => return None,
    })
}

/// True when `pathlib.Path` prints `path` unchanged (no `//`, `/./`,
/// `/../`, trailing `/` or `/.`), so `wrote <path>` reads the same.
fn plain_path_text(path: &str) -> bool {
    path.starts_with('/')
        && !(path.contains("//")
            || path.contains("/./")
            || path.ends_with("/.")
            || path.contains("/../")
            || path.ends_with("/..")
            || (path.len() > 1 && path.ends_with('/')))
}

/// An int that converts to a float exactly (|n| < 2^53).
fn small_int(value: &Canon) -> Option<i64> {
    let Canon::Int(n) = value else {
        return None;
    };
    let n: i64 = n.to_string().parse().ok()?;
    (n.abs() < (1_i64 << 53)).then_some(n)
}

/// A plain decimal (`-12`, `7.5`): Python's `float()` and Rust's parser
/// read it the same way.
fn plain_decimal(text: &str) -> bool {
    let body = text.strip_prefix('-').unwrap_or(text);
    let (whole, frac) = match body.split_once('.') {
        Some((whole, frac)) => (whole, Some(frac)),
        None => (body, None),
    };
    let digits = |part: &str| {
        !part.is_empty() && part.len() <= 15 && part.bytes().all(|b| b.is_ascii_digit())
    };
    digits(whole) && frac.is_none_or(digits)
}

/// A `time_baseline_*` value `time_saved.baseline` reads the same way here
/// (`float(value)` when truthy).
fn baseline_ok(value: Option<&Canon>) -> bool {
    let minutes = match value {
        None | Some(Canon::Null | Canon::Bool(_)) => return true,
        Some(int @ Canon::Int(_)) => match small_int(int) {
            Some(n) => n as f64,
            None => return false,
        },
        Some(Canon::Float(f)) if f.is_finite() => *f,
        Some(Canon::Str(text)) if text.is_empty() => return true,
        Some(Canon::Str(text)) if plain_decimal(text) => match text.parse::<f64>() {
            Ok(f) => f,
            Err(_) => return false,
        },
        Some(_) => return false,
    };
    // `{:>5.0f}` of an exact half: keep the rounding rule out of question.
    (minutes - minutes.trunc()).abs() != 0.5
}

fn leap(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// `%Y-%m-%dT%H:%M:%SZ` with zero-padded fields and a real date and time:
/// the only stamps `strptime` and the native reader both read the same way.
fn utc_stamp(text: &str) -> bool {
    let b = text.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return false;
    }
    let num = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = &text[range];
        if part.bytes().all(|b| b.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    };
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        num(0..4),
        num(5..7),
        num(8..10),
        num(11..13),
        num(14..16),
        num(17..19),
    ) else {
        return false;
    };
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap(year) => 29,
        2 => 28,
        _ => return false,
    };
    (1971..=9000).contains(&year)
        && (1..=days).contains(&day)
        && hour < 24
        && minute < 60
        && second < 60
}

/// True when a value holds NaN or an infinity: serde skips such a row and
/// Python counts it.
fn nonfinite_inside(value: &Canon) -> bool {
    match value {
        Canon::Float(f) => !f.is_finite(),
        Canon::List(items) => items.iter().any(nonfinite_inside),
        Canon::Map(pairs) => pairs
            .iter()
            .any(|(k, v)| nonfinite_inside(k) || nonfinite_inside(v)),
        _ => false,
    }
}

/// One `cli.run` or `web.timing` line of the event log, in a shape
/// `time_saved._runs` and the native reader read the same way.
fn timed_row_ok(line: &str) -> bool {
    let Ok(row) = careerbrew_vault::json_yaml::json_read(line) else {
        return false;
    };
    let Some(pairs) = plain_map(&row) else {
        return false;
    };
    if nonfinite_inside(&row) {
        return false;
    }
    let event = match get(pairs, "event") {
        Some(Canon::Str(event)) => event.as_str(),
        _ => return true,
    };
    if event != "cli.run" && event != "web.timing" {
        return true;
    }
    // `in ACTIONS` raises on a list or a map.
    for key in ["saves", "step"] {
        if matches!(get(pairs, key), Some(Canon::List(_) | Canon::Map(_))) {
            return false;
        }
    }
    for key in ["ms", "seconds"] {
        if let Some(int @ Canon::Int(_)) = get(pairs, key) {
            if small_int(int).is_none() {
                return false;
            }
        }
    }
    matches!(get(pairs, "ts"), Some(Canon::Str(ts)) if utc_stamp(ts))
}

/// The event log, when Python's `read_text().splitlines()` and the native
/// reader split it into the same lines and every timed row is safe.
fn event_log_ok(log: &Path) -> Option<()> {
    let meta = match std::fs::symlink_metadata(log) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(()),
        Err(_) => return None,
    };
    if !meta.is_file() {
        return None;
    }
    let text = String::from_utf8(std::fs::read(log).ok()?).ok()?;
    if text.contains([
        '\r', '\u{b}', '\u{c}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{85}', '\u{2028}', '\u{2029}',
    ]) {
        return None;
    }
    for line in text.split('\n') {
        let timed = line.contains("\"cli.run\"") || line.contains("\"web.timing\"");
        if timed && !timed_row_ok(line) {
            return None;
        }
    }
    Some(())
}

fn negative_zero_inside(value: &Canon) -> bool {
    match value {
        Canon::Float(f) => *f == 0.0 && f.is_sign_negative(),
        Canon::List(items) => items.iter().any(negative_zero_inside),
        Canon::Map(pairs) => pairs.iter().any(|(_, v)| negative_zero_inside(v)),
        _ => false,
    }
}

fn as_f64(value: &Canon) -> Option<f64> {
    match value {
        Canon::Float(f) => Some(*f),
        int @ Canon::Int(_) => small_int(int).map(|n| n as f64),
        _ => None,
    }
}

fn int_of(value: Option<&Canon>) -> Option<i64> {
    small_int(value?)
}

/// `str()` of an int or a float, as an f-string prints it.
fn py_number(value: &Canon) -> Option<String> {
    match value {
        Canon::Float(f) => Some(careerbrew_reports::py_repr(*f)),
        int @ Canon::Int(_) => small_int(int).map(|n| n.to_string()),
        _ => None,
    }
}

/// `json.dumps(indent=2)` text as `indent=1`: halve each line's indent
/// (JSON strings never hold a raw newline).
fn indent_one(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let body = line.trim_start_matches(' ');
        let pad = line.len() - body.len();
        out.push_str(&" ".repeat(pad / 2));
        out.push_str(body);
    }
    out
}

/// Printable ASCII lines only.
fn printable_lines(text: &str) -> bool {
    text.split('\n').all(printable)
}

/// The text lines `cmd_metrics_time_saved` prints, or None on any shape it
/// does not expect.
fn time_saved_text(data: &Pairs, path: &str) -> Option<String> {
    let mut out = String::new();
    let Some(Canon::List(weeks)) = get(data, "weeks") else {
        return None;
    };
    for week in weeks {
        let week = plain_map(week)?;
        let Some(Canon::Str(name)) = get(week, "week") else {
            return None;
        };
        let saved = as_f64(get(week, "minutes_saved")?)?;
        let runs = int_of(get(week, "runs"))?;
        out.push_str(&format!("{name}  {saved:>7.1} min saved  {runs} run(s)\n"));
        for (action, cell) in plain_map(get(week, "actions")?)? {
            let Canon::Str(action) = action else {
                return None;
            };
            let cell = plain_map(cell)?;
            let runs = int_of(get(cell, "runs"))?;
            if runs == 0 {
                continue;
            }
            let spent = as_f64(get(cell, "minutes_spent")?)?;
            let Some(Canon::Str(time)) = get(cell, "time") else {
                return None;
            };
            out.push_str(&format!(
                "  {action:<10} {runs} run(s)  {spent:.1} min spent ({})\n",
                time_source(time)?
            ));
        }
    }
    let baselines = plain_map(get(data, "baselines")?)?;
    // Python's dict keeps `ACTIONS` order; the value here is sorted.
    for (action, _, _) in careerbrew_reports::time_saved::ACTIONS {
        let row = plain_map(get(baselines, action)?)?;
        let minutes = as_f64(get(row, "minutes")?)?;
        if (minutes - minutes.trunc()).abs() == 0.5 {
            return None;
        }
        let Some(Canon::Str(kind)) = get(row, "kind") else {
            return None;
        };
        out.push_str(&format!(
            "baseline  {action:<10} {minutes:>5.0} min  ({kind})\n"
        ));
    }
    out.push_str(&format!(
        "time-saved: {} min over {} run(s); {}/{} baselines measured; wrote {path}\n",
        py_number(get(data, "minutes_saved_total")?)?,
        int_of(get(data, "runs_total"))?,
        int_of(get(data, "measured_baselines"))?,
        baselines.len(),
    ));
    printable_lines(&out).then_some(out)
}

fn time_saved(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let root = ctx.root.to_str()?;
    if !plain_path_text(root) {
        return None;
    }
    let vault = vault_context()?;
    if vault.root != ctx.root {
        return None;
    }
    let store = vault.store();
    let settings = careerbrew_vault::settings::read_settings(&store).ok()?;
    let settings = plain_map(&settings)?;
    for (action, _, _) in careerbrew_reports::time_saved::ACTIONS {
        if !baseline_ok(get(settings, &format!("time_baseline_{action}"))) {
            return None;
        }
    }
    let log = ctx.root.join("logs").join("events.jsonl");
    event_log_ok(&log)?;
    let value = careerbrew_reports::time_saved::compute(&log).ok()?;
    let data = careerbrew_reports::canon(&value);
    if negative_zero_inside(&data) || nonfinite_inside(&data) {
        return None;
    }
    let pairs = plain_map(&data)?;
    let file_text = careerbrew_vault::json_yaml::json_write(&data).ok()?;
    let path = ctx.root.join("out").join("time-saved.json");
    let stdout = if args.flag("json") {
        let mut text = indent_one(&file_text);
        text.push('\n');
        text
    } else {
        time_saved_text(pairs, &format!("{root}/out/time-saved.json"))?
    };
    // `out/` is plaintext by design; an old sealed file would need the key.
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if !meta.is_file() => return None,
        Ok(_) => {
            let raw = std::fs::read(&path).ok()?;
            if raw.starts_with(careerbrew_vault::MAGIC) || raw.starts_with(b"gAAAAA") {
                return None;
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return None,
    }
    make_dirs(&ctx.root, &["out"])?;
    store.write_text_if_changed(&path, &file_text).ok()?;
    said(stdout, 0)
}

// ---------- cards publish ----------

/// `machine.guard("cards")`: a corporate role (or a machine file Python
/// could read another way) refuses here, and Python prints its error.
/// Copied from `rust20`'s `Files::personal`.
fn personal_role(store: &Store, root: &Path) -> Option<()> {
    let info = store
        .read_yaml(&root.join("machine.yaml"), Canon::Null, false)
        .ok()?;
    let role = match &info {
        Canon::Null => None,
        Canon::List(items) if items.is_empty() => None,
        data => get(plain_map(data)?, "role"),
    };
    match role {
        Some(Canon::Str(role)) if role == "corporate" => None,
        Some(Canon::Str(_)) | None => Some(()),
        Some(_) => None,
    }
}

/// The repository whose `knowledge/` and `prompts/` Python reads (copied
/// from `rust20`).
fn cards_repo() -> Option<PathBuf> {
    match std::env::var("CAREERBREW_NATIVE_REPO") {
        Ok(repo) if repo.starts_with('/') => Some(PathBuf::from(repo)),
        Ok(_) => None,
        Err(_) => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .ok(),
    }
}

/// `public.cards_dir()` (copied from `rust20`).
fn cards_dir() -> Option<PathBuf> {
    match std::env::var("CAREERBREW_PUBLIC_CARDS") {
        Ok(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        Ok(_) | Err(std::env::VarError::NotPresent) => Some(cards_repo()?.join("knowledge/cards")),
        Err(_) => None,
    }
}

/// `sorted(folder.glob("*.yaml")) if folder.is_dir() else []`, or None for
/// a hidden name, a non-file or a name that is not UTF-8 (from `rust20`).
fn deck_files(folder: &Path) -> Option<Vec<PathBuf>> {
    if !folder.is_dir() {
        return Some(Vec::new());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(folder).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name().into_string().ok()?;
        if !name.to_ascii_lowercase().ends_with(".yaml") {
            continue;
        }
        if !name.ends_with(".yaml") || name.starts_with('.') || !entry.path().is_file() {
            return None;
        }
        names.push(name);
    }
    names.sort();
    Some(names.into_iter().map(|name| folder.join(name)).collect())
}

/// A repo file Python reads with `Path.read_text` (universal newlines, so a
/// `\r` goes to Python) and a safe YAML load.
fn repo_yaml(path: &Path) -> Option<Canon> {
    let raw = std::fs::read(path).ok()?;
    if raw.starts_with(careerbrew_vault::MAGIC) {
        return None;
    }
    let text = String::from_utf8(raw).ok()?;
    if text.contains('\r') {
        return None;
    }
    careerbrew_vault::json_yaml::yaml_read(&text).ok()
}

/// `(data or {}).get("cards", [])` where each card must be a map: None
/// when Python would crash or read it differently.
fn card_rows(data: &Canon) -> Option<Vec<Pairs>> {
    if !truthy(data) {
        return Some(Vec::new());
    }
    match get(plain_map(data)?, "cards") {
        None => Some(Vec::new()),
        Some(Canon::List(items)) => items.iter().map(|c| plain_map(c).cloned()).collect(),
        Some(_) => None,
    }
}

/// Absent, or a list of text.
fn texts_or_absent(card: &Pairs, key: &str) -> bool {
    match get(card, key) {
        None => true,
        Some(Canon::List(items)) => items.iter().all(|item| matches!(item, Canon::Str(_))),
        _ => false,
    }
}

/// Text that is printable ASCII or `\n`: `strip`, `split`, `len` and
/// `casefold` read it the same way in both languages.
fn card_text(text: &str) -> bool {
    text.chars().all(|c| c == '\n' || (' '..='~').contains(&c))
}

/// `str(r.get(key) or "")` for a text, null or absent field; None for any
/// other value (its `str()` is not ported).
fn row_text(row: &Pairs, key: &str) -> Option<String> {
    match get(row, key) {
        None | Some(Canon::Null) => Some(String::new()),
        Some(Canon::Str(text)) if card_text(text) => Some(text.clone()),
        Some(_) => None,
    }
}

/// `bool(r.get(key))` for a bool, null or absent field.
fn row_flag(row: &Pairs, key: &str) -> Option<bool> {
    match get(row, key) {
        None | Some(Canon::Null) => Some(false),
        Some(Canon::Bool(flag)) => Some(*flag),
        Some(_) => None,
    }
}

/// `public.lint` of a card with these fields (the domain crate's port).
fn card_problems(fields: &[(&str, &str)]) -> Vec<String> {
    let card = Canon::Map(
        fields
            .iter()
            .map(|(key, value)| {
                (
                    Canon::Str((*key).to_owned()),
                    Canon::Str((*value).to_owned()),
                )
            })
            .collect(),
    );
    careerbrew_tutor::public::lint(&careerbrew_setup::value::from_canon(&card))
}

/// The input rows: `yaml.safe_load(text) or []`, a `cards` list when it is
/// a map, every row a map. None for YAML the reader is not checked on
/// (anchors, tags, merge keys, directives, complex keys, tabs).
fn publish_rows(text: &str) -> Option<Vec<Pairs>> {
    if text.contains(['&', '*', '!', '\r', '\t', '\u{feff}'])
        || text.contains("<<")
        || text.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with('%') || line.starts_with('?')
        })
    {
        return None;
    }
    let data = careerbrew_vault::json_yaml::yaml_read(text).ok()?;
    let rows = match &data {
        value if !truthy(value) => return Some(Vec::new()),
        Canon::Map(_) => match get(plain_map(&data)?, "cards") {
            None => return Some(Vec::new()),
            Some(Canon::List(items)) => items,
            Some(_) => return None,
        },
        Canon::List(items) => items,
        _ => return None,
    };
    rows.iter().map(|row| plain_map(row).cloned()).collect()
}

/// `load()` then `by_q`: the public deck as `{id: card}` (first-seen order,
/// the last card of an id), and each question hash's id (the last wins).
fn deck_by_question() -> Option<Vec<(String, String)>> {
    let mut public: Vec<(String, Pairs)> = Vec::new();
    for path in deck_files(&cards_dir()?)? {
        for card in card_rows(&repo_yaml(&path)?)? {
            let Some(Canon::Str(id)) = get(&card, "id") else {
                return None;
            };
            let question_ok = matches!(get(&card, "question"), Some(Canon::Str(_)));
            if !question_ok || !texts_or_absent(&card, "match") {
                return None;
            }
            let id = id.clone();
            match public.iter_mut().find(|(known, _)| *known == id) {
                Some(slot) => slot.1 = card,
                None => public.push((id, card)),
            }
        }
    }
    let mut by_q: Vec<(String, String)> = Vec::new();
    for (id, card) in &public {
        let Some(Canon::Str(question)) = get(card, "question") else {
            return None;
        };
        let hash = careerbrew_tutor::public::qhash(question);
        match by_q.iter_mut().find(|(known, _)| *known == hash) {
            Some(slot) => slot.1 = id.clone(),
            None => by_q.push((hash, id.clone())),
        }
    }
    Some(by_q)
}

/// What `publish` decides for the rows, before any write: the problem
/// lines, or the counts of public cards, private explanations and skips.
struct Verdict {
    problems: Vec<String>,
    public: usize,
    private: usize,
    skipped: usize,
}

fn publish_verdict(rows: &[Pairs], by_q: &[(String, String)]) -> Option<Verdict> {
    let mut new: Vec<String> = Vec::new();
    let mut new_private: Vec<String> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    let mut skipped = 0;
    for (n, row) in rows.iter().enumerate() {
        let src = row_text(row, "source_id")?;
        if row_flag(row, "skip")? {
            row_text(row, "reason")?;
            if !src.is_empty() {
                skipped += 1;
            }
            continue;
        }
        let question_raw = row_text(row, "question")?;
        let rewrite = row_text(row, "rewrite")?;
        let answer = row_text(row, "answer")?;
        let topic = row_text(row, "topic")?;
        let explanation = row_text(row, "explanation")?;
        let private = row_flag(row, "private")?;
        match get(row, "tags") {
            None | Some(Canon::Null) => {}
            Some(Canon::List(items)) if items.iter().all(|t| matches!(t, Canon::Str(_))) => {}
            Some(_) => return None,
        }
        let hash = careerbrew_tutor::public::qhash(&question_raw);
        let known = by_q
            .iter()
            .find(|(h, _)| *h == hash)
            .map(|(_, id)| id.clone())
            .filter(|id| !id.is_empty());
        let question = question_raw.trim().to_owned();
        let id = match (private, rewrite.is_empty()) {
            (_, false) => rewrite,
            (false, true) => known.unwrap_or_else(|| format!("kb-{hash}")),
            (true, true) if !src.is_empty() => format!("me-{src}"),
            (true, true) => format!("me-{}", careerbrew_tutor::public::qhash(&question)),
        };
        let topic = if topic.is_empty() {
            "general".to_owned()
        } else {
            topic.to_ascii_lowercase()
        };
        let answer = answer.trim().to_owned();
        let explanation = format!("{}\n", explanation.trim());
        let bad = card_problems(&[
            ("id", &id),
            ("question", &question),
            ("answer", &answer),
            ("topic", &topic),
            ("explanation", &explanation),
        ]);
        if !bad.is_empty() {
            problems.push(format!("card {}: {}", n + 1, bad.join("; ")));
            continue;
        }
        let into = if private { &mut new_private } else { &mut new };
        if !into.contains(&id) {
            into.push(id);
        }
    }
    Some(Verdict {
        problems,
        public: new.len(),
        private: new_private.len(),
        skipped,
    })
}

/// `cards publish` while it writes nothing: `--dry-run`, or a card that
/// fails the bar (`PublishError`, exit 1). Every read and check comes
/// before the one write, the `tutor/` folder `paths.sub` makes.
fn cards_publish(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let file = args.text("file")?;
    if file.is_empty() || !printable(file) {
        return None;
    }
    let vault = vault_context()?;
    if vault.root != ctx.root {
        return None;
    }
    let store = vault.store();
    personal_role(&store, &ctx.root)?;
    // `store.read_text(src) or ""`: a missing file reads as no rows.
    let text = match std::fs::symlink_metadata(file) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(_) => return None,
        Ok(meta) if !meta.is_file() => return None,
        Ok(_) => {
            let raw = std::fs::read(file).ok()?;
            if raw.starts_with(careerbrew_vault::MAGIC) || raw.starts_with(b"gAAAAA") {
                return None;
            }
            String::from_utf8(raw).ok()?
        }
    };
    let rows = publish_rows(&text)?;
    // `prompt_version()` reads the prompt first; a missing one raises.
    let prompt = std::fs::read(cards_repo()?.join("prompts/card-explainer.md")).ok()?;
    if String::from_utf8(prompt).ok()?.contains('\r') {
        return None;
    }
    let by_q = deck_by_question()?;
    // `skipped()` and `private_cards()`, read through the vault without
    // creating a key.
    let tutor = ctx.root.join("tutor");
    if std::fs::symlink_metadata(&tutor).is_ok_and(|meta| !meta.is_dir()) {
        return None;
    }
    let skipped = store
        .read_json(&tutor.join("public-skipped.json"), Canon::Null, false)
        .ok()?;
    if truthy(&skipped) {
        plain_map(&skipped)?;
    }
    let private = store
        .read_yaml(&tutor.join("private-explanations.yaml"), Canon::Null, false)
        .ok()?;
    for card in card_rows(&private)? {
        if !matches!(get(&card, "id"), Some(Canon::Str(_))) {
            return None;
        }
    }
    let verdict = publish_verdict(&rows, &by_q)?;
    let (stdout, code) = if !verdict.problems.is_empty() {
        (
            format!(
                "{} card(s) not published; fix them and publish again:\n{}\n",
                verdict.problems.len(),
                verdict.problems.join("\n")
            ),
            1,
        )
    } else if args.flag("dry_run") {
        (
            format!(
                "dry run: {} public card(s) and {} private explanation(s) pass the bar, {} marked personal; nothing written\n",
                verdict.public, verdict.private, verdict.skipped
            ),
            0,
        )
    } else {
        // A publish writes the deck (YAML at width 100), the skips and the
        // private explanations: Python runs it.
        return None;
    };
    if !printable_lines(&stdout) {
        return None;
    }
    make_dirs(&ctx.root, &["tutor"])?;
    said(stdout, code)
}

// ---------- wave 7: work project add errors ----------

/// Python's `repr()` of printable ASCII text with no quote or backslash.
fn py_repr(text: &str) -> Option<String> {
    (printable(text) && !text.contains(['\'', '\\'])).then(|| format!("'{text}'"))
}

/// `projects.parse_days`: the days, or the `ProjectError` text. None when
/// the text is not printable ASCII or too long to read for sure.
fn parse_days(text: &str) -> Option<Result<u64, String>> {
    let error = || -> Option<Result<u64, String>> {
        Some(Err(format!(
            "stale-after must look like 7d or 2w, not {}",
            py_repr(text)?
        )))
    };
    if !printable(text) {
        return None;
    }
    let body = text.trim_matches(' ');
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    let unit = body[digits..].trim_start_matches(' ');
    if digits == 0 || !matches!(unit, "" | "d" | "w") {
        return error();
    }
    if digits > 12 {
        return None;
    }
    let value: u64 = body[..digits].parse().ok()?;
    if value < 1 {
        return error();
    }
    Some(Ok(value * if unit == "w" { 7 } else { 1 }))
}

/// What `date.fromisoformat` does with an event day.
enum Day {
    /// A plain `YYYY-MM-DD` day: Python keeps it as typed.
    Valid,
    /// Text Python surely rejects (empty, or a character no ISO form has).
    Invalid,
    /// Any other form (a week date, a basic form): left to Python.
    Unsure,
}

fn iso_day(text: &str) -> Day {
    if text.is_empty()
        || text
            .chars()
            .any(|c| !(c.is_ascii_digit() || c == '-' || c == 'W'))
    {
        return Day::Invalid;
    }
    let b = text.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return Day::Unsure;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        text[..4].parse::<i32>(),
        text[5..7].parse::<u32>(),
        text[8..10].parse::<u32>(),
    ) else {
        return Day::Unsure;
    };
    if year >= 1 && civil_days(i64::from(year), month, day).is_some() {
        Day::Valid
    } else {
        Day::Unsure
    }
}

/// `projects.graph_problems`, as the `"; "`-joined `ProjectError` parts;
/// None when a record holds a parent or links Python reads differently.
fn graph_messages(every: &[(String, Canon)]) -> Option<Vec<String>> {
    let mut names: Vec<&str> = every.iter().map(|(n, _)| n.as_str()).collect();
    names.sort_unstable();
    let record = |name: &str| every.iter().find(|(n, _)| n == name).map(|(_, r)| r);
    let mut out = Vec::new();
    for name in &names {
        let rec = record(name)?;
        let parent = parent_of(rec)?;
        if parent == *name {
            out.push(format!("{name}: a project cannot be its own parent"));
        } else if !parent.is_empty() && !names.contains(&parent.as_str()) {
            out.push(format!(
                "{name}: parent {parent} is not a project (fix: add {parent} first, or leave parent empty)"
            ));
        }
        for link in links_of(rec)? {
            if link == *name {
                out.push(format!("{name}: a project cannot link to itself"));
            } else if !names.contains(&link.as_str()) {
                out.push(format!(
                    "{name}: link {link} is not a project (fix: add {link} first, or drop the link)"
                ));
            }
        }
    }
    let mut loops: Vec<Vec<String>> = Vec::new();
    for name in &names {
        let mut chain = vec![(*name).to_owned()];
        let mut cur = parent_of(record(name)?)?;
        while names.contains(&cur.as_str()) && !chain.contains(&cur) {
            chain.push(cur.clone());
            cur = parent_of(record(&cur)?)?;
        }
        if chain.contains(&cur) && chain.last() != Some(&cur) {
            let at = chain.iter().position(|n| *n == cur)?;
            let ring = chain[at..].to_vec();
            let mut key = ring.clone();
            key.sort();
            if !loops.contains(&key) {
                let first = key[0].clone();
                loops.push(key);
                let mut shown = ring;
                shown.push(cur.clone());
                out.push(format!(
                    "{first}: parents loop: {} (fix: clear one of these parents)",
                    shown.join(" -> ")
                ));
            }
        }
    }
    Some(out)
}

/// The `ProjectError` `projects.add` raises for these records, or None
/// when it raises none (the add then saves a random id: Python runs it).
#[allow(clippy::too_many_arguments)]
fn add_error(
    every: &[(String, Canon)],
    name: &str,
    stale: &str,
    title: &str,
    group: &str,
    parent: &str,
    links: &[String],
    kind: &str,
    event_on: &str,
) -> Option<String> {
    if !every.iter().all(|(n, _)| !n.is_empty() && printable(n)) {
        return None;
    }
    if every.iter().any(|(n, _)| n == name) {
        return Some(format!(
            "project {name} already exists (see `careerbrew work project show {name}`)"
        ));
    }
    if let Err(message) = parse_days(stale)? {
        return Some(message);
    }
    for (value, key, limit) in [(title, "title", 200), (group, "group", 60)] {
        if !value.is_empty() && value != "none" {
            if let Err(message) = label(value, key, limit)? {
                return Some(message);
            }
        }
    }
    let parent = if parent == "none" {
        ""
    } else {
        parent.trim_matches(' ')
    };
    let mut kind_now = "";
    if !kind.is_empty() {
        if kind != "prep" && kind != "none" {
            return Some(format!(
                "unknown project kind {kind}; use prep (or none to clear it)"
            ));
        }
        kind_now = if kind == "none" { "" } else { kind };
    }
    if !event_on.is_empty() && event_on != "none" {
        match iso_day(event_on.trim_matches(' ')) {
            Day::Invalid => {
                return Some(format!(
                    "the event day looks like 2026-11-03, not {}",
                    py_repr(event_on)?
                ))
            }
            Day::Unsure => return None,
            Day::Valid if kind_now != "prep" => {
                return Some("only a prep project has an event day (fix: add --kind prep)".into())
            }
            Day::Valid => {}
        }
    }
    let mut unique: Vec<Canon> = Vec::new();
    for link in links {
        let link = Canon::Str(link.clone());
        if !unique.contains(&link) {
            unique.push(link);
        }
    }
    let mut fresh: Pairs = Vec::new();
    if !parent.is_empty() {
        fresh.push((Canon::Str("parent".into()), Canon::Str(parent.into())));
    }
    if !unique.is_empty() {
        fresh.push((Canon::Str("links".into()), Canon::List(unique)));
    }
    let mut with_new = every.to_vec();
    with_new.push((name.to_owned(), Canon::Map(fresh)));
    let problems = graph_messages(&with_new)?;
    (!problems.is_empty()).then(|| problems.join("; "))
}

/// `work project add`: served only when `projects.add` raises
/// `ProjectError`, which it does before any write but the lock.
fn project_add(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let name = printable_name(args)?;
    let text = |key: &str| args.text(key).unwrap_or("");
    let (stale, title, group, parent, kind, event_on) = (
        args.text("stale_after")?,
        text("title"),
        text("group"),
        text("parent"),
        text("kind"),
        text("event_on"),
    );
    let links = args.list("link");
    let every_text = [stale, title, group, parent, kind, event_on];
    if !every_text.iter().all(|t| printable(t)) || !links.iter().all(|t| printable(t)) {
        return None;
    }
    let vault = vault_context()?;
    let store = vault.store();
    let settings = read_map(&store, &ctx.root.join("settings.yaml"))?;
    // `new_record` reads `calendar.now()` before it parses the days.
    if !clock_safe(&settings) {
        return None;
    }
    if !valid_name(name) {
        return writer_early_error(
            ctx,
            "a project name uses lowercase letters, digits, and dashes (max 40)",
        );
    }
    writer_error(ctx, |every| {
        add_error(
            every, name, stale, title, group, parent, links, kind, event_on,
        )
    })
}

// ---------- wave 7: work project show ----------

/// The pinned `_now()` in UTC seconds when Python reads it for sure: a
/// `CAREERBREW_NOW` with an offset, or one in a `UTC` zone setting.
fn pinned_utc(settings: &Pairs) -> Option<i64> {
    let pinned = std::env::var("CAREERBREW_NOW").ok()?;
    if !simple_now(&pinned) {
        return None;
    }
    let long = if pinned.len() == 16 || pinned.len() == 22 {
        let (head, zone) = pinned.split_at(16);
        format!("{head}:00{zone}")
    } else {
        pinned.clone()
    };
    if long.len() == 25 {
        return Some(local_secs(&long[..19])? - offset_secs(&long[19..])?);
    }
    match get(settings, "timezone") {
        Some(Canon::Str(zone)) if zone == "UTC" || zone == "Etc/UTC" => {}
        _ => return None,
    }
    local_secs(&long)
}

/// Days since 1970-01-01 of a valid proleptic Gregorian day.
fn civil_days(year: i64, month: u32, day: u32) -> Option<i64> {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let size = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if day < 1 || day > size {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// `YYYY-MM-DDTHH:MM:SS` as seconds since the epoch, read as UTC.
fn local_secs(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() != 19
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<u32> {
        let part = &text[from..to];
        if part.bytes().all(|c| c.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    };
    let days = civil_days(i64::from(num(0, 4)?), num(5, 7)?, num(8, 10)?)?;
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(days * 86_400 + i64::from(hour * 3600 + minute * 60 + second))
}

/// `+HH:MM` or `-HH:MM` as seconds east of UTC.
fn offset_secs(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() != 6 || b[3] != b':' || !b[1..3].iter().chain(&b[4..]).all(u8::is_ascii_digit) {
        return None;
    }
    let (hours, minutes): (i64, i64) = (text[1..3].parse().ok()?, text[4..].parse().ok()?);
    if hours > 23 || minutes > 59 {
        return None;
    }
    match b[0] {
        b'+' => Some(hours * 3600 + minutes * 60),
        b'-' => Some(-(hours * 3600 + minutes * 60)),
        _ => None,
    }
}

/// A `checked_at` stamp as `check` writes it (`+00:00`, whole seconds).
fn stamp_secs(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() != 25 || !text.ends_with("+00:00") || b[10] != b'T' {
        return None;
    }
    local_secs(&text[..19])
}

/// A text field `p.get(key, "")` that prints when truthy: None for a
/// value Python would print another way.
fn shown_text(pairs: &Pairs, key: &str) -> Option<String> {
    match get(pairs, key) {
        None => Some(String::new()),
        Some(value) if !truthy(value) => Some(String::new()),
        Some(Canon::Str(text)) if printable(text) => Some(text.clone()),
        Some(_) => None,
    }
}

/// One source row of `projects.show`.
struct Row {
    kind: String,
    reference: String,
    /// Tenths of a day since the last check; None when never checked.
    age: Option<i64>,
    marked: bool,
    stale: bool,
    stale_reason: String,
}

fn show_rows(record: &Pairs, limit: i64, now: Option<i64>) -> Option<Vec<Row>> {
    let Some(Canon::List(sources)) = get(record, "sources") else {
        return None;
    };
    let mut rows = Vec::new();
    for source in sources {
        let s = plain_map(source)?;
        let (Some(Canon::Str(kind)), Some(Canon::Str(reference))) = (get(s, "kind"), get(s, "ref"))
        else {
            return None;
        };
        // A link may have no reader (a sign-in host, a host outside the
        // policy): `no_reader` is not ported.
        if !printable(kind) || !printable(reference) || reference.contains("://") {
            return None;
        }
        let age = match get(s, "checked_at") {
            None => None,
            Some(value) if !truthy(value) => None,
            Some(Canon::Str(stamp)) => {
                let secs = now? - stamp_secs(stamp)?;
                if secs < 0 || secs % 8640 != 0 {
                    return None;
                }
                Some(secs / 8640)
            }
            Some(_) => return None,
        };
        let marked = match get(s, "reference") {
            None => false,
            Some(Canon::Bool(flag)) => *flag,
            Some(_) => return None,
        };
        let stale = !marked && age.is_none_or(|tenths| tenths > limit * 10);
        let stale_reason = shown_text(s, "stale_reason")?;
        rows.push(Row {
            kind: kind.clone(),
            reference: reference.clone(),
            age,
            marked,
            stale,
            stale_reason,
        });
    }
    // Never checked first, then oldest; references last (a stable sort).
    rows.sort_by_key(|r| (r.marked, r.age.is_some(), -r.age.unwrap_or(0)));
    Some(rows)
}

/// The open task ids `priorities.load()` keeps, when every task has a
/// shape it reads the same way.
fn open_task_ids(store: &Store, root: &Path) -> Option<Vec<String>> {
    let top = read_map(store, &root.join("work").join("tasks.yaml"))?;
    let tasks = match get(&top, "tasks") {
        None => return Some(Vec::new()),
        Some(Canon::List(items)) => items,
        Some(_) => return None,
    };
    let mut open = Vec::new();
    for task in tasks {
        let pairs = plain_map(task)?;
        if get(pairs, "link").is_some() {
            return None;
        }
        let is_open = match get(pairs, "status") {
            None => true,
            Some(Canon::Str(status)) => status == "open",
            Some(_) => false,
        };
        if is_open {
            let Some(Canon::Str(id)) = get(pairs, "id") else {
                return None;
            };
            open.push(id.clone());
        }
    }
    Some(open)
}

/// `_age(round(days, 1))` for an age in tenths of a day.
fn age_text(age: Option<i64>) -> Option<String> {
    match age {
        None => Some("never checked".into()),
        Some(tenths) => Some(format!(
            "{}d ago",
            fmt_g(&format!("{}.{}", tenths / 10, tenths % 10))?
        )),
    }
}

/// `projects.reader` with no plugin and a ref with no link.
fn reader_of(kind: &str) -> &'static str {
    if ["memory", "url", "tracker", "repo", "doc", "chat"].contains(&kind) {
        "careerbrew"
    } else {
        "you"
    }
}

/// `p.get("proposed", [])` as (kind, ref) pairs.
fn proposed_of(record: &Pairs) -> Option<Vec<(String, String)>> {
    match get(record, "proposed") {
        None => Some(Vec::new()),
        Some(Canon::List(items)) => items
            .iter()
            .map(|item| {
                let pairs = plain_map(item)?;
                match (get(pairs, "kind"), get(pairs, "ref")) {
                    (Some(Canon::Str(kind)), Some(Canon::Str(reference)))
                        if printable(kind) && printable(reference) =>
                    {
                        Some((kind.clone(), reference.clone()))
                    }
                    _ => None,
                }
            })
            .collect(),
        Some(_) => None,
    }
}

/// `work project show <name>` when stdout is not a terminal and no plugin
/// is installed (so `may_show_business()` is false).
fn project_show(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.env.stdout_is_terminal {
        return None;
    }
    let name = printable_name(args)?;
    // A plugin may hand reads to the agent or allow business text.
    if !absent(&ctx.root.join("plugins.lock.json")) {
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
    let every = load_projects(&store, &ctx.root)?;
    if !every.iter().all(|(n, _)| !n.is_empty() && printable(n)) {
        return None;
    }
    if let Some(message) = unknown_text(&every, name)? {
        make_dirs(&ctx.root, &["work"])?;
        return project_error(ctx, &message);
    }
    let record = plain_map(&every.iter().find(|(n, _)| n == name)?.1)?;
    if matches!(get(record, "kind"), Some(Canon::Str(kind)) if kind == "prep") {
        return None;
    }
    let limit = match get(record, "stale_after_days") {
        None => 7,
        Some(Canon::Int(n)) => n.to_string().parse::<i64>().ok()?,
        Some(_) => return None,
    };
    if !(-1_000_000..=1_000_000).contains(&limit) {
        return None;
    }
    let rows = show_rows(record, limit, pinned_utc(&settings))?;
    let Some(Canon::List(tasks)) = get(record, "tasks") else {
        return None;
    };
    let mut linked: Vec<&str> = Vec::new();
    for task in tasks {
        match task {
            Canon::Str(task) if printable(task) => linked.push(task),
            _ => return None,
        }
    }
    let proposed = proposed_of(record)?;
    let group = shown_text(record, "group")?;
    let parent = shown_text(record, "parent")?;
    let links = links_of(&Canon::Map(record.clone()))?;
    if !links.iter().all(|link| printable(link)) {
        return None;
    }
    let mut children = Vec::new();
    let mut linked_from = Vec::new();
    for (other, rec) in &every {
        let pairs = plain_map(rec)?;
        if get(pairs, "parent") == Some(&Canon::Str(name.to_owned())) {
            children.push(other.as_str());
        }
        if links_of(rec)?.iter().any(|link| link == name) {
            linked_from.push(other.as_str());
        }
    }
    children.sort_unstable();
    linked_from.sort_unstable();
    let open = open_task_ids(&store, &ctx.root)?;
    let mut out = format!("{name}: stale after {limit}d\n");
    for (key, value) in [("group", &group), ("parent", &parent)] {
        if !value.is_empty() {
            out.push_str(&format!("  {key}: {value}\n"));
        }
    }
    let links: Vec<&str> = links.iter().map(String::as_str).collect();
    for (label, items) in [
        ("children", &children),
        ("links", &links),
        ("linked from", &linked_from),
    ] {
        if !items.is_empty() {
            out.push_str(&format!("  {label}: {}\n", items.join(", ")));
        }
    }
    for row in &rows {
        let who = reader_of(&row.kind);
        let mark = if row.marked {
            "ref  "
        } else if row.stale {
            "STALE"
        } else {
            "ok   "
        };
        out.push_str(&format!(
            "  {mark}  {:<8} {}  {}  read by {who}\n",
            row.kind,
            row.reference,
            age_text(row.age)?
        ));
        if row.marked {
            out.push_str(&format!(
                "         reference, never stale: marked as a reference (track it: careerbrew work project reference {name} {} --off)\n",
                row.reference
            ));
        } else if !row.stale_reason.is_empty() {
            out.push_str(&format!("         why stale: {}\n", row.stale_reason));
        }
    }
    for (kind, reference) in &proposed {
        out.push_str(&format!(
            "  proposed: {kind} {reference} (add: careerbrew work project source {name} {kind} {reference})\n"
        ));
    }
    for task in linked {
        if open.iter().any(|id| id == task) {
            out.push_str(&format!("  next: {task}\n"));
        }
    }
    said(out, 0)
}

// ---------- wave 7: work project sync ----------

const SYNC_OFF: &str =
    "project memory sync is off (turn it on: careerbrew config set project_memory_sync true)";

const SYNC_IDLE: &str = concat!(
    "memory in: 0 changed file(s): 0 new source(s), 0 proposed, 0 link(s), 0 status line(s)\n",
    "memory out: 0 new note(s) in 0 project(s)\n",
    "note: business reading is off here: no notes written (run it in your terminal, or a plugin sets agent_may_read_business)\n",
);

/// `work project sync` when it changes nothing: every project has no
/// source and no memory file, no feed event exists, and stdout is not a
/// terminal (so no note may be written).
fn project_sync(ctx: &Ctx) -> Option<Outcome> {
    if ctx.env.stdout_is_terminal {
        return None;
    }
    if !absent(&ctx.root.join("plugins.lock.json")) {
        return None;
    }
    let vault = vault_context()?;
    let store = vault.store();
    let settings = read_map(&store, &ctx.root.join("settings.yaml"))?;
    let enabled = match get(&settings, "project_memory_sync") {
        None => true,
        Some(Canon::Bool(flag)) => *flag,
        Some(_) => return None,
    };
    let work = ctx.root.join("work");
    for path in [work.clone(), work.join("feed")] {
        if !(path.is_dir() || absent(&path)) {
            return None;
        }
    }
    // `feed.personal_ids()` reads every feed event: none here.
    if !absent(&work.join("feed").join("events.jsonl")) {
        return None;
    }
    let every = load_projects(&store, &ctx.root)?;
    for (_, record) in &every {
        let pairs = plain_map(record)?;
        // `normalize_sources` rewrites old refs; with no source it is a no-op.
        if !matches!(get(pairs, "sources"), Some(Canon::List(items)) if items.is_empty()) {
            return None;
        }
        if get(pairs, "memory").is_some_and(truthy) {
            return None;
        }
    }
    if enabled {
        // `sync_in` reads the sync state; a non-map one would crash.
        let state = store
            .read_json(&work.join("memory-sync.json"), Canon::Null, false)
            .ok()?;
        if truthy(&state) {
            plain_map(&state)?;
        }
    }
    under_lock(ctx, || ())?;
    if enabled {
        let lock = work.join(".memory-sync.lock");
        careerbrew_vault::lock::with_exact_lock(&lock, || ()).ok()?;
    }
    make_dirs(&ctx.root, &["work", "feed"])?;
    if enabled {
        said(SYNC_IDLE.to_owned(), 0)
    } else {
        said(format!("{SYNC_OFF}\n"), 0)
    }
}

// ---------- wave 7: drill progress ----------

/// The data root as `paths.data_root()` prints it, or None when `pathlib`
/// could print it another way (copied from `explain_scaffold`).
fn printed_root(ctx: &Ctx) -> Option<String> {
    let root_text = ctx.root.to_str()?;
    let given = std::env::var("CAREERBREW_HOME").ok()?;
    let odd = root_text.contains("//")
        || root_text.contains("/./")
        || root_text.ends_with("/.")
        || root_text.ends_with('/');
    (given == root_text && !odd && printable(root_text)).then(|| root_text.to_owned())
}

/// The topics of `tutor.items()` in first-seen order, from the base bank
/// alone (no plugin knowledge, no generated gap items).
fn base_topics() -> Option<Vec<String>> {
    let repo = cards_repo()?;
    if !absent(&repo.join("knowledge").join("knowledge")) {
        return None;
    }
    let data = repo_yaml(&repo.join("knowledge").join("topics.yaml"))?;
    let items = match &data {
        value if !truthy(value) => return Some(Vec::new()),
        Canon::Map(_) => match get(plain_map(&data)?, "items") {
            None => return Some(Vec::new()),
            Some(Canon::List(items)) => items,
            Some(_) => return None,
        },
        _ => return None,
    };
    // `out[it["id"]] = ...`: a later item of an id keeps the first one's place.
    let mut by_id: Vec<(String, String)> = Vec::new();
    for item in items {
        let pairs = plain_map(item)?;
        let (Some(Canon::Str(id)), Some(Canon::Str(topic))) =
            (get(pairs, "id"), get(pairs, "topic"))
        else {
            return None;
        };
        if !printable(topic) {
            return None;
        }
        match by_id.iter_mut().find(|(known, _)| known == id) {
            Some(slot) => slot.1 = topic.clone(),
            None => by_id.push((id.clone(), topic.clone())),
        }
    }
    let mut topics: Vec<String> = Vec::new();
    for (_, topic) in by_id {
        if !topics.contains(&topic) {
            topics.push(topic);
        }
    }
    Some(topics)
}

/// `drill progress` before any card, attempt, readiness row or gap item:
/// every mastery is 0, so the page and the line hold no float or clock.
fn drill_progress(ctx: &Ctx) -> Option<Outcome> {
    use careerbrew_reports::charts::{Fmt, Report};
    let root = printed_root(ctx)?;
    // A plugin may add knowledge items (`plugins.resources`).
    if !absent(&ctx.root.join("plugins.lock.json")) {
        return None;
    }
    let vault = vault_context()?;
    if vault.root != ctx.root {
        return None;
    }
    let store = vault.store();
    personal_role(&store, &ctx.root)?;
    let settings = read_map(&store, &ctx.root.join("settings.yaml"))?;
    if !clock_safe(&settings) {
        return None;
    }
    let tutor = ctx.root.join("tutor");
    let out_dir = ctx.root.join("out");
    for dir in [&tutor, &out_dir] {
        if !(dir.is_dir() || absent(dir)) {
            return None;
        }
    }
    for file in [
        "cards.json",
        "attempts.jsonl",
        "readiness.jsonl",
        "gap-items.yaml",
    ] {
        if !absent(&tutor.join(file)) {
            return None;
        }
    }
    let page = out_dir.join("progress.html");
    if !(absent(&page) || std::fs::symlink_metadata(&page).is_ok_and(|m| m.is_file())) {
        return None;
    }
    let topics = base_topics()?;
    let mut rep = Report::new(
        "Interview readiness",
        "Spaced repetition (FSRS) with binary rubrics. Private; stays on this machine.",
    );
    rep.kpis(&[
        ("readiness (core)".into(), "0%".into()),
        ("attempts".into(), "0".into()),
        ("due now".into(), "0".into()),
    ]);
    let rows: Vec<(String, f64)> = topics.into_iter().map(|t| (t, 0.0)).collect();
    rep.bars("Mastery by topic", &rows, 1.0, Fmt::Percent0);
    rep.line("Rubric score per attempt", &[], 1.0, Fmt::Percent0);
    rep.line("Readiness over time", &[], 1.0, Fmt::Percent0);
    rep.bars("Most-missed rubric criteria (count)", &[], 1.0, Fmt::Fixed0);
    make_dirs(&ctx.root, &["tutor"])?;
    make_dirs(&ctx.root, &["out"])?;
    std::fs::write(&page, rep.html()).ok()?;
    said(
        format!("readiness 0% over 0 attempt(s); chart: {root}/out/progress.html\n"),
        0,
    )
}
