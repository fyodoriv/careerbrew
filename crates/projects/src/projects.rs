//! Project records, readiness gates and atomic project-file mutations.

use crate::{
    chars, field, now, one_line, read_yaml, stamp, text, today, truthy, write_yaml, ProjectError,
    Result,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Map, Value};
use std::cell::Cell;
use std::path::{Path, PathBuf};

pub const GATES: &[&str] = &["tool", "links", "questions", "quiz"];
pub const UPDATE_KINDS: &[&str] = &["company", "project", "team", "other", "approval"];
const GATE_TEXT: &[&str] = &[
    "try the tool hands-on (the app, the coding pad, the deck)",
    "read every source link in the pack",
    "write your own questions",
    "pass the agent quiz on the format, rubric, questions, and logistics",
];
const SIGN_IN_HOSTS: &[&str] = &[
    "docs.google.com",
    "drive.google.com",
    "outlook.office.com",
    "outlook.office365.com",
    "outlook.live.com",
    "sharepoint.com",
    "teams.microsoft.com",
    "slack.com",
];

thread_local! {
    static LOCK_DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct DepthGuard;
impl Drop for DepthGuard {
    fn drop(&mut self) {
        LOCK_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}

/// One data-root project-file transaction. Nested calls on the same thread
/// share its lock; separate threads and processes exclude each other.
pub fn with_lock<T>(root: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let nested = LOCK_DEPTH.with(|depth| depth.get() > 0);
    if nested {
        LOCK_DEPTH.with(|depth| depth.set(depth.get() + 1));
        let _guard = DepthGuard;
        return f();
    }
    let path = projects_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    careerbrew_vault::lock::with_lock(&path, || {
        LOCK_DEPTH.with(|depth| depth.set(1));
        let _guard = DepthGuard;
        f()
    })?
}

pub fn projects_path(root: &Path) -> PathBuf {
    root.join("work/projects.yaml")
}

pub fn load(root: &Path) -> Result<Value> {
    let data = read_yaml(root, "work/projects.yaml", json!({"projects": {}}))?;
    Ok(field(&data, "projects")
        .cloned()
        .unwrap_or_else(|| json!({})))
}

pub fn save(root: &Path, every: &Value) -> Result<()> {
    write_yaml(root, "work/projects.yaml", &json!({"projects": every}))
}

fn get<'a>(every: &'a Value, name: &str) -> Result<&'a Value> {
    every.get(name).ok_or_else(|| {
        let mut have = every
            .as_object()
            .map(|rows| rows.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        have.sort();
        ProjectError::project(format!(
            "unknown project {name} (fix: `careerbrew work project add {name}`; have: {})",
            if have.is_empty() {
                "none yet".into()
            } else {
                have.join(", ")
            }
        ))
    })
}

fn get_mut<'a>(every: &'a mut Value, name: &str) -> Result<&'a mut Value> {
    if every.get(name).is_none() {
        return Err(ProjectError::project(format!("unknown project {name}")));
    }
    Ok(every.get_mut(name).expect("checked project"))
}

fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=40).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

pub fn new_record() -> Value {
    json!({
        "incarnation_id": uuid::Uuid::new_v4().simple().to_string(),
        "created_at": stamp(),
        "memory": "",
        "stale_after_days": 7,
        "sources": [],
        "tasks": []
    })
}

#[derive(Default)]
pub struct AddOptions {
    pub title: String,
    pub group: String,
    pub parent: String,
    pub links: Vec<String>,
    pub kind: String,
    pub event_on: String,
}

fn set_optional(record: &mut Value, key: &str, value: &str) {
    if value.is_empty() {
        record.as_object_mut().expect("project object").remove(key);
    } else {
        record[key] = json!(value);
    }
}

fn graph_problems(every: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for (name, project) in every.as_object().into_iter().flat_map(|rows| rows.iter()) {
        let parent = text(field(project, "parent"));
        if parent == *name {
            out.push(format!("{name}: a project cannot be its own parent"));
        } else if !parent.is_empty() && every.get(&parent).is_none() {
            out.push(format!("{name}: parent {parent} is not a project"));
        }
        for link in crate::strings(field(project, "links")) {
            if link == *name {
                out.push(format!("{name}: a project cannot link to itself"));
            } else if every.get(&link).is_none() {
                out.push(format!("{name}: link {link} is not a project"));
            }
        }
        let mut chain = vec![name.clone()];
        let mut current = parent;
        while every.get(&current).is_some() && !chain.contains(&current) {
            chain.push(current.clone());
            current = text(field(&every[&current], "parent"));
        }
        if chain.contains(&current) && chain.last() != Some(&current) {
            let start = chain.iter().position(|item| item == &current).unwrap_or(0);
            let mut loop_names = chain[start..].to_vec();
            loop_names.push(current);
            out.push(format!("{name}: parents loop: {}", loop_names.join(" -> ")));
        }
    }
    out
}

