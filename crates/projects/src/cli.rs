//! Component adapters for rust-09's four assigned CLI handlers.

use crate::{field, priorities, project_priority, python_float, text};
use serde_json::{json, Value};
use std::path::Path;

pub fn work_priority(
    root: &Path,
    action: &str,
    project: &str,
    rank: Option<usize>,
    matches: Option<Vec<String>>,
) -> (i32, String) {
    let result = match action {
        "set" => project_priority::set_project(root, project, rank, matches).map(|_| ()),
        "remove" => project_priority::remove(root, project).map(|_| ()),
        _ => Ok(()),
    };
    if let Err(error) = result {
        return (2, format!("error: {}\n", error.message));
    }
    let mut out = String::new();
    if matches!(action, "set" | "remove") {
        out.push_str("saved: the PM update and the web app use it now; rebuild today's saved plan for `day next` with: careerbrew day plan\n");
    }
    let rows = match project_priority::summary(root) {
        Ok(rows) => rows,
        Err(error) => return (2, format!("error: {}\n", error.message)),
    };
    if rows.is_empty() {
        out.push_str("no project priority yet: careerbrew work priority set <project> --match '<glob>' (globs match repos like */agentbrew, ticket keys like PROJ-*, or titles like *invoice*)\n");
        return (0, out);
    }
    out.push_str(
        "project priority, rank 1 first (the PM update, the plan, and Next step follow it):\n",
    );
    for row in rows {
        let open = row["open_tasks"].as_u64().unwrap_or(0);
        let today = row["today"].as_u64().unwrap_or(0);
        out.push_str(&format!(
            "  {}. {:<16} {:<32} {} open task{}, {} item{} today\n",
            row["rank"].as_u64().unwrap_or(0),
            text(field(&row, "name")),
            crate::strings(field(&row, "match")).join(", "),
            open,
            if open == 1 { "" } else { "s" },
            today,
            if today == 1 { "" } else { "s" },
        ));
    }
    out.push_str("change it: careerbrew work priority set <project> --rank <n> [--match <glob> ...], or careerbrew work priority remove <project>\n");
    (0, out)
}

pub fn work_task_set(
    root: &Path,
    id: &str,
    impact: Option<i64>,
    urgency: Option<i64>,
    hours: Option<Value>,
) -> (i32, String) {
    if impact.is_none() && urgency.is_none() && hours.is_none() {
        return (
            2,
            "nothing to change: pass --impact, --urgency, or --hours\n".into(),
        );
    }
    match priorities::regrade(root, id, impact, urgency, hours) {
        Ok(task) => (
            0,
            format!(
                "{}: impact {}, urgency {}, {}h (the plan and the ranks use it now)\n",
                id,
                task["impact"].as_i64().unwrap_or(0),
                task["urgency"].as_i64().unwrap_or(0),
                python_float(task["estimate_h"].as_f64().unwrap_or(0.0))
            ),
        ),
        Err(error) => (2, format!("error: {}\n", error.message)),
    }
}

pub fn work_impact(
    root: &Path,
    id: &str,
    expected: &str,
    who: &str,
    metric: &str,
    result: &str,
    evidence: &[String],
) -> (i32, String) {
    match priorities::record_impact(root, id, expected, who, metric, result, evidence) {
        Ok(task) => {
            let plan = task["expected_impact"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            let measured = task["measured_impact"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            let mut out = format!(
                "{}: expected impact {} ({}); measured {}, {} evidence link(s)\n",
                id,
                if plan.is_empty() { "not set" } else { "set" },
                if plan.is_empty() {
                    "none".into()
                } else {
                    plan.keys().cloned().collect::<Vec<_>>().join(", ")
                },
                if text(measured.get("result")).is_empty() {
                    "not yet"
                } else {
                    "yes"
                },
                measured
                    .get("evidence")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0)
            );
            if !text(measured.get("result")).is_empty() {
                out.push_str("the daily update and promo evidence use the result now\n");
            }
            (0, out)
        }
        Err(error) => (2, format!("error: {} (breaks US-87)\n", error.message)),
    }
}

pub fn work_done(root: &Path, id: &str, hours: Value) -> (i32, String) {
    match priorities::done(root, id, hours) {
        Ok(task) => (
            0,
            format!(
                "done {} ({}h vs {}h planned)\n",
                id,
                python_float(task["actual_h"].as_f64().unwrap_or(0.0)),
                python_float(task["estimate_h"].as_f64().unwrap_or(0.0))
            ),
        ),
        Err(error) => (2, format!("error: {}\n", error.message)),
    }
}

pub fn invocation(exit: i32, stdout: String) -> Value {
    json!({"exit": exit, "stdout": stdout})
}
