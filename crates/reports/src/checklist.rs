//! `checklist.build`: the Getting started steps, each marked done from data
//! careerbrew already has. The work-tool, watcher, task, goal and inbox
//! readers belong to other steps (rust-17, rust-18, rust-19, rust-23), so they
//! arrive through the typed `Facts` seam; the settings read is native.

use crate::{int, truthy, Result};
use serde_json::{json, Value};

/// The raw records the done marks read, from their owner modules.
pub trait Facts {
    /// `work.evidence.sources_list()`.
    fn sources(&self) -> Result<Vec<Value>>;
    /// `day.watch.read_state()`.
    fn watch_state(&self) -> Result<Value>;
    /// `work.priorities.load()`.
    fn tasks(&self) -> Result<Vec<Value>>;
    /// `goals.load()`.
    fn goals(&self) -> Result<Value>;
    /// `day.inbox.load()`.
    fn inbox(&self) -> Result<Vec<Value>>;
}

pub const STEPS: &[(&str, &str, &str, &str, &str)] = &[
    ("connect", "Connect a work tool", "careerbrew reads your commits, PRs, tickets, and messages from it, so the log and the suggestions fill in", "#/setup", "careerbrew connect"),
    ("watch", "Turn on the day watcher", "it keeps Today, the day log, and the suggestions fresh while you work", "#/setup", "careerbrew day watch on"),
    ("task", "Add your first task", "the planner fits it into your day and Next step names it", "#/week", "careerbrew work task \"<title>\" --hours 1"),
    ("goal", "Set a goal", "Teach me and the plan work toward it", "#/goals", "careerbrew goals set \"<title>\" --date <YYYY-MM-DD>"),
    ("decide", "Accept or dismiss one suggestion", "a suggestion changes nothing until you decide; Today explains each one", "#/today", "careerbrew day inbox"),
    ("pm", "Say where your PM update goes", "Copy update and Send use it", "#/setup", "careerbrew config set pm_slack <#channel>"),
];

fn setting_or(settings: &Value, key: &str, default: Value) -> Value {
    settings.get(key).cloned().unwrap_or(default)
}

pub fn build(facts: &dyn Facts) -> Result<Value> {
    let settings = crate::settings()?;
    let goals = facts.goals()?;
    let state = facts.watch_state()?;
    let done_marks = [
        !facts.sources()?.is_empty(),
        matches!(
            state.get("state").and_then(Value::as_str),
            Some("on" | "paused")
        ),
        !facts.tasks()?.is_empty(),
        truthy(goals.get("queue")) || truthy(goals.get("long_term")),
        facts.inbox()?.iter().any(|item| {
            matches!(
                item.get("status").and_then(Value::as_str),
                Some("accepted" | "dismissed" | "answered")
            )
        }),
        truthy(Some(&setting_or(&settings, "pm_slack", json!(""))))
            || truthy(Some(&setting_or(&settings, "pm_email", json!("")))),
    ];
    let steps = STEPS
        .iter()
        .zip(done_marks)
        .map(|((id, title, why, view, command), done)| {
            json!({"id": id, "title": title, "why": why, "view": view, "command": command, "done": done})
        })
        .collect::<Vec<_>>();
    let done = done_marks.iter().filter(|d| **d).count() as i64;
    let hidden = !truthy(Some(&setting_or(
        &settings,
        "first_run_checklist",
        json!(true),
    )));
    Ok(json!({
        "steps": steps,
        "done": int(done),
        "total": int(STEPS.len() as i64),
        "hidden": hidden,
        "show": !hidden && done < STEPS.len() as i64,
    }))
}