fn check_graph(every: &Value) -> Result<()> {
    let problems = graph_problems(every);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(ProjectError::project(problems.join("; ")))
    }
}

pub fn add(root: &Path, name: &str, options: AddOptions) -> Result<Value> {
    with_lock(root, || {
        if !valid_name(name) {
            return Err(ProjectError::project(
                "a project name uses lowercase letters, digits, and dashes (max 40)",
            ));
        }
        let mut every = load(root)?;
        if every.get(name).is_some() {
            return Err(ProjectError::project(format!(
                "project {name} already exists"
            )));
        }
        if !options.kind.is_empty() && options.kind != "prep" {
            return Err(ProjectError::project(format!(
                "unknown project kind {}",
                options.kind
            )));
        }
        if !options.event_on.is_empty() {
            crate::date(&options.event_on)
                .map_err(|_| ProjectError::project("the event day looks like 2026-11-03"))?;
            if options.kind != "prep" {
                return Err(ProjectError::project(
                    "only a prep project has an event day (fix: add --kind prep)",
                ));
            }
        }
        let mut record = new_record();
        set_optional(
            &mut record,
            "title",
            &one_line(&options.title, "title", 200)?,
        );
        set_optional(
            &mut record,
            "group",
            &one_line(&options.group, "group", 60)?,
        );
        set_optional(&mut record, "parent", &options.parent);
        set_optional(&mut record, "kind", &options.kind);
        set_optional(&mut record, "event_on", &options.event_on);
        if !options.links.is_empty() {
            record["links"] = json!(options.links);
        }
        every[name] = record;
        check_graph(&every)?;
        let answer = every[name].clone();
        save(root, &every)?;
        Ok(answer)
    })
}

pub fn edit_parent(root: &Path, name: &str, parent: &str) -> Result<()> {
    with_lock(root, || {
        let mut every = load(root)?;
        get_mut(&mut every, name)?["parent"] = json!(parent);
        check_graph(&every)
    })
}

pub fn remove(root: &Path, name: &str) -> Result<()> {
    with_lock(root, || {
        let mut every = load(root)?;
        get(&every, name)?;
        let used = every
            .as_object()
            .into_iter()
            .flat_map(|rows| rows.iter())
            .any(|(other, project)| {
                other != name
                    && (text(field(project, "parent")) == name
                        || crate::strings(field(project, "links")).contains(&name.to_owned()))
            });
        if used {
            return Err(ProjectError::project(
                "another project names it as a parent or link",
            ));
        }
        every.as_object_mut().expect("project map").remove(name);
        save(root, &every)
    })
}

fn source_ref(kind: &str, reference: &str) -> String {
    let reference = reference.trim();
    if kind == "chat" && reference.starts_with('#') {
        reference[1..].to_owned()
    } else {
        reference.to_owned()
    }
}

pub fn add_source(root: &Path, name: &str, kind: &str, reference: &str) -> Result<()> {
    with_lock(root, || {
        if !["doc", "tracker", "repo", "chat", "memory", "url"].contains(&kind) {
            return Err(ProjectError::project(format!("unknown source kind {kind}")));
        }
        let reference = source_ref(kind, reference);
        let mut every = load(root)?;
        let project = get_mut(&mut every, name)?;
        let mut proposals = project["proposed"].as_array().cloned().unwrap_or_default();
        proposals.retain(|proposal| {
            text(field(proposal, "kind")) != kind || text(field(proposal, "ref")) != reference
        });
        project["proposed"] = Value::Array(proposals);
        let sources = project["sources"].as_array_mut().expect("source list");
        if sources.iter().any(|source| {
            text(field(source, "kind")) == kind && text(field(source, "ref")) == reference
        }) {
            return Err(ProjectError::project(format!(
                "{kind} {reference} is already a source of {name}"
            )));
        }
        sources.push(json!({"kind": kind, "ref": reference, "checked_at": null, "status": ""}));
        save(root, &every)
    })
}

