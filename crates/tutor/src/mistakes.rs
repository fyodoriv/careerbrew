//! Mistake records, prevention cards, repeat metrics, and exercise helpers.

use crate::{chars, field, now, read_jsonl, stamp_at, text, Result};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

fn path() -> &'static str {
    "tutor/mistakes.jsonl"
}

fn tip(kind: &str) -> &'static str {
    match kind {
        "exception" => "Before you run it, trace the code once with the smallest input; check every index and None access.",
        "timeout" => "Estimate the complexity before coding. Nested loops over n are O(n^2): look for a hash map, two pointers, or sorting first.",
        "edge-case" => "Write the edge cases first: empty input, one element, duplicates, negatives, and the largest values.",
        "wrong-answer" => "Walk through a failing example by hand and compare each step with what your code does.",
        "complexity" => "The answer is right but grows too fast: name the bottleneck loop and replace it with a better structure.",
        "style" => "Readable code is part of the grade: clear names, no unused variables, simple conditions.",
        _ => "Re-read the lesson and explain it back in one sentence before trying again.",
    }
}

pub fn load(root: &Path) -> Result<Vec<Value>> {
    read_jsonl(root, path())
}

pub fn record(
    root: &Path,
    public_dir: &Path,
    source: &str,
    pattern: &str,
    kind: &str,
    detail: &str,
    custom_tip: Option<&str>,
) -> Result<Value> {
    let seen = load(root)?
        .into_iter()
        .filter(|mistake| {
            text(field(mistake, "kind")) == kind && text(field(mistake, "pattern")) == pattern
        })
        .collect::<Vec<_>>();
    let row = json!({
        "at": stamp_at(now()),
        "source": source,
        "pattern": pattern,
        "kind": kind,
        "detail": chars(detail, 300),
        "tip": custom_tip.unwrap_or_else(|| tip(kind)),
        "repeat": !seen.is_empty(),
    });
    crate::append_jsonl(root, path(), std::slice::from_ref(&row))?;
    if seen.is_empty() {
        let question = format!(
            "[{pattern}] You made a {} mistake ({}). What do you do to prevent it?",
            kind.replace('-', " "),
            chars(detail, 120)
        );
        crate::cards::add(
            root,
            &question,
            text(field(&row, "tip")).as_str(),
            &["mistakes".to_owned(), pattern.to_owned()],
            "mistakes",
        )?;
    }
    let _ = public_dir;
    Ok(row)
}

pub fn from_exercise(
    root: &Path,
    public_dir: &Path,
    exercise: &Value,
    outcome: &Value,
    grade: &Value,
) -> Result<Vec<Value>> {
    let source = text(field(exercise, "id"));
    let pattern = text(field(exercise, "pattern"));
    let pitfalls = field(exercise, "pitfalls")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let custom = |kind: &str| field(&pitfalls, kind).and_then(Value::as_str);
    let mut found = Vec::new();
    if crate::truthy(field(outcome, "timeout")) {
        found.push(record(
            root,
            public_dir,
            &source,
            &pattern,
            "timeout",
            &text(field(outcome, "error")),
            custom("timeout"),
        )?);
    } else if crate::truthy(field(outcome, "error")) {
        found.push(record(
            root,
            public_dir,
            &source,
            &pattern,
            "exception",
            &text(field(outcome, "error")),
            custom("exception"),
        )?);
    } else {
        let failing = field(outcome, "results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|result| !crate::truthy(field(result, "ok")))
            .collect::<Vec<_>>();
        if let Some(result) = failing.first() {
            let index = field(result, "i")
                .and_then(Value::as_u64)
                .unwrap_or_default() as usize;
            let note = field(exercise, "tests")
                .and_then(Value::as_array)
                .and_then(|tests| tests.get(index))
                .map(|test| text(field(test, "note")))
                .unwrap_or_default();
            if crate::truthy(field(result, "error")) {
                let suffix = if note.is_empty() { String::new() } else { format!(": {note}") };
                found.push(record(root, public_dir, &source, &pattern, "exception", &format!("{} (test {}{})", text(field(result, "error")), index + 1, suffix), custom("exception"))?);
            } else if Regex::new(r"(?i)empty|single|one element|duplicate|negative|zero|large|overflow|edge|null|none")
                .expect("mistake edge regex")
                .is_match(&note)
            {
                found.push(record(root, public_dir, &source, &pattern, "edge-case", &format!("failed on {note}"), custom("edge-case"))?);
            } else {
                let suffix = if note.is_empty() { String::new() } else { format!(": {note}") };
                found.push(record(root, public_dir, &source, &pattern, "wrong-answer", &format!("test {}{}", index + 1, suffix), custom("wrong-answer"))?);
            }
        } else if field(grade, "complexity")
            .filter(|complexity| crate::truthy(Some(complexity)))
            .is_some_and(|complexity| !crate::truthy(field(complexity, "ok")))
        {
            let complexity = &grade["complexity"];
            found.push(record(
                root,
                public_dir,
                &source,
                &pattern,
                "complexity",
                &format!("expected {}", text(field(complexity, "expected"))),
                custom("complexity").or_else(|| custom("timeout")),
            )?);
        }
    }
    Ok(found)
}

pub fn repeat_rate(root: &Path) -> Result<Option<f64>> {
    let mistakes = load(root)?;
    if mistakes.is_empty() {
        return Ok(None);
    }
    let repeats = mistakes
        .iter()
        .filter(|mistake| crate::truthy(field(mistake, "repeat")))
        .count() as f64;
    Ok(Some(careerbrew_projects::python_round(
        repeats / mistakes.len() as f64,
        3,
    )))
}

pub fn summary(root: &Path) -> Result<Value> {
    let mistakes = load(root)?;
    let mut counts = BTreeMap::<String, u64>::new();
    let mut order = Vec::new();
    for mistake in &mistakes {
        let key = format!(
            "{}: {}",
            text(field(mistake, "pattern")),
            text(field(mistake, "kind"))
        );
        if !counts.contains_key(&key) {
            order.push(key.clone());
        }
        *counts.entry(key).or_default() += 1;
    }
    order.sort_by(|left, right| counts[right].cmp(&counts[left]));
    let top = order
        .into_iter()
        .take(8)
        .map(|key| json!([key, counts[&key]]))
        .collect::<Vec<_>>();
    let recent = mistakes.iter().rev().take(10).cloned().collect::<Vec<_>>();
    Ok(json!({
        "total": mistakes.len(),
        "repeat_rate": repeat_rate(root)?,
        "top": top,
        "recent": recent,
    }))
}
