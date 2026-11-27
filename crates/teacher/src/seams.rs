//! Typed seams for steps that are not merged yet.
//!
//! - [`Runner`]: `exercises.run` and `exercises.grade` (rust-22, PR 236).
//! - [`Loops`]: `interview.loop_for` (rust-19, PR 238).
//! - [`Planner`]: `goals.replan` (the day planner).
//!
//! The synthetic providers read the same fixtures as the Python oracle
//! (`tests/fixtures/rust21/`). Reconciling them with the real rust-19 and
//! rust-22 components is a listed follow-up. The exercise catalog, attempts,
//! status and submit bookkeeping here are the read and record side of
//! `tutor/exercises.py` that the teacher needs; they do not run learner code.

use crate::{err, field, text, truthy, Ctx, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The exercise runner (rust-22): runs learner code and grades the outcome.
pub trait Runner {
    fn run(&self, exercise: &Value, code: &str) -> Result<Value>;
    fn grade(&self, exercise: &Value, code: &str, outcome: &Value) -> Result<Value>;
}

/// Interview loops (rust-19): the step ids of a company's loop.
pub trait Loops {
    fn loop_steps(&self, company: &str) -> Result<Vec<String>>;
}

/// The day planner: rebuild today's plan after a goal change.
pub trait Planner {
    fn replan(&self) -> Result<Value>;
}

pub struct Seams {
    pub runner: Box<dyn Runner>,
    pub loops: Box<dyn Loops>,
    pub planner: Box<dyn Planner>,
}

fn read_fixture(path: &Path) -> Result<Value> {
    let data = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&data)?)
}

/// The synthetic runner: outcome and grade keyed by the submitted code.
pub struct SyntheticRunner {
    outcomes: Value,
}

impl Runner for SyntheticRunner {
    fn run(&self, _exercise: &Value, code: &str) -> Result<Value> {
        self.outcomes
            .get(code)
            .and_then(|row| row.get("outcome"))
            .cloned()
            .ok_or_else(|| err("KeyError", format!("'{code}'")))
    }

    fn grade(&self, _exercise: &Value, code: &str, _outcome: &Value) -> Result<Value> {
        self.outcomes
            .get(code)
            .and_then(|row| row.get("grade"))
            .cloned()
            .ok_or_else(|| err("KeyError", format!("'{code}'")))
    }
}

pub struct SyntheticLoops {
    loops: Value,
}

impl Loops for SyntheticLoops {
    fn loop_steps(&self, company: &str) -> Result<Vec<String>> {
        Ok(careerbrew_tutor::strings(self.loops.get(company)))
    }
}

pub struct SyntheticPlanner;

impl Planner for SyntheticPlanner {
    fn replan(&self) -> Result<Value> {
        Ok(json!({"synthetic": "replan"}))
    }
}

impl Seams {
    pub fn synthetic(repo: &Path) -> Result<Self> {
        let fixtures = repo.join("tests/fixtures/rust21");
        let runner = read_fixture(&fixtures.join("runner.json"))?;
        let loops = read_fixture(&fixtures.join("loops.json"))?;
        Ok(Self {
            runner: Box::new(SyntheticRunner {
                outcomes: runner["outcomes"].clone(),
            }),
            loops: Box::new(SyntheticLoops {
                loops: loops["loops"].clone(),
            }),
            planner: Box::new(SyntheticPlanner),
        })
    }
}

/// `exercises.PATTERNS`.
pub const PATTERNS: &[&str] = &[
    "arrays and hashing",
    "two pointers",
    "sliding window",
    "stack",
    "binary search",
    "linked list",
    "trees",
    "tries",
    "heap",
    "backtracking",
    "graphs",
    "advanced graphs",
    "1-d dynamic programming",
    "2-d dynamic programming",
    "greedy",
    "intervals",
    "math and geometry",
    "bit manipulation",
];

fn exercises_dir(ctx: &Ctx) -> PathBuf {
    std::env::var_os("CAREERBREW_EXERCISES")
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.repo.join("knowledge/exercises"))
}

