//! Native CLI handlers owned by `rust-09` (see `steps/mod.rs`): `work
//! task-set` (`priorities.regrade`) and `work done` (`priorities.done`),
//! which rewrite `work/tasks.yaml`, and `work done` also appends one
//! `metrics.jsonl` row.
//!
//! They serve only inputs whose result is certain on both sides:
//! - the data root is plaintext (a sealed write differs on every run);
//! - `work/tasks.yaml` is absent, or its text is exactly what
//!   `pyyaml::safe_dump` writes for the value it reads (so the rewrite
//!   changes nothing but the edited fields), a mapping whose `tasks` is a
//!   list of mappings, each with an `id`, and none with an old `link`
//!   (which `priorities.load` would migrate);
//! - every value they print is an int, a plain text, or a float whose
//!   `repr` this module writes the same way (0, or 0.001 to 1e15);
//! - every argument is printable ASCII, and the task id holds no quote
//!   or backslash (a `KeyError` prints its `repr`).
//!
//! Anything else returns None before any write, and Python runs it.
use super::rust10::work::{self, get, plain, quote, served, Pairs};
use super::Ctx;
use crate::args::{Parsed, Value};
use crate::Outcome;
use careerbrew_vault::canon::Canon;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust09.rs`.
pub const COMMANDS: &[&str] = &["work.task-set", "work.done"];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    // `say` prints through `privacy.redact`, which reads these files.
    if ctx.redact_inputs_present() {
        return None;
    }
    match name {
        "work.task-set" => task_set(args, ctx),
        "work.done" => done(args, ctx),
        _ => None,
    }
}

const MAX_HOURS: f64 = 200.0;
const MAX_RATIO: f64 = 10.0;

/// `repr(float)` for the values this module prints, else None.
fn float_repr(value: f64) -> Option<String> {
    if !value.is_finite() || value < 0.0 || (value != 0.0 && !(1e-3..1e15).contains(&value)) {
        return None;
    }
    Some(careerbrew_career::py::float_repr(value))
}

/// `str(value)` for a stored value this module prints, else None.
fn shown(value: &Canon) -> Option<String> {
    match value {
        Canon::Int(n) => Some(n.to_string()),
        Canon::Float(f) => float_repr(*f),
        Canon::Str(s) if plain(s) => Some(s.clone()),
        _ => None,
    }
}

/// `cli.main` on a caught error: `error: <message>`, exit 2, one
/// `cli.error` row and no `cli.run` row.
fn cli_error(ctx: &Ctx, command: &str, class: &str, message: &str) -> Option<Outcome> {
    work::emit(
        ctx,
        "cli.error",
        &[
            ("command", quote(command)),
            ("error", quote(class)),
            ("via", quote(ctx.env.caps.via)),
        ],
    );
    Some(Outcome {
        stdout: format!("error: {message}\n"),
        stderr: String::new(),
        code: 2,
        emit_event: false,
    })
}

/// The `KeyError` an unknown task raises, as `cli.main` prints it.
fn unknown_task(ctx: &Ctx, command: &str, id: &str) -> Option<Outcome> {
    cli_error(
        ctx,
        command,
        "keyerror",
        &format!("'unknown task {id} (see `careerbrew work plan`)'"),
    )
}

/// `priorities.hours(value)`: the hours, or the `ValueError` text.
fn hours(text: &str) -> Option<Result<f64, String>> {
    let h: f64 = text.parse().ok()?;
    if h.is_finite() && (0.05..=MAX_HOURS).contains(&h) {
        return Some(Ok(h));
    }
    let shown = float_repr(h)?;
    Some(Err(format!(
        "hours must be a number from 0.05 to 200, not {shown}"
    )))
}

fn number<'a>(args: &'a Parsed, name: &str) -> Option<Option<&'a str>> {
    match args.get(name) {
        Value::Missing => Some(None),
        Value::Number(text) => Some(Some(text)),
        _ => None,
    }
}

/// A task id this module may compare and print.
fn plain_id(id: &str) -> bool {
    !id.is_empty() && plain(id) && !id.contains('\'') && !id.contains('"') && !id.contains('\\')
}

struct Tasks {
    path: PathBuf,
    vault: careerbrew_vault::transfer::Context,
    tasks: Vec<Pairs>,
}

/// `priorities.load()` in a plaintext data root, when the file is in the
/// exact form `safe_dump` writes and no task needs a migration.
fn load(ctx: &Ctx) -> Option<Tasks> {
    let vault = work::vault()?;
    let store = vault.store();
    if store.encryption_enabled() {
        return None;
    }
    let path = ctx.root.join("work").join("tasks.yaml");
    let raw = work::read_text(&store, &path)?;
    drop(store);
    let tasks = match raw {
        None => Vec::new(),
        Some(raw) => {
            let value = careerbrew_vault::json_yaml::yaml_read(&raw).ok()?;
            if crate::pyyaml::safe_dump(&value)? != raw {
                return None;
            }
            let Canon::Map(top) = value else {
                return None;
            };
            if !work::keys_unique(&top) {
                return None;
            }
            match get(&top, "tasks") {
                None => Vec::new(),
                Some(Canon::List(items)) => {
                    let mut out = Vec::new();
                    for item in items {
                        let Canon::Map(t) = item else { return None };
                        if !work::keys_unique(t)
                            || get(t, "id").is_none()
                            || get(t, "link").is_some()
                        {
                            return None;
                        }
                        out.push(t.clone());
                    }
                    out
                }
                Some(_) => return None,
            }
        }
    };
    Some(Tasks { path, vault, tasks })
}

/// `t["id"] == task_id`.
fn is_task(t: &Pairs, id: &str) -> bool {
    matches!(get(t, "id"), Some(Canon::Str(own)) if own == id)
}

/// `t[key] = value`: an existing key keeps its place, a new one goes last.
fn set(t: &mut Pairs, key: &str, value: Canon) {
    match t
        .iter_mut()
        .find(|(k, _)| matches!(k, Canon::Str(k) if k == key))
    {
        Some((_, old)) => *old = value,
        None => t.push((Canon::Str(key.to_owned()), value)),
    }
}

/// `priorities.save(tasks)`: the text `store.write_yaml` writes.
fn dump(tasks: &[Pairs]) -> Option<String> {
    let list = tasks.iter().cloned().map(Canon::Map).collect();
    crate::pyyaml::safe_dump(&Canon::Map(vec![(
        Canon::Str("tasks".into()),
        Canon::List(list),
    )]))
}

fn write(tasks: &Tasks, text: &str) -> Option<()> {
    let parent = tasks.path.parent()?;
    std::fs::create_dir_all(parent).ok()?;
    tasks.vault.store().write_text(&tasks.path, text).ok()
}

// ---------- work task-set ----------

fn task_set(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let id = args.text("task")?;
    if !plain_id(id) {
        return None;
    }
    let (impact, urgency) = (args.int("impact"), args.int("urgency"));
    let given = |name| !matches!(args.get(name), Value::Missing);
    if (given("impact") && impact.is_none()) || (given("urgency") && urgency.is_none()) {
        return None;
    }
    let estimate = number(args, "hours")?;
    if impact.is_none() && urgency.is_none() && estimate.is_none() {
        return served(
            "nothing to change: pass --impact, --urgency, or --hours\n".into(),
            2,
        );
    }
    for (name, value) in [("impact", impact), ("urgency", urgency)] {
        if value.is_some_and(|v| !(1..=5).contains(&v)) {
            return cli_error(
                ctx,
                "work.task-set",
                "valueerror",
                &format!("{name} is 1 to 5"),
            );
        }
    }
    let mut tasks = load(ctx)?;
    let Some(at) = tasks.tasks.iter().position(|t| is_task(t, id)) else {
        return unknown_task(ctx, "work.task-set", id);
    };
    let t = &mut tasks.tasks[at];
    if let Some(v) = impact {
        set(t, "impact", Canon::Int(v.into()));
    }
    if let Some(v) = urgency {
        set(t, "urgency", Canon::Int(v.into()));
    }
    if let Some(text) = estimate {
        match hours(text)? {
            Ok(h) => set(t, "estimate_h", Canon::Float(h)),
            Err(message) => return cli_error(ctx, "work.task-set", "valueerror", &message),
        }
    }
    let line = format!(
        "{id}: impact {}, urgency {}, {}h (the plan and the ranks use it now)\n",
        shown(get(t, "impact")?)?,
        shown(get(t, "urgency")?)?,
        shown(get(t, "estimate_h")?)?
    );
    let text = dump(&tasks.tasks)?;
    write(&tasks, &text)?;
    work::emit(
        ctx,
        "work.task",
        &[("action", quote("regrade")), ("status", quote("ok"))],
    );
    served(line, 0)
}

// ---------- work done ----------

/// `priorities.estimate(t)` for an int or float estimate (or none).
fn estimate(t: &Pairs) -> Option<f64> {
    let h = match get(t, "estimate_h") {
        None => 1.0,
        Some(Canon::Int(n)) => n.to_string().parse().ok()?,
        Some(Canon::Float(f)) => *f,
        Some(_) => return None,
    };
    Some(if h.is_finite() && h > 0.0 && h <= MAX_HOURS {
        h
    } else {
        1.0
    })
}

fn done(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let id = args.text("id")?;
    let Value::Number(raw) = args.get("hours") else {
        return None;
    };
    if !plain_id(id) {
        return None;
    }
    let actual = match hours(raw)? {
        Ok(h) => h,
        Err(message) => return cli_error(ctx, "work.done", "valueerror", &message),
    };
    let mut tasks = load(ctx)?;
    let Some(at) = tasks.tasks.iter().position(|t| is_task(t, id)) else {
        return unknown_task(ctx, "work.done", id);
    };
    let t = &mut tasks.tasks[at];
    if matches!(get(t, "status"), Some(Canon::Str(s)) if s == "done") {
        return cli_error(
            ctx,
            "work.done",
            "valueerror",
            &format!("task {id} is already done"),
        );
    }
    // Only a text status (or none) compares as Python does.
    if !matches!(get(t, "status"), None | Some(Canon::Str(_))) {
        return None;
    }
    let source = match get(t, "source") {
        None | Some(Canon::Null) => "user".to_owned(),
        Some(Canon::Str(s)) if s.is_empty() => "user".to_owned(),
        Some(Canon::Str(s)) if plain(s) => s.clone(),
        Some(_) => return None,
    };
    let settings = work::read_map_yaml(&tasks.vault.store(), &ctx.root.join("settings.yaml"))?;
    let clock = work::clock(&settings)?;
    let done_at = careerbrew_career::when::iso_seconds(&clock.at);
    set(t, "status", Canon::Str("done".into()));
    set(t, "actual_h", Canon::Float(actual));
    set(t, "done_at", Canon::Str(done_at.clone()));
    let planned = shown(get(t, "estimate_h")?)?;
    let ratio = MAX_RATIO.min(actual / estimate(t)?.max(0.25));
    // A ratio exactly halfway between two 3-place decimals: Python's
    // `round` and the formatter must agree on the tie, so leave it.
    if (ratio * 16.0).fract() == 0.0 && (ratio * 1000.0).fract() != 0.0 {
        return None;
    }
    let value = float_repr(careerbrew_projects::python_round(ratio, 3))?;
    let actual_text = float_repr(actual)?;
    let text = dump(&tasks.tasks)?;
    let metrics = ctx.root.join("metrics.jsonl");
    let row = format!(
        "{{\"at\": {}, \"metric\": \"estimate_ratio\", \"source\": {}, \"task\": {}, \"value\": {value}}}\n",
        quote(&done_at),
        quote(&source),
        quote(id)
    );
    write(&tasks, &text)?;
    append(&metrics, &row)?;
    served(
        format!("done {id} ({actual_text}h vs {planned}h planned)\n"),
        0,
    )
}

/// `store.append_jsonl` for a plaintext file: under its `.<name>.lock`.
fn append(path: &Path, row: &str) -> Option<()> {
    careerbrew_vault::lock::append_jsonl_text(path, None, false, row).ok()
}
