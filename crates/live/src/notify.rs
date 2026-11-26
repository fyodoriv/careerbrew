//! Opt-in notification selection, deduplication, quiet/snooze state and fake notifier adapters.

use crate::{
    chars, digest, emit, field, notify_error, now, one_line, parse_time, projects, read_json,
    read_yaml, setting_bool, setting_i64, setting_text, stamp, text, truthy, write_json_if_changed,
    Result,
};
use careerbrew_feed::feed;
use careerbrew_vault::canon::Canon;
use chrono::{DateTime, FixedOffset, NaiveDate, SecondsFormat, TimeDelta, TimeZone};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

pub const ACTIONS: &[&str] = &["read", "accept", "dismiss", "snooze"];
pub const OPEN: &[&str] = &["new", "snoozed", "read"];
pub const PENDING: &[Option<&str>] = &[
    None,
    Some("held:quiet"),
    Some("held:batch"),
    Some("held:project"),
    Some("held:snooze"),
];
pub const SNOOZE_TIMES: usize = 3;
pub const SNOOZE_DAYS: i64 = 14;
const TEXT: &[&str] = &[
    "source",
    "kind",
    "sub",
    "headline",
    "why",
    "next",
    "priority",
    "priority_why",
];
const REVIEW_KINDS: &[&str] = &["review", "review-request", "review_request", "pr-review"];

pub fn load(root: &Path) -> Result<Value> {
    let state = read_json(root, "day/notifications.json", json!({}))?;
    Ok(
        json!({"items": field(&state, "items").cloned().unwrap_or_else(|| json!([])), "popups": field(&state, "popups").cloned().unwrap_or_else(|| json!([]))}),
    )
}

fn save(root: &Path, state: &Value) -> Result<()> {
    write_json_if_changed(root, "day/notifications.json", state).map(|_| ())
}

fn locked<T>(root: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let path = root.join("day/notifications.lock");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    careerbrew_vault::lock::with_exact_lock(&path, f)?
}

fn dt(value: &str) -> DateTime<FixedOffset> {
    parse_time(value).unwrap_or_else(now)
}

fn iso(value: DateTime<FixedOffset>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub fn settings(root: &Path) -> Result<Value> {
    Ok(json!({
        "enabled": setting_bool(root, "notify_events", false)?,
        "min_priority": setting_i64(root, "notify_min_priority", 4)?,
        "quiet_hours": setting_text(root, "notify_quiet_hours", "19:00-08:00")?,
        "daily_cap": setting_i64(root, "notify_daily_cap", 6)?,
        "batch_minutes": setting_i64(root, "notify_batch_minutes", 10)?,
        "snooze_minutes": setting_i64(root, "notify_snooze_minutes", 60)?,
        "backend": setting_text(root, "notify_backend", "auto")?,
    }))
}

pub fn in_quiet(now_: DateTime<FixedOffset>, spec: &str) -> bool {
    if spec.is_empty() {
        return false;
    }
    let Some((start, end)) = spec.split_once('-') else {
        return false;
    };
    let time = now_.format("%H:%M").to_string();
    if start < end {
        start <= time.as_str() && time.as_str() < end
    } else {
        time.as_str() >= start || time.as_str() < end
    }
}

pub fn kind_of(row: &Value) -> String {
    let kind = text(field(row, "kind"));
    if kind == "page" {
        return if text(field(row, "status")) == "acknowledged" {
            "page-ack".into()
        } else {
            "page".into()
        };
    }
    if REVIEW_KINDS.contains(&kind.as_str())
        || row["tags"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|tag| text(Some(tag)) == "review")
    {
        return "review".into();
    }
    if ["dm", "mention", "mail"].contains(&kind.as_str()) {
        kind
    } else {
        "ask".into()
    }
}

fn top_project(keys: &[String], entries: &[Value]) -> String {
    careerbrew_projects::project_priority::matching(keys, entries)
        .filter(|hit| hit["rank"].as_i64() == Some(1))
        .map(|hit| text(field(&hit, "name")))
        .unwrap_or_default()
}

fn source_name(source: &str) -> String {
    match source {
        "slack" => "Slack".into(),
        "pagerduty" => "PagerDuty".into(),
        "outlook" => "Outlook".into(),
        "github" => "GitHub".into(),
        "gitlab" => "GitLab".into(),
        "jira" => "Jira".into(),
        "careerbrew" => "careerbrew".into(),
        "" => "a tool".into(),
        other => other.into(),
    }
}

pub fn ask_key(row: &Value, entries: &[Value]) -> String {
    let key = feed::thread_key(row, entries);
    format!(
        "feed:{}",
        if key.is_empty() {
            text(field(row, "id"))
        } else {
            key
        }
    )
}

fn kind_facts(kind: &str) -> (&'static str, &'static str, &'static str, i64) {
    match kind {
        "page" => (
            "a page is assigned to you",
            "an incident on you is open and nobody has acknowledged it",
            "acknowledge it in {src}, then fix it or hand it off",
            5,
        ),
        "page-ack" => (
            "a page you acknowledged is still open",
            "it stays on you until it is resolved",
            "resolve it in {src} or hand it off",
            3,
        ),
        "review" => (
            "a review is requested from you",
            "someone is waiting on your review to move on",
            "open the review in {src} and approve it or comment",
            4,
        ),
        "dm" => (
            "a direct message asks you",
            "it was sent to you directly and you have not replied",
            "reply in {src}",
            4,
        ),
        "mention" => (
            "someone mentioned you",
            "a mention asks you and you have not replied in the thread",
            "reply in the thread in {src}",
            3,
        ),
        "mail" => (
            "important mail for you",
            "it is marked important or flagged and you have not replied",
            "reply to it in {src}",
            3,
        ),
        _ => (
            "something asks you",
            "a rule marks it as an ask and you have not replied",
            "open it in {src} and reply",
            2,
        ),
    }
}

fn suggestion_facts(kind: &str) -> (&'static str, &'static str, i64) {
    match kind {
        "agent-task" => (
            "an agent proposes a task",
            "it is never planned until you accept it",
            3,
        ),
        "plugin" => (
            "a plugin suggests something",
            "the plugin waits for your decision; nothing changes until you decide",
            3,
        ),
        "new-task" => (
            "a ticket assigned to you has no task",
            "no plan includes work that is assigned to you",
            3,
        ),
        "goal" => (
            "a new goal is suggested",
            "your top promotion gap has no upcoming goal",
            3,
        ),
        "task-done" => (
            "a task looks done",
            "marking it done keeps your plan and the PM update right",
            2,
        ),
        "source" => (
            "a project has a new related source",
            "its work then counts for the project",
            2,
        ),
        _ => (
            "careerbrew has a suggestion",
            "nothing changes until you decide",
            3,
        ),
    }
}

fn labels(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "page" => ("page", "pages"),
        "review" => ("review request", "review requests"),
        "dm" => ("direct message", "direct messages"),
        "mention" => ("mention", "mentions"),
        "mail" => ("important mail", "important mails"),
        "ask" => ("ask", "asks"),
        _ => ("suggestion", "suggestions"),
    }
}

fn https_host(value: &str) -> String {
    let authority = value
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""))
        .unwrap_or("");
    if authority.contains('@') {
        return String::new();
    }
    careerbrew_sources::http::parse_url(value)
        .ok()
        .filter(|url| url.scheme == "https" && !url.host.is_empty() && url.port == 443)
        .map(|url| url.host)
        .unwrap_or_default()
}

fn quiet_project(root: &Path, name: &str, day: &str) -> bool {
    projects(root)
        .ok()
        .and_then(|rows| rows.get(name).cloned())
        .is_some_and(|project| {
            let until = text(field(&project, "quiet_until"));
            !until.is_empty() && until.as_str() > day
        })
}

#[allow(clippy::too_many_arguments)]
fn is_quiet(
    root: &Path,
    keys: &[String],
    explicit: &str,
    url: &str,
    entries: &[Value],
    known: &Map<String, Value>,
    github_hosts: &BTreeSet<String>,
    day: &str,
) -> bool {
    if known.contains_key(explicit) {
        return quiet_project(root, explicit, day);
    }
    let host = https_host(url);
    let item = (!host.is_empty())
        .then(|| careerbrew_memory::research::github_item(url))
        .flatten();
    if let Some(item) = &item {
        let identity = (
            host.to_lowercase(),
            item[0].to_lowercase(),
            item[1].to_lowercase(),
            item[2].clone(),
            item[3].clone(),
        );
        let owners = known
            .iter()
            .filter(|(_, project)| {
                project["sources"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|source| {
                        if text(field(source, "kind")) != "url" {
                            return false;
                        }
                        let reference = text(field(source, "ref"));
                        let source_host = https_host(&reference);
                        careerbrew_memory::research::github_item(&reference).is_some_and(
                            |source_item| {
                                (
                                    source_host.to_lowercase(),
                                    source_item[0].to_lowercase(),
                                    source_item[1].to_lowercase(),
                                    source_item[2].clone(),
                                    source_item[3].clone(),
                                ) == identity
                            },
                        )
                    })
            })
            .map(|(name, _)| name.clone())
            .collect::<BTreeSet<_>>();
        if owners.len() == 1 {
            return quiet_project(root, &owners.into_iter().next().expect("owner"), day);
        }
        if owners.len() > 1 {
            return false;
        }
        if github_hosts.contains(&host) {
            let repo = item[..2].join("/").to_lowercase();
            let owners = known
                .iter()
                .filter(|(_, project)| {
                    project["sources"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|source| {
                            text(field(source, "kind")) == "repo"
                                && field(source, "reference") != Some(&json!(true))
                                && text(field(source, "ref")).to_lowercase() == repo
                        })
                })
                .map(|(name, _)| name.clone())
                .collect::<BTreeSet<_>>();
            if owners.len() == 1 {
                return quiet_project(root, &owners.into_iter().next().expect("owner"), day);
            }
            if owners.len() > 1 {
                return false;
            }
        }
    }
    careerbrew_projects::project_priority::matching(keys, entries)
        .map(|hit| text(field(&hit, "name")))
        .is_some_and(|name| known.contains_key(&name) && quiet_project(root, &name, day))
}

