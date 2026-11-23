//! Focus explanation formatting shared by the priority component.

use crate::{field, python_float, python_g, text};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn blocks() -> Value {
    json!({
        "now": "minute", "today": "minute", "week": "minute", "month": "minute",
        "quarter": "day", "year": "day", "company": "day", "tomorrow": "minute", "stale": "day"
    })
}

pub fn due_text(days: i64) -> String {
    if days < 0 {
        format!(
            "overdue by {} day{}",
            -days,
            if days == -1 { "" } else { "s" }
        )
    } else if days == 0 {
        "due today".into()
    } else {
        format!("due in {days} day{}", if days == 1 { "" } else { "s" })
    }
}

pub fn task_why(task: &Value, positions: &BTreeMap<String, usize>, total: usize) -> String {
    let parts = &task["parts"];
    let urgency = parts["urgency"].as_f64().unwrap_or(0.0);
    let due = field(parts, "due_days").and_then(Value::as_i64);
    let urgency_text = if due.is_some_and(|days| days <= 2) {
        format!(
            "urgency {} + 3 because it is {}",
            python_g(urgency - 3.0),
            due_text(due.unwrap_or(0))
        )
    } else {
        format!("urgency {}", python_g(urgency))
    };
    let bonus = if parts["project_bonus"].as_f64().unwrap_or(0.0) != 0.0 {
        format!(
            " + project bonus {} for {} (priority {} of {})",
            python_g(parts["project_bonus"].as_f64().unwrap_or(0.0)),
            text(field(parts, "project")),
            parts["project_rank"].as_i64().unwrap_or(0),
            parts["projects"].as_i64().unwrap_or(0)
        )
    } else {
        String::new()
    };
    let id = text(field(task, "id"));
    format!(
        "score {} = (impact {} + {urgency_text}{bonus}) / sqrt({}h); rank {} of {total} open tasks by value per hour",
        python_float(task["score"].as_f64().unwrap_or(0.0)),
        python_g(parts["impact"].as_f64().unwrap_or(0.0)),
        python_g(parts["size_h"].as_f64().unwrap_or(0.0)),
        positions.get(&id).copied().unwrap_or(0),
    )
}
