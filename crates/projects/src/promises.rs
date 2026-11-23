//! Promise detection, stable identities, lifecycle and closure.

use crate::{chars, field, now, projects, text, today, ProjectError, Result};
use chrono::{Datelike, Duration, NaiveDate};
use regex::Regex;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const STEPS: &[&str] = &["research", "draft", "status"];
pub const CLOSED: &[&str] = &["done", "dropped"];
const KINDS: &[&str] = &[
    "dm", "message", "mention", "reply", "mail", "comment", "event",
];
const WEEKDAYS: &[&str] = &[
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

fn plain(value: &str) -> String {
    let links = Regex::new(r"(?i)<(https?://[^|>\s]+)(?:\|([^>]*))?>").expect("Slack link regex");
    let mentions = Regex::new(r"<[@#!][^>]*>").expect("mention regex");
    let value = links.replace_all(value, |found: &regex::Captures<'_>| {
        found
            .get(2)
            .map(|label| label.as_str())
            .unwrap_or_default()
            .to_owned()
    });
    mentions
        .replace_all(&value, "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn due_from(value: &str, said: NaiveDate) -> String {
    let iso = Regex::new(r"\b(20\d\d-\d\d-\d\d)\b").expect("ISO day regex");
    if let Some(found) = iso.captures(value) {
        return NaiveDate::parse_from_str(&found[1], "%Y-%m-%d")
            .map(|day| day.to_string())
            .unwrap_or_default();
    }
    let low = value.to_lowercase();
    let same_day = Regex::new(r"\b(?:today|tonight|this\s+(?:morning|afternoon|evening)|eod|end\s+of\s+(?:the\s+)?day|asap|in\s+(?:an?|a\s+few|\d+)\s+(?:minutes?|mins?|hours?|hrs?))\b").expect("same day regex");
    if same_day.is_match(&low) {
        return said.to_string();
    }
    if Regex::new(r"\btomorrow\b")
        .expect("tomorrow regex")
        .is_match(&low)
    {
        return (said + Duration::days(1)).to_string();
    }
    if let Some(found) = Regex::new(r"\bin\s+(\d{1,2})\s+days?\b")
        .expect("days regex")
        .captures(&low)
    {
        return (said + Duration::days(found[1].parse().unwrap_or(0))).to_string();
    }
    if Regex::new(r"\b(?:this\s+week|end\s+of\s+(?:the\s+)?week|eow)\b")
        .expect("week regex")
        .is_match(&low)
    {
        return (said + Duration::days((4 - said.weekday() as i64).max(0))).to_string();
    }
    if low.contains("next week") {
        return (said + Duration::days(7 - said.weekday() as i64)).to_string();
    }
    let weekday =
        Regex::new(r"\b(next\s+)?(monday|tuesday|wednesday|thursday|friday|saturday|sunday)\b")
            .expect("weekday regex");
    if let Some(found) = weekday.captures(&low) {
        let target = WEEKDAYS
            .iter()
            .position(|day| *day == &found[2])
            .unwrap_or(0) as i64;
        let ahead = (target - said.weekday() as i64).rem_euclid(7);
        return (said + Duration::days(ahead + if found.get(1).is_some() { 7 } else { 0 }))
            .to_string();
    }
    String::new()
}

pub fn detect(value: &str, said: NaiveDate) -> Option<Value> {
    let plain = plain(value);
    let lead = Regex::new(r#"(?i)(?:^|[\s,;:(\"'“])(I(?:'|’)ll|I\s+will|I(?:'|’)m\s+going\s+to|I\s+am\s+going\s+to|let\s+me|I\s+can)\s+([^.!?\n]+)"#).expect("promise lead regex");
    let not = Regex::new(r"(?i)^(?:not|never|no\b|probably\s+not|be\s+(?:out|off|away|back|ooo|on\s+(?:leave|vacation|holiday|pto))|know|have\s+to\s+check\s+with\s+you|let\s+you\s+decide)\b").expect("promise refusal regex");
    for found in lead.captures_iter(&plain) {
        let what = found[2]
            .trim_matches(|c: char| matches!(c, ' ' | ',' | ';' | ':' | '-' | '—'))
            .to_owned();
        let lead_text = found[1]
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let end = found.get(0).map(|matched| matched.end()).unwrap_or(0);
        if plain[end..].starts_with('?')
            || not.is_match(&what)
            || what.split_whitespace().count() < 2
        {
            continue;
        }
        let due = due_from(&what, said);
        if lead_text == "i can" && due.is_empty() {
            continue;
        }
        let title = chars(&what, 120);
        let mut initial = title
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect::<String>())
            .unwrap_or_default();
        initial.push_str(&title.chars().skip(1).collect::<String>());
        return Some(json!({"what": initial, "due": due}));
    }
    None
}

pub fn name_for(event_id: &str) -> String {
    format!("promise-{:x}", Sha256::digest(event_id.as_bytes()))[..16].to_owned()
}

fn said_date(at: &str) -> NaiveDate {
    chrono::DateTime::parse_from_rfc3339(&at.replace('Z', "+00:00"))
        .map(|value| value.date_naive())
        .unwrap_or_else(|_| today())
}

fn candidate(row: &Value) -> bool {
    text(field(row, "actor")) == "me"
        && KINDS.contains(&text(field(row, "kind")).as_str())
        && !crate::truthy(field(row, "personal"))
        && text(field(row, "source")) != "pagerduty"
}

fn thread_key(row: &Value) -> String {
    text(field(row, "thread"))
}

fn to_whom(row: &Value, rows: &[Value]) -> String {
    let key = thread_key(row);
    if !key.is_empty() {
        let mut before = rows
            .iter()
            .filter(|other| {
                text(field(other, "actor")) == "other"
                    && text(field(other, "at")) <= text(field(row, "at"))
                    && text(field(other, "id")) != text(field(row, "id"))
                    && thread_key(other) == key
                    && !text(field(other, "from")).is_empty()
            })
            .collect::<Vec<_>>();
        before.sort_by_key(|other| (text(field(other, "at")), text(field(other, "id"))));
        if let Some(other) = before.last() {
            return chars(&text(field(other, "from")), 80);
        }
    }
    let channel = text(field(row, "channel"));
    if channel.is_empty() {
        String::new()
    } else {
        chars(&format!("#{channel}"), 80)
    }
}

fn parent(label: &str, every: &Value) -> String {
    let want = label.trim().to_lowercase();
    if want.is_empty() {
        return String::new();
    }
    every
        .as_object()
        .into_iter()
        .flat_map(|rows| rows.iter())
        .find_map(|(name, project)| {
            (text(field(project, "kind")) != "promise"
                && [
                    name.to_lowercase(),
                    text(field(project, "title")).trim().to_lowercase(),
                ]
                .contains(&want))
            .then(|| name.clone())
        })
        .unwrap_or_default()
}

pub fn track(root: &Path, rows: &[Value], pairs: &[(Value, String)]) -> Result<Vec<String>> {
    let every = projects::load(root)?;
    let mut made = Vec::new();
    for (row, original) in pairs {
        if !candidate(row) || original.is_empty() {
            continue;
        }
        let name = name_for(&text(field(row, "id")));
        if every.get(&name).is_some() || projects::load(root)?.get(&name).is_some() {
            continue;
        }
        let Some(found) = detect(original, said_date(&text(field(row, "at")))) else {
            continue;
        };
        let url = text(field(row, "url"));
        let link = if url.starts_with("https://") || url.starts_with("http://") {
            url.as_str()
        } else {
            ""
        };
        let promise = json!({
            "event": text(field(row, "id")), "to": to_whom(row, rows), "due": text(field(&found, "due")),
            "made_at": text(field(row, "at")), "steps": {}
        });
        projects::add_promise(
            root,
            &name,
            &text(field(&found, "what")),
            &parent(&text(field(row, "project")), &every),
            link,
            if text(field(row, "source")) == "slack" {
                "chat"
            } else {
                "url"
            },
            promise,
        )?;
        made.push(name);
    }
    Ok(made)
}

pub fn scan(root: &Path, rows: &[Value], originals: &Map<String, Value>) -> Result<Value> {
    let selected = rows
        .iter()
        .filter(|row| candidate(row) && originals.contains_key(&text(field(row, "id"))))
        .cloned()
        .collect::<Vec<_>>();
    let pairs = selected
        .iter()
        .map(|row| {
            let original = originals
                .get(&text(field(row, "id")))
                .map(|value| text(field(value, "text")))
                .unwrap_or_default();
            (row.clone(), original)
        })
        .collect::<Vec<_>>();
    let made = track(root, rows, &pairs)?;
    Ok(json!({"events": selected.len(), "new": made.len()}))
}

pub fn add(
    root: &Path,
    title: &str,
    to: &str,
    due: &str,
    link: &str,
    parent_name: &str,
) -> Result<String> {
    let due = if due.is_empty() {
        String::new()
    } else {
        NaiveDate::parse_from_str(due.trim(), "%Y-%m-%d")
            .map(|day| day.to_string())
            .map_err(|_| ProjectError::project("the due day looks like 2026-11-03"))?
    };
    if !link.is_empty() && !link.starts_with("https://") && !link.starts_with("http://") {
        return Err(ProjectError::project("a promise's link is an http(s) link"));
    }
    let made_at = now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let every = projects::load(root)?;
    let mut number = 0;
    let name = loop {
        let name = name_for(&format!("manual:{made_at}:{title}:{number}"));
        if every.get(&name).is_none() {
            break name;
        }
        number += 1;
    };
    projects::add_promise(
        root,
        &name,
        title,
        parent_name,
        link,
        "url",
        json!({"event": "", "to": to.split_whitespace().collect::<Vec<_>>().join(" "), "due": due, "made_at": made_at, "steps": {}}),
    )?;
    Ok(name)
}

pub fn next_step(project: &Value) -> String {
    let done = field(&project["promise"], "steps")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    STEPS
        .iter()
        .find(|step| !done.contains_key(**step))
        .unwrap_or(&"status")
        .to_string()
}

pub fn view(name: &str, project: &Value, business: bool) -> Value {
    let promise = field(project, "promise")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let link = project["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|source| ["chat", "url"].contains(&text(field(source, "kind")).as_str()))
        .map(|source| text(field(source, "ref")))
        .unwrap_or_default();
    let due = text(field(&promise, "due"));
    let archived = !text(field(project, "archived_at")).is_empty();
    let mut row = json!({
        "name": name, "due": due, "overdue": !due.is_empty() && due < today().to_string(),
        "made_at": text(field(&promise, "made_at")), "event": text(field(&promise, "event")),
        "url": link, "parent": text(field(project, "parent")), "steps": field(&promise, "steps").cloned().unwrap_or_else(|| json!({})),
        "next": next_step(project), "closed": if archived { text(field(&promise, "closed")) } else { String::new() }
    });
    if business {
        row["title"] = json!(text(field(project, "title")));
        row["to"] = json!(text(field(&promise, "to")));
    }
    row
}

pub fn open_list(root: &Path, business: bool, include_closed: bool) -> Result<Vec<Value>> {
    let every = projects::load(root)?;
    let mut rows = every
        .as_object()
        .into_iter()
        .flat_map(|records| records.iter())
        .filter(|(_, project)| {
            text(field(project, "kind")) == "promise"
                && (include_closed || text(field(project, "archived_at")).is_empty())
        })
        .map(|(name, project)| view(name, project, business))
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| {
        (
            crate::truthy(field(row, "closed")),
            text(field(row, "due")).is_empty(),
            text(field(row, "due")),
            text(field(row, "made_at")),
            text(field(row, "name")),
        )
    });
    Ok(rows)
}

fn get_promise(root: &Path, name: &str) -> Result<Value> {
    let every = projects::load(root)?;
    let project = every
        .get(name)
        .cloned()
        .ok_or_else(|| ProjectError::project(format!("no promise {name}")))?;
    if text(field(&project, "kind")) != "promise" {
        return Err(ProjectError::project(format!("no promise {name}")));
    }
    Ok(project)
}

pub fn close(root: &Path, name: &str, how: &str) -> Result<()> {
    if !CLOSED.contains(&how) {
        return Err(ProjectError::project("a promise closes as done or dropped"));
    }
    get_promise(root, name)?;
    projects::close_promise(root, name, how)
}

pub fn step(root: &Path, name: &str, which: &str, value: &str) -> Result<Value> {
    if !STEPS.contains(&which) {
        return Err(ProjectError::project(
            "a promise step is one of research, draft, status",
        ));
    }
    if !text(field(&get_promise(root, name)?, "archived_at")).is_empty() {
        return Err(ProjectError::project("promise is closed"));
    }
    projects::promise_step(root, name, which, value)
}

pub fn handoff_items(root: &Path, business: bool) -> Result<Vec<Value>> {
    Ok(open_list(root, true, false)?
        .into_iter()
        .map(|row| {
            let mut item =
                json!({"id": text(field(&row, "name")), "section": "promises", "live": true});
            if !text(field(&row, "url")).is_empty() {
                item["link"] = row["url"].clone();
            }
            if business {
                let mut value = text(field(&row, "title"));
                if !text(field(&row, "to")).is_empty() {
                    value += &format!(" (to {})", text(field(&row, "to")));
                }
                if !text(field(&row, "due")).is_empty() {
                    value += &format!(", due {}", text(field(&row, "due")));
                }
                item["text"] = json!(value);
            }
            item
        })
        .collect())
}