pub fn candidates(root: &Path, now_: Option<DateTime<FixedOffset>>) -> Result<Vec<Value>> {
    std::fs::create_dir_all(root.join("work/feed"))?;
    let now_ = now_.unwrap_or_else(now);
    let entries = careerbrew_projects::project_priority::entries(root)?;
    let known = projects(root)?;
    let mut github_hosts = BTreeSet::new();
    if known.values().any(|project| {
        project["sources"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|source| text(field(source, "kind")) == "repo")
    }) {
        for connector in careerbrew_feed::sources::inventory(root)? {
            if text(field(&connector, "kind")) == "github" {
                let host = https_host(&text(field(&connector, "api")));
                if !host.is_empty() {
                    github_hosts.insert(if host == "api.github.com" {
                        "github.com".into()
                    } else {
                        host
                    });
                }
            }
        }
    }
    let day = now_.date_naive().format("%Y-%m-%d").to_string();
    let mut out = Vec::new();
    for row in feed::open_asks(root, None, false)? {
        let kind = kind_of(&row);
        let (what, why, next, base) = kind_facts(&kind);
        let source = source_name(&text(field(&row, "source")));
        let url = text(field(&row, "url"));
        let mut keys = careerbrew_projects::project_priority::keys(
            &text(field(&row, "title")),
            &careerbrew_projects::project_priority::repo_of("", &url),
            &url,
            "",
            &entries,
        );
        keys.push(text(field(&row, "channel")).to_lowercase());
        let project_quiet = is_quiet(root, &keys, "", &url, &entries, &known, &github_hosts, &day);
        let top = top_project(&keys, &entries);
        let label_kind = if kind == "page-ack" { "page" } else { &kind };
        out.push(json!({
            "key": ask_key(&row, &entries),
            "ref": text(field(&row, "id")),
            "project_quiet": project_quiet,
            "at": field(&row, "at").cloned().unwrap_or(Value::Null),
            "source": field(&row, "source").cloned().unwrap_or(Value::Null),
            "kind": if kind == "page-ack" { "page" } else { &kind },
            "sub": kind,
            "headline": format!("{source}: {what}"),
            "why": format!("{why}{}", if top.is_empty() { "" } else { "; it is on your rank-1 project" }),
            "next": next.replace("{src}", &source),
            "priority": (base + i64::from(!top.is_empty())).min(5),
            "priority_why": format!("{base} for a {}{}", labels(label_kind).0, if top.is_empty() { String::new() } else { format!(", +1 for {top}, your rank-1 project") }),
        }));
    }
    for item in inbox_load(root)? {
        if text(field(&item, "status")) != "open" || text(field(&item, "type")) != "proposal" {
            continue;
        }
        let data = field(&item, "data").cloned().unwrap_or_else(|| json!({}));
        let (what, why, base) = suggestion_facts(&text(field(&item, "kind")));
        let link = text(field(&data, "link"));
        let title = [
            text(field(&data, "title")),
            text(field(&data, "task")),
            text(field(&data, "what")),
        ]
        .into_iter()
        .find(|value| !value.is_empty())
        .unwrap_or_default();
        let keys = careerbrew_projects::project_priority::keys(
            &title,
            &careerbrew_projects::project_priority::repo_of("", &link),
            &link,
            &text(field(&data, "project")),
            &entries,
        );
        let project_quiet = is_quiet(
            root,
            &keys,
            &text(field(&data, "project")),
            &link,
            &entries,
            &known,
            &github_hosts,
            &day,
        );
        let top = top_project(&keys, &entries);
        let goal = text(field(&item, "kind")) == "goal";
        out.push(json!({
            "key": format!("inbox:{}", text(field(&item, "id"))),
            "ref": text(field(&item, "id")),
            "project_quiet": project_quiet,
            "item": text(field(&item, "id")),
            "at": field(&item, "created_at").cloned().unwrap_or_else(|| json!(iso(now_))),
            "source": "careerbrew",
            "kind": "suggestion",
            "sub": text(field(&item, "kind")),
            "headline": format!("careerbrew: {what}"),
            "why": format!("{why}{}", if top.is_empty() { "" } else { "; it is on your rank-1 project" }),
            "next": if goal { "approve or dismiss it in the web app" } else { "accept or dismiss it" },
            "priority": (base + i64::from(!top.is_empty())).min(5),
            "priority_why": format!("{base} for a suggestion{}", if top.is_empty() { String::new() } else { format!(", +1 for {top}, your rank-1 project") }),
        }));
    }
    let tired = snoozed_kinds(root, now_)?;
    for candidate in &mut out {
        let at = dt(&text(field(candidate, "at")));
        candidate["fresh"] = json!(now_ - at <= TimeDelta::hours(24));
        let sub = text(field(candidate, "sub"));
        if let Some(count) = tired.get(&sub) {
            candidate["priority"] =
                json!((candidate["priority"].as_i64().unwrap_or_default() - 1).max(1));
            candidate["priority_why"] = json!(format!(
                "{}, -1: you snoozed {} like it in {SNOOZE_DAYS} days",
                text(field(candidate, "priority_why")),
                count
            ));
        }
    }
    Ok(out)
}

fn snoozed_kinds(root: &Path, now_: DateTime<FixedOffset>) -> Result<BTreeMap<String, usize>> {
    let cut = now_ - TimeDelta::days(SNOOZE_DAYS);
    let mut counts = BTreeMap::new();
    for row in load(root)?["items"].as_array().into_iter().flatten() {
        let count = row["snoozes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|at| dt(&text(Some(at))) >= cut)
            .count();
        if count > 0 {
            let kind = [text(field(row, "sub")), text(field(row, "kind"))]
                .into_iter()
                .find(|value| !value.is_empty())
                .unwrap_or_default();
            *counts.entry(kind).or_insert(0) += count;
        }
    }
    Ok(counts
        .into_iter()
        .filter(|(_, count)| *count >= SNOOZE_TIMES)
        .collect())
}

fn store_digest(parts: &[Value]) -> String {
    let mut bytes = Vec::new();
    for part in parts {
        let canon = careerbrew_setup::value::to_canon(part);
        let text = careerbrew_vault::json_yaml::json_compact_python(&canon).expect("digest JSON");
        bytes.extend(text.as_bytes());
        bytes.push(0);
    }
    digest(&String::from_utf8_lossy(&bytes))[..24].to_owned()
}

fn nid(key: &str) -> String {
    format!("n-{}", &store_digest(&[json!("notify"), json!(key)])[..10])
}

fn unique_json(values: impl IntoIterator<Item = Value>) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.to_string()))
        .collect()
}

fn optional(row: &mut Value, key: &str, value: Value) {
    let present = match &value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
        _ => true,
    };
    if present {
        row[key] = value;
    } else if let Some(object) = row.as_object_mut() {
        object.remove(key);
    }
}

