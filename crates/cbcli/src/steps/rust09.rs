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
//!
//! `work priority show`, `set` and `remove` (`project_priority`) live in
//! the `priority` module below, with their own limits.
use super::rust10::work::{self, get, plain, quote, served, Pairs};
use super::Ctx;
use crate::args::{Parsed, Value};
use crate::Outcome;
use careerbrew_vault::canon::Canon;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust09.rs`.
pub const COMMANDS: &[&str] = &[
    "work.task-set",
    "work.done",
    "work.priority.show",
    "work.priority.set",
    "work.priority.remove",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    // `say` prints through `privacy.redact`, which reads these files.
    if ctx.redact_inputs_present() {
        return None;
    }
    match name {
        "work.task-set" => task_set(args, ctx),
        "work.done" => done(args, ctx),
        "work.priority.show" | "work.priority.set" | "work.priority.remove" => {
            priority::run(name, args, ctx)
        }
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

/// `work priority show`, `set` and `remove`: `cmd_work_priority` over
/// `project_priority` and the `project_priority` setting (`config`).
///
/// They serve only these inputs, and refuse the rest before any write:
/// - the data root is plaintext, and `settings.yaml` is absent or exactly
///   what `pyyaml::safe_dump` writes for the mapping it reads;
/// - `project_priority` is absent, null, or a list of printable ASCII texts;
/// - every argument is printable ASCII, and a project name the validator
///   rejects holds no quote or backslash (its error prints its `repr`);
/// - the summary's inputs give fixed counts: no `work/projects.yaml` (no
///   project links a task), no day results file for today, `work/tasks.yaml`
///   in the form `load` reads, no open task with links or a title that may
///   hold a ticket key (a `-` before a digit), and no pattern with `[`.
mod priority {
    use super::super::rust10::work::{self, get, keys_unique, plain, quote, served, Pairs};
    use super::super::Ctx;
    use crate::args::{Parsed, Value};
    use crate::Outcome;
    use careerbrew_vault::canon::Canon;
    use std::path::PathBuf;

    const KEY: &str = "project_priority";

    const SAVED: &str = "saved: the PM update and the web app use it now; rebuild today's saved plan for `day next` with: careerbrew day plan\n";

    struct Entry {
        name: String,
        globs: Vec<String>,
    }

    struct Settings {
        path: PathBuf,
        vault: careerbrew_vault::transfer::Context,
        pairs: Pairs,
    }

    /// `config_io.read_settings()` in a plaintext data root, when the file
    /// is in the exact form `store.write_yaml` writes.
    fn settings(ctx: &Ctx) -> Option<Settings> {
        let vault = work::vault()?;
        let store = vault.store();
        if store.encryption_enabled() {
            return None;
        }
        let path = ctx.root.join("settings.yaml");
        let raw = work::read_text(&store, &path)?;
        drop(store);
        let pairs = match raw {
            None => Vec::new(),
            Some(raw) => {
                let value = careerbrew_vault::json_yaml::yaml_read(&raw).ok()?;
                if crate::pyyaml::safe_dump(&value)? != raw {
                    return None;
                }
                match value {
                    Canon::Map(pairs) if keys_unique(&pairs) => pairs,
                    _ => return None,
                }
            }
        };
        Some(Settings { path, vault, pairs })
    }

    /// One `name=glob|glob` text as `project_priority.entries()` reads it.
    fn entry_of(text: &str) -> Entry {
        let (name, pats) = text.split_once('=').unwrap_or((text, ""));
        let mut globs: Vec<String> = pats
            .split('|')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect();
        if globs.is_empty() {
            globs.push(name.to_owned());
        }
        Entry {
            name: name.to_owned(),
            globs,
        }
    }

    /// `project_priority.entries()`, rank order.
    fn entries(settings: &Settings) -> Option<Vec<Entry>> {
        let raw = match get(&settings.pairs, KEY) {
            None | Some(Canon::Null) => return Some(Vec::new()),
            Some(Canon::List(items)) => items,
            Some(_) => return None,
        };
        let mut out = Vec::new();
        for item in raw {
            let Canon::Str(text) = item else { return None };
            if !plain(text) {
                return None;
            }
            out.push(entry_of(text));
        }
        Some(out)
    }

    /// `_text(e)`: the bare name when its only pattern is the name.
    fn entry_text(e: &Entry) -> String {
        if e.globs.len() == 1 && e.globs[0] == e.name {
            e.name.clone()
        } else {
            format!("{}={}", e.name, e.globs.join("|"))
        }
    }

    /// `[a-z0-9][a-z0-9-]{0,39}`, whole.
    fn good_name(name: &str) -> bool {
        let b = name.as_bytes();
        (1..=40).contains(&b.len())
            && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
            && b.iter()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
    }

    /// `config._priority(v)` for printable ASCII: the stored list, or the
    /// `ValueError` text (None when its `repr` would need escapes).
    fn validate(v: &str) -> Option<Result<Vec<String>, String>> {
        let mut out: Vec<String> = Vec::new();
        for raw in v.split(',').map(|x| x.trim_matches(' ')) {
            if raw.is_empty() {
                continue;
            }
            let (name, eq, pats) = match raw.split_once('=') {
                Some((name, pats)) => (name.trim_matches(' '), true, pats.trim_matches(' ')),
                None => (raw, false, ""),
            };
            let globs: Vec<&str> = pats
                .split('|')
                .map(|g| g.trim_matches(' '))
                .filter(|g| !g.is_empty())
                .collect();
            if !good_name(name) {
                if name.contains(['\'', '"', '\\']) {
                    return None;
                }
                return Some(Err(format!(
                    "project '{name}': use lowercase letters, digits, and dashes (max 40), for example tools=*/repo-a|*/repo-b"
                )));
            }
            if out.iter().any(|x| x.split('=').next() == Some(name)) {
                return Some(Err(format!(
                    "project {name} is listed twice; each project has one rank"
                )));
            }
            if eq && globs.is_empty() {
                return Some(Err(format!(
                    "project {name}: no pattern after '=' (fix: {name}=PROJ-* or just {name})"
                )));
            }
            if globs.iter().any(|g| g.contains([' ', '=']) || g.len() > 80) {
                return Some(Err(format!(
                    "project {name}: a pattern has no spaces or '=' and at most 80 characters"
                )));
            }
            out.push(if globs.is_empty() {
                name.to_owned()
            } else {
                format!("{name}={}", globs.join("|"))
            });
        }
        if out.len() > 20 {
            return Some(Err("at most 20 projects".into()));
        }
        Some(Ok(out))
    }

    /// `fnmatch.fnmatchcase` for a pattern whose only wildcards are `*` and
    /// `?` (no `[`), on ASCII text.
    fn glob(text: &[u8], pat: &[u8]) -> bool {
        let (mut t, mut p) = (0, 0);
        let (mut star, mut mark) = (None, 0);
        while t < text.len() {
            if p < pat.len() && pat[p] == b'*' {
                star = Some(p);
                mark = t;
                p += 1;
            } else if p < pat.len() && (pat[p] == b'?' || pat[p] == text[t]) {
                t += 1;
                p += 1;
            } else if let Some(s) = star {
                p = s + 1;
                mark += 1;
                t = mark;
            } else {
                return false;
            }
        }
        pat[p..].iter().all(|c| *c == b'*')
    }

    /// The open-task count per project that `summary()` makes (its day
    /// count is 0: no results file), or None when an input could give
    /// another count in Python.
    fn summary(ctx: &Ctx, settings: &Settings, es: &[Entry]) -> Option<Vec<usize>> {
        if es
            .iter()
            .any(|e| e.globs.iter().any(|g| g.contains('[') || !plain(g)))
        {
            return None;
        }
        let work_dir = ctx.root.join("work");
        if work_dir.join("projects.yaml").exists() {
            return None;
        }
        let clock = work::clock(&settings.pairs)?;
        let today = careerbrew_career::when::strftime(&clock.at, "%Y-%m-%d");
        let day = ctx.root.join("day");
        if day.join(format!("{today}.jsonl")).exists() {
            return None;
        }
        let tasks = super::load(ctx)?;
        let mut counts = vec![0; es.len()];
        for t in &tasks.tasks {
            let open = match get(t, "status") {
                None => true,
                Some(Canon::Str(s)) => s == "open",
                Some(_) => false,
            };
            if !open || es.is_empty() {
                continue;
            }
            match get(t, "links") {
                None | Some(Canon::Null) => {}
                Some(Canon::List(items)) if items.is_empty() => {}
                Some(_) => return None,
            }
            let title = match get(t, "title") {
                None | Some(Canon::Null) => "",
                Some(Canon::Str(s)) if plain(s) => s.as_str(),
                Some(_) => return None,
            };
            let b = title.as_bytes();
            if b.windows(2).any(|w| w[0] == b'-' && w[1].is_ascii_digit()) {
                return None;
            }
            if title.is_empty() {
                continue;
            }
            let key = title.to_ascii_lowercase();
            if let Some(at) = es.iter().position(|e| {
                e.globs
                    .iter()
                    .any(|g| glob(key.as_bytes(), g.to_ascii_lowercase().as_bytes()))
            }) {
                counts[at] += 1;
            }
        }
        // `paths.sub` makes these folders on the Python side.
        std::fs::create_dir_all(&work_dir).ok()?;
        std::fs::create_dir_all(&day).ok()?;
        Some(counts)
    }

    fn summary_text(es: &[Entry], counts: &[usize]) -> String {
        if es.is_empty() {
            return "no project priority yet: careerbrew work priority set <project> --match '<glob>' (globs match repos like */agentbrew, ticket keys like PROJ-*, or titles like *invoice*)\n".into();
        }
        let mut out = String::from(
            "project priority, rank 1 first (the PM update, the plan, and Next step follow it):\n",
        );
        for (i, (e, open)) in es.iter().zip(counts).enumerate() {
            out.push_str(&format!(
                "  {}. {:<16} {:<32} {} open task{}, 0 items today\n",
                i + 1,
                e.name,
                e.globs.join(", "),
                open,
                if *open == 1 { "" } else { "s" },
            ));
        }
        out.push_str("change it: careerbrew work priority set <project> --rank <n> [--match <glob> ...], or careerbrew work priority remove <project>\n");
        out
    }

    /// `cli.main` on a caught `ValueError`.
    fn value_error(ctx: &Ctx, message: &str) -> Option<Outcome> {
        work::emit(
            ctx,
            "cli.error",
            &[
                ("command", quote("work.priority")),
                ("error", quote("valueerror")),
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

    /// The settings text with `project_priority` set to `list`, or removed.
    fn new_settings(settings: &Settings, list: Option<&[String]>) -> Option<String> {
        let mut pairs = settings.pairs.clone();
        match list {
            Some(list) => {
                let value = Canon::List(list.iter().cloned().map(Canon::Str).collect());
                match pairs
                    .iter_mut()
                    .find(|(k, _)| matches!(k, Canon::Str(k) if k == KEY))
                {
                    Some((_, old)) => *old = value,
                    None => pairs.push((Canon::Str(KEY.into()), value)),
                }
            }
            None => pairs.retain(|(k, _)| !matches!(k, Canon::Str(k) if k == KEY)),
        }
        crate::pyyaml::safe_dump(&Canon::Map(pairs))
    }

    /// `config.set_value` / `config.unset`: the write under the settings lock.
    fn write(settings: &Settings, text: &str) -> Option<()> {
        careerbrew_vault::lock::with_lock(&settings.path, || {
            settings.vault.store().write_text(&settings.path, text).ok()
        })
        .ok()?
    }

    pub(super) fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
        let settings = settings(ctx)?;
        let es = entries(&settings)?;
        match name {
            "work.priority.show" => {
                let counts = summary(ctx, &settings, &es)?;
                served(summary_text(&es, &counts), 0)
            }
            "work.priority.set" => set(args, ctx, &settings, es),
            "work.priority.remove" => remove(args, ctx, &settings, es),
            _ => None,
        }
    }

    fn set(args: &Parsed, ctx: &Ctx, settings: &Settings, es: Vec<Entry>) -> Option<Outcome> {
        let project = args.text("project")?;
        let globs = args.list("match");
        if !plain(project) || globs.iter().any(|g| !plain(g)) {
            return None;
        }
        let rank = match args.get("rank") {
            Value::Missing => None,
            Value::Int(n) => Some(*n),
            _ => return None,
        };
        if rank.is_some_and(|r| r < 1) {
            return value_error(ctx, "rank starts at 1 (the top project)");
        }
        let cur = es.iter().position(|e| e.name == project);
        let new = Entry {
            name: project.to_owned(),
            globs: if !globs.is_empty() {
                globs.to_vec()
            } else if let Some(at) = cur {
                es[at].globs.clone()
            } else {
                vec![project.to_owned()]
            },
        };
        let mut others: Vec<Entry> = es.into_iter().filter(|e| e.name != project).collect();
        let pos = match (rank, cur) {
            (Some(r), _) => usize::try_from(r - 1).ok()?,
            (None, Some(at)) => at,
            (None, None) => others.len(),
        }
        .min(others.len());
        others.insert(pos, new);
        let count = others.len();
        let joined = others.iter().map(entry_text).collect::<Vec<_>>().join(", ");
        let list = match validate(&joined)? {
            Ok(list) => list,
            Err(message) => return value_error(ctx, &format!("{KEY}: {message}")),
        };
        let after: Vec<Entry> = list.iter().map(|text| entry_of(text)).collect();
        let counts = summary(ctx, settings, &after)?;
        let text = new_settings(settings, Some(&list))?;
        write(settings, &text)?;
        work::emit(ctx, "config.set", &[("status", quote("ok"))]);
        work::emit(
            ctx,
            "work.priority",
            &[
                ("action", quote("set")),
                ("projects", count.to_string()),
                ("status", quote("ok")),
            ],
        );
        served(format!("{SAVED}{}", summary_text(&after, &counts)), 0)
    }

    fn remove(args: &Parsed, ctx: &Ctx, settings: &Settings, es: Vec<Entry>) -> Option<Outcome> {
        let project = args.text("project")?;
        if !plain(project) {
            return None;
        }
        if !es.iter().any(|e| e.name == project) {
            let names = es
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let have = if names.is_empty() { "none yet" } else { &names };
            return value_error(
                ctx,
                &format!("unknown project {project} in {KEY} (have: {have})"),
            );
        }
        let rest: Vec<Entry> = es.into_iter().filter(|e| e.name != project).collect();
        let count = rest.len();
        let list = if rest.is_empty() {
            None
        } else {
            let joined = rest.iter().map(entry_text).collect::<Vec<_>>().join(", ");
            match validate(&joined)? {
                Ok(list) => Some(list),
                Err(message) => return value_error(ctx, &format!("{KEY}: {message}")),
            }
        };
        let after: Vec<Entry> = list.iter().flatten().map(|text| entry_of(text)).collect();
        let counts = summary(ctx, settings, &after)?;
        let text = new_settings(settings, list.as_deref())?;
        write(settings, &text)?;
        if list.is_some() {
            work::emit(ctx, "config.set", &[("status", quote("ok"))]);
        }
        work::emit(
            ctx,
            "work.priority",
            &[
                ("action", quote("remove")),
                ("projects", count.to_string()),
                ("status", quote("ok")),
            ],
        );
        served(format!("{SAVED}{}", summary_text(&after, &counts)), 0)
    }
}
