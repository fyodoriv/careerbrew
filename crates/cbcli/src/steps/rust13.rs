//! Native CLI handlers owned by `rust-13` (see `steps/mod.rs`): the parts of
//! `work feed` and `work team` whose Python result is fixed text or reads one
//! small record.
//!
//! Python's `cmd_work_feed` and `cmd_work_team` (`src/careerbrew/cli.py`)
//! are the reference. Each handler serves only these cases and refuses
//! (exit 64, Python runs it) before any write otherwise:
//! - `work feed rules` with no `work/feed-rules.yaml` (the built-in rules),
//! - `work feed show` and `work feed links` with no feed events file,
//! - `work feed open` when names are hidden (stdout is not a terminal and no
//!   plugin is installed, so no plugin policy can allow business reading),
//! - `work team list` (text and `--json`) when names are hidden and the
//!   roster has the plain shape `team.load` reads.
//!
//! Wave 4 adds, each only in the case described at its handler:
//! - `work team add` and `work team remove` (a plain roster, no feed data, a
//!   plaintext data folder for a write; their errors in any folder),
//! - `work team feed` and `work feed retriage` with no feed data,
//! - `work feed mark` and `work feed research` with no feed events (their
//!   `FeedError` lines).
//!
//! Wave 7 adds the `FeedError` lines Python prints before it reads or
//! writes any feed data:
//! - `work feed add FILE` with an input that has no events, or whose first
//!   non-blank line can never be JSON (no `--plugin`, no `--source`),
//! - `work feed transcript` with no `feed_tool_sources`, or with a tool map
//!   and no transcript file,
//! - `work feed cycle` with no required source anywhere.
//!
//! Every case also refuses when Python's `privacy.redact` would read an
//! input (`Ctx::redact_inputs_present`).
use super::Ctx;
use crate::args::Parsed;
use crate::{events, Outcome};
use careerbrew_vault::canon::Canon;
use careerbrew_vault::store::Store;
use careerbrew_vault::transfer::Context;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust13.rs`.
pub const COMMANDS: &[&str] = &[
    "work.feed.rules",
    "work.feed.show",
    "work.feed.links",
    "work.feed.open",
    "work.team.list",
    "work.team.add",
    "work.team.remove",
    "work.team.feed",
    "work.feed.retriage",
    "work.feed.mark",
    "work.feed.research",
    "work.feed.add",
    "work.feed.transcript",
    "work.feed.cycle",
];

const HIDDEN: &str =
    "stdout is not a terminal and no plugin policy allows agents to read business data";

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.redact_inputs_present() || !ctx.root.is_absolute() {
        return None;
    }
    match name {
        "work.feed.rules" => rules(ctx),
        "work.feed.show" => no_events(ctx, NO_EVENTS),
        "work.feed.links" => no_events(
            ctx,
            if args.flag("pending") {
                "no kept event with links to research"
            } else {
                "no kept event has links"
            },
        ),
        "work.feed.open" => open(ctx),
        "work.team.list" => team_list(args, ctx),
        "work.team.add" => team_add(args, ctx),
        "work.team.remove" => team_remove(args, ctx),
        "work.team.feed" => team_feed(args, ctx),
        "work.feed.retriage" => feed_retriage(ctx),
        "work.feed.mark" => feed_mark(args, ctx),
        "work.feed.research" => feed_research(args, ctx),
        "work.feed.add" => feed_add(args, ctx),
        "work.feed.transcript" => feed_transcript(args, ctx),
        "work.feed.cycle" => feed_cycle(args, ctx),
        _ => None,
    }
}

const NO_EVENTS: &str = "no events yet: careerbrew connect slack | pagerduty | outlook, then careerbrew work feed pull (plugins: careerbrew work feed add <file.jsonl> --plugin <name>)";

fn done(stdout: String) -> Option<Outcome> {
    Some(Outcome {
        stdout,
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}

/// True when Python's `may_show_business()` is false for sure: stdout is
/// not a terminal, and no plugin lock exists (no plugin, so no policy).
/// `run` has already refused when the lock exists.
fn hidden(ctx: &Ctx) -> bool {
    !ctx.env.stdout_is_terminal
}

/// True when nothing (not even a dangling symlink) is at `path`.
fn absent(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_err()
}

/// `paths.sub(...)`: Python makes each parent folder. Refuses (None) when a
/// part exists as anything but a plain folder, where Python's `mkdir` fails
/// or follows a link.
fn make_dirs(root: &Path, parts: &[&str]) -> Option<()> {
    let mut at: PathBuf = root.to_path_buf();
    let mut chain = Vec::new();
    for part in parts {
        at = at.join(part);
        chain.push(at.clone());
    }
    for dir in &chain {
        if let Ok(meta) = std::fs::symlink_metadata(dir) {
            if !meta.is_dir() {
                return None;
            }
        }
    }
    std::fs::create_dir_all(&at).ok()?;
    Some(())
}

/// `feed.BUILTIN_RULES` as `cmd_work_feed` prints them: name, the `when`
/// clause (Python's `str` of each value), and the set fields in `SET_KEYS`
/// order.
const BUILTIN_RULES: &[(&str, &str, &str)] = &[
    ("own-action", "actor=me", "triage=mine"),
    (
        "page-handled",
        "source=pagerduty, actor=me, status=['acknowledged', 'resolved']",
        "evidence=true",
    ),
    ("bot", "actor=bot", "triage=noise"),
    (
        "page-for-me",
        "source=pagerduty, to_me=True, status=['triggered', 'acknowledged']",
        "triage=act",
    ),
    ("important-mail", "kind=mail, importance=high", "triage=act"),
    ("flagged-mail", "kind=mail, status=flagged", "triage=act"),
    (
        "asked-me",
        "kind=['dm', 'mention'], to_me=True",
        "triage=act",
    ),
    (
        "review-for-me",
        "kind=['review', 'review-request', 'review_request'], to_me=True",
        "triage=act",
    ),
    (
        "default",
        "any event",
        "triage=fyi, personal=false, evidence=false",
    ),
];

/// `work feed rules` with no rules file of the owner's: the built-ins only.
fn rules(ctx: &Ctx) -> Option<Outcome> {
    let root = &ctx.root;
    if !absent(&root.join("work/feed-rules.yaml")) {
        return None;
    }
    make_dirs(root, &["work"])?;
    let mut out = String::new();
    for (name, when, sets) in BUILTIN_RULES {
        out.push_str(&format!("built-in  {name:<16} when {when} -> {sets}\n"));
    }
    out.push_str(
        "add your own (they run first): careerbrew data edit work/feed-rules.yaml, for example:\n",
    );
    out.push_str("  rules: [{name: alerts, when: {source: slack, channel: 'alerts-*'}, triage: noise}, {name: family, when: {text: [dentist]}, personal: true}]\n");
    out.push_str("then: careerbrew work feed retriage\n");
    done(out)
}

/// `work feed show` and `work feed links` before any event is stored:
/// `feed.load()` reads no file and returns no rows.
fn no_events(ctx: &Ctx, line: &str) -> Option<Outcome> {
    let root = &ctx.root;
    if !absent(&root.join("work/feed/events.jsonl")) {
        return None;
    }
    make_dirs(root, &["work", "feed"])?;
    done(format!("{line}\n"))
}

/// `work feed open` checks `may_show_business()` before it reads anything.
fn open(ctx: &Ctx) -> Option<Outcome> {
    if !hidden(ctx) {
        return None;
    }
    done(format!("hidden: {HIDDEN}\n"))
}

fn key_present() -> bool {
    careerbrew_vault::transfer::Context::from_env().is_ok_and(|ctx| ctx.key_file.is_file())
}

/// `serde_json::Value`, named through a public alias of a domain crate:
/// this crate does not depend on `serde_json` directly (a private copy of
/// the alias in `rust16`).
type Value = <<careerbrew_sources::connectors::Fetch<'static> as ReturnOf>::R as OkOf>::T;

trait ReturnOf {
    type R;
}

impl<R> ReturnOf for &'static mut dyn FnMut(&str) -> R {
    type R = R;
}

trait OkOf {
    type T;
}

impl<T, E> OkOf for Result<T, E> {
    type T = T;
}

/// One roster field as `team.load` keeps it: `str(value or "")`. Some(true)
/// for a non-empty string, Some(false) for none; None for a value whose
/// Python reading this module does not repeat (a number, a list, ...).
fn field_set(value: Option<&Value>) -> Option<bool> {
    match value {
        None => Some(false),
        Some(value) if value.is_null() => Some(false),
        Some(value) => value.as_str().map(|text| !text.is_empty()),
    }
}

/// Which of `name`, `chat`, `email`, `login` each teammate has, as
/// `team.load` returns the roster, or None to refuse.
fn roster(root: &Path) -> Option<Vec<[bool; 4]>> {
    let path = root.join("work/team.yaml");
    let meta = match std::fs::symlink_metadata(&path) {
        Err(_) => return Some(Vec::new()),
        Ok(meta) => meta,
    };
    if !meta.is_file() || !key_present() {
        return None;
    }
    let data: Value =
        careerbrew_projects::read_yaml(root, "work/team.yaml", Default::default()).ok()?;
    let mut out = Vec::new();
    if data.is_null() {
        return Some(out);
    }
    let Some(team) = data
        .as_object()?
        .get("team")
        .filter(|value| !value.is_null())
    else {
        return Some(out);
    };
    for row in team.as_array()? {
        let Some(fields) = row.as_object() else {
            // `isinstance(r, dict)` skips it.
            continue;
        };
        let get = |key: &str| field_set(fields.get(key));
        if !get("name")? {
            continue;
        }
        out.push([true, get("chat")?, get("email")?, get("login")?]);
    }
    Some(out)
}

/// `work team list` with names hidden: the text form, or `--json` with each
/// field as true or false (`json.dumps(..., indent=2)`).
fn team_list(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if !hidden(ctx) {
        return None;
    }
    let root = &ctx.root;
    let rows = roster(root)?;
    make_dirs(root, &["work"])?;
    const KEYS: [&str; 4] = ["name", "chat", "email", "login"];
    if args.flag("json") {
        if rows.is_empty() {
            return done("[]\n".to_owned());
        }
        let items = rows
            .iter()
            .map(|row| {
                let fields = KEYS
                    .iter()
                    .zip(row)
                    .map(|(key, set)| format!("    \"{key}\": {set}"))
                    .collect::<Vec<_>>()
                    .join(",\n");
                format!("  {{\n{fields}\n  }}")
            })
            .collect::<Vec<_>>()
            .join(",\n");
        return done(format!("[\n{items}\n]\n"));
    }
    if rows.is_empty() {
        return done("no teammates yet: careerbrew work team add \"Sam Lee\" --chat U0SAM --login samlee --email sam@example.com\n".to_owned());
    }
    let mut out = format!("{} teammate(s)\n", rows.len());
    for row in &rows {
        let ids = KEYS[1..]
            .iter()
            .zip(&row[1..])
            .filter(|(_, set)| **set)
            .map(|(key, _)| *key)
            .collect::<Vec<_>>()
            .join(", ");
        let ids = if ids.is_empty() {
            "name only".to_owned()
        } else {
            ids
        };
        out.push_str(&format!("- (name hidden): {ids}\n"));
    }
    out.push_str(&format!("names hidden: {HIDDEN}\n"));
    done(out)
}

// ---------- wave 4: work team add, remove and feed; work feed retriage, mark and research ----------

/// One event field: a lowercase id or an int, as `events.clean` keeps them.
#[derive(Clone, Copy)]
enum Field<'a> {
    Id(&'a str),
    Int(usize),
}

/// Appends one row as `events.emit` writes it: `ts`, `v` and the fields,
/// sorted by key, in `json.dumps(sort_keys=True)` form.
fn emit(ctx: &Ctx, event: &str, fields: &[(&str, Field)]) {
    if !ctx.env.caps.events_on {
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let stamp = events::stamp(secs);
    let mut pairs: Vec<(&str, Field)> = vec![
        ("event", Field::Id(event)),
        ("ts", Field::Id(&stamp)),
        ("v", Field::Id(events::version())),
    ];
    pairs.extend_from_slice(fields);
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    let mut line = String::from("{");
    for (i, (key, value)) in pairs.iter().enumerate() {
        if i > 0 {
            line.push_str(", ");
        }
        crate::render::py_string(key, &mut line);
        line.push_str(": ");
        match value {
            Field::Id(text) => crate::render::py_string(text, &mut line),
            Field::Int(n) => line.push_str(&n.to_string()),
        }
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
    emit(
        ctx,
        "cli.error",
        &[
            ("command", Field::Id(command)),
            ("error", Field::Id(class)),
            ("via", Field::Id(ctx.env.caps.via)),
        ],
    );
    Some(Outcome {
        stdout: format!("error: {message}\n"),
        stderr: String::new(),
        code: 2,
        emit_event: false,
    })
}

/// Printable ASCII only: Python's `str.split`, `lower` and `len` read it
/// as Rust does, and it prints the same under every locale.
fn printable(text: &str) -> bool {
    text.chars().all(|c| (' '..='~').contains(&c))
}

/// `" ".join(value.split())` for printable ASCII.
fn one_line(value: &str) -> String {
    value
        .split(' ')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The vault context, when it resolves the key path as Python's
/// `keyfile.key_path` does (as in `rust24`).
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

/// The value under a string key; Err when the key repeats (an order
/// question this module does not answer).
fn map_get<'a>(pairs: &'a [(Canon, Canon)], key: &str) -> Result<Option<&'a Canon>, ()> {
    let mut found = None;
    for (k, v) in pairs {
        if matches!(k, Canon::Str(s) if s == key) {
            if found.is_some() {
                return Err(());
            }
            found = Some(v);
        }
    }
    Ok(found)
}

const TEAM_KEYS: [&str; 4] = ["name", "chat", "email", "login"];

/// `team.load()`: `[{name, chat, email, login}]` as strings, or None for any
/// shape whose Python reading this module does not repeat (a value that is
/// not a string, a repeated key, a name `lower()` could read differently).
fn team_rows(store: &Store, root: &Path) -> Option<Vec<[String; 4]>> {
    let path = root.join("work/team.yaml");
    match std::fs::symlink_metadata(&path) {
        Err(_) => return Some(Vec::new()),
        Ok(meta) if !meta.is_file() => return None,
        Ok(_) => {}
    }
    let data = store.read_yaml(&path, Canon::Map(Vec::new()), false).ok()?;
    let pairs = match &data {
        Canon::Map(pairs) => pairs.as_slice(),
        other if !truthy(other) => &[],
        _ => return None,
    };
    let items = match map_get(pairs, "team").ok()? {
        None => return Some(Vec::new()),
        Some(Canon::List(items)) => items.as_slice(),
        Some(other) if !truthy(other) => return Some(Vec::new()),
        Some(_) => return None,
    };
    let mut out = Vec::new();
    for item in items {
        let Canon::Map(fields) = item else {
            // `isinstance(r, dict)` skips it.
            continue;
        };
        let mut row: [String; 4] = Default::default();
        for (slot, key) in row.iter_mut().zip(TEAM_KEYS) {
            *slot = match map_get(fields, key).ok()? {
                None | Some(Canon::Null) => String::new(),
                Some(Canon::Str(text)) => text.clone(),
                Some(_) => return None,
            };
        }
        if row[0].is_empty() {
            continue;
        }
        if !row[0].is_ascii() {
            return None;
        }
        out.push(row);
    }
    Some(out)
}

/// `team._save(rows)`: the YAML text `store.write_yaml` writes, or None
/// when the writer cannot prove it identical.
fn team_yaml(rows: &[[String; 4]]) -> Option<String> {
    let items = rows
        .iter()
        .map(|row| {
            Canon::Map(
                TEAM_KEYS
                    .iter()
                    .zip(row)
                    .filter(|(_, value)| !value.is_empty())
                    .map(|(key, value)| (Canon::Str((*key).to_owned()), Canon::Str(value.clone())))
                    .collect(),
            )
        })
        .collect();
    crate::pyyaml::safe_dump(&Canon::Map(vec![(
        Canon::Str("team".to_owned()),
        Canon::List(items),
    )]))
}

/// True when `path` is absent or a plain file (a lock Python opens with
/// `open("a")`).
fn plain_or_absent(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Err(_) => true,
        Ok(meta) => meta.is_file(),
    }
}

/// True when every part of `root/parts` is absent or a plain folder.
fn dirs_ok(root: &Path, parts: &[&str]) -> bool {
    let mut at = root.to_path_buf();
    for part in parts {
        at = at.join(part);
        if let Ok(meta) = std::fs::symlink_metadata(&at) {
            if !meta.is_dir() {
                return false;
            }
        }
    }
    true
}

/// `open(path, "a")` and close: an empty lock file when it was missing.
fn touch(path: &Path) -> Option<()> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()
        .map(|_| ())
}

/// True when `feed.retriage()` has no event to look at and no rule file of
/// the owner's to read: it then changes nothing and counts 0.
fn retriage_trivial(root: &Path) -> bool {
    dirs_ok(root, &["work", "feed"])
        && absent(&root.join("work/feed/events.jsonl"))
        && absent(&root.join("work/feed/originals.jsonl"))
        && absent(&root.join("work/feed-rules.yaml"))
        && plain_or_absent(&root.join("work/feed/.lock"))
}

/// `feed.retriage()` in the case `retriage_trivial` allows: the folders,
/// the lock file, and the event row. Returns the changed count (0).
fn retriage(ctx: &Ctx) -> Option<usize> {
    make_dirs(&ctx.root, &["work", "feed"])?;
    touch(&ctx.root.join("work/feed/.lock"))?;
    emit(
        ctx,
        "work.feed.retriage",
        &[
            ("changed", Field::Int(0)),
            ("count", Field::Int(0)),
            ("status", Field::Id("ok")),
        ],
    );
    Some(0)
}

/// What a team writer needs before it touches anything: the vault, the
/// roster, and a plain lock path.
fn team_setup(ctx: &Ctx) -> Option<(Context, Vec<[String; 4]>)> {
    let root = &ctx.root;
    if !dirs_ok(root, &["work"]) || !plain_or_absent(&root.join("work/.team.lock")) {
        return None;
    }
    let vault = vault_context()?;
    let rows = team_rows(&vault.store(), root)?;
    Some((vault, rows))
}

/// Takes `work/.team.lock` as `team._locked()` does, and writes the roster
/// text under it when there is one.
fn team_write(ctx: &Ctx, store: &Store, text: Option<&str>) -> Option<()> {
    make_dirs(&ctx.root, &["work"])?;
    let lock = ctx.root.join("work/.team.lock");
    careerbrew_vault::lock::with_exact_lock(&lock, || -> Option<()> {
        if let Some(text) = text {
            store
                .write_text(&ctx.root.join("work/team.yaml"), text)
                .ok()?;
        }
        Some(())
    })
    .ok()?
}

/// `team.FIELDS` a row has, as `cmd_work_team` lists them.
fn ids_of(row: &[String; 4]) -> String {
    let ids = ["chat", "email", "login"]
        .iter()
        .zip(&row[1..])
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, _)| *key)
        .collect::<Vec<_>>()
        .join(", ");
    if ids.is_empty() {
        "a name only".to_owned()
    } else {
        ids
    }
}

/// `work team add NAME [--chat] [--email] [--login]` with names hidden:
/// its `TeamError` lines in any folder; a change only when the data folder
/// is plaintext and the feed has nothing to retriage.
fn team_add(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if !hidden(ctx) {
        return None;
    }
    let raw = [
        args.text("name")?,
        args.text("chat").unwrap_or(""),
        args.text("email").unwrap_or(""),
        args.text("login").unwrap_or(""),
    ];
    if !raw.iter().all(|text| printable(text)) {
        return None;
    }
    let error = |message: &str| cli_error(ctx, "work.team", "teamerror", message);
    let name = one_line(raw[0]);
    if name.len() > 80 {
        return error("a name is at most 80 characters");
    }
    if name.is_empty() {
        return error(
            "a teammate needs a name (fix: careerbrew work team add \"Sam Lee\" --chat U0SAM)",
        );
    }
    let mut given: [String; 3] = Default::default();
    for (i, what) in ["--chat", "--email", "--login"].iter().enumerate() {
        let value = one_line(raw[i + 1]);
        if value.len() > 120 {
            return error(&format!("{what} is at most 120 characters"));
        }
        given[i] = value;
    }
    given[1] = given[1].to_ascii_lowercase();
    given[2] = given[2].trim_start_matches('@').to_owned();
    let chat = given[0].as_bytes();
    if !chat.is_empty()
        && !(matches!(chat[0], b'U' | b'W')
            && (3..=31).contains(&chat.len())
            && chat[1..]
                .iter()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()))
    {
        return error(
            "--chat is a Slack user id such as U0SAM (Slack: the person's profile, More, Copy member ID)",
        );
    }
    if !given[1].is_empty() && !given[1].contains('@') {
        // `{email!r}`: plain single quotes unless it holds a quote or a
        // backslash.
        if given[1].contains(['\'', '"', '\\']) {
            return None;
        }
        return error(&format!("--email '{}' is not an email address", given[1]));
    }
    let (vault, mut rows) = team_setup(ctx)?;
    let store = vault.store();
    let lower = name.to_ascii_lowercase();
    let found = rows
        .iter()
        .position(|row| row[0].to_ascii_lowercase() == lower);
    let added = found.is_none();
    let at = match found {
        Some(at) => at,
        None => {
            rows.push([name.clone(), String::new(), String::new(), String::new()]);
            rows.len() - 1
        }
    };
    let before = rows[at].clone();
    for (i, value) in given.iter().enumerate() {
        if !value.is_empty() {
            rows[at][i + 1] = value.clone();
        }
    }
    let changed = added || rows[at] != before;
    let text = if changed {
        // A sealed write differs on every run; leave it to Python.
        if store.encryption_enabled() || !retriage_trivial(&ctx.root) {
            return None;
        }
        Some(team_yaml(&rows)?)
    } else {
        None
    };
    team_write(ctx, &store, text.as_deref())?;
    emit(
        ctx,
        "work.team",
        &[
            ("action", Field::Id(if added { "add" } else { "set" })),
            ("count", Field::Int(rows.len())),
            ("status", Field::Id("ok")),
        ],
    );
    let ids = ids_of(&rows[at]);
    let moved = if changed {
        format!("feed events retriaged: {} changed", retriage(ctx)?)
    } else {
        "no change".to_owned()
    };
    done(format!("on your team: a teammate ({ids}); {moved}\n"))
}

/// `work team remove NAME`: the "not on your team" error in any folder; a
/// removal only when the data folder is plaintext and the feed has nothing
/// to retriage.
fn team_remove(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let given = args.text("name")?;
    if !printable(given) {
        return None;
    }
    let name = one_line(given).to_ascii_lowercase();
    let (vault, rows) = team_setup(ctx)?;
    let store = vault.store();
    let kept: Vec<[String; 4]> = rows
        .iter()
        .filter(|row| row[0].to_ascii_lowercase() != name)
        .cloned()
        .collect();
    if kept.len() == rows.len() {
        team_write(ctx, &store, None)?;
        return cli_error(
            ctx,
            "work.team",
            "teamerror",
            &format!(
                "that name is not on your team ({} teammate(s); see: careerbrew work team list)",
                rows.len()
            ),
        );
    }
    if store.encryption_enabled() || !retriage_trivial(&ctx.root) {
        return None;
    }
    let text = team_yaml(&kept)?;
    team_write(ctx, &store, Some(&text))?;
    emit(
        ctx,
        "work.team",
        &[
            ("action", Field::Id("remove")),
            ("count", Field::Int(kept.len())),
            ("status", Field::Id("ok")),
        ],
    );
    let changed = retriage(ctx)?;
    done(format!(
        "removed from your team; feed events retriaged: {changed} changed\n"
    ))
}

/// `work feed retriage` with no feed data: nothing to triage again.
fn feed_retriage(ctx: &Ctx) -> Option<Outcome> {
    let root = &ctx.root;
    if !retriage_trivial(root) {
        return None;
    }
    // `team.matcher()` loads the roster; a shape Python cannot load stops it.
    team_rows(&vault_context()?.store(), root)?;
    let changed = retriage(ctx)?;
    done(format!(
        "triaged 0 event(s) with kept originals again: {changed} changed\n"
    ))
}

/// True for a `CAREERBREW_NOW` that `datetime.fromisoformat` reads (or none):
/// `YYYY-MM-DDTHH:MM[:SS]` with no zone, `Z` or `+HH:MM`, in range.
fn clock_ok() -> bool {
    let Ok(value) = std::env::var("CAREERBREW_NOW") else {
        return true;
    };
    if value.is_empty() {
        return true;
    }
    let b = value.as_bytes();
    let num = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = b.get(range)?;
        if !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(part).ok()?.parse().ok()
    };
    let body = || -> Option<usize> {
        let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
        let (hour, minute) = (num(11..13)?, num(14..16)?);
        if b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' {
            return None;
        }
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        if year < 1000 || day == 0 || day > days || hour > 23 || minute > 59 {
            return None;
        }
        if b.get(16) == Some(&b':') {
            if num(17..19)? > 59 {
                return None;
            }
            return Some(19);
        }
        Some(16)
    };
    let Some(at) = body() else {
        return false;
    };
    match &b[at..] {
        b"" | b"Z" => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] => {
            [h1, h2, m1, m2].iter().all(|c| c.is_ascii_digit())
                && (h1 - b'0') * 10 + (h2 - b'0') < 24
                && (m1 - b'0') * 10 + (m2 - b'0') < 60
        }
        _ => false,
    }
}

/// `work team feed` with no feed events: no settings or projects file to
/// read, and a clock Python reads.
fn team_feed(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let root = &ctx.root;
    let days = args.int("days")?;
    args.int("top")?;
    if !(-3650..=3650).contains(&days)
        || !clock_ok()
        || !dirs_ok(root, &["work", "feed"])
        || !absent(&root.join("work/feed/events.jsonl"))
        || !absent(&root.join("work/projects.yaml"))
        || !absent(&root.join("settings.yaml"))
    {
        return None;
    }
    make_dirs(root, &["work", "feed"])?;
    done(format!(
        "no kept teammate events in the last {days} day(s) (add teammates: careerbrew work team add \"<name>\" --chat <id>)\n"
    ))
}

/// True when `feed._locked()` can be taken and finds no events file.
fn feed_lock_no_events(root: &Path) -> bool {
    dirs_ok(root, &["work", "feed"])
        && absent(&root.join("work/feed/events.jsonl"))
        && plain_or_absent(&root.join("work/feed/.lock"))
}

/// `work feed mark ID VALUE` with no feed events: every value finds no row.
fn feed_mark(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let id = args.text("id")?;
    args.text("value")?;
    let root = &ctx.root;
    if !printable(id) || !feed_lock_no_events(root) {
        return None;
    }
    make_dirs(root, &["work", "feed"])?;
    touch(&root.join("work/feed/.lock"))?;
    cli_error(
        ctx,
        "work.feed",
        "feederror",
        &format!("no feed event {id} (see: careerbrew work feed show --show)"),
    )
}

/// `work feed research ID FILE` with no feed events: a JSON file that is
/// not a non-empty list, or any list (no event has that id).
fn feed_research(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let id = args.text("id")?;
    let file = args.text("file")?;
    let root = &ctx.root;
    if !printable(id) || file.is_empty() || file.starts_with('~') || !feed_lock_no_events(root) {
        return None;
    }
    let meta = std::fs::metadata(file).ok()?;
    if !meta.is_file() {
        return None;
    }
    let text = String::from_utf8(std::fs::read(file).ok()?).ok()?;
    if text.starts_with('\u{feff}') {
        return None;
    }
    // A text the reader rejects may still be JSON to Python: refuse it.
    let entries = careerbrew_vault::json_yaml::json_read(&text).ok()?;
    if !matches!(&entries, Canon::List(items) if !items.is_empty()) {
        return cli_error(
            ctx,
            "work.feed",
            "feederror",
            "research is a JSON list: one {url, summary} or {url, opened: false, reason} per link",
        );
    }
    make_dirs(root, &["work", "feed"])?;
    touch(&root.join("work/feed/.lock"))?;
    cli_error(
        ctx,
        "work.feed",
        "feederror",
        &format!("no feed event {id} (see: careerbrew work feed links)"),
    )
}

// ---------- wave 7: the FeedError lines of work feed add, transcript and cycle ----------

const NO_TOOL_MAP: &str = "no tool map (fix: careerbrew config set feed_tool_sources 'mcp__chat__*=chat, mcp__tickets__*=tickets')";
const NO_TRANSCRIPT: &str =
    "the transcript file does not exist (fix: pass the session's .jsonl file)";
const NO_REQUIRED: &str =
    "no required sources (fix: --require slack,jira,mail, or careerbrew config set feed_tool_sources)";
const LINE_FIX: &str = "(fix: one JSON object per line with id, at, and title)";

/// How many entries `config.get("feed_tool_sources")` has: 0 for no
/// setting (the default `[]`) or an empty list, the count for a list of
/// strings, None for any other value or a read error.
fn tool_source_count(root: &Path) -> Option<usize> {
    let path = root.join("settings.yaml");
    if absent(&path) {
        return Some(0);
    }
    let context = vault_context()?;
    let data = context
        .store()
        .read_yaml(&path, Canon::Map(Vec::new()), false)
        .ok()?;
    let pairs = match &data {
        Canon::Map(pairs) => pairs.as_slice(),
        other if !truthy(other) => &[],
        _ => return None,
    };
    match map_get(pairs, "feed_tool_sources").ok()? {
        None => Some(0),
        Some(Canon::List(items)) if items.iter().all(|i| matches!(i, Canon::Str(_))) => {
            Some(items.len())
        }
        Some(_) => None,
    }
}

/// `work feed transcript FILE`: `transcript.store` raises before it reads
/// the file when no tool map is set, and before it reads anything else when
/// the file is not there.
fn feed_transcript(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let file = args.text("file")?;
    let root = &ctx.root;
    let tools = tool_source_count(root)?;
    if tools == 0 {
        make_dirs(root, &[])?;
        return cli_error(ctx, "work.feed", "feederror", NO_TOOL_MAP);
    }
    // `Path.expanduser`, and `Path("")` (the current folder), go to Python.
    if file.is_empty() || file.starts_with('~') || Path::new(file).is_file() {
        return None;
    }
    make_dirs(root, &[])?;
    cli_error(ctx, "work.feed", "feederror", NO_TRANSCRIPT)
}

/// `work feed cycle` when `feed.mark_cycle` finds no required source: no
/// tool map, no owner live sources, no plugin, and a `--require` with no
/// name in it.
fn feed_cycle(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    // `[x.strip() for x in a.require.split(",") if x.strip()]` is empty.
    if let Some(require) = args.text("require") {
        if !require.chars().all(|c| matches!(c, ',' | ' ' | '\t')) {
            return None;
        }
    }
    let root = &ctx.root;
    if !absent(&root.join("work/live-sources.yaml"))
        || !absent(&root.join("plugins.lock.json"))
        || !absent(&root.join("plugins"))
    {
        return None;
    }
    if tool_source_count(root)? != 0 {
        return None;
    }
    make_dirs(root, &[])?;
    cli_error(ctx, "work.feed", "feederror", NO_REQUIRED)
}

/// The first character of a text `json.loads` can read as a value (after
/// its own whitespace): a string, an object, an array, `null`, `true`,
/// `false`, `NaN`, `Infinity`, or a number.
fn json_start(c: char) -> bool {
    matches!(c, '"' | '{' | '[' | 'n' | 't' | 'f' | 'N' | 'I' | '-') || c.is_ascii_digit()
}

/// `work feed add FILE` (no `--plugin`, no `--source`): Python reads the
/// file, `json.loads` of the whole text fails, and then either no line has
/// an event ("no events in the input") or the first non-blank line is not
/// JSON. Any input that could hold an event goes to Python.
fn feed_add(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let file = args.text("file")?;
    if args.text("plugin").is_some_and(|p| !p.is_empty())
        || args.text("source").is_some_and(|s| !s.is_empty())
    {
        return None;
    }
    // `-` reads stdin; `~` is `expanduser`.
    if file.is_empty() || file == "-" || file.starts_with('~') {
        return None;
    }
    let meta = std::fs::metadata(file).ok()?;
    if !meta.is_file() {
        return None;
    }
    let text = String::from_utf8(std::fs::read(file).ok()?).ok()?;
    // `read_text` turns `\r` into `\n`, and `str.splitlines` also splits
    // on these; `str.strip` and `str.trim` differ on \x1c to \x1f.
    if text.chars().any(|c| {
        matches!(
            c,
            '\r' | '\u{0b}' | '\u{0c}' | '\u{1c}'..='\u{1f}' | '\u{85}' | '\u{2028}' | '\u{2029}'
        )
    }) {
        return None;
    }
    let mut verdict = None;
    for (n, line) in text.split('\n').enumerate() {
        if line.trim().is_empty() {
            // Blank to `str.strip`; JSON whitespace too only when it is
            // spaces and tabs.
            if line.chars().any(|c| c != ' ' && c != '\t') {
                return None;
            }
            continue;
        }
        let first = line.trim_start_matches([' ', '\t']).chars().next()?;
        if json_start(first) {
            return None;
        }
        verdict = Some(n + 1);
        break;
    }
    let message = match verdict {
        Some(n) => format!("line {n}: not JSON {LINE_FIX}"),
        None => format!("no events in the input {LINE_FIX}"),
    };
    make_dirs(&ctx.root, &[])?;
    cli_error(ctx, "work.feed", "feederror", &message)
}
