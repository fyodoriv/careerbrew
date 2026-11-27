//! The `careerbrew.prep` story bank, as far as the corporate guard case needs it.
//!
//! rust-followup: the prep checklist, runs, history and stale marks.

use crate::{sub, Result};
use careerbrew_tutor::{now, read_yaml_abs, write_yaml_abs};
use serde_json::{json, Value};
use std::path::Path;

pub fn stories(root: &Path) -> Result<Vec<Value>> {
    let data = read_yaml_abs(&sub(root, "prep/stories.yaml")?, json!({}))?;
    Ok(data
        .get("stories")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

pub fn add_story(root: &Path, title: &str, competencies: &[&str]) -> Result<Value> {
    let mut all = stories(root)?;
    let story = json!({
        "id": format!("s{:02}", all.len() + 1),
        "title": title,
        "competencies": competencies,
        "situation": "",
        "action": "",
        "result": "",
        "added": now().date_naive().format("%Y-%m-%d").to_string(),
    });
    all.push(story.clone());
    write_yaml_abs(&sub(root, "prep/stories.yaml")?, &json!({"stories": all}))?;
    Ok(story)
}
