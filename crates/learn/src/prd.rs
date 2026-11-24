//! Product requirements document scaffolds and deterministic validation.

use crate::{explain, field, text, Result};
use careerbrew_projects::projects;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

pub const HEADINGS: &[&str] = &[
    "Problem",
    "Requirements",
    "Decisions",
    "Open questions",
    "Sources",
];
pub const OPTIONAL: &str = "Pushback";

fn weeks_pattern() -> Regex {
    Regex::new(r"(?i)^(\d+(?:\.\d+)?)\s+dev weeks?$").expect("weeks")
}

pub fn weeks(value: f64) -> String {
    format!(
        "{} dev week{}",
        careerbrew_projects::python_g(value),
        if value == 1.0 { "" } else { "s" }
    )
}

pub fn check(text: &str) -> Vec<String> {
    let sections = explain::sections(text);
    let got = sections
        .iter()
        .map(|(heading, _)| explain::base(heading))
        .collect::<Vec<_>>();
    let with_pushback = vec![
        "Problem",
        "Requirements",
        "Decisions",
        "Pushback",
        "Open questions",
        "Sources",
    ];
    let mut problems = Vec::new();
    if got != HEADINGS && got != with_pushback {
        problems.push(format!("headings must be exactly, in order: {}, with an optional Pushback after Decisions (got: {})", HEADINGS.join(", "), if got.is_empty() { "none".into() } else { got.join(", ") }));
    }
    let intro = text.split("\n## ").next().unwrap_or("");
    let slot_pattern = Regex::new(r"(?m)^Plan slot:\s*(.*?)\s*(?:\[S\d+\]\s*)*$").expect("slot");
    let mut slot = None;
    if let Some(found) = slot_pattern.captures(intro) {
        let value = found[1].trim();
        if let Some(weeks) = weeks_pattern().captures(value) {
            slot = weeks[1].parse::<f64>().ok();
        } else {
            problems.push(format!(
                "Plan slot: give it in dev weeks, as 'N dev weeks' (got: {})",
                if value.is_empty() { "empty" } else { value }
            ));
        }
    } else {
        problems
            .push("Plan slot: add a 'Plan slot: N dev weeks' line above the first section".into());
    }
    let sources = explain::sources_table(text);
    problems.extend(explain::source_problems(&sources));
    let cite = Regex::new(r"\[(S\d+)\]").expect("citation");
    let ids = |value: &str| {
        cite.captures_iter(value)
            .map(|found| found[1].to_owned())
            .collect::<Vec<_>>()
    };
    let mut used = BTreeSet::new();
    used.extend(ids(intro));
    let bullet = Regex::new(r"^(\s*)(?:[-*+]|\d+\.)\s+(.*)$").expect("bullet");
    let mut total = 0.0;
    let mut pushback: Option<Vec<String>> = None;
    for (heading, lines) in &sections {
        let name = explain::base(heading);
        if name == "Sources" {
            continue;
        }
        let bullets = lines
            .iter()
            .filter_map(|line| bullet.captures(line).map(|found| found[2].to_owned()))
            .collect::<Vec<_>>();
        let (head, rows) = explain::table(lines);
        for claim in bullets
            .iter()
            .cloned()
            .chain(rows.iter().map(|row| row.join(" | ")))
        {
            let citations = ids(&claim);
            used.extend(citations.iter().cloned());
            if citations.is_empty() {
                problems.push(format!(
                    "{name}: no citation on: {}",
                    claim.chars().take(60).collect::<String>()
                ));
            } else if citations.iter().any(|id| !sources.contains_key(id)) {
                problems.push(format!(
                    "{name}: cites {}, which is not in Sources",
                    citations
                        .iter()
                        .filter(|id| !sources.contains_key(*id))
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            } else if citations.iter().all(|id| {
                sources[id]
                    .get("kind")
                    .is_some_and(|kind| kind.to_lowercase() == "lead")
            }) {
                problems.push(format!(
                    "{name}: backed only by leads: {}",
                    claim.chars().take(60).collect::<String>()
                ));
            }
        }
        let columns = head
            .iter()
            .map(|column| column.to_lowercase())
            .collect::<Vec<_>>();
        if name == "Requirements" {
            if rows.is_empty() {
                problems.push("Requirements: a table with Id, Requirement, Estimate, and Source columns is required".into());
            } else if !columns.contains(&"estimate".into()) {
                problems.push("Requirements: the table needs an Estimate column".into());
            } else {
                let estimate_index = columns
                    .iter()
                    .position(|column| column == "estimate")
                    .unwrap();
                for row in &rows {
                    let estimate = row.get(estimate_index).map_or("", String::as_str);
                    if let Some(found) = weeks_pattern().captures(estimate) {
                        total += found[1].parse::<f64>().unwrap_or(0.0);
                    } else {
                        problems.push(format!(
                            "Requirements {}: estimate in dev weeks, as 'N dev weeks' (got: {})",
                            row.first().map_or("", String::as_str),
                            if estimate.is_empty() {
                                "empty"
                            } else {
                                estimate
                            }
                        ));
                    }
                }
            }
        }
        if name == "Decisions" {
            if !["state", "who", "when"]
                .iter()
                .all(|column| columns.contains(&column.to_string()))
            {
                problems.push("Decisions: the table needs State, Who, and When columns".into());
            } else {
                let state_index = columns.iter().position(|column| column == "state").unwrap();
                for row in &rows {
                    let state = row
                        .get(state_index)
                        .map_or("", String::as_str)
                        .to_lowercase();
                    if !["decided", "proposed"].contains(&state.as_str()) {
                        problems.push(format!(
                            "Decisions: State must be decided or proposed (got: {})",
                            if state.is_empty() { "empty" } else { &state }
                        ));
                    } else if row.join(" ").to_lowercase().contains("considering")
                        && state != "proposed"
                    {
                        problems
                            .push("Decisions: 'considering' means proposed, not decided".into());
                    }
                }
            }
        }
        if name == OPTIONAL {
            pushback = Some(bullets.clone());
        }
        if name == "Open questions" {
            for bullet in bullets {
                if !bullet.to_lowercase().contains("who:") {
                    problems.push(format!(
                        "Open questions: each question names who can answer (Who: ...): {}",
                        bullet.chars().take(60).collect::<String>()
                    ));
                }
            }
        }
    }
    if let Some(slot) = slot {
        if total > slot {
            if !pushback.as_ref().is_some_and(|rows| !rows.is_empty()) {
                problems.push(format!("Pushback: the requirements need {} but the plan slot is {}; add a Pushback section that names the gap and what to cut or add", weeks(total), weeks(slot)));
            } else if !pushback
                .unwrap()
                .iter()
                .any(|bullet| bullet.to_lowercase().contains("dev week"))
            {
                problems.push(format!(
                    "Pushback: name the gap in dev weeks ({} needed, {} in the slot)",
                    weeks(total),
                    weeks(slot)
                ));
            }
        }
    }
    for id in sources.keys() {
        if !used.contains(id) {
            problems.push(format!("Sources {id}: unused (cite it or remove it)"));
        }
    }
    problems
}

pub fn scaffold(root: &Path, name: &str, slot: Option<f64>) -> Result<String> {
    let project = projects::show(root, name)?;
    let slot = slot.map(weeks).unwrap_or_else(|| "TODO dev weeks".into());
    let mut lines = vec![
        format!("# {name}: PRD"), String::new(),
        format!("<!-- reading list from `careerbrew work project show {name}`: re-read each source now and record the date or sha -->"), String::new(),
        format!("Plan slot: {slot}"), String::new(),
        "## Problem".into(), String::new(), "- TODO".into(), String::new(),
        "## Requirements".into(), String::new(), "| Id | Requirement | Estimate | Source |".into(), "|---|---|---|---|".into(), "| R1 | TODO | TODO dev weeks | |".into(), String::new(),
        "## Decisions".into(), String::new(), "| Decision | State | Who | When | Source |".into(), "|---|---|---|---|---|".into(), "| TODO | proposed | TODO | TODO | |".into(), String::new(),
        "## Open questions".into(), String::new(), "- TODO Who: TODO".into(), String::new(),
        "## Sources".into(), String::new(), "| Id | Kind | Locator | Read |".into(), "|---|---|---|---|".into(),
    ];
    for (index, source) in field(&project, "sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        lines.push(format!(
            "| S{} | primary | {} | TODO |",
            index + 1,
            text(field(source, "ref"))
        ));
    }
    Ok(lines.join("\n") + "\n")
}
