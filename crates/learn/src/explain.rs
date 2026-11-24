//! Layered project explanations with a citation on every claim.

use crate::{field, text, Result};
use careerbrew_projects::projects;
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const FULL: &[&str] = &[
    "One-minute version",
    "Context map",
    "Containers",
    "Codemap",
    "Invariants and boundaries",
    "Why it is this way",
    "Current state",
    "Gotchas",
    "Read next",
    "Check yourself",
    "Glossary",
    "Sources",
];
pub const SHORT: &[&str] = &[
    "One-minute version",
    "Containers",
    "Codemap",
    "Gotchas",
    "Read next",
    "Sources",
];
pub const DEEP: &[&str] = &[
    "One-minute version",
    "Context map",
    "Containers",
    "Codemap",
    "How it runs",
    "Invariants and boundaries",
    "Why it is this way",
    "Current state",
    "Gotchas",
    "Read next",
    "Check yourself",
    "Glossary",
    "Sources",
];

fn cite_pattern() -> Regex {
    Regex::new(r"\[(S\d+)\]").expect("citation regex")
}
fn bullet_pattern() -> Regex {
    Regex::new(r"^(\s*)(?:[-*+]|\d+\.)\s+(.*)$").expect("bullet regex")
}
fn row_pattern() -> Regex {
    Regex::new(r"^\s*\|(.*)\|\s*$").expect("table regex")
}

pub fn sections(text: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            out.push((heading.trim().to_owned(), Vec::new()));
        } else if let Some((_, lines)) = out.last_mut() {
            lines.push(line.to_owned());
        }
    }
    out
}

pub fn base(heading: &str) -> String {
    Regex::new(r"\s*\(.*\)\s*$")
        .expect("heading suffix")
        .replace(heading, "")
        .into_owned()
}