pub fn check(root: &Path, name: &str, reference: Option<&str>, status: &str) -> Result<usize> {
    with_lock(root, || {
        let mut every = load(root)?;
        let project = get_mut(&mut every, name)?;
        let mut count = 0;
        for source in project["sources"].as_array_mut().expect("source list") {
            if reference.is_none_or(|wanted| text(field(source, "ref")) == wanted) {
                source["checked_at"] = json!(stamp());
                source
                    .as_object_mut()
                    .expect("source object")
                    .remove("stale_reason");
                if !status.is_empty() {
                    source["status"] = json!(status);
                }
                count += 1;
            }
        }
        if count == 0 {
            return Err(ProjectError::project("no matching source"));
        }
        save(root, &every)?;
        Ok(count)
    })
}

pub fn record_update(root: &Path, name: &str, kind: &str, value: &str) -> Result<Value> {
    with_lock(root, || {
        if !UPDATE_KINDS.contains(&kind) {
            return Err(ProjectError::project(format!("unknown update kind {kind}")));
        }
        let value = one_line(value, "an update", 200)?;
        if value.is_empty() {
            return Err(ProjectError::project("an update needs one line of text"));
        }
        let mut every = load(root)?;
        let project = get_mut(&mut every, name)?;
        let row = json!({"at": stamp(), "kind": kind, "text": value});
        let mut updates = project["updates"].as_array().cloned().unwrap_or_default();
        updates.push(row.clone());
        let start = updates.len().saturating_sub(20);
        project["updates"] = Value::Array(updates[start..].to_vec());
        save(root, &every)?;
        Ok(row)
    })
}

pub fn session_record(root: &Path, name: &str) -> Result<(Value, bool)> {
    let every = load(root)?;
    let record = get(&every, name)?.clone();
    let identity = text(field(&record, "incarnation_id"));
    if identity.is_empty() {
        return Ok((record, true));
    }
    if identity.len() != 32
        || !identity
            .bytes()
            .all(|b| b.is_ascii_hexdigit() || b.is_ascii_lowercase())
    {
        return Err(ProjectError::project("saved project identity is invalid"));
    }
    Ok((record, false))
}

pub fn root_name(every: &Value, name: &str) -> String {
    let mut chain = vec![name.to_owned()];
    let mut current = every
        .get(name)
        .map(|p| text(field(p, "parent")))
        .unwrap_or_default();
    while every.get(&current).is_some() && !chain.contains(&current) {
        chain.push(current.clone());
        current = text(field(&every[&current], "parent"));
    }
    if chain.contains(&current) {
        chain[chain.iter().position(|item| item == &current).unwrap_or(0)..]
            .iter()
            .min()
            .cloned()
            .unwrap_or(name.into())
    } else {
        chain.last().cloned().unwrap_or(name.into())
    }
}

pub fn notes_path(every: &Value, name: &str) -> String {
    let mut current = name.to_owned();
    let mut seen = Vec::new();
    while let Some(project) = every.get(&current) {
        if seen.contains(&current) {
            break;
        }
        let memory = text(field(project, "memory"));
        if !memory.is_empty() {
            return memory;
        }
        seen.push(current.clone());
        current = text(field(project, "parent"));
    }
    String::new()
}

fn host(reference: &str) -> String {
    reference
        .split_once("://")
        .map(|(_, rest)| {
            rest.split('/')
                .next()
                .unwrap_or_default()
                .split('@')
                .next_back()
                .unwrap_or_default()
                .split(':')
                .next()
                .unwrap_or_default()
                .to_lowercase()
        })
        .unwrap_or_default()
}

pub fn reference_reason(source: &Value) -> String {
    if field(source, "reference") == Some(&Value::Bool(true)) {
        return "marked as a reference".into();
    }
    if field(source, "reference") == Some(&Value::Bool(false)) {
        return String::new();
    }
    let host = host(&text(field(source, "ref")));
    if SIGN_IN_HOSTS
        .iter()
        .any(|known| host == *known || host.ends_with(&format!(".{known}")))
    {
        format!("careerbrew has no reader for {host} links (they open a sign-in page)")
    } else {
        String::new()
    }
}