fn order(exercise: &Value) -> f64 {
    field(exercise, "order")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

/// `exercises.load`: every catalog file, in curriculum order (stable sort).
pub fn load(ctx: &Ctx) -> Result<Vec<Value>> {
    let dir = exercises_dir(ctx);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = std::fs::read_dir(&dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yaml"))
        .collect::<Vec<_>>();
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let data = std::fs::read_to_string(&file)?;
        let canon = careerbrew_vault::json_yaml::yaml_read(&data)
            .map_err(|_| err("ValueError", "invalid exercise file"))?;
        out.push(careerbrew_setup::value::from_canon(&canon));
    }
    out.sort_by(|left, right| order(left).total_cmp(&order(right)));
    Ok(out)
}

/// `exercises.get`.
pub fn get(ctx: &Ctx, id: &str) -> Result<Value> {
    load(ctx)?
        .into_iter()
        .find(|exercise| text(field(exercise, "id")) == id)
        .ok_or_else(|| err("KeyError", format!("'unknown exercise {id}'")))
}

const ATTEMPTS: &str = "tutor/exercise-attempts.jsonl";

/// `exercises.attempts`.
pub fn attempts(ctx: &Ctx, id: Option<&str>) -> Result<Vec<Value>> {
    Ok(crate::read_jsonl(&ctx.root, ATTEMPTS)?
        .into_iter()
        .filter(|row| id.is_none_or(|id| text(field(row, "exercise")) == id))
        .collect())
}

/// `exercises.status`: per exercise, solved, best score, attempts, best time.
pub fn status(ctx: &Ctx) -> Result<serde_json::Map<String, Value>> {
    let mut out = serde_json::Map::new();
    let synced = crate::read_json(&ctx.root, "tutor/synced-solved.json", json!({}))?;
    for row in attempts(ctx, None)? {
        let id = text(field(&row, "exercise"));
        let entry = out.entry(id).or_insert_with(
            || json!({"attempts": 0, "solved": false, "best": 0.0, "best_seconds": null}),
        );
        entry["attempts"] = json!(entry["attempts"].as_i64().unwrap_or(0) + 1);
        let score = field(&row, "score").and_then(Value::as_f64).unwrap_or(0.0);
        let best = entry["best"].as_f64().unwrap_or(0.0);
        if score > best {
            entry["best"] = field(&row, "score").cloned().unwrap_or(json!(0.0));
        }
        if truthy(field(&row, "correct")) {
            entry["solved"] = json!(true);
            let seconds = field(&row, "seconds").filter(|value| truthy(Some(value)));
            if let Some(seconds) = seconds {
                let better = entry["best_seconds"]
                    .as_f64()
                    .is_none_or(|best| seconds.as_f64().unwrap_or(f64::MAX) < best);
                if better {
                    entry["best_seconds"] = seconds.clone();
                }
            }
        }
    }
    for id in careerbrew_tutor::strings(field(&synced, "exercises")) {
        let entry = out.entry(id).or_insert_with(
            || json!({"attempts": 0, "solved": false, "best": 0.0, "best_seconds": null}),
        );
        entry["synced"] = json!(true);
    }
    Ok(out)
}

pub fn solved(status: &serde_json::Map<String, Value>, id: &str) -> bool {
    status
        .get(id)
        .is_some_and(|row| truthy(field(row, "solved")))
}

/// `exercises.next_exercise`.
pub fn next_exercise(ctx: &Ctx, focus: &[String], skip: &[String]) -> Result<Option<Value>> {
    let st = status(ctx)?;
    let todo = load(ctx)?
        .into_iter()
        .filter(|exercise| {
            let id = text(field(exercise, "id"));
            !skip.contains(&id)
                && !st
                    .get(&id)
                    .is_some_and(|row| truthy(field(row, "solved")) || truthy(field(row, "synced")))
        })
        .collect::<Vec<_>>();
    let focused = todo
        .iter()
        .filter(|exercise| focus.contains(&text(field(exercise, "pattern"))))
        .cloned()
        .collect::<Vec<_>>();
    let todo = if !focus.is_empty() && !focused.is_empty() {
        focused
    } else {
        todo
    };
    Ok(todo.into_iter().next())
}

/// `exercises.submit`: run and grade through the runner seam, store the
/// attempt and log mistakes. Returns everything the teacher needs.
pub fn submit(
    ctx: &Ctx,
    id: &str,
    code: &str,
    seconds: Option<i64>,
    hints_used: usize,
) -> Result<Value> {
    let exercise = get(ctx, id)?;
    let outcome = ctx.seams.runner.run(&exercise, code)?;
    let grade = ctx.seams.runner.grade(&exercise, code, &outcome)?;
    let first = attempts(ctx, Some(id))?.is_empty();
    let complexity_ok = match field(&grade, "complexity") {
        None | Some(Value::Null) => Value::Null,
        Some(complexity) => complexity.get("ok").cloned().unwrap_or(Value::Null),
    };
    let row = json!({
        "exercise": id,
        "at": crate::stamp(&ctx.root),
        "seconds": seconds,
        "hints": hints_used,
        "correct": field(&grade, "correct").cloned().unwrap_or(Value::Null),
        "score": field(&grade, "score").cloned().unwrap_or(Value::Null),
        "passed": field(&grade, "passed").cloned().unwrap_or(Value::Null),
        "total": field(&grade, "total").cloned().unwrap_or(Value::Null),
        "first": first,
        "complexity_ok": complexity_ok,
        "code": code,
    });
    careerbrew_tutor::append_jsonl(&ctx.root, ATTEMPTS, std::slice::from_ref(&row))?;
    let found = careerbrew_tutor::mistakes::from_exercise(
        &ctx.root,
        &ctx.public,
        &exercise,
        &outcome,
        &grade,
    )?;
    let correct = truthy(field(&grade, "correct"));
    careerbrew_tutor::emit(
        &ctx.root,
        "exercise.submit",
        vec![
            (
                "status",
                careerbrew_vault::canon::Canon::Str(if correct { "pass" } else { "fail" }.into()),
            ),
            (
                "seconds",
                careerbrew_vault::canon::Canon::Int(seconds.unwrap_or(0).into()),
            ),
        ],
    )?;
    let tests = field(&exercise, "tests")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let shown = field(&outcome, "results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| !truthy(field(row, "ok")))
        .take(3)
        .map(|row| {
            let index = field(row, "i").and_then(Value::as_u64).unwrap_or(0) as usize;
            let test = tests.get(index).cloned().unwrap_or(json!({}));
            let mut row = row.clone();
            row["args"] = match test.get("args") {
                Some(args) => args.clone(),
                None => test.get("ops").cloned().unwrap_or(Value::Null),
            };
            row["expected"] = test.get("expected").cloned().unwrap_or(Value::Null);
            row
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "grade": grade,
        "error": field(&outcome, "error").cloned().unwrap_or(Value::Null),
        "failing": shown,
        "mistakes": found,
        "solution": if correct { field(&exercise, "solution").cloned().unwrap_or(Value::Null) } else { Value::Null },
    }))
}