fn merge(
    state: &mut Value,
    candidates: &[Value],
    now_: DateTime<FixedOffset>,
    settings: &Value,
) -> Result<usize> {
    let mut by_key = state["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| (text(field(row, "key")), row.clone()))
        .collect::<BTreeMap<_, _>>();
    let live = candidates
        .iter()
        .map(|candidate| text(field(candidate, "key")))
        .collect::<BTreeSet<_>>();
    let mut grouped = BTreeMap::<String, Vec<Value>>::new();
    for candidate in candidates {
        grouped
            .entry(text(field(candidate, "key")))
            .or_default()
            .push(candidate.clone());
    }
    for row in state["items"].as_array_mut().into_iter().flatten() {
        let key = text(field(row, "key"));
        let current = grouped.get(&key).cloned().unwrap_or_default();
        let quiet_refs = current
            .iter()
            .filter(|candidate| truthy(field(candidate, "project_quiet")))
            .map(|candidate| text(field(candidate, "ref")))
            .collect::<BTreeSet<_>>();
        let ready_refs = current
            .iter()
            .filter(|candidate| !truthy(field(candidate, "project_quiet")))
            .map(|candidate| text(field(candidate, "ref")))
            .collect::<BTreeSet<_>>();
        let held = row["held_refs"].as_array().cloned().unwrap_or_default();
        let released = held
            .iter()
            .filter(|ref_| ready_refs.contains(&text(Some(ref_))))
            .cloned()
            .collect::<Vec<_>>();
        let mut held = held
            .into_iter()
            .filter(|ref_| quiet_refs.contains(&text(Some(ref_))))
            .collect::<Vec<_>>();
        if row.get("pending_refs").is_none()
            && row.get("held_refs").is_none()
            && row.get("snoozed_until_by_ref").is_none()
            && text(field(row, "status")) == "new"
            && PENDING.contains(&field(row, "popup").and_then(Value::as_str))
        {
            optional(
                row,
                "pending_refs",
                json!(row["refs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|ref_| quiet_refs
                        .union(&ready_refs)
                        .collect::<BTreeSet<_>>()
                        .contains(&text(Some(ref_))))
                    .cloned()
                    .collect::<Vec<_>>()),
            );
        }
        if text(field(row, "status")) == "new" {
            let mut pending = row["pending_refs"].as_array().cloned().unwrap_or_default();
            let snoozed = row["snoozed_until_by_ref"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            let due = snoozed
                .iter()
                .filter(|(_, until)| dt(&text(Some(until))) <= now_)
                .map(|(ref_, _)| ref_.clone())
                .collect::<BTreeSet<_>>();
            held.extend(
                due.iter()
                    .filter(|ref_| quiet_refs.contains(*ref_))
                    .map(|ref_| json!(ref_)),
            );
            held = unique_json(held);
            held = held
                .into_iter()
                .rev()
                .take(50)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            pending.extend(
                due.iter()
                    .filter(|ref_| ready_refs.contains(*ref_))
                    .map(|ref_| json!(ref_)),
            );
            optional(
                row,
                "snoozed_until_by_ref",
                json!(snoozed
                    .into_iter()
                    .filter(|(ref_, _)| !due.contains(ref_)
                        && quiet_refs
                            .union(&ready_refs)
                            .collect::<BTreeSet<_>>()
                            .contains(ref_))
                    .collect::<Map<_, _>>()),
            );
            if due.iter().any(|ref_| ready_refs.contains(ref_)) {
                row["popup"] = Value::Null;
            }
            let newly_held = pending
                .iter()
                .filter(|ref_| quiet_refs.contains(&text(Some(ref_))))
                .cloned()
                .collect::<Vec<_>>();
            held.extend(newly_held);
            held = unique_json(held);
            let mut pending = pending
                .into_iter()
                .filter(|ref_| ready_refs.contains(&text(Some(ref_))))
                .collect::<Vec<_>>();
            pending.extend(released.clone());
            optional(row, "pending_refs", json!(unique_json(pending)));
            if !released.is_empty() {
                row["popup"] = Value::Null;
            }
        }
        optional(row, "held_refs", json!(held));
    }
    let stamp_ = iso(now_);
    let mut created = 0;
    for candidate in candidates {
        let key = text(field(candidate, "key"));
        let Some(_existing) = by_key.get(&key).cloned() else {
            if candidate["priority"].as_i64().unwrap_or_default()
                < settings["min_priority"].as_i64().unwrap_or_default()
                || !truthy(field(candidate, "fresh"))
            {
                continue;
            }
            let mut row = json!({"id": nid(&key), "key": key, "item": text(field(candidate, "item")), "refs": [text(field(candidate, "ref"))], "count": 1, "at": field(candidate, "at").cloned().unwrap_or(Value::Null)});
            for text_key in TEXT {
                row[text_key] = field(candidate, text_key).cloned().unwrap_or(Value::Null);
            }
            if truthy(field(candidate, "project_quiet")) {
                row["held_refs"] = json!([text(field(candidate, "ref"))]);
            } else {
                row["pending_refs"] = json!([text(field(candidate, "ref"))]);
            }
            for (key, value) in [
                ("status", json!("new")),
                ("popup", Value::Null),
                ("created_at", json!(stamp_.clone())),
                ("updated_at", json!(stamp_.clone())),
                ("acted_at", json!("")),
                ("snooze_until", json!("")),
                ("resolved", json!("")),
            ] {
                row[key] = value;
            }
            state["items"]
                .as_array_mut()
                .expect("items")
                .push(row.clone());
            by_key.insert(key, row);
            created += 1;
            continue;
        };
        let index = state["items"]
            .as_array()
            .into_iter()
            .flatten()
            .position(|row| text(field(row, "key")) == key)
            .expect("existing row");
        let row = &mut state["items"][index];
        if !truthy(field(candidate, "project_quiet"))
            || !grouped[&key]
                .iter()
                .any(|other| !truthy(field(other, "project_quiet")))
        {
            for text_key in TEXT {
                row[text_key] = field(candidate, text_key).cloned().unwrap_or(Value::Null);
            }
        }
        if row["refs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|ref_| text(Some(ref_)) == text(field(candidate, "ref")))
        {
            continue;
        }
        let refs = row["refs"]
            .as_array()
            .into_iter()
            .flatten()
            .cloned()
            .chain([field(candidate, "ref").cloned().unwrap_or(Value::Null)])
            .collect::<Vec<_>>();
        row["refs"] = json!(refs
            .into_iter()
            .rev()
            .take(50)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>());
        if truthy(field(candidate, "project_quiet")) {
            let held = row["held_refs"]
                .as_array()
                .into_iter()
                .flatten()
                .cloned()
                .chain([field(candidate, "ref").cloned().unwrap_or(Value::Null)])
                .collect::<Vec<_>>();
            row["held_refs"] = json!(held
                .into_iter()
                .rev()
                .take(50)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>());
        } else if text(field(row, "status")) == "new"
            && (row.get("held_refs").is_some() || row.get("snoozed_until_by_ref").is_some())
        {
            let pending = row["pending_refs"]
                .as_array()
                .into_iter()
                .flatten()
                .cloned()
                .chain([field(candidate, "ref").cloned().unwrap_or(Value::Null)])
                .collect::<Vec<_>>();
            row["pending_refs"] = json!(pending
                .into_iter()
                .rev()
                .take(50)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>());
            row["popup"] = Value::Null;
        }
        row["count"] = json!(row["count"].as_i64().unwrap_or_default() + 1);
        row["updated_at"] = json!(stamp_.clone());
        if dt(&text(field(candidate, "at"))) > dt(&text(field(row, "at"))) {
            row["at"] = field(candidate, "at").cloned().unwrap_or(Value::Null);
        }
        let handled = !["new", "snoozed"].contains(&text(field(row, "status")).as_str());
        if handled
            && truthy(field(candidate, "fresh"))
            && candidate["priority"].as_i64().unwrap_or_default()
                >= settings["min_priority"].as_i64().unwrap_or_default()
            && dt(&text(field(candidate, "at")))
                > dt(&text(field(row, "acted_at")).pipe_empty(&text(field(row, "created_at"))))
        {
            row["status"] = json!("new");
            row["popup"] = Value::Null;
            row["resolved"] = json!("");
            if !truthy(field(candidate, "project_quiet")) {
                let pending = row["pending_refs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .cloned()
                    .chain([field(candidate, "ref").cloned().unwrap_or(Value::Null)])
                    .collect::<Vec<_>>();
                row["pending_refs"] = json!(pending
                    .into_iter()
                    .rev()
                    .take(50)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>());
            }
        }
    }
    for row in state["items"].as_array_mut().into_iter().flatten() {
        if OPEN.contains(&text(field(row, "status")).as_str())
            && !live.contains(&text(field(row, "key")))
        {
            row["status"] = json!("done");
            row["resolved"] = json!(if text(field(row, "key")).starts_with("feed:") {
                "closed"
            } else {
                "decided"
            });
            row["updated_at"] = json!(stamp_.clone());
            row["acted_at"] = json!(stamp_.clone());
        } else if text(field(row, "status")) == "snoozed"
            && !text(field(row, "snooze_until")).is_empty()
            && dt(&text(field(row, "snooze_until"))) <= now_
        {
            row["status"] = json!("new");
            row["popup"] = Value::Null;
            row["snooze_until"] = json!("");
        }
    }
    Ok(created)
}

trait PipeEmpty {
    fn pipe_empty(self, default: &str) -> String;
}
impl PipeEmpty for String {
    fn pipe_empty(self, default: &str) -> String {
        if self.is_empty() {
            default.into()
        } else {
            self
        }
    }
}

fn count(rows: &[Value]) -> String {
    let mut counts = BTreeMap::new();
    for row in rows {
        *counts.entry(text(field(row, "kind"))).or_insert(0) += 1;
    }
    let mut order = counts.keys().cloned().collect::<Vec<_>>();
    order.sort_by_key(|kind| {
        (
            std::cmp::Reverse(
                rows.iter()
                    .filter(|row| text(field(row, "kind")) == *kind)
                    .map(|row| row["priority"].as_i64().unwrap_or_default())
                    .max()
                    .unwrap_or_default(),
            ),
            kind.clone(),
        )
    });
    order
        .into_iter()
        .map(|kind| {
            let count = counts[&kind];
            format!(
                "{count} {}",
                if count == 1 {
                    labels(&kind).0
                } else {
                    labels(&kind).1
                }
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn actions(row: &Value) -> Vec<String> {
    if text(field(row, "kind")) != "suggestion" {
        return vec!["Open".into(), "Snooze".into()];
    }
    if text(field(row, "sub")) == "goal" {
        vec!["Open".into(), "Dismiss".into(), "Snooze".into()]
    } else {
        vec!["Accept".into(), "Dismiss".into(), "Snooze".into()]
    }
}

fn compose(root: &Path, rows: &[Value], now_: DateTime<FixedOffset>, quiet: bool) -> Result<Value> {
    let mut rows = rows.to_vec();
    rows.sort_by_key(|row| {
        (
            std::cmp::Reverse(row["priority"].as_i64().unwrap_or_default()),
            text(field(row, "at")),
        )
    });
    let top = &rows[0];
    let (subtitle, message, actions, target) = if rows.len() == 1 {
        (
            text(field(top, "headline")),
            format!(
                "Why: {}. Next: {}.",
                text(field(top, "why")),
                text(field(top, "next"))
            ),
            actions(top),
            text(field(top, "id")),
        )
    } else {
        let lead = if quiet {
            "While you were away".into()
        } else {
            format!("{} new for you", rows.len())
        };
        (
            format!("{lead}: {}", count(&rows)),
            format!(
                "Most urgent: {}. Next: open careerbrew to act on each one.",
                text(field(top, "headline"))
            ),
            vec!["Open".into(), "Snooze".into()],
            String::new(),
        )
    };
    let ids = rows
        .iter()
        .map(|row| text(field(row, "id")))
        .collect::<Vec<_>>();
    let subtitle = crate::redact(root, &subtitle)?;
    let message = crate::redact(root, &message)?;
    Ok(json!({
        "id": format!("p-{}", &store_digest(&[json!(iso(now_)), json!(ids)])[..10]),
        "at": iso(now_),
        "ids": ids,
        "target": target,
        "quiet": quiet,
        "subtitle": chars(&subtitle, 120),
        "message": chars(&message, 240),
        "actions": actions,
    }))
}

fn decide(
    root: &Path,
    state: &mut Value,
    now_: DateTime<FixedOffset>,
    settings: &Value,
    kind: &str,
) -> Result<(String, Option<Value>, usize)> {
    let waiting = state["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| {
            text(field(row, "status")) == "new"
                && PENDING.contains(&field(row, "popup").and_then(Value::as_str))
        })
        .cloned()
        .collect::<Vec<_>>();
    for row in state["items"].as_array_mut().into_iter().flatten() {
        if row.get("pending_refs").is_none()
            && (row.get("held_refs").is_some() || row.get("snoozed_until_by_ref").is_some())
            && text(field(row, "status")) == "new"
            && PENDING.contains(&field(row, "popup").and_then(Value::as_str))
        {
            row["popup"] = json!(if row.get("held_refs").is_some() {
                "held:project"
            } else {
                "held:snooze"
            });
        }
    }
    let pending = waiting
        .into_iter()
        .filter(|row| {
            row.get("pending_refs").is_some()
                || (row.get("held_refs").is_none() && row.get("snoozed_until_by_ref").is_none())
        })
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return Ok((String::new(), None, 0));
    }
    let today = state["popups"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|popup| {
            dt(&text(field(popup, "at")))
                .with_timezone(&now_.timezone())
                .date_naive()
                == now_.date_naive()
        })
        .count();
    let last = state["popups"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|popup| dt(&text(field(popup, "at"))))
        .max();
    let (reason, mark) = if kind == "none" {
        ("off", "off")
    } else if in_quiet(now_, &text(field(settings, "quiet_hours"))) {
        ("quiet", "held:quiet")
    } else if today >= settings["daily_cap"].as_i64().unwrap_or_default() as usize {
        ("cap", "capped")
    } else if last.is_some_and(|last| {
        now_ - last < TimeDelta::minutes(settings["batch_minutes"].as_i64().unwrap_or_default())
    }) {
        ("batch", "held:batch")
    } else {
        ("", "")
    };
    if !reason.is_empty() {
        for row in state["items"].as_array_mut().into_iter().flatten() {
            if pending
                .iter()
                .any(|pending| text(field(pending, "id")) == text(field(row, "id")))
            {
                row["popup"] = json!(mark);
            }
        }
        return Ok((reason.into(), None, pending.len()));
    }
    let quiet = pending
        .iter()
        .any(|row| text(field(row, "popup")) == "held:quiet");
    let mut popup = compose(root, &pending, now_, quiet)?;
    popup["refs"] = json!(pending
        .iter()
        .map(|row| (
            text(field(row, "id")),
            row.get("pending_refs")
                .cloned()
                .unwrap_or_else(|| row["refs"].clone())
        ))
        .collect::<Map<_, _>>());
    popup["observed_refs"] = json!(pending
        .iter()
        .map(|row| (text(field(row, "id")), row["refs"].clone()))
        .collect::<Map<_, _>>());
    popup["launch_state"] = json!("reserved");
    for row in state["items"].as_array_mut().into_iter().flatten() {
        if pending
            .iter()
            .any(|pending| text(field(pending, "id")) == text(field(row, "id")))
        {
            row["popup"] = json!("sent");
            row["popup_id"] = popup["id"].clone();
            row["delivered_at"] = popup["at"].clone();
            row.as_object_mut()
                .expect("notification")
                .remove("pending_refs");
        }
    }
    state["popups"]
        .as_array_mut()
        .expect("popups")
        .push(popup.clone());
    let popups = state["popups"].as_array().cloned().unwrap_or_default();
    state["popups"] = json!(popups
        .into_iter()
        .rev()
        .take(100)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>());
    Ok(("sent".into(), Some(popup), pending.len()))
}

pub fn backend(root: &Path) -> Result<(String, String, String)> {
    if std::env::var("CAREERBREW_NOTIFY").unwrap_or_else(|_| "on".into()) == "off" {
        return Ok(("none".into(), String::new(), "off (CAREERBREW_NOTIFY=off); every notification is in the web app's Notifications view".into()));
    }
    let want = setting_text(root, "notify_backend", "auto")?;
    let choices = if want == "auto" {
        vec!["alerter", "terminal-notifier"]
    } else if want == "osascript" {
        Vec::new()
    } else {
        vec![want.as_str()]
    };
    for kind in choices {
        if let Some(exe) = which(kind) {
            let note = if kind == "alerter" {
                "buttons Open, Accept, Dismiss, and Snooze"
            } else {
                "a click opens the notification in the web app (buttons need alerter: brew install vjeantet/tap/alerter)"
            };
            return Ok((kind.into(), exe, format!("{kind}: {note}")));
        }
    }
    if matches!(want.as_str(), "alerter" | "terminal-notifier") {
        let fix = if want == "alerter" {
            "brew install vjeantet/tap/alerter"
        } else {
            "brew install terminal-notifier"
        };
        return Ok(("none".into(), String::new(), format!("none: {want} is not installed (fix: {fix}, or careerbrew config set notify_backend auto)")));
    }
    if (cfg!(target_os = "macos")
        || std::env::var("CAREERBREW_RUST14_PLATFORM").as_deref() == Ok("darwin"))
        && which("osascript").is_some()
    {
        return Ok(("osascript".into(), "osascript".into(), "osascript: plain pop-ups that do not open careerbrew (for clickable ones: brew install terminal-notifier, or alerter for buttons)".into()));
    }
    Ok(("none".into(), String::new(), "none: no notifier on this system; every notification is in the web app's Notifications view".into()))
}

fn which(program: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

pub fn process(root: &Path, now_: Option<DateTime<FixedOffset>>) -> Result<(Value, Option<Value>)> {
    let now_ = now_.unwrap_or_else(now);
    let settings = settings(root)?;
    let (kind, _, _) = backend(root)?;
    let mut stats = json!({"enabled": settings["enabled"].clone(), "new": 0, "open": 0, "reason": "", "batched": 0, "backend": kind});
    if !truthy(field(&settings, "enabled")) {
        return Ok((stats, None));
    }
    let candidates = candidates(root, Some(now_))?;
    locked(root, || {
        let mut state = load(root)?;
        stats["new"] = json!(merge(&mut state, &candidates, now_, &settings)?);
        let (reason, popup, batched) = decide(root, &mut state, now_, &settings, &kind)?;
        stats["reason"] = json!(reason);
        stats["batched"] = json!(batched);
        state["items"] = json!(state["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| OPEN.contains(&text(field(row, "status")).as_str())
                || now_ - dt(&text(field(row, "at"))) <= TimeDelta::days(14))
            .cloned()
            .collect::<Vec<_>>());
        stats["open"] = json!(state["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| OPEN.contains(&text(field(row, "status")).as_str()))
            .count());
        save(root, &state)?;
        Ok((stats, popup))
    })
}

pub fn run(root: &Path, wait: bool, now_: Option<DateTime<FixedOffset>>) -> Result<Value> {
    let (mut stats, popup) = process(root, now_)?;
    let launched = match &popup {
        Some(popup) => deliver(root, popup, wait)?,
        None => false,
    };
    if popup.is_some() && !launched {
        stats["reason"] = json!("not_sent");
    } else if popup.is_some() && launched && text(field(&stats, "backend")) == "alerter" && !wait {
        stats["reason"] = json!("scheduled");
    }
    emit(
        root,
        "notify.check",
        vec![
            (
                "status",
                Canon::Str(if truthy(field(&stats, "enabled")) {
                    text(field(&stats, "reason")).pipe_empty("ok")
                } else {
                    "off".into()
                }),
            ),
            ("source_kind", Canon::Str(text(field(&stats, "backend")))),
            (
                "new",
                Canon::Int(stats["new"].as_i64().unwrap_or_default().into()),
            ),
            ("sent", Canon::Int(i64::from(launched).into())),
            (
                "batched",
                Canon::Int(if launched {
                    stats["batched"].as_i64().unwrap_or_default().into()
                } else {
                    0.into()
                }),
            ),
            (
                "held",
                Canon::Int(
                    if matches!(text(field(&stats, "reason")).as_str(), "quiet" | "batch") {
                        stats["batched"].as_i64().unwrap_or_default().into()
                    } else {
                        0.into()
                    },
                ),
            ),
            (
                "capped",
                Canon::Int(if text(field(&stats, "reason")) == "cap" {
                    stats["batched"].as_i64().unwrap_or_default().into()
                } else {
                    0.into()
                }),
            ),
        ],
    )?;
    Ok(stats)
}

fn prepare_launch(root: &Path, popup_id: &str, claim: &str) -> Result<Option<Value>> {
    let current = candidates(root, None)?
        .into_iter()
        .map(|candidate| {
            (
                (
                    text(field(&candidate, "key")),
                    text(field(&candidate, "ref")),
                ),
                candidate,
            )
        })
        .collect::<BTreeMap<_, _>>();
    locked(root, || {
        let mut state = load(root)?;
        let Some(mut popup) = state["popups"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|popup| text(field(popup, "id")) == popup_id)
            .cloned()
        else {
            return Ok(None);
        };
        if text(field(&popup, "launch_state")).pipe_empty("launched") != "reserved" {
            return Ok(None);
        }
        let rows = state["items"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| (text(field(row, "id")), row.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut ready = Vec::new();
        let mut selected = Map::new();
        for ident in popup["ids"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|value| text(Some(value)))
        {
            let Some(mut row) = rows.get(&ident).cloned() else {
                continue;
            };
            if text(field(&row, "status")) != "new" {
                continue;
            }
            let refs = popup["refs"]
                .as_object()
                .and_then(|refs| refs.get(&ident))
                .cloned()
                .unwrap_or_else(|| row["refs"].clone());
            let live = refs
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|ref_| {
                    current
                        .get(&(text(field(&row, "key")), text(Some(ref_))))
                        .cloned()
                })
                .collect::<Vec<_>>();
            let held = live
                .iter()
                .filter(|candidate| truthy(field(candidate, "project_quiet")))
                .map(|candidate| text(field(candidate, "ref")))
                .collect::<Vec<_>>();
            let eligible = live
                .iter()
                .filter(|candidate| !truthy(field(candidate, "project_quiet")))
                .collect::<Vec<_>>();
            let old_held = row["held_refs"].as_array().cloned().unwrap_or_default();
            optional(
                &mut row,
                "held_refs",
                json!(unique_json(
                    old_held
                        .into_iter()
                        .chain(held.iter().map(|ref_| json!(ref_)))
                )
                .into_iter()
                .rev()
                .take(50)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()),
            );
            if !eligible.is_empty() {
                for key in TEXT {
                    row[key] = field(eligible.last().expect("eligible"), key)
                        .cloned()
                        .unwrap_or(Value::Null);
                }
                selected.insert(
                    ident.clone(),
                    json!(eligible
                        .iter()
                        .map(|candidate| text(field(candidate, "ref")))
                        .collect::<Vec<_>>()),
                );
                ready.push(row.clone());
            } else if !held.is_empty() {
                row["popup"] = json!("held:project");
                row["popup_id"] = json!("");
            } else if live.is_empty() {
                let observed = popup["observed_refs"]
                    .as_object()
                    .and_then(|refs| refs.get(&ident))
                    .cloned()
                    .unwrap_or(refs);
                let other = current
                    .iter()
                    .filter(|((key, ref_), _)| {
                        *key == text(field(&row, "key"))
                            && !observed
                                .as_array()
                                .into_iter()
                                .flatten()
                                .any(|seen| text(Some(seen)) == *ref_)
                    })
                    .map(|(_, candidate)| candidate)
                    .collect::<Vec<_>>();
                let remaining = current
                    .iter()
                    .filter(|((key, _), _)| *key == text(field(&row, "key")))
                    .map(|(_, candidate)| candidate)
                    .collect::<Vec<_>>();
                if !other.is_empty() {
                    for candidate in &other {
                        if !row["refs"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|ref_| text(Some(ref_)) == text(field(candidate, "ref")))
                        {
                            let refs = row["refs"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .cloned()
                                .chain([field(candidate, "ref").cloned().unwrap_or(Value::Null)])
                                .collect::<Vec<_>>();
                            row["refs"] = json!(refs
                                .into_iter()
                                .rev()
                                .take(50)
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .collect::<Vec<_>>());
                            row["count"] = json!(row["count"].as_i64().unwrap_or_default() + 1);
                        }
                    }
                    let held_values = unique_json(
                        row["held_refs"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .cloned()
                            .chain(
                                other
                                    .iter()
                                    .filter(|candidate| truthy(field(candidate, "project_quiet")))
                                    .map(|candidate| {
                                        field(candidate, "ref").cloned().unwrap_or(Value::Null)
                                    }),
                            ),
                    )
                    .into_iter()
                    .rev()
                    .take(50)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>();
                    optional(&mut row, "held_refs", json!(held_values));
                    let pending_values = unique_json(
                        row["pending_refs"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .cloned()
                            .chain(
                                other
                                    .iter()
                                    .filter(|candidate| !truthy(field(candidate, "project_quiet")))
                                    .map(|candidate| {
                                        field(candidate, "ref").cloned().unwrap_or(Value::Null)
                                    }),
                            ),
                    )
                    .into_iter()
                    .rev()
                    .take(50)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>();
                    optional(&mut row, "pending_refs", json!(pending_values));
                    row["popup"] = json!(if row.get("pending_refs").is_some() {
                        Value::Null
                    } else {
                        json!("held:project")
                    });
                    row["popup_id"] = json!("");
                } else if !remaining.is_empty() {
                    let quiet_refs =
                        remaining
                            .iter()
                            .filter(|candidate| {
                                truthy(field(candidate, "project_quiet"))
                                    && row["held_refs"].as_array().into_iter().flatten().any(
                                        |ref_| text(Some(ref_)) == text(field(candidate, "ref")),
                                    )
                            })
                            .count();
                    row["popup"] = json!(if quiet_refs > 0 {
                        "held:project"
                    } else {
                        "sent"
                    });
                    row["popup_id"] = json!("");
                } else {
                    row["status"] = json!("done");
                    row["resolved"] = json!(if text(field(&row, "key")).starts_with("feed:") {
                        "closed"
                    } else {
                        "decided"
                    });
                    row["popup_id"] = json!("");
                }
            }
            if let Some(index) = state["items"]
                .as_array()
                .into_iter()
                .flatten()
                .position(|item| text(field(item, "id")) == ident)
            {
                state["items"][index] = row;
            }
        }
        if ready.is_empty() {
            state["popups"] = json!(state["popups"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|popup| text(field(popup, "id")) != popup_id)
                .cloned()
                .collect::<Vec<_>>());
            save(root, &state)?;
            return Ok(None);
        }
        let mut fresh = compose(root, &ready, now(), truthy(field(&popup, "quiet")))?;
        fresh["id"] = json!(popup_id);
        fresh["refs"] = json!(selected);
        fresh["observed_refs"] = field(&popup, "observed_refs")
            .cloned()
            .unwrap_or_else(|| json!({}));
        fresh["launch_state"] = json!("launching");
        fresh["launch_claim"] = json!(claim);
        popup = fresh;
        state["popups"] = json!(state["popups"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|old| if text(field(old, "id")) == popup_id {
                popup.clone()
            } else {
                old.to_owned()
            })
            .collect::<Vec<_>>());
        save(root, &state)?;
        Ok(Some(popup))
    })
}

fn rollback_launch(root: &Path, popup_id: &str, claim: &str) -> Result<()> {
    locked(root, || {
        let mut state = load(root)?;
        let Some(popup) = state["popups"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|popup| text(field(popup, "id")) == popup_id)
            .cloned()
        else {
            return Ok(());
        };
        let launch_state = text(field(&popup, "launch_state")).pipe_empty("launched");
        if launch_state != "reserved"
            && !(launch_state == "launching"
                && !claim.is_empty()
                && text(field(&popup, "launch_claim")) == claim)
        {
            return Ok(());
        }
        for row in state["items"].as_array_mut().into_iter().flatten() {
            if text(field(row, "popup_id")) == popup_id && text(field(row, "status")) == "new" {
                let refs = popup["refs"]
                    .as_object()
                    .and_then(|refs| refs.get(&text(field(row, "id"))))
                    .cloned()
                    .unwrap_or_else(|| row["refs"].clone());
                row["popup"] = Value::Null;
                row["popup_id"] = json!("");
                row["pending_refs"] = refs;
            }
        }
        state["popups"] = json!(state["popups"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|popup| text(field(popup, "id")) != popup_id)
            .cloned()
            .collect::<Vec<_>>());
        save(root, &state)
    })
}

fn mark_launched(root: &Path, popup_id: &str, claim: &str) -> Result<()> {
    locked(root, || {
        let mut state = load(root)?;
        for popup in state["popups"].as_array_mut().into_iter().flatten() {
            if text(field(popup, "id")) == popup_id
                && text(field(popup, "launch_state")) == "launching"
                && text(field(popup, "launch_claim")) == claim
            {
                popup["launch_state"] = json!("launched");
                popup.as_object_mut().expect("popup").remove("launch_claim");
            }
        }
        save(root, &state)
    })
}

fn output_with_timeout(mut child: Child, seconds: u64) -> Result<Output> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        if let Some(_status) = child.try_wait()? {
            return child.wait_with_output().map_err(Into::into);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(crate::error(
                "TimeoutExpired",
                "notifier exceeded its bound",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn deliver(root: &Path, popup: &Value, wait: bool) -> Result<bool> {
    let (kind, exe, _) = backend(root)?;
    let popup_id = text(field(popup, "id"));
    if kind == "alerter" && !wait {
        let current = std::env::current_exe()?;
        Command::new(current)
            .arg("present")
            .arg(&popup_id)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        return Ok(true);
    }
    let claim = uuid::Uuid::new_v4().simple().to_string();
    let current = careerbrew_vault::lock::with_lock(&root.join("work/projects.yaml"), || {
        prepare_launch(root, &popup_id, &claim)
    })??;
    let Some(current) = current else {
        return Ok(false);
    };
    let args = notifier_args(&current, &exe);
    let child = match Command::new(&exe)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_error) => {
            let _ = careerbrew_vault::lock::with_lock(&root.join("work/projects.yaml"), || {
                rollback_launch(root, &popup_id, &claim)
            });
            emit(
                root,
                "notify.error",
                vec![
                    ("source_kind", Canon::Str(kind)),
                    ("error", Canon::Str("OSError".into())),
                    ("story", Canon::Str("us-62".into())),
                ],
            )?;
            return Ok(false);
        }
    };
    careerbrew_vault::lock::with_lock(&root.join("work/projects.yaml"), || {
        mark_launched(root, &popup_id, &claim)
    })??;
    let seconds = if kind == "alerter" { 3630 } else { 10 };
    let output = match output_with_timeout(child, seconds) {
        Ok(output) => output,
        Err(error) => {
            emit(
                root,
                "notify.error",
                vec![
                    ("source_kind", Canon::Str(kind)),
                    ("error", Canon::Str(error.class.into())),
                    ("story", Canon::Str("us-62".into())),
                ],
            )?;
            return Ok(false);
        }
    };
    if !output.status.success() {
        emit(
            root,
            "notify.error",
            vec![
                ("source_kind", Canon::Str(kind)),
                ("error", Canon::Str("ExitStatus".into())),
                ("story", Canon::Str("us-62".into())),
            ],
        )?;
        return Ok(false);
    }
    if kind == "alerter" {
        let text_ = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let _ = apply_click(root, &current, &text_, &kind)?;
    }
    Ok(true)
}

fn notifier_args(popup: &Value, exe: &str) -> Vec<String> {
    let kind = Path::new(exe)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(exe);
    if kind == "osascript" {
        return vec![
            "-e".into(),
            "on run argv".into(),
            "-e".into(),
            "display notification (item 1 of argv) with title \"careerbrew\"".into(),
            "-e".into(),
            "end run".into(),
            format!(
                "{}. {}",
                text(field(popup, "subtitle")),
                text(field(popup, "message"))
            ),
        ];
    }
    let mut args = vec![
        "-title".into(),
        "careerbrew".into(),
        "-subtitle".into(),
        text(field(popup, "subtitle")),
        "-message".into(),
        text(field(popup, "message")),
    ];
    if kind == "alerter" {
        args.extend([
            "-actions".into(),
            popup["actions"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|value| text(Some(value)))
                .collect::<Vec<_>>()
                .join(","),
            "-closeLabel".into(),
            "Close".into(),
        ]);
    }
    args.extend([
        "-group".into(),
        format!(
            "careerbrew-{}",
            text(field(popup, "target")).pipe_empty("list")
        ),
    ]);
    if kind == "alerter" {
        args.extend(["-timeout".into(), "3600".into()]);
    } else if kind == "terminal-notifier" {
        args.extend([
            "-execute".into(),
            click_command(&text(field(popup, "target"))),
        ]);
    }
    args
}

fn click_command(nid: &str) -> String {
    let executable = std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "live-component".into());
    let mut parts = vec!["env".to_owned()];
    for key in ["CAREERBREW_HOME", "CAREERBREW_KEY_FILE"] {
        if let Ok(value) = std::env::var(key) {
            parts.push(format!("{key}={value}"));
        }
    }
    parts.extend([executable, "notify".into(), "open".into()]);
    if !nid.is_empty() {
        parts.push(nid.into());
    }
    parts
        .into_iter()
        .map(|part| format!("'{part}'"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn present(root: &Path, popup_id: &str) -> Result<String> {
    let (kind, _, _) = backend(root)?;
    if kind != "alerter" {
        rollback_launch(root, popup_id, "")?;
        return Ok(String::new());
    }
    let popup = json!({"id": popup_id});
    deliver(root, &popup, true)?;
    Ok(String::new())
}

fn apply_click(root: &Path, popup: &Value, output: &str, kind: &str) -> Result<String> {
    let action = match output.lines().last().unwrap_or("").trim() {
        "@CONTENTCLICKED" | "Open" => "open",
        "Accept" => "accept",
        "Dismiss" => "dismiss",
        "Snooze" => "snooze",
        _ => "",
    };
    let target = text(field(popup, "target"));
    if action.is_empty() || (target.is_empty() && !["open", "snooze"].contains(&action)) {
        return Ok(String::new());
    }
    let result = (|| -> Result<String> {
        if !target.is_empty() {
            let refs = popup["refs"]
                .as_object()
                .and_then(|refs| refs.get(&target))
                .and_then(Value::as_array)
                .map(|refs| {
                    refs.iter()
                        .map(|value| text(Some(value)))
                        .collect::<Vec<_>>()
                });
            act(
                root,
                &target,
                if action == "open" { "read" } else { action },
                "popup",
                refs.as_deref(),
            )?;
        } else if action == "snooze" {
            for nid in popup["ids"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|value| text(Some(value)))
            {
                let refs = popup["refs"]
                    .as_object()
                    .and_then(|refs| refs.get(&nid))
                    .and_then(Value::as_array)
                    .map(|refs| {
                        refs.iter()
                            .map(|value| text(Some(value)))
                            .collect::<Vec<_>>()
                    });
                let _ = act(root, &nid, "snooze", "popup", refs.as_deref());
            }
        } else {
            emit(
                root,
                "notify.act",
                vec![
                    ("status", Canon::Str("open".into())),
                    ("source_kind", Canon::Str("popup".into())),
                ],
            )?;
        }
        Ok(action.to_owned())
    })();
    match result {
        Ok(action) => {
            // Browser opening is outside the synthetic component boundary; the
            // notification action and its content-free receipt are complete.
            Ok(action)
        }
        Err(error) => {
            emit(
                root,
                "notify.error",
                vec![
                    ("source_kind", Canon::Str(kind.into())),
                    ("error", Canon::Str(error.class.into())),
                    ("story", Canon::Str("us-62".into())),
                ],
            )?;
            Ok(String::new())
        }
    }
}

pub fn act(
    root: &Path,
    nid: &str,
    action: &str,
    via: &str,
    selected_refs: Option<&[String]>,
) -> Result<Value> {
    if !ACTIONS.contains(&action) {
        return Err(notify_error(format!(
            "unknown action {action}; use one of {}",
            ACTIONS.join(", ")
        )));
    }
    let now_ = now();
    let row =
        locked(root, || {
            let mut state = load(root)?;
            let Some(index) = state["items"]
                .as_array()
                .into_iter()
                .flatten()
                .position(|row| text(field(row, "id")) == nid)
            else {
                return Err(notify_error(format!(
                    "no notification {nid} (see: careerbrew notify list)"
                )));
            };
            let mut row = state["items"][index].clone();
            if action != "read" && !OPEN.contains(&text(field(&row, "status")).as_str()) {
                return Err(notify_error(format!(
                    "{nid} is already {}",
                    text(field(&row, "status"))
                )));
            }
            if action == "accept" && text(field(&row, "kind")) != "suggestion" {
                return Err(notify_error(format!(
                    "{nid} is not a suggestion: open it, snooze it, or dismiss it"
                )));
            }
            if text(field(&row, "kind")) == "suggestion" && ["accept", "dismiss"].contains(&action)
            {
                let item = text(field(&row, "item"));
                if !inbox_load(root)?.into_iter().any(|item_| {
                    text(field(&item_, "id")) == item && text(field(&item_, "status")) == "open"
                }) {
                    row["status"] = json!("done");
                    row["resolved"] = json!("decided");
                    row["acted_at"] = json!(iso(now_));
                    row["updated_at"] = json!(iso(now_));
                    state["items"][index] = row;
                    save(root, &state)?;
                    return Err(notify_error(format!(
                        "{nid}: the suggestion is already decided in the inbox; nothing changed"
                    )));
                }
                if action == "accept"
                    && text(field(&row, "kind")) == "suggestion"
                    && text(field(&row, "sub")) == "goal"
                    && via == "ui"
                {
                    inbox_decide_ui(root, &item).map_err(|error| notify_error(error.message))?;
                } else {
                    inbox_decide(root, &item, action, false)
                        .map_err(|error| notify_error(error.message))?;
                }
            }
            let deferred_refs = row["held_refs"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|value| text(Some(value)))
                .chain(
                    row["snoozed_until_by_ref"]
                        .as_object()
                        .into_iter()
                        .flat_map(|refs| refs.keys())
                        .cloned(),
                )
                .collect::<BTreeSet<_>>();
            let partial = selected_refs.is_some_and(|selected| {
                !deferred_refs.is_empty()
                    && !selected.iter().any(|ref_| deferred_refs.contains(ref_))
            });
            let deferred_popup = if row.get("held_refs").is_some() {
                "held:project"
            } else {
                "held:snooze"
            };
            match action {
                "read" => {
                    if OPEN.contains(&text(field(&row, "status")).as_str()) && partial {
                        row["status"] = json!("new");
                        row["popup"] = json!(deferred_popup);
                        row["popup_id"] = json!("");
                    } else if OPEN.contains(&text(field(&row, "status")).as_str()) {
                        row["status"] = json!("read");
                    }
                }
                "snooze" => {
                    if partial {
                        let mut snoozed = row["snoozed_until_by_ref"]
                            .as_object()
                            .cloned()
                            .unwrap_or_default();
                        for ref_ in selected_refs.unwrap_or(&[]) {
                            snoozed.insert(
                                ref_.clone(),
                                json!(iso(now_
                                    + TimeDelta::minutes(setting_i64(
                                        root,
                                        "notify_snooze_minutes",
                                        60
                                    )?))),
                            );
                        }
                        row["status"] = json!("new");
                        row["popup"] = json!(deferred_popup);
                        row["popup_id"] = json!("");
                        row["snoozed_until_by_ref"] = json!(snoozed);
                    } else {
                        row["status"] = json!("snoozed");
                        row["snooze_until"] = json!(iso(now_
                            + TimeDelta::minutes(setting_i64(root, "notify_snooze_minutes", 60)?)));
                        row["popup"] = Value::Null;
                    }
                    let snoozes = row["snoozes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .cloned()
                        .chain([json!(iso(now_))])
                        .collect::<Vec<_>>();
                    row["snoozes"] = json!(snoozes
                        .into_iter()
                        .rev()
                        .take(10)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>());
                }
                _ => {
                    row["status"] = json!(if action == "accept" {
                        "done"
                    } else {
                        "dismissed"
                    });
                    row["resolved"] = json!(if action == "accept" {
                        "accepted"
                    } else {
                        "dismissed"
                    });
                }
            }
            row["acted_at"] = json!(iso(now_));
            row["updated_at"] = json!(iso(now_));
            state["items"][index] = row.clone();
            save(root, &state)?;
            Ok(row)
        })?;
    emit(
        root,
        "notify.act",
        vec![
            ("status", Canon::Str(action.into())),
            ("source_kind", Canon::Str(via.into())),
        ],
    )?;
    Ok(row)
}

pub fn status(root: &Path) -> Result<Value> {
    let settings = settings(root)?;
    let (kind, _, note) = backend(root)?;
    let now_ = now();
    let state = load(root)?;
    Ok(json!({
        "enabled": settings["enabled"].clone(),
        "min_priority": settings["min_priority"].clone(),
        "quiet_hours": settings["quiet_hours"].clone(),
        "daily_cap": settings["daily_cap"].clone(),
        "batch_minutes": settings["batch_minutes"].clone(),
        "snooze_minutes": settings["snooze_minutes"].clone(),
        "backend": settings["backend"].clone(),
        "backend_kind": kind,
        "backend_note": note,
        "quiet_now": in_quiet(now_, &text(field(&settings, "quiet_hours"))),
        "sent_today": state["popups"].as_array().into_iter().flatten().filter(|popup| dt(&text(field(popup, "at"))).with_timezone(&now_.timezone()).date_naive() == now_.date_naive()).count(),
        "open": state["items"].as_array().into_iter().flatten().filter(|row| OPEN.contains(&text(field(row, "status")).as_str())).count(),
        "unread": state["items"].as_array().into_iter().flatten().filter(|row| text(field(row, "status")) == "new").count(),
    }))
}

pub fn view(root: &Path) -> Result<Value> {
    let every = feed::load(root)?;
    let feed_rows = every
        .iter()
        .map(|row| (text(field(row, "id")), row))
        .collect::<BTreeMap<_, _>>();
    let entries = careerbrew_projects::project_priority::entries(root)?;
    let live_feed = feed::open_asks(root, Some(&every), false)?
        .iter()
        .map(|row| ask_key(row, &entries))
        .collect::<BTreeSet<_>>();
    let items = inbox_load(root)?
        .into_iter()
        .map(|item| (text(field(&item, "id")), item))
        .collect::<BTreeMap<_, _>>();
    let notification_state = load(root)?;
    let unconfirmed = notification_state["popups"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|popup| {
            matches!(
                text(field(popup, "launch_state")).as_str(),
                "reserved" | "launching"
            )
        })
        .map(|popup| text(field(popup, "id")))
        .collect::<BTreeSet<_>>();
    let mut rows = Vec::new();
    for original in notification_state["items"].as_array().into_iter().flatten() {
        let mut row = original.clone();
        let is_open = OPEN.contains(&text(field(&row, "status")).as_str());
        if is_open
            && (if text(field(&row, "kind")) == "suggestion" {
                items
                    .get(&text(field(&row, "item")))
                    .is_none_or(|item| text(field(item, "status")) != "open")
            } else {
                !live_feed.contains(&text(field(&row, "key")))
            })
        {
            row["status"] = json!("done");
            row["resolved"] = json!(if text(field(&row, "kind")) == "suggestion" {
                "decided"
            } else {
                "closed"
            });
        }
        let is_open = OPEN.contains(&text(field(&row, "status")).as_str());
        let (title, url, link_label, accept_label, actions) =
            if text(field(&row, "kind")) == "suggestion" {
                let item = items.get(&text(field(&row, "item")));
                let title = item
                    .map(|item| text(field(item, "text")))
                    .unwrap_or_default();
                let title = if item.is_some_and(|item| text(field(item, "kind")) == "plugin") {
                    format!(
                        "{title} (suggested by plugin {})",
                        item.map(|item| text(
                            field(item, "data").and_then(|data| field(data, "plugin"))
                        ))
                        .unwrap_or_default()
                    )
                } else {
                    title
                };
                let accept = if item.is_some_and(|item| text(field(item, "kind")) == "goal") {
                    "Approve goal"
                } else {
                    "Accept"
                };
                let actions =
                    (if is_open && item.is_some_and(|item| text(field(item, "status")) == "open") {
                        vec!["accept"]
                    } else {
                        Vec::new()
                    })
                    .into_iter()
                    .chain(if is_open {
                        vec!["snooze", "dismiss"]
                    } else {
                        Vec::new()
                    })
                    .collect::<Vec<_>>();
                (
                    title,
                    "#/today".into(),
                    "See it in Today".into(),
                    accept.into(),
                    actions,
                )
            } else {
                let empty = json!({});
                let feed_row = row["refs"]
                    .as_array()
                    .and_then(|refs| refs.last())
                    .and_then(|ref_| feed_rows.get(&text(Some(ref_))))
                    .map_or(&empty, |value| value);
                if truthy(field(feed_row, "personal")) {
                    continue;
                }
                let actions = if is_open {
                    vec!["snooze", "dismiss"]
                } else {
                    Vec::new()
                };
                (
                    text(field(feed_row, "title")),
                    text(field(feed_row, "url")),
                    format!("Open in {}", source_name(&text(field(&row, "source")))),
                    String::new(),
                    actions,
                )
            };
        let mut out = json!({
            "id": field(&row, "id").cloned().unwrap_or(Value::Null),
            "kind": field(&row, "kind").cloned().unwrap_or(Value::Null),
            "source": field(&row, "source").cloned().unwrap_or(Value::Null),
            "headline": field(&row, "headline").cloned().unwrap_or(Value::Null),
            "why": field(&row, "why").cloned().unwrap_or(Value::Null),
            "next": field(&row, "next").cloned().unwrap_or(Value::Null),
            "priority": field(&row, "priority").cloned().unwrap_or(Value::Null),
            "priority_why": field(&row, "priority_why").cloned().unwrap_or(Value::Null),
            "count": field(&row, "count").cloned().unwrap_or(Value::Null),
            "at": field(&row, "at").cloned().unwrap_or(Value::Null),
            "status": field(&row, "status").cloned().unwrap_or(Value::Null),
            "snooze_until": field(&row, "snooze_until").cloned().unwrap_or(Value::Null),
            "resolved": field(&row, "resolved").cloned().unwrap_or(Value::Null),
            "title": title,
            "url": url,
            "link_label": link_label,
            "accept_label": accept_label,
            "actions": actions,
        });
        let popup_state = text(field(&row, "popup"));
        out["popup"] = json!(if unconfirmed.contains(&text(field(&row, "popup_id"))) {
            String::from("delivery not confirmed")
        } else {
            match popup_state.as_str() {
                "sent" => "shown as a pop-up".into(),
                "held:quiet" => "held for quiet hours: it pops up after them".into(),
                "held:batch" => "held to arrive with the next pop-up".into(),
                "held:project" => "held until the project's quiet period ends".into(),
                "held:snooze" => "held until the snoozed ask is due".into(),
                "capped" => "no pop-up: the daily cap was reached".into(),
                "off" => "no pop-up: pop-ups are off here".into(),
                _ => "waiting for the next check".into(),
            }
        });
        rows.push(out);
    }
    rows.sort_by_key(|row| std::cmp::Reverse(dt(&text(field(row, "at")))));
    rows.sort_by_key(|row| {
        (
            !OPEN.contains(&text(field(row, "status")).as_str()),
            text(field(row, "status")) != "new",
            std::cmp::Reverse(row["priority"].as_i64().unwrap_or_default()),
        )
    });
    Ok(json!({"items": rows, "status": status(root)?}))
}

pub fn inbox_load(root: &Path) -> Result<Vec<Value>> {
    Ok(read_json(root, "day/inbox.json", json!({}))?["items"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

fn inbox_locked<T>(root: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let path = root.join("day/inbox.lock");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    careerbrew_vault::lock::with_exact_lock(&path, f)?
}

pub fn inbox_offer(root: &Path, task: &Value) -> Result<Value> {
    inbox_locked(root, || {
        let id = format!(
            "agent-task-{}",
            &store_digest(&[json!("agent-task"), json!(text(field(task, "id")))])[..8]
        );
        let title = one_line(&text(field(task, "title")));
        let source = text(field(task, "source")).pipe_empty("agent");
        let why = one_line(&text(field(task, "why"))).pipe_empty("no reason given");
        let item = json!({
            "id": id,
            "type": "proposal",
            "kind": "agent-task",
            "text": format!("Plan {}: {} (added by {}: {})", text(field(task, "id")), title, source, why),
            "why": format!("{} added it: {}", if source == "agent" { "an agent".into() } else { source.clone() }, why),
            "data": {"task": text(field(task, "id")), "title": title, "source": source, "why": why},
        });
        let mut items = inbox_load(root)?;
        if let Some(old) = items
            .iter()
            .find(|old| text(field(old, "id")) == text(field(&item, "id")))
        {
            if text(field(old, "status")) != "open" {
                return Ok(json!({"id": item["id"].clone(), "status": "decided"}));
            }
            let created = field(old, "created_at")
                .cloned()
                .unwrap_or_else(|| json!(stamp()));
            items.retain(|old| text(field(old, "id")) != text(field(&item, "id")));
            let mut item = item;
            item["status"] = json!("open");
            item["created_at"] = created;
            items.push(item.clone());
            write_json_if_changed(root, "day/inbox.json", &json!({"items": items}))?;
            return Ok(json!({"id": item["id"].clone(), "status": "updated"}));
        }
        let mut item = item;
        item["status"] = json!("open");
        item["created_at"] = json!(stamp());
        items.push(item.clone());
        write_json_if_changed(root, "day/inbox.json", &json!({"items": items}))?;
        Ok(json!({"id": item["id"].clone(), "status": "new"}))
    })
}

pub fn task_record(
    root: &Path,
    title: &str,
    source: &str,
    why: &str,
    status: &str,
    links: &[String],
) -> Result<Value> {
    let mut tasks = careerbrew_projects::priorities::load(root)?;
    let id = format!("t{:03}", tasks.len() + 1);
    let task = json!({
        "id": id,
        "title": title,
        "impact": 3,
        "urgency": 3,
        "estimate_h": 1.0,
        "competencies": [],
        "due": Value::Null,
        "status": status,
        "source": source,
        "added_at": stamp(),
        "why": why,
        "links": careerbrew_projects::priorities::links(&json!({"links": links})),
    });
    tasks.push(task.clone());
    careerbrew_projects::priorities::save(root, &tasks)?;
    Ok(task)
}

fn proposal_task(
    root: &Path,
    title: &str,
    source: &str,
    why: &str,
    links: &[String],
) -> Result<Value> {
    task_record(root, title, source, why, "open", links)
}

fn set_task_status(root: &Path, task_id: &str, status: &str) -> Result<()> {
    let mut tasks = careerbrew_projects::priorities::load(root)?;
    for task in &mut tasks {
        if text(field(task, "id")) == task_id {
            task["status"] = json!(status);
        }
    }
    Ok(careerbrew_projects::priorities::save(root, &tasks)?)
}

fn inbox_decide(root: &Path, item_id: &str, action: &str, goal_ui: bool) -> Result<()> {
    inbox_locked(root, || {
        let mut items = inbox_load(root)?;
        let Some(index) = items
            .iter()
            .position(|item| text(field(item, "id")) == item_id)
        else {
            return Err(crate::error("InboxError", "unknown inbox item"));
        };
        if text(field(&items[index], "status")) != "open" {
            return Err(crate::error(
                "InboxError",
                format!(
                    "{item_id} is already {}",
                    text(field(&items[index], "status"))
                ),
            ));
        }
        if text(field(&items[index], "type")) != "proposal" {
            return Err(crate::error("InboxError", format!("{item_id} is a question: answer it with `careerbrew day answer {item_id} \"...\"`")));
        }
        let kind = text(field(&items[index], "kind"));
        let data = field(&items[index], "data")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if action == "accept" {
            match kind.as_str() {
                "goal" => {
                    if !goal_ui {
                        return Err(notify_error("goal suggestions are approved in the web app only (careerbrew, then the Today view)"));
                    }
                }
                "task-done" => {
                    let task_id = text(field(&data, "task"));
                    let current = careerbrew_projects::priorities::load(root)?
                        .into_iter()
                        .find(|task| text(field(task, "id")) == task_id)
                        .unwrap_or_else(|| json!({}));
                    let estimate = careerbrew_projects::priorities::estimate(&current);
                    careerbrew_projects::priorities::done(root, &task_id, json!(estimate))?;
                }
                "new-task" => {
                    proposal_task(
                        root,
                        &text(field(&data, "title")),
                        "proposal",
                        "a ticket assigned to you",
                        &[],
                    )?;
                }
                "agent-task" => set_task_status(root, &text(field(&data, "task")), "open")?,
                "source" => {
                    let project = text(field(&data, "project"));
                    let source_kind = text(field(&data, "kind"));
                    let reference = text(field(&data, "ref"));
                    let exists = projects(root)?
                        .get(&project)
                        .map(|project| {
                            project["sources"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .any(|source| {
                                    text(field(source, "kind")) == source_kind
                                        && text(field(source, "ref")) == reference
                                })
                        })
                        .unwrap_or(false);
                    if !exists {
                        careerbrew_projects::projects::add_source(
                            root,
                            &project,
                            &source_kind,
                            &reference,
                        )?;
                    }
                }
                "feed-rule" => {
                    let rule = field(&data, "rule").cloned().unwrap_or_else(|| json!({}));
                    let existing = feed::rules(root)?
                        .into_iter()
                        .filter(|rule| !truthy(field(rule, "builtin")))
                        .collect::<Vec<_>>();
                    let mut rules = vec![rule];
                    rules.extend(existing);
                    crate::write_yaml(root, "work/feed-rules.yaml", &json!({"rules": rules}))?;
                    feed::retriage(root)?;
                }
                "plugin" => {
                    let task = text(field(&data, "task"));
                    if !task.is_empty() {
                        let why = one_line(&format!(
                            "plugin {}: {}",
                            text(field(&data, "plugin")),
                            text(field(&items[index], "why"))
                        ));
                        let links = if text(field(&data, "link")).is_empty() {
                            Vec::new()
                        } else {
                            vec![text(field(&data, "link"))]
                        };
                        proposal_task(root, &task, "proposal", &why, &links)?;
                    }
                }
                _ => {}
            }
        } else if kind == "agent-task" {
            set_task_status(root, &text(field(&data, "task")), "dismissed")?;
        }
        items[index]["status"] = json!(if action == "accept" {
            "accepted"
        } else {
            "dismissed"
        });
        items[index]["decided_at"] = json!(crate::stamp());
        write_json_if_changed(root, "day/inbox.json", &json!({"items": items}))?;
        if action == "accept" {
            emit(
                root,
                "day.inbox.accept",
                vec![
                    ("status", Canon::Str("ok".into())),
                    ("kind", Canon::Str(kind)),
                ],
            )?;
        }
        Ok(())
    })
}

pub fn goals_load(root: &Path) -> Result<Value> {
    let data = read_yaml(root, "goals.yaml", json!({}))?;
    let queue = field(&data, "queue")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| {
            field(&data, "upcoming")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        });
    let today = now().date_naive().format("%Y-%m-%d").to_string();
    let current = queue
        .iter()
        .filter(|goal| !text(field(goal, "date")).is_empty() && text(field(goal, "date")) >= today)
        .min_by_key(|goal| text(field(goal, "date")))
        .or_else(|| {
            queue
                .iter()
                .find(|goal| text(field(goal, "date")).is_empty())
        })
        .cloned()
        .unwrap_or(Value::Null);
    Ok(json!({
        "long_term": field(&data, "long_term").cloned().unwrap_or_else(|| json!([])),
        "queue": queue,
        "upcoming": current,
        "history": field(&data, "history").cloned().unwrap_or_else(|| json!([])),
    }))
}

fn clean_goal(data: &Value) -> Value {
    json!({
        "id": text(field(data, "id")),
        "kind": text(field(data, "kind")).pipe_empty("custom"),
        "title": one_line(&text(field(data, "title"))),
        "date": field(data, "date").cloned().unwrap_or(Value::Null),
        "company": field(data, "company").cloned().unwrap_or(Value::Null),
        "topics": field(data, "topics").cloned().unwrap_or_else(|| json!([])),
        "minutes_per_day": field(data, "minutes_per_day").and_then(Value::as_i64).unwrap_or(90),
        "set_at": field(data, "set_at").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(crate::utc_stamp),
    })
}

fn goal_plan_blocks(day: &str) -> Vec<Value> {
    vec![
        json!({"carry_over": false, "end": format!("{day}T09:45+00:00"), "goal": true, "reason": "your goal", "start": format!("{day}T09:00+00:00"), "task": "goal:exercise", "title": "Coding: 2 exercise(s) in arrays and hashing"}),
        json!({"carry_over": false, "end": format!("{day}T10:10+00:00"), "goal": true, "reason": "your goal", "start": format!("{day}T09:50+00:00"), "task": "goal:cards", "title": "Flashcards: due reviews, then new cards"}),
        json!({"carry_over": false, "end": format!("{day}T10:35+00:00"), "goal": true, "reason": "your goal", "start": format!("{day}T10:15+00:00"), "task": "goal:lesson", "title": "Lesson: learn and confirm one topic"}),
    ]
}

#[derive(Clone)]
struct PlanBlock {
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
}

impl PlanBlock {
    fn minutes(&self) -> i64 {
        (self.end - self.start).num_minutes()
    }
}

fn plan_time(value: &str) -> Option<DateTime<FixedOffset>> {
    parse_time(value).or_else(|| DateTime::parse_from_str(value, "%Y-%m-%dT%H:%M%:z").ok())
}

fn plan_at(day: &str, clock: &str) -> Result<DateTime<FixedOffset>> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map_err(|_| crate::error("ValueError", "invalid goal schedule day"))?;
    let (hour, minute) = clock
        .split_once(':')
        .and_then(|(hour, minute)| Some((hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?)))
        .ok_or_else(|| crate::error("ValueError", "invalid goal schedule clock"))?;
    let naive = date
        .and_hms_opt(hour, minute, 0)
        .ok_or_else(|| crate::error("ValueError", "invalid goal schedule clock"))?;
    careerbrew_feed::local_zone()
        .from_local_datetime(&naive)
        .earliest()
        .map(|at| at.fixed_offset())
        .ok_or_else(|| crate::error("ValueError", "invalid local goal schedule clock"))
}

fn plan_free_blocks(
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
    busy: &[PlanBlock],
) -> Vec<PlanBlock> {
    let mut blocks = Vec::new();
    let mut current = start;
    let mut ordered = busy.to_vec();
    ordered.sort_by_key(|block| block.start);
    for event in ordered {
        if event.end <= current || event.start >= end {
            continue;
        }
        if event.start > current {
            blocks.push(PlanBlock {
                start: current,
                end: event.start.min(end),
            });
        }
        current = current.max(event.end);
    }
    if current < end {
        blocks.push(PlanBlock {
            start: current,
            end,
        });
    }
    blocks
        .into_iter()
        .filter(|block| block.minutes() >= 1)
        .collect()
}

fn place_plan_block(blocks: &mut Vec<PlanBlock>, minutes: i64) -> Option<PlanBlock> {
    let slot = blocks.iter_mut().find(|block| block.minutes() >= minutes)?;
    let placed = PlanBlock {
        start: slot.start,
        end: slot.start + TimeDelta::minutes(minutes),
    };
    slot.start = placed.end + TimeDelta::minutes(5);
    blocks.retain(|block| block.minutes() >= 25);
    Some(placed)
}

fn scheduled_plan_row(row: &Value, placed: &PlanBlock, carry_over: bool) -> Value {
    let mut answer = json!({
        "carry_over": carry_over,
        "end": placed.end.format("%Y-%m-%dT%H:%M%:z").to_string(),
        "reason": text(field(row, "reason")),
        "start": placed.start.format("%Y-%m-%dT%H:%M%:z").to_string(),
        "task": text(field(row, "task")),
        "title": text(field(row, "title")),
    });
    if truthy(field(row, "goal")) {
        answer["goal"] = json!(true);
    }
    answer
}

fn replan_goal(root: &Path, day: &str) -> Result<()> {
    let path = format!("day/plan-{day}.json");
    let existing = read_json(root, &path, json!({}))?;
    let existing_empty = existing.as_object().is_none_or(Map::is_empty);
    let day_date = NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map_err(|_| crate::error("ValueError", "invalid goal schedule day"))?;
    let start = plan_at(day, &setting_text(root, "work_start", "09:00")?)?;
    let end = plan_at(day, &setting_text(root, "work_end", "17:30")?)?;
    let meetings = existing["meetings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|meeting| {
            Ok(PlanBlock {
                start: plan_time(&text(field(meeting, "start")))
                    .ok_or_else(|| crate::error("ValueError", "invalid meeting start"))?,
                end: plan_time(&text(field(meeting, "end")))
                    .ok_or_else(|| crate::error("ValueError", "invalid meeting end"))?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let free_minutes = plan_free_blocks(start, end, &meetings)
        .iter()
        .map(PlanBlock::minutes)
        .sum::<i64>();
    let mut remaining = plan_free_blocks(start, end, &meetings);
    let mut budget = free_minutes * 4 / 5;
    let mut schedule = Vec::new();
    let mut unplaced = Vec::new();
    for goal in goal_plan_blocks(day) {
        let minutes = (plan_time(&text(field(&goal, "end")))
            .ok_or_else(|| crate::error("ValueError", "invalid goal block end"))?
            - plan_time(&text(field(&goal, "start")))
                .ok_or_else(|| crate::error("ValueError", "invalid goal block start"))?)
        .num_minutes();
        let placed = if minutes <= budget {
            place_plan_block(&mut remaining, minutes)
        } else {
            None
        };
        if let Some(placed) = placed {
            budget -= minutes;
            schedule.push(scheduled_plan_row(&goal, &placed, false));
        } else {
            unplaced.push(text(field(&goal, "task")));
        }
    }
    let yesterday = day_date
        .pred_opt()
        .ok_or_else(|| crate::error("ValueError", "invalid goal schedule carry-over"))?;
    let previous = read_json(
        root,
        &format!("day/plan-{}.json", yesterday.format("%Y-%m-%d")),
        json!({}),
    )?;
    let carry = previous["schedule"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| text(field(row, "task")))
        .collect::<BTreeSet<_>>();
    let mut ranked = careerbrew_projects::priorities::rank(root, day_date)?;
    ranked.sort_by_key(|task| !carry.contains(&text(field(task, "id"))));
    for mut task in ranked {
        let task_id = text(field(&task, "id"));
        let minutes = (careerbrew_projects::priorities::estimate(&task) * 60.0) as i64;
        let placed = if minutes <= budget {
            place_plan_block(&mut remaining, minutes)
        } else {
            None
        };
        if let Some(placed) = placed {
            budget -= minutes;
            let mut reasons = Vec::new();
            let due = text(field(&task, "due"));
            if !due.is_empty() {
                reasons.push(format!("due {due}"));
            }
            let project = text(field(&task["parts"], "project"));
            if !project.is_empty() {
                reasons.push(format!(
                    "project {project} (priority {} of {})",
                    text(field(&task["parts"], "project_rank")),
                    text(field(&task["parts"], "projects")),
                ));
            }
            reasons.push(format!("impact {}", text(field(&task, "impact"))));
            task["reason"] = json!(reasons.join("; "));
            task["task"] = json!(task_id.clone());
            schedule.push(scheduled_plan_row(&task, &placed, carry.contains(&task_id)));
        } else {
            unplaced.push(task_id);
        }
    }
    let mut plan = if existing_empty {
        json!({
            "calendar_connected": false,
            "calendar_errors": [],
            "calibration": 1.0,
            "day": day,
            "focus_minutes": free_minutes * 4 / 5,
            "free_minutes": free_minutes,
            "meetings": [],
        })
    } else {
        existing
    };
    plan["schedule"] = json!(schedule);
    plan["unplaced"] = json!(unplaced);
    if plan["day"].is_null() {
        plan["day"] = json!(day);
    }
    crate::write_json(root, &path, &plan)
}

pub fn inbox_decide_ui(root: &Path, item_id: &str) -> Result<()> {
    let kind = inbox_load(root)?
        .into_iter()
        .find(|item| text(field(item, "id")) == item_id)
        .map(|item| text(field(&item, "kind")))
        .unwrap_or_default();
    if kind == "goal" {
        let data = inbox_load(root)?
            .into_iter()
            .find(|item| text(field(item, "id")) == item_id)
            .and_then(|item| field(&item, "data").cloned())
            .unwrap_or_else(|| json!({}));
        let mut goals = read_yaml(root, "goals.yaml", json!({"queue": []}))?;
        goals["queue"]
            .as_array_mut()
            .expect("goal queue")
            .push(clean_goal(&data));
        crate::write_yaml(root, "goals.yaml", &goals)?;
        let day = now().date_naive().format("%Y-%m-%d").to_string();
        replan_goal(root, &day)?;
        std::fs::create_dir_all(root.join("tutor"))?;
        std::fs::create_dir_all(root.join("work"))?;
    }
    inbox_decide(root, item_id, "accept", true)
}

pub fn inbox_dismiss(root: &Path, item_id: &str) -> Result<()> {
    inbox_decide(root, item_id, "dismiss", false)
}
