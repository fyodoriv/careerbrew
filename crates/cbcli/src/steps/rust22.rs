//! Native CLI handlers owned by `rust-22` (see `steps/mod.rs`): the
//! exercise list.
//!
//! `exercises list` reads the exercise catalog (plain YAML in the repo, or
//! `CAREERBREW_EXERCISES`), the owner's attempts and the synced solved list.
//! It refuses (Python then runs it) on a redaction input, a corporate machine
//! role, a catalog entry Python could list or format differently (a hidden
//! or non-file `*.yaml`, an order that is not a whole number, a text field
//! that is not text), and an attempt row Python would crash on.
//!
//! Left in Python: `exercises run`, `submit`, `grade` and `sync` run learner
//! code or reach an account; they have no parity cases in this step.
//!
//! The small value helpers here are private copies of the ones in
//! `rust20.rs`: step modules may not share code outside their own file.
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;
use careerbrew_vault::canon::Canon;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust22.rs`.
pub const COMMANDS: &[&str] = &["exercises.list"];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, _args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| serve(name, ctx)))
        .ok()
        .flatten()
}

fn serve(name: &str, ctx: &Ctx) -> Option<Outcome> {
    if name != "exercises.list" || ctx.redact_inputs_present() {
        return None;
    }
    let vault = careerbrew_vault::transfer::Context::from_env().ok()?;
    if vault.root != ctx.root {
        return None;
    }
    personal(&vault)?;
    list(&vault)
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

fn text<'a>(value: &'a Canon, key: &str) -> Option<&'a str> {
    match get(value, key) {
        Some(Canon::Str(text)) => Some(text),
        _ => None,
    }
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

/// `exercises.exercises_dir()`: `CAREERBREW_EXERCISES` when set and not
/// empty, else the checkout's `knowledge/exercises`.
fn exercises_dir() -> Option<PathBuf> {
    match std::env::var("CAREERBREW_EXERCISES") {
        Ok(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        Ok(_) | Err(std::env::VarError::NotPresent) => {
            let repo = match std::env::var("CAREERBREW_NATIVE_REPO") {
                Ok(repo) if repo.starts_with('/') => PathBuf::from(repo),
                Ok(_) => return None,
                Err(_) => Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .canonicalize()
                    .ok()?,
            };
            Some(repo.join("knowledge/exercises"))
        }
        Err(_) => None,
    }
}

/// One catalog row: order, pattern, difficulty, id.
struct Exercise {
    order: i64,
    pattern: String,
    difficulty: String,
    id: String,
}

/// `exercises.load()`: `sorted(dir.glob("*.yaml"))`, parsed, then a stable
/// sort by `order`.
fn load() -> Option<Vec<Exercise>> {
    let dir = exercises_dir()?;
    if !dir.is_dir() {
        return Some(Vec::new());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&dir).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name().into_string().ok()?;
        if !name.ends_with(".yaml") {
            continue;
        }
        if name.starts_with('.') || !entry.path().is_file() {
            return None;
        }
        names.push(name);
    }
    names.sort();
    let mut out = Vec::new();
    for name in names {
        let raw = std::fs::read(dir.join(name)).ok()?;
        if raw.starts_with(careerbrew_vault::MAGIC) {
            return None;
        }
        let data = careerbrew_vault::json_yaml::yaml_read(&String::from_utf8(raw).ok()?).ok()?;
        if !plain_map(&data) {
            return None;
        }
        let order = match get(&data, "order")? {
            Canon::Int(order) => order.to_string().parse().ok()?,
            _ => return None,
        };
        out.push(Exercise {
            order,
            pattern: text(&data, "pattern")?.to_owned(),
            difficulty: text(&data, "difficulty")?.to_owned(),
            id: text(&data, "id")?.to_owned(),
        });
    }
    out.sort_by_key(|exercise| exercise.order);
    Some(out)
}

#[derive(Default)]
struct Status {
    attempts: usize,
    solved: bool,
    synced: bool,
}

fn is_number(value: &Canon) -> bool {
    matches!(value, Canon::Int(_)) || matches!(value, Canon::Float(f) if f.is_finite())
}

/// `exercises.status()`, the part the list prints: attempts, solved, synced.
fn status(vault: &careerbrew_vault::transfer::Context) -> Option<BTreeMap<String, Status>> {
    let store = vault.store();
    let mut out: BTreeMap<String, Status> = BTreeMap::new();
    let synced = store
        .read_json(
            &vault.root.join("tutor/synced-solved.json"),
            Canon::Null,
            false,
        )
        .ok()?;
    let synced = match &synced {
        Canon::Null => Vec::new(),
        Canon::List(items) if items.is_empty() => Vec::new(),
        Canon::Map(pairs) if pairs.is_empty() => Vec::new(),
        data if plain_map(data) => match get(data, "exercises") {
            None => Vec::new(),
            Some(Canon::List(items)) => items
                .iter()
                .map(|item| match item {
                    Canon::Str(id) => Some(id.clone()),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?,
            Some(_) => return None,
        },
        _ => return None,
    };
    let rows = store
        .read_jsonl(&vault.root.join("tutor/exercise-attempts.jsonl"), false)
        .ok()?;
    for row in &rows {
        if !plain_map(row) {
            return None;
        }
        let id = text(row, "exercise")?;
        if !get(row, "score").is_some_and(is_number) {
            return None;
        }
        let correct = match get(row, "correct")? {
            Canon::Bool(correct) => *correct,
            _ => return None,
        };
        match get(row, "seconds") {
            None | Some(Canon::Null) => {}
            Some(seconds) if is_number(seconds) => {}
            Some(_) => return None,
        }
        let entry = out.entry(id.to_owned()).or_default();
        entry.attempts += 1;
        entry.solved |= correct;
    }
    for id in synced {
        out.entry(id).or_default().synced = true;
    }
    Some(out)
}

fn list(vault: &careerbrew_vault::transfer::Context) -> Option<Outcome> {
    let status = status(vault)?;
    let mut out = String::new();
    for exercise in load()? {
        let mark = match status.get(&exercise.id) {
            Some(s) if s.solved => "solved".to_owned(),
            Some(s) if s.synced => "synced".to_owned(),
            Some(s) if s.attempts > 0 => format!("{} tries", s.attempts),
            _ => String::new(),
        };
        out.push_str(&format!(
            "{:>3}  {:<24} {:<6} {:<36} {mark}\n",
            exercise.order, exercise.pattern, exercise.difficulty, exercise.id
        ));
    }
    Some(Outcome {
        stdout: out,
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}
