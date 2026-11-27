//! Native CLI handlers owned by `rust-21` (see `steps/mod.rs`): the
//! teacher's written feedback, its improvement list when there is nothing to
//! propose, and the goal guard for callers without a terminal.
//!
//! Each handler refuses (Python then runs the command) on a redaction input,
//! a corporate machine role, a pinned clock it cannot read, or any input
//! where Python's text handling differs from Rust's. A refusal always comes
//! before a write.
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
