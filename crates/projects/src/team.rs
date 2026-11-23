//! Team roster validation, whole-word matching and roster-file locking.

use crate::{field, read_yaml, text, write_yaml, ProjectError, Result};
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;

const FIELDS: &[&str] = &["chat", "email", "login"];

pub fn load(root: &Path) -> Result<Vec<Value>> {
    let data = read_yaml(root, "work/team.yaml", json!({"team": []}))?;
    Ok(data["team"].as_array().into_iter().flatten().filter(|row| !text(field(row, "name")).is_empty()).map(|row| {
        json!({"name": text(field(row, "name")), "chat": text(field(row, "chat")), "email": text(field(row, "email")), "login": text(field(row, "login"))})
    }).collect())
}

fn save(root: &Path, rows: &[Value]) -> Result<()> {
    let rows = rows
        .iter()
        .map(|row| {
            let mut out = serde_json::Map::new();
            for key in ["name", "chat", "email", "login"] {
                let value = text(field(row, key));
                if !value.is_empty() {
                    out.insert(key.into(), json!(value));
                }
            }
            Value::Object(out)
        })
        .collect::<Vec<_>>();
    write_yaml(root, "work/team.yaml", &json!({"team": rows}))
}

fn one_line(value: &str, what: &str, limit: usize) -> Result<String> {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.chars().count() > limit {
        return Err(ProjectError::team(format!(
            "{what} is at most {limit} characters"
        )));
    }
    Ok(value)
}

pub fn add(root: &Path, name: &str, chat: &str, email: &str, login: &str) -> Result<Value> {
    let name = one_line(name, "a name", 80)?;
    if name.is_empty() {
        return Err(ProjectError::team("a teammate needs a name"));
    }
    let given = json!({
        "chat": one_line(chat, "--chat", 120)?,
        "email": one_line(email, "--email", 120)?.to_lowercase(),
        "login": one_line(login, "--login", 120)?.trim_start_matches('@').to_owned(),
    });
    let chat_id = text(field(&given, "chat"));
    if !chat_id.is_empty()
        && !Regex::new(r"^[UW][A-Z0-9]{2,30}$")
            .expect("chat id regex")
            .is_match(&chat_id)
    {
        return Err(ProjectError::team(
            "--chat is a Slack user id such as U0SAM",
        ));
    }
    let email = text(field(&given, "email"));
    if !email.is_empty() && !email.contains('@') {
        return Err(ProjectError::team("not an email address"));
    }
    let lock_path = root.join("work/team");
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    careerbrew_vault::lock::with_lock(&lock_path, || -> Result<Value> {
        let mut rows = load(root)?;
        let existing = rows
            .iter()
            .position(|row| text(field(row, "name")).to_lowercase() == name.to_lowercase());
        let added = existing.is_none();
        let index = existing.unwrap_or_else(|| {
            rows.push(json!({"name": name, "chat": "", "email": "", "login": ""}));
            rows.len() - 1
        });
        let before = rows[index].clone();
        for key in FIELDS {
            let value = text(field(&given, key));
            if !value.is_empty() {
                rows[index][key] = json!(value);
            }
        }
        let changed = added || rows[index] != before;
        if changed {
            save(root, &rows)?;
        }
        let mut answer = rows[index].as_object().cloned().unwrap_or_default();
        answer.insert("changed".into(), json!(changed));
        Ok(Value::Object(answer))
    })?
}

pub fn remove(root: &Path, name: &str) -> Result<()> {
    let name = name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let lock_path = root.join("work/team");
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    careerbrew_vault::lock::with_lock(&lock_path, || -> Result<()> {
        let rows = load(root)?;
        let kept = rows
            .iter()
            .filter(|row| text(field(row, "name")).to_lowercase() != name)
            .cloned()
            .collect::<Vec<_>>();
        if kept.len() == rows.len() {
            return Err(ProjectError::team("that name is not on your team"));
        }
        save(root, &kept)
    })?
}

fn word_match(pattern: &Regex, value: &str) -> bool {
    pattern.find_iter(value).any(|found| {
        let before = value[..found.start()].chars().next_back();
        let after = value[found.end()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '@')
            && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

pub fn matches(rows: &[Value], actor: &str, sender: &str, value: &str) -> Vec<String> {
    let who = [actor.to_lowercase(), sender.to_lowercase()];
    let mut out = Vec::new();
    for row in rows {
        let name = text(field(row, "name"));
        let ids = [
            name.clone(),
            text(field(row, "chat")),
            text(field(row, "email")),
            text(field(row, "login")),
        ]
        .into_iter()
        .filter(|id| !id.is_empty())
        .map(|id| id.to_lowercase())
        .collect::<Vec<_>>();
        let by_actor = who.iter().any(|who| !who.is_empty() && ids.contains(who));
        let by_name = Regex::new(&regex::escape(&name))
            .ok()
            .is_some_and(|pattern| word_match(&pattern, value));
        let chat = text(field(row, "chat"));
        let login = text(field(row, "login"));
        let by_id = [
            (!chat.is_empty()).then(|| format!("<@{}>", regex::escape(&chat))),
            (!login.is_empty()).then(|| format!("@{}", regex::escape(&login))),
        ]
        .into_iter()
        .flatten()
        .any(|id| {
            Regex::new(&format!("(?i){id}"))
                .ok()
                .is_some_and(|pattern| word_match(&pattern, value))
        });
        if by_actor || by_name || by_id {
            out.push(name);
        }
    }
    out.sort();
    out.dedup();
    out
}