fn age_days(source: &Value) -> Option<f64> {
    let checked = text(field(source, "checked_at"));
    if checked.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(&checked).ok().map(|value| {
        (now().with_timezone(&Utc) - value.with_timezone(&Utc)).num_milliseconds() as f64
            / 86_400_000.0
    })
}

fn stale(source: &Value, limit: i64) -> bool {
    reference_reason(source).is_empty() && age_days(source).is_none_or(|age| age > limit as f64)
}

fn source_view(source: &Value, limit: i64, archived: bool) -> Value {
    let reason = reference_reason(source);
    json!({
        "kind": text(field(source, "kind")),
        "ref": text(field(source, "ref")),
        "checked_at": field(source, "checked_at").cloned().unwrap_or(Value::Null),
        "status": text(field(source, "status")),
        "stale": !archived && stale(source, limit),
        "reference": !reason.is_empty(),
        "reference_reason": reason,
    })
}

pub fn show(root: &Path, name: &str) -> Result<Value> {
    let every = load(root)?;
    let project = get(&every, name)?;
    let limit = field(project, "stale_after_days")
        .and_then(Value::as_i64)
        .unwrap_or(7);
    let sources = project["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|source| source_view(source, limit, false))
        .collect::<Vec<_>>();
    Ok(json!({
        "name": name,
        "sources": sources,
        "prep": readiness(name, project),
        "title": text(field(project, "title")),
        "group": text(field(project, "group")),
        "parent": text(field(project, "parent")),
    }))
}

pub fn list_rows(root: &Path) -> Result<Vec<Value>> {
    let every = load(root)?;
    Ok(every.as_object().into_iter().flat_map(|rows| rows.iter()).map(|(name, project)| {
        json!({"name": name, "archived": !text(field(project, "archived_at")).is_empty(), "prep": readiness(name, project)})
    }).collect())
}

pub fn watch_list(root: &Path) -> Result<Vec<Value>> {
    let every = load(root)?;
    let mut out = Vec::new();
    for (name, project) in every.as_object().into_iter().flat_map(|rows| rows.iter()) {
        if text(field(project, "kind")) == "promise" {
            continue;
        }
        let latest = project["updates"]
            .as_array()
            .and_then(|rows| rows.last())
            .cloned()
            .unwrap_or(Value::Null);
        if let Some(object) = latest.as_object() {
            let mut restricted = object.clone();
            restricted.remove("text");
            out.push(json!({"name": name, "latest_update": Value::Object(restricted)}));
        } else {
            out.push(json!({"name": name, "latest_update": latest}));
        }
    }
    Ok(out)
}

pub fn tree(root: &Path) -> Result<Vec<Value>> {
    let every = load(root)?;
    let mut out = Vec::new();
    fn walk(name: &str, depth: usize, every: &Value, out: &mut Vec<Value>, seen: &mut Vec<String>) {
        if seen.contains(&name.to_owned()) || text(field(&every[name], "kind")) == "promise" {
            return;
        }
        seen.push(name.to_owned());
        out.push(json!({"name": name, "depth": depth}));
        let mut children = every
            .as_object()
            .into_iter()
            .flat_map(|rows| rows.iter())
            .filter(|(_, project)| text(field(project, "parent")) == name)
            .map(|(child, _)| child.clone())
            .collect::<Vec<_>>();
        children.sort_by_key(|child| {
            (
                text(field(&every[child], "group")).is_empty(),
                child.clone(),
            )
        });
        for child in children {
            walk(&child, depth + 1, every, out, seen);
        }
    }
    let mut names = every
        .as_object()
        .into_iter()
        .flat_map(|rows| rows.keys().cloned())
        .collect::<Vec<_>>();
    names.sort_by_key(|name| {
        let project = &every[name];
        let grouped = !text(field(project, "group")).is_empty();
        let parent = text(field(project, "parent"));
        (
            !grouped,
            !parent.is_empty() && every.get(&parent).is_some(),
            text(field(project, "group")),
            name.clone(),
        )
    });
    let mut seen = Vec::new();
    for name in names {
        let parent = text(field(&every[&name], "parent"));
        if parent.is_empty() || every.get(&parent).is_none() {
            walk(&name, 0, &every, &mut out, &mut seen);
        }
    }
    Ok(out)
}

pub fn stale_sources(root: &Path) -> Result<Vec<Value>> {
    let every = load(root)?;
    let mut out = Vec::new();
    for (name, project) in every.as_object().into_iter().flat_map(|rows| rows.iter()) {
        if !text(field(project, "archived_at")).is_empty() {
            continue;
        }
        let limit = field(project, "stale_after_days")
            .and_then(Value::as_i64)
            .unwrap_or(7);
        for source in project["sources"].as_array().into_iter().flatten() {
            if stale(source, limit) {
                out.push(json!({"project": name, "kind": text(field(source, "kind")), "ref": text(field(source, "ref"))}));
            }
        }
    }
    Ok(out)
}

pub fn task_projects(root: &Path) -> Result<Map<String, Value>> {
    let every = load(root)?;
    let mut out = Map::new();
    for (name, project) in every.as_object().into_iter().flat_map(|rows| rows.iter()) {
        for task in crate::strings(field(project, "tasks")) {
            out.entry(task).or_insert_with(|| json!(name));
        }
    }
    Ok(out)
}

pub fn link_task(root: &Path, name: &str, task: &str) -> Result<()> {
    with_lock(root, || {
        let mut every = load(root)?;
        let project = get_mut(&mut every, name)?;
        let tasks = project["tasks"].as_array_mut().expect("task list");
        if !tasks.contains(&json!(task)) {
            tasks.push(json!(task));
        }
        save(root, &every)
    })
}

pub fn readiness(name: &str, project: &Value) -> Value {
    if text(field(project, "kind")) != "prep" {
        return Value::Null;
    }
    let marks = field(project, "gates")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let links = project["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|source| text(field(source, "kind")) != "memory")
        .cloned()
        .collect::<Vec<_>>();
    let read = field(&marks, "links").cloned().unwrap_or_else(|| json!({}));
    let unread = links
        .iter()
        .filter(|source| read.get(text(field(source, "ref"))).is_none())
        .cloned()
        .collect::<Vec<_>>();
    let quizzes = project["quizzes"].as_array().cloned().unwrap_or_default();
    let last = quizzes.last().cloned().unwrap_or(Value::Null);
    let mut rows = Vec::new();
    for (index, gate) in GATES.iter().enumerate() {
        let (done, at, note, fix) = match *gate {
            "links" => {
                let done = !links.is_empty() && unread.is_empty();
                let at = if done {
                    links
                        .iter()
                        .filter_map(|source| read.get(text(field(source, "ref"))))
                        .cloned()
                        .max_by_key(|value| text(Some(value)))
                        .unwrap_or(Value::Null)
                } else {
                    Value::Null
                };
                let note = if links.is_empty() {
                    "no links in the pack yet".into()
                } else {
                    format!(
                        "{} of {} read by you",
                        links.len() - unread.len(),
                        links.len()
                    )
                };
                let fix = if let Some(source) = unread.first() {
                    format!(
                        "careerbrew work project gate {name} links {}",
                        text(field(source, "ref"))
                    )
                } else {
                    format!("careerbrew work project source {name} url <link>")
                };
                (done, at, note, fix)
            }
            "quiz" => {
                let done = !last.is_null() && last["missed"].as_i64() == Some(0);
                let note = if last.is_null() {
                    "not taken yet".into()
                } else {
                    format!(
                        "last: {} of {}, {} miss(es)",
                        last["asked"].as_i64().unwrap_or(0) - last["missed"].as_i64().unwrap_or(0),
                        last["asked"].as_i64().unwrap_or(0),
                        last["missed"].as_i64().unwrap_or(0)
                    )
                };
                let fix = format!("careerbrew work project quiz {name} --asked <n> --missed <n>")
                    + &if last["missed"].as_i64().is_some_and(|missed| missed > 0) {
                        format!(
                            " (re-quiz the {} miss(es))",
                            last["missed"].as_i64().unwrap_or(0)
                        )
                    } else {
                        String::new()
                    };
                (
                    done,
                    if done {
                        last["at"].clone()
                    } else {
                        Value::Null
                    },
                    note,
                    fix,
                )
            }
            marked => {
                let at = field(&marks, marked).cloned().unwrap_or(Value::Null);
                (
                    !at.is_null() && !text(Some(&at)).is_empty(),
                    at,
                    String::new(),
                    format!("careerbrew work project gate {name} {marked}"),
                )
            }
        };
        rows.push(json!({"gate": gate, "text": GATE_TEXT[index], "done": done, "at": at, "note": note, "fix": if done { "" } else { &fix }}));
    }
    let done = rows.iter().filter(|row| truthy(field(row, "done"))).count();
    let quiz = if last.is_null() {
        Value::Null
    } else {
        let asked = last["asked"].as_f64().unwrap_or(0.0);
        let missed = last["missed"].as_f64().unwrap_or(0.0);
        json!({"at": last["at"], "asked": last["asked"], "missed": last["missed"], "score": crate::python_round((asked - missed) / asked, 2)})
    };
    json!({"event_on": text(field(project, "event_on")), "done": done, "total": GATES.len(), "ready": done == GATES.len(), "gates": rows, "quiz": quiz, "quizzes": quizzes.len()})
}

pub fn gate(root: &Path, name: &str, which: &str, refs: &[String], undo: bool) -> Result<Value> {
    with_lock(root, || {
        let mut every = load(root)?;
        let project = get_mut(&mut every, name)?;
        if text(field(project, "kind")) != "prep" {
            return Err(ProjectError::project("not a prep project"));
        }
        if !["tool", "links", "questions"].contains(&which) {
            return Err(ProjectError::project("gate is not marked by hand"));
        }
        if (which == "links") != !refs.is_empty() {
            return Err(ProjectError::project("wrong refs for gate"));
        }
        let mut marks = field(project, "gates")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if which == "links" {
            let have = project["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|source| text(field(source, "kind")) != "memory")
                .map(|source| text(field(source, "ref")))
                .collect::<Vec<_>>();
            if let Some(missing) = refs.iter().find(|reference| !have.contains(reference)) {
                return Err(ProjectError::project(format!(
                    "{name} has no link {missing}"
                )));
            }
            let mut read = field(&marks, "links").cloned().unwrap_or_else(|| json!({}));
            for reference in refs {
                if undo {
                    read.as_object_mut().expect("link marks").remove(reference);
                } else {
                    read[reference] = json!(stamp());
                }
            }
            marks["links"] = read;
        } else if undo {
            marks.as_object_mut().expect("gate marks").remove(which);
        } else {
            marks[which] = json!(stamp());
        }
        project["gates"] = marks;
        let answer = readiness(name, project);
        save(root, &every)?;
        Ok(answer)
    })
}

pub fn quiz(root: &Path, name: &str, asked: i64, missed: i64) -> Result<Value> {
    with_lock(root, || {
        let mut every = load(root)?;
        let project = get_mut(&mut every, name)?;
        if text(field(project, "kind")) != "prep" {
            return Err(ProjectError::project("not a prep project"));
        }
        if !(1..=500).contains(&asked) || missed < 0 || missed > asked {
            return Err(ProjectError::project(
                "a quiz asks 1 to 500 questions and misses 0 to all of them",
            ));
        }
        let mut quizzes = project["quizzes"].as_array().cloned().unwrap_or_default();
        if quizzes
            .last()
            .and_then(|last| last["missed"].as_i64())
            .is_some_and(|last| last > asked)
        {
            return Err(ProjectError::project("a re-quiz asks each miss again"));
        }
        quizzes.push(json!({"at": stamp(), "asked": asked, "missed": missed}));
        project["quizzes"] = Value::Array(quizzes[quizzes.len().saturating_sub(20)..].to_vec());
        let answer = readiness(name, project);
        save(root, &every)?;
        Ok(answer)
    })
}

pub fn archive(root: &Path, name: &str, on: bool, force: bool) -> Result<()> {
    with_lock(root, || {
        let mut every = load(root)?;
        let project = get_mut(&mut every, name)?;
        let ready = readiness(name, project);
        if on && !ready.is_null() && !truthy(field(&ready, "ready")) && !force {
            let open = ready["gates"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|gate| !truthy(field(gate, "done")))
                .cloned()
                .collect::<Vec<_>>();
            let names = open
                .iter()
                .map(|gate| text(field(gate, "gate")))
                .collect::<Vec<_>>()
                .join(", ");
            let fix = open
                .first()
                .map(|gate| text(field(gate, "fix")))
                .unwrap_or_default();
            return Err(ProjectError::project(format!("{name} is not ready: {} of {} gate(s) open ({names}); fix: {fix} (the event was cancelled? archive it with --force)", open.len(), GATES.len())));
        }
        if on {
            project["archived_at"] = json!(stamp());
        } else {
            project
                .as_object_mut()
                .expect("project object")
                .remove("archived_at");
        }
        if text(field(project, "kind")) == "promise" {
            if on {
                if text(field(&project["promise"], "closed")).is_empty() {
                    project["promise"]["closed"] = json!("done");
                }
            } else {
                project["promise"]
                    .as_object_mut()
                    .expect("promise object")
                    .remove("closed");
            }
        }
        save(root, &every)
    })
}

pub fn gate_blocks(root: &Path, day: NaiveDate) -> Result<Vec<Value>> {
    let every = load(root)?;
    let mut out = Vec::new();
    for (name, project) in every.as_object().into_iter().flat_map(|rows| rows.iter()) {
        let on = NaiveDate::parse_from_str(&text(field(project, "event_on")), "%Y-%m-%d");
        let Ok(on) = on else { continue };
        if !text(field(project, "archived_at")).is_empty()
            || !(day <= on && on <= day.succ_opt().unwrap_or(day))
        {
            continue;
        }
        let ready = readiness(name, project);
        if ready.is_null() {
            continue;
        }
        for gate in ready["gates"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|gate| !truthy(field(gate, "done")))
        {
            let when = if on == day { "today" } else { "tomorrow" };
            out.push(json!({
                "id": format!("gate:{name}:{}", text(field(gate, "gate"))),
                "title": format!("Prep gate: {}", text(field(gate, "text"))),
                "minutes": 15,
                "gate": true,
                "project": name,
                "reason": format!("the event is {when} and this gate is open (see: careerbrew work project show {name})")
            }));
        }
    }
    Ok(out)
}

pub fn add_promise(
    root: &Path,
    name: &str,
    title: &str,
    parent: &str,
    link: &str,
    source_kind: &str,
    promise: Value,
) -> Result<()> {
    with_lock(root, || {
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() {
            return Err(ProjectError::project(
                "a promise needs one line of text: what you promised",
            ));
        }
        let mut every = load(root)?;
        if every.get(name).is_some() {
            return Err(ProjectError::project(format!(
                "project {name} already exists"
            )));
        }
        if !parent.is_empty() {
            get(&every, parent)?;
        }
        let mut record = new_record();
        record["kind"] = json!("promise");
        record["title"] = json!(one_line(&chars(&title, 120), "a promise", 120)?);
        record["promise"] = promise;
        set_optional(&mut record, "parent", parent);
        if !link.is_empty() {
            record["sources"] = json!([{"kind": source_kind, "ref": link, "checked_at": null, "status": "", "reference": true}]);
        }
        every[name] = record;
        save(root, &every)
    })
}

pub fn close_promise(root: &Path, name: &str, how: &str) -> Result<()> {
    with_lock(root, || {
        let mut every = load(root)?;
        let project = get_mut(&mut every, name)?;
        let mut promise = field(project, "promise")
            .cloned()
            .unwrap_or_else(|| json!({}));
        promise
            .as_object_mut()
            .expect("promise object")
            .remove("pm_removed");
        promise
            .as_object_mut()
            .expect("promise object")
            .remove("manual_kept_day");
        promise["closed"] = json!(how);
        project["promise"] = promise;
        project["archived_at"] = json!(stamp());
        save(root, &every)
    })
}

pub fn promise_step(root: &Path, name: &str, which: &str, value: &str) -> Result<Value> {
    with_lock(root, || {
        let value = one_line(value, "a step note", 200)?;
        if value.is_empty() {
            return Err(ProjectError::project("a step needs one line of text"));
        }
        let mut every = load(root)?;
        let project = get_mut(&mut every, name)?;
        let at = stamp();
        project["promise"]["steps"][which] = json!(at);
        let row = json!({"at": at, "kind": "project", "text": format!("{which}: {value}")});
        let mut updates = project["updates"].as_array().cloned().unwrap_or_default();
        updates.push(row.clone());
        project["updates"] = Value::Array(updates[updates.len().saturating_sub(20)..].to_vec());
        save(root, &every)?;
        Ok(row)
    })
}

pub fn valid_identity(record: &Value) -> bool {
    let id = text(field(record, "incarnation_id"));
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn current_day() -> NaiveDate {
    today()
}