pub fn table(lines: &[String]) -> (Vec<String>, Vec<Vec<String>>) {
    let row = row_pattern();
    let separator = Regex::new(r":?-{2,}:?").expect("separator");
    let mut rows = lines
        .iter()
        .filter_map(|line| {
            row.captures(line).map(|found| {
                found[1]
                    .split('|')
                    .map(|cell| cell.trim().to_owned())
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    rows.retain(|cells| {
        !cells
            .iter()
            .all(|cell| separator.is_match(cell) && separator.find(cell).unwrap().as_str() == cell)
    });
    if rows.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        (rows[0].clone(), rows[1..].to_vec())
    }
}

fn plain(value: &str) -> String {
    let mut out = Regex::new(r"`[^`]*`")
        .expect("code")
        .replace_all(value, " ")
        .into_owned();
    out = Regex::new(r"https?://\S+")
        .expect("URL")
        .replace_all(&out, " ")
        .into_owned();
    out = Regex::new(r"\[[^\]]*\]\([^)]*\)")
        .expect("link")
        .replace_all(&out, " ")
        .into_owned();
    out = cite_pattern().replace_all(&out, " ").into_owned();
    Regex::new(r"<[^>]+>")
        .expect("HTML")
        .replace_all(&out, " ")
        .into_owned()
}

pub fn sources_table(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    for (heading, lines) in sections(text) {
        if base(&heading) == "Sources" {
            let (head, rows) = table(&lines);
            let columns = head
                .iter()
                .map(|cell| cell.to_lowercase())
                .collect::<Vec<_>>();
            return rows
                .into_iter()
                .filter(|row| !row.is_empty() && !row[0].is_empty())
                .map(|row| {
                    let id = row[0].clone();
                    let values = columns
                        .iter()
                        .cloned()
                        .zip(row.iter().cloned())
                        .collect::<BTreeMap<_, _>>();
                    (id, values)
                })
                .collect();
        }
    }
    BTreeMap::new()
}

pub fn source_problems(sources: &BTreeMap<String, BTreeMap<String, String>>) -> Vec<String> {
    let locator = Regex::new(r"^(https?://\S+|[A-Z][A-Z0-9]+-\d+|doc-[\w-]+|[\w./-]+@[0-9a-f]{7,40}|[\w.-]*/[\w./-]*|[\w-]+\.\w+)$").expect("locator");
    let read = Regex::new(r"^(\d{4}-\d{2}-\d{2}|[0-9a-f]{7,40})$").expect("read");
    let mut problems = Vec::new();
    if sources.is_empty() {
        problems.push("Sources: a table with Id, Kind, Locator, Read columns is required".into());
    }
    for (id, source) in sources {
        let kind = source.get("kind").map_or("", String::as_str);
        if !["primary", "lead"].contains(&kind.to_lowercase().as_str()) {
            problems.push(format!("Sources {id}: Kind must be primary or lead"));
        }
        if !locator.is_match(source.get("locator").map_or("", String::as_str)) {
            problems.push(format!(
                "Sources {id}: Locator must be a URL, ticket key, doc id, repo@sha, or path"
            ));
        }
        if !read.is_match(source.get("read").map_or("", String::as_str)) {
            problems.push(format!(
                "Sources {id}: Read must be a date (YYYY-MM-DD) or a commit sha"
            ));
        }
    }
    problems
}

fn citation_ids(value: &str) -> Vec<String> {
    cite_pattern()
        .captures_iter(value)
        .map(|found| found[1].to_owned())
        .collect()
}

pub fn check(text: &str, short: bool, deep: bool) -> Vec<String> {
    let want = if short {
        SHORT
    } else if deep {
        DEEP
    } else {
        FULL
    };
    let sections = sections(text);
    let got = sections
        .iter()
        .map(|(heading, _)| base(heading))
        .collect::<Vec<_>>();
    let mut problems = Vec::new();
    if got != want {
        problems.push(format!(
            "headings must be exactly, in order: {} (got: {})",
            want.join(", "),
            if got.is_empty() {
                "none".into()
            } else {
                got.join(", ")
            }
        ));
    }
    let sources = sources_table(text);
    problems.extend(source_problems(&sources));
    let mut used = BTreeSet::new();
    let mut seen_acronyms = BTreeSet::new();
    let acronym = Regex::new(r"\b[A-Z][A-Z0-9]*[A-Z][A-Z0-9]*s?\b").expect("acronym");
    let bullets_pattern = bullet_pattern();
    let command = Regex::new(r"`[^`]+`").expect("command");
    for (heading, lines) in &sections {
        let name = base(heading);
        if name == "Sources" {
            continue;
        }
        let bullets = lines
            .iter()
            .filter_map(|line| {
                bullets_pattern
                    .captures(line)
                    .map(|found| (found[1].to_owned(), found[2].to_owned()))
            })
            .collect::<Vec<_>>();
        let (head, rows) = table(lines);
        let claims = bullets
            .iter()
            .map(|(_, bullet)| bullet.clone())
            .chain(rows.iter().map(|row| row.join(" | ")))
            .collect::<Vec<_>>();
        for claim in claims {
            let ids = citation_ids(&claim);
            used.extend(ids.iter().cloned());
            if ids.is_empty() {
                problems.push(format!(
                    "{name}: no citation on: {}",
                    claim.chars().take(60).collect::<String>()
                ));
                continue;
            }
            for id in &ids {
                if !sources.contains_key(id) {
                    problems.push(format!("{name}: cites {id}, which is not in Sources"));
                }
            }
            if ids.iter().all(|id| sources.contains_key(id))
                && ids.iter().all(|id| {
                    sources[id]
                        .get("kind")
                        .is_some_and(|kind| kind.to_lowercase() == "lead")
                })
            {
                problems.push(format!(
                    "{name}: backed only by leads: {}",
                    claim.chars().take(60).collect::<String>()
                ));
            }
        }
        if name == "One-minute version"
            && (bullets.len() != 3 || bullets.iter().any(|(indent, _)| !indent.is_empty()))
        {
            problems.push("One-minute version: exactly 3 flat bullets (what, why, status)".into());
        }
        if name == "Containers"
            && !head
                .iter()
                .any(|column| column.to_lowercase() == "deploys as")
        {
            problems.push("Containers: the table needs a 'Deploys as' column".into());
        }
        if name == "Why it is this way" {
            let columns = head
                .iter()
                .map(|column| column.to_lowercase())
                .collect::<Vec<_>>();
            if !["state", "who", "when"]
                .iter()
                .all(|column| columns.contains(&column.to_string()))
            {
                problems.push(
                    "Why it is this way: the table needs State, Who, and When columns".into(),
                );
            } else {
                let state_index = columns.iter().position(|column| column == "state").unwrap();
                for row in rows {
                    let state = row
                        .get(state_index)
                        .map_or("", String::as_str)
                        .to_lowercase();
                    if !["decided", "proposed"].contains(&state.as_str()) {
                        problems.push(format!(
                            "Why it is this way: State must be decided or proposed (got: {})",
                            if state.is_empty() { "empty" } else { &state }
                        ));
                    } else if row.join(" ").to_lowercase().contains("considering")
                        && state != "proposed"
                    {
                        problems.push(
                            "Why it is this way: 'considering' means proposed, not decided".into(),
                        );
                    }
                }
            }
        }
        if name == "How it runs" {
            if bullets.is_empty() {
                problems.push("How it runs: list each step you ran, with its command".into());
            }
            for (_, bullet) in &bullets {
                if !command.is_match(bullet) {
                    problems.push(format!(
                        "How it runs: name the command each step ran, in backticks: {}",
                        bullet.chars().take(60).collect::<String>()
                    ));
                }
            }
        }
        if name == "Current state" {
            for (_, bullet) in &bullets {
                if !bullet.to_lowercase().contains("who:") {
                    problems.push(format!(
                        "Current state: each open question names who can answer (Who: ...): {}",
                        bullet.chars().take(60).collect::<String>()
                    ));
                }
            }
        }
        if name == "Check yourself" {
            if bullets
                .iter()
                .filter(|(indent, _)| indent.is_empty())
                .count()
                != 3
            {
                problems.push("Check yourself: exactly 3 questions".into());
            }
            if lines.join("\n").matches("<details>").count() < 3 {
                problems.push("Check yourself: hide each answer in <details>".into());
            }
        }
        if name != "Glossary" {
            for line in lines {
                let plain = plain(line);
                for found in acronym.find_iter(&plain) {
                    let value = found.as_str();
                    let key = if value.ends_with('s')
                        && value[..value.len() - 1]
                            .chars()
                            .all(|c| !c.is_ascii_lowercase())
                    {
                        &value[..value.len() - 1]
                    } else {
                        value
                    };
                    if seen_acronyms.contains(key) {
                        continue;
                    }
                    seen_acronyms.insert(key.to_owned());
                    let first = Regex::new(&format!(
                        r"{}\s*\(|\({}s?\)",
                        regex::escape(value),
                        regex::escape(key)
                    ))
                    .expect("acronym definition");
                    if !first.is_match(&plain) {
                        problems.push(format!("{name}: define {key} inline at first use, as 'Full name ({key})' or '{key} (full name)'"));
                    }
                }
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

pub fn scaffold(root: &Path, name: &str, short: bool, deep: bool) -> Result<String> {
    let project = projects::show(root, name)?;
    let headings = if short {
        SHORT
    } else if deep {
        DEEP
    } else {
        FULL
    };
    let mut lines = vec![format!("# {name}: explained"), String::new(), format!("<!-- reading list from `careerbrew work project show {name}`: re-read each source now and record the date or sha -->"), String::new()];
    for heading in headings {
        lines.extend([
            format!("## {heading}"),
            String::new(),
            "- TODO".into(),
            String::new(),
        ]);
    }
    lines.truncate(lines.len() - 2);
    lines.extend([
        "| Id | Kind | Locator | Read |".into(),
        "|---|---|---|---|".into(),
    ]);
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
