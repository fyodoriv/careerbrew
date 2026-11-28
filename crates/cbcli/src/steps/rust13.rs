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
//! Every case also refuses when Python's `privacy.redact` would read an
//! input (`Ctx::redact_inputs_present`).
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;
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
