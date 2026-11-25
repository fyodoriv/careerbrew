//! Feed facts, raw replies and original/analysis transactions.

use crate::{
    compact, emit, field, now, now_iso, now_utc, read_json, read_jsonl, read_yaml, text, truthy,
    write_json_if_changed, write_jsonl, ComponentProvider, FeedError, Result,
};
use careerbrew_vault::canon::Canon;
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, TimeDelta, Utc};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const TRIAGES: &[&str] = &["act", "fyi", "noise", "mine"];
pub const ACTORS: &[&str] = &["me", "other", "bot"];
pub const SET_KEYS: &[&str] = &["triage", "personal", "evidence"];
pub const FACT_KEYS: &[&str] = &[
    "id",
    "source",
    "kind",
    "at",
    "title",
    "actor",
    "actor_id",
    "to_me",
    "channel",
    "from",
    "status",
    "importance",
    "url",
    "thread",
    "tags",
    "project",
];
const WHEN_KEYS: &[&str] = &[
    "actor",
    "to_me",
    "team",
    "text",
    "source",
    "kind",
    "channel",
    "from",
    "status",
    "importance",
    "title",
    "origin",
];
const LINKS_MAX: usize = 20;
type IngestOutcome = (usize, usize, usize, Vec<(Value, String)>);

pub fn feed_dir(root: &Path) -> PathBuf {
    root.join("work/feed")
}

pub fn events_path(root: &Path) -> PathBuf {
    feed_dir(root).join("events.jsonl")
}

pub fn originals_path(root: &Path) -> PathBuf {
    feed_dir(root).join("originals.jsonl")
}

pub fn cycle_path(root: &Path) -> PathBuf {
    feed_dir(root).join("feed-cycle.json")
}

pub fn load(root: &Path) -> Result<Vec<Value>> {
    read_jsonl(root, "work/feed/events.jsonl")
}

pub fn originals(root: &Path) -> Result<BTreeMap<String, Value>> {
    let mut out = BTreeMap::new();
    for row in read_jsonl(root, "work/feed/originals.jsonl")? {
        out.insert(text(field(&row, "id")), row);
    }
    Ok(out)
}

fn valid_source(source: &str) -> bool {
    Regex::new(r"^[a-z0-9][a-z0-9._-]{0,39}$")
        .expect("source regex")
        .is_match(source)
}

pub fn source_name(value: &str) -> Result<String> {
    let source = value.trim().to_lowercase();
    if !valid_source(&source) {
        return Err(FeedError::feed(format!(
            "source {:?} must be a short lowercase name such as slack or jira",
            source.chars().take(40).collect::<String>()
        )));
    }
    Ok(source)
}

fn short(value: &Value, limit: usize) -> String {
    text(Some(value))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}

pub fn project_label(value: Option<&Value>) -> String {
    let value = match value {
        Some(Value::Object(_)) => field(value.unwrap_or(&Value::Null), "title")
            .or_else(|| field(value.unwrap_or(&Value::Null), "name")),
        value => value,
    };
    match value {
        Some(Value::String(value)) => value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(120)
            .collect(),
        _ => String::new(),
    }
}

pub fn utc(value: &Value) -> Result<String> {
    let raw = text(Some(value)).trim().replace("Z", "+00:00");
    let parsed = DateTime::parse_from_rfc3339(&raw)
        .ok()
        .map(|value| value.with_timezone(&Utc))
        .or_else(|| {
            NaiveDateTime::parse_from_str(&raw, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .and_then(|value| {
                    careerbrew_memory::memory_sync::local_naive(crate::local_zone(), value)
                })
                .map(|value| value.with_timezone(&Utc))
        })
        .or_else(|| {
            NaiveDate::parse_from_str(&raw, "%Y-%m-%d")
                .ok()
                .and_then(|value| value.and_hms_opt(0, 0, 0))
                .and_then(|value| {
                    careerbrew_memory::memory_sync::local_naive(crate::local_zone(), value)
                })
                .map(|value| value.with_timezone(&Utc))
        });
    parsed
        .map(|value| value.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .ok_or_else(|| {
            FeedError::feed(format!(
                "at {:?} is not an ISO time such as 2026-10-21T14:03:00Z",
                text(Some(value)).chars().take(40).collect::<String>()
            ))
        })
}

pub fn clean(item: &Value, where_: &str) -> Result<Value> {
    let Some(object) = item.as_object() else {
        return Err(FeedError::feed(format!(
            "{where_}: an event is a JSON object with id, at, title (and source, kind, actor)"
        )));
    };
    let missing = ["id", "at", "title"]
        .iter()
        .filter(|key| short(object.get(**key).unwrap_or(&Value::Null), 200).is_empty())
        .map(|key| (*key).to_owned())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(FeedError::feed(format!(
            "{where_}: missing {} (fix: every event needs id, at, and title)",
            missing.join(", ")
        )));
    }
    let source = text(object.get("source")).trim().to_lowercase();
    let source = if source.is_empty() {
        "plugin".to_owned()
    } else {
        source
    };
    if !valid_source(&source) {
        return Err(FeedError::feed(format!(
            "{where_}: source {:?} must be a short lowercase name such as slack or pagerduty",
            source.chars().take(40).collect::<String>()
        )));
    }
    let actor = text(object.get("actor"));
    let actor = if actor.is_empty() { "other" } else { &actor };
    if !ACTORS.contains(&actor) {
        return Err(FeedError::feed(format!(
            "{where_}: actor must be one of me, other, bot"
        )));
    }
    let tags = object.get("tags").cloned().unwrap_or_else(|| json!([]));
    let Some(tags) = tags.as_array() else {
        return Err(FeedError::feed(format!(
            "{where_}: tags is a list of words"
        )));
    };
    if tags.iter().any(|tag| !tag.is_string()) {
        return Err(FeedError::feed(format!(
            "{where_}: tags is a list of words"
        )));
    }
    let tags = tags
        .iter()
        .map(|tag| {
            tag.as_str()
                .unwrap_or_default()
                .chars()
                .take(40)
                .collect::<String>()
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let at = utc(object.get("at").unwrap_or(&Value::Null))?;
    Ok(json!({
        "id": short(object.get("id").unwrap_or(&Value::Null), 200),
        "source": source,
        "kind": short(object.get("kind").filter(|value| !text(Some(value)).is_empty()).unwrap_or(&json!("event")), 40).to_lowercase(),
        "at": at,
        "title": short(object.get("title").unwrap_or(&Value::Null), 200),
        "actor": actor,
        "actor_id": short(object.get("actor_id").unwrap_or(&Value::Null), 120),
        "to_me": truthy(object.get("to_me")),
        "url": text(object.get("url")).chars().take(500).collect::<String>(),
        "thread": short(object.get("thread").unwrap_or(&Value::Null), 200),
        "tags": tags,
        "personal": truthy(object.get("personal")),
        "text": text(object.get("text")).chars().take(20000).collect::<String>(),
        "original": object.get("original").cloned().unwrap_or_else(|| json!({})),
        "channel": short(object.get("channel").unwrap_or(&Value::Null), 120),
        "from": short(object.get("from").unwrap_or(&Value::Null), 120),
        "status": short(object.get("status").unwrap_or(&Value::Null), 120),
        "importance": short(object.get("importance").unwrap_or(&Value::Null), 120),
        "project": project_label(object.get("project")),
    }))
}

fn check_rule(raw: &Value, where_: &str) -> Result<Value> {
    let Some(rule) = raw.as_object() else {
        return Err(FeedError::feed(format!("{where_}: a rule is a mapping")));
    };
    let unknown = rule
        .keys()
        .filter(|key| !["name", "when", "triage", "personal", "evidence"].contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(FeedError::feed(format!("{where_}: unknown key(s)")));
    }
    let when = rule.get("when").cloned().unwrap_or_else(|| json!({}));
    let Some(when_map) = when.as_object() else {
        return Err(FeedError::feed(format!("{where_}: `when` is a mapping")));
    };
    for (key, value) in when_map {
        if !WHEN_KEYS.contains(&key.as_str()) {
            return Err(FeedError::feed(format!(
                "{where_}: unknown key {key} in when"
            )));
        }
        if matches!(key.as_str(), "to_me" | "team") && !value.is_boolean() {
            return Err(FeedError::feed(format!("{where_}: {key} is true or false")));
        }
    }
    if !SET_KEYS.iter().any(|key| rule.contains_key(*key)) {
        return Err(FeedError::feed(format!(
            "{where_}: set triage, personal or evidence"
        )));
    }
    if let Some(triage) = rule.get("triage").and_then(Value::as_str) {
        if !TRIAGES.contains(&triage) {
            return Err(FeedError::feed(format!(
                "{where_}: triage is one of act, fyi, noise, mine"
            )));
        }
    }
    Ok(raw.clone())
}

pub fn rules(root: &Path) -> Result<Vec<Value>> {
    let data = read_yaml(root, "work/feed-rules.yaml", json!({}))?;
    let own = field(&data, "rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for (index, raw) in own.iter().enumerate() {
        let mut rule = check_rule(raw, &format!("work/feed-rules.yaml rule {}", index + 1))?;
        let name = text(field(&rule, "name"));
        rule["name"] = json!(if name.is_empty() {
            format!("rule-{}", index + 1)
        } else {
            name
        });
        rule["builtin"] = json!(false);
        out.push(rule);
    }
    out.extend([
        json!({"name": "own-action", "when": {"actor": "me"}, "triage": "mine", "builtin": true}),
        json!({"name": "page-handled", "when": {"source": "pagerduty", "actor": "me", "status": ["acknowledged", "resolved"]}, "evidence": true, "builtin": true}),
        json!({"name": "bot", "when": {"actor": "bot"}, "triage": "noise", "builtin": true}),
        json!({"name": "page-for-me", "when": {"source": "pagerduty", "to_me": true, "status": ["triggered", "acknowledged"]}, "triage": "act", "builtin": true}),
        json!({"name": "important-mail", "when": {"kind": "mail", "importance": "high"}, "triage": "act", "builtin": true}),
        json!({"name": "flagged-mail", "when": {"kind": "mail", "status": "flagged"}, "triage": "act", "builtin": true}),
        json!({"name": "asked-me", "when": {"kind": ["dm", "mention"], "to_me": true}, "triage": "act", "builtin": true}),
        json!({"name": "review-for-me", "when": {"kind": ["review", "review-request", "review_request"], "to_me": true}, "triage": "act", "builtin": true}),
        json!({"name": "default", "when": {}, "triage": "fyi", "personal": false, "evidence": false, "builtin": true}),
    ]);
    Ok(out)
}

fn glob_matches(value: &str, pattern: &str) -> bool {
    glob::Pattern::new(pattern)
        .map(|pattern| pattern.matches(value))
        .unwrap_or(false)
}

fn matches(when: &Value, facts: &Value) -> bool {
    let Some(when) = when.as_object() else {
        return false;
    };
    for (key, want) in when {
        let have = field(facts, key).unwrap_or(&Value::Null);
        if let Some(want) = want.as_bool() {
            if truthy(Some(have)) != want {
                return false;
            }
            continue;
        }
        let patterns = if let Some(want) = want.as_str() {
            vec![want.to_owned()]
        } else {
            want.as_array()
                .map(|items| items.iter().map(|item| text(Some(item))).collect())
                .unwrap_or_default()
        };
        let value = text(Some(have)).to_lowercase();
        let matched = if key == "text" {
            patterns
                .iter()
                .any(|pattern| value.contains(&pattern.to_lowercase()))
        } else {
            patterns
                .iter()
                .any(|pattern| glob_matches(&value, &pattern.to_lowercase()))
        };
        if !matched {
            return false;
        }
    }
    true
}

pub fn links(value: &str) -> Vec<String> {
    let slack = Regex::new(r"(?i)<(https?://[^|>\s]+)(?:\|[^>]*)?>").expect("Slack link regex");
    let bare = Regex::new(r#"(?i)https?://[^\s<>|\"'`,*]+"#).expect("bare link regex");
    let mut found = slack
        .captures_iter(value)
        .filter_map(|capture| capture.get(1).map(|item| item.as_str().to_owned()))
        .collect::<Vec<_>>();
    let remainder = slack.replace_all(value, " ");
    found.extend(
        bare.find_iter(&remainder)
            .filter(|item| {
                remainder[..item.start()]
                    .chars()
                    .next_back()
                    .is_none_or(|previous| {
                        !matches!(previous, '<' | '|')
                            && !(previous.is_alphanumeric() || previous == '_')
                    })
            })
            .map(|item| item.as_str().to_owned()),
    );
    let mut out = Vec::new();
    for mut url in found {
        url = url
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&");
        url = url
            .trim_end_matches(['.', ',', ';', ':', '!', '?', ']', '}'])
            .to_owned();
        while url.ends_with(')') && url.matches(')').count() > url.matches('(').count() {
            url.pop();
            url = url
                .trim_end_matches(['.', ',', ';', ':', '!', '?'])
                .to_owned();
        }
        url = url.chars().take(500).collect();
        if !out.contains(&url) {
            out.push(url);
        }
    }
    out.truncate(LINKS_MAX);
    out
}

pub fn signal(item: &Value) -> String {
    if text(field(item, "actor")) != "me"
        || text(field(item, "channel")).is_empty()
        || matches!(
            text(field(item, "kind")).as_str(),
            "dm" | "mail" | "meeting"
        )
    {
        return String::new();
    }
    let pinned = field(item, "original")
        .and_then(|original| field(original, "pinned_to"))
        .is_some_and(|value| truthy(Some(value)));
    let launch = Regex::new(r"(?i)\b(?:launch(?:ed|ing)|announc(?:ing|ed|ement)|(?:now|is|went|are) live|rolled out|released)\b")
        .expect("launch regex")
        .is_match(&text(field(item, "text")));
    if pinned || launch {
        "launch".into()
    } else if !text(field(item, "thread")).is_empty() && field(item, "thread") != field(item, "id")
    {
        "check-in".into()
    } else {
        String::new()
    }
}

pub fn analyze(
    item: &Value,
    rules: &[Value],
    origin: &str,
    mark: Option<bool>,
    team_rows: &[Value],
) -> Value {
    let mut facts = Map::new();
    for key in FACT_KEYS {
        facts.insert(
            (*key).into(),
            field(item, key).cloned().unwrap_or(Value::Null),
        );
    }
    let original_text = text(field(item, "text"));
    let match_text = if text(field(item, "actor")) == "bot" {
        ""
    } else {
        &original_text
    };
    let mates = careerbrew_projects::team::matches(
        team_rows,
        &text(field(item, "actor_id")),
        &text(field(item, "from")),
        match_text,
    );
    let mut decided = BTreeMap::<String, Value>::new();
    let mut by = BTreeMap::<String, String>::new();
    let mut named_team = BTreeMap::<String, bool>::new();
    for rule in rules {
        let when = field(rule, "when").cloned().unwrap_or_else(|| json!({}));
        let mut inputs = facts.clone();
        inputs.insert("origin".into(), json!(origin));
        inputs.insert("text".into(), json!(original_text));
        inputs.insert("team".into(), json!(mates));
        if matches(&when, &Value::Object(inputs)) {
            for key in SET_KEYS {
                if let Some(value) = field(rule, key) {
                    if !decided.contains_key(*key) {
                        decided.insert((*key).into(), value.clone());
                        by.insert((*key).into(), text(field(rule, "name")));
                        named_team.insert((*key).into(), field(&when, "team").is_some());
                    }
                }
            }
        }
    }
    if !mates.is_empty()
        && decided.get("triage") == Some(&json!("noise"))
        && !named_team.get("triage").copied().unwrap_or(false)
    {
        decided.insert("triage".into(), json!("fyi"));
        by.insert("triage".into(), "teammate".into());
    }
    let (personal, personal_by) = if let Some(mark) = mark {
        (mark, "owner".to_owned())
    } else if truthy(field(item, "personal")) {
        (true, "source".to_owned())
    } else {
        (
            truthy(decided.get("personal")),
            if truthy(decided.get("personal")) {
                format!("rule:{}", by.get("personal").cloned().unwrap_or_default())
            } else {
                String::new()
            },
        )
    };
    let mut row = facts;
    row.insert("origin".into(), json!(origin));
    row.insert(
        "triage".into(),
        decided.get("triage").cloned().unwrap_or(Value::Null),
    );
    row.insert(
        "rule".into(),
        json!(by.get("triage").cloned().unwrap_or_default()),
    );
    row.insert("personal".into(), json!(personal));
    row.insert("personal_by".into(), json!(personal_by));
    row.insert("team".into(), json!(mates));
    row.insert("links".into(), json!(links(&original_text)));
    row.insert("signal".into(), json!(signal(item)));
    let evidence_rule = if truthy(decided.get("evidence")) {
        by.get("evidence").cloned().unwrap_or_default()
    } else {
        String::new()
    };
    row.insert("evidence_rule".into(), json!(evidence_rule));
    row.insert(
        "evidence".into(),
        json!(
            !text(row.get("evidence_rule")).is_empty()
                && row.get("triage") == Some(&json!("mine"))
                && !personal
        ),
    );
    if let Some(mark) = mark {
        row.insert("marked".into(), json!(mark));
    }
    Value::Object(row)
}

fn keep_old(row: &mut Value, old: Option<&Value>) {
    if let Some(old) = old {
        if let Some(research) = old.get("research").and_then(Value::as_array) {
            let links = row
                .get("links")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let kept = research
                .iter()
                .filter(|entry| links.contains(field(entry, "url").unwrap_or(&Value::Null)))
                .cloned()
                .collect::<Vec<_>>();
            if !kept.is_empty() {
                row["research"] = json!(kept);
            }
        }
        if old
            .get("ask_mark")
            .is_some_and(|value| !text(Some(value)).is_empty())
        {
            row["ask_mark"] = old["ask_mark"].clone();
            if old
                .get("ask_marked_at")
                .is_some_and(|value| !text(Some(value)).is_empty())
            {
                row["ask_marked_at"] = old["ask_marked_at"].clone();
            }
        }
    }
}

fn note_stored(
    root: &Path,
    sources: &BTreeSet<String>,
    ids: &[String],
    components: &dyn ComponentProvider,
) -> Result<()> {
    let mut batch = read_json(root, "work/feed/feed-cycle.json", json!({}))?;
    let declaration = components.declaration(root)?;
    if text(field(&batch, "identity")) != declaration.identity {
        batch = json!({});
    }
    let mut have = field(&batch, "stored")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| text(Some(value)))
        .collect::<BTreeSet<_>>();
    have.extend(sources.iter().cloned());
    let mut acquired = field(&batch, "acquired")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for source in sources {
        acquired.insert(source.clone(), json!(crate::now_utc_iso()));
    }
    let mut known_ids = field(&batch, "ids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| text(Some(value)))
        .collect::<BTreeSet<_>>();
    known_ids.extend(ids.iter().cloned());
    let failed = field(&batch, "failed_sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| text(Some(value)))
        .filter(|source| !sources.contains(source))
        .collect::<BTreeSet<_>>();
    batch["identity"] = json!(declaration.identity);
    batch["stored"] = json!(have.into_iter().collect::<Vec<_>>());
    batch["acquired"] = Value::Object(acquired);
    batch["ids"] = json!(known_ids.into_iter().collect::<Vec<_>>());
    batch["failed_sources"] = json!(failed.into_iter().collect::<Vec<_>>());
    write_json_if_changed(root, "work/feed/feed-cycle.json", &batch)?;
    Ok(())
}

fn save(
    root: &Path,
    rows: &BTreeMap<String, Value>,
    originals: &BTreeMap<String, Value>,
) -> Result<()> {
    let mut rows = rows.values().cloned().collect::<Vec<_>>();
    rows.sort_by_key(|row| (text(field(row, "at")), text(field(row, "id"))));
    write_jsonl(root, "work/feed/events.jsonl", &rows)?;
    let mut originals = originals.values().cloned().collect::<Vec<_>>();
    originals.sort_by_key(|row| (text(field(row, "at")), text(field(row, "id"))));
    write_jsonl(root, "work/feed/originals.jsonl", &originals)
}

pub fn ingest(
    root: &Path,
    items: &[Value],
    origin: &str,
    source_kind: &str,
    sources: &[String],
    components: &dyn ComponentProvider,
) -> Result<Value> {
    let rule_list = rules(root)?;
    let team_rows = careerbrew_projects::team::load(root)?;
    let cleaned = items
        .iter()
        .enumerate()
        .map(|(index, item)| clean(item, &format!("event {}", index + 1)))
        .collect::<Result<Vec<_>>>()?;
    let mut stored_now = cleaned
        .iter()
        .map(|item| text(field(item, "source")))
        .collect::<BTreeSet<_>>();
    for source in sources {
        stored_now.insert(source_name(source)?);
    }
    let ids = cleaned
        .iter()
        .map(|item| text(field(item, "id")))
        .collect::<Vec<_>>();
    let (new, updated, pruned, fresh) = careerbrew_vault::lock::with_exact_lock(
        &feed_dir(root).join(".lock"),
        || -> Result<IngestOutcome> {
            if !stored_now.is_empty() {
                note_stored(root, &stored_now, &ids, components)?;
            }
            let mut rows = load(root)?
                .into_iter()
                .map(|row| (text(field(&row, "id")), row))
                .collect::<BTreeMap<_, _>>();
            let mut originals = originals(root)?;
            let mut new = 0usize;
            let mut updated = 0usize;
            let mut fresh = Vec::new();
            for item in &cleaned {
                let id = text(field(item, "id"));
                let old = rows.get(&id).cloned();
                let mark = old
                    .as_ref()
                    .and_then(|row| row.get("marked"))
                    .and_then(Value::as_bool);
                let mut row = analyze(item, &rule_list, origin, mark, &team_rows);
                keep_old(&mut row, old.as_ref());
                let original = json!({"id": id, "origin": origin, "at": text(field(item, "at")), "text": text(field(item, "text")), "original": field(item, "original").cloned().unwrap_or_else(|| json!({}))});
                if old.as_ref() == Some(&row) && originals.get(&id) == Some(&original) {
                    continue;
                }
                if old.is_none() {
                    fresh.push((row.clone(), text(field(item, "text"))));
                    new += 1;
                } else {
                    updated += 1;
                }
                rows.insert(id.clone(), row);
                originals.insert(id, original);
            }
            let keep_days = field(
                &read_yaml(root, "settings.yaml", json!({}))?,
                "feed_keep_days",
            )
            .and_then(Value::as_i64)
            .unwrap_or(30);
            let cut = (now_utc() - TimeDelta::days(keep_days))
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string();
            let old = originals
                .keys()
                .filter(|id| {
                    originals
                        .get(*id)
                        .is_some_and(|row| text(field(row, "at")) < cut)
                })
                .cloned()
                .collect::<Vec<_>>();
            for id in &old {
                originals.remove(id);
            }
            let pruned = old.len();
            if new > 0 || updated > 0 || pruned > 0 {
                save(root, &rows, &originals)?;
            }
            Ok((new, updated, pruned, fresh))
        },
    )??;
    emit(
        root,
        "work.feed.ingest",
        vec![
            ("status", Canon::Str("ok".into())),
            ("source_kind", Canon::Str(source_kind.into())),
            ("new", Canon::Int(new.into())),
            ("updated", Canon::Int(updated.into())),
            ("pruned", Canon::Int(pruned.into())),
        ],
    )?;
    components.track_fresh(&fresh)?;
    Ok(json!({"new": new, "updated": updated, "pruned": pruned}))
}

fn nonempty(value: &Value) -> bool {
    !value.is_null()
        && !matches!(value, Value::String(value) if value.is_empty())
        && !matches!(value, Value::Array(value) if value.is_empty())
        && !matches!(value, Value::Object(value) if value.is_empty())
}

fn first_field(object: &Value, keys: &[&str]) -> Value {
    let fields = field(object, "fields").and_then(Value::as_object);
    for key in keys {
        for value in [object.get(*key), fields.and_then(|fields| fields.get(*key))]
            .into_iter()
            .flatten()
        {
            if nonempty(value) {
                return value.clone();
            }
        }
    }
    Value::Null
}

fn name(value: &Value) -> String {
    match value {
        Value::Object(_) => {
            let inner = field(value, "emailAddress")
                .filter(|value| value.is_object())
                .unwrap_or(value);
            ["name", "address", "email"]
                .iter()
                .map(|key| text(field(inner, key)))
                .find(|value| !value.is_empty())
                .unwrap_or_default()
                .trim()
                .to_owned()
        }
        Value::String(value) => value.trim().to_owned(),
        _ => String::new(),
    }
}

fn epoch_or_value(value: &Value) -> Value {
    let seconds = match value {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.parse::<f64>().ok(),
        _ => None,
    };
    match seconds
        .filter(|seconds| seconds.is_finite())
        .and_then(|seconds| DateTime::from_timestamp(seconds as i64, 0))
    {
        Some(value) => json!(value.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        None => value.clone(),
    }
}

fn ticket(object: &Value, source: &str) -> Value {
    let key = first_field(object, &["key", "id"]);
    let updated = first_field(object, &["updated", "updated_at", "updatedAt"]);
    let status = name(&first_field(object, &["status"]));
    let summary = first_field(object, &["summary", "title"]);
    let description = first_field(object, &["description", "text"]);
    let key_text = text(Some(&key));
    let updated_text = text(Some(&updated));
    let id = if !key_text.is_empty() && !updated_text.is_empty() {
        format!(
            "{source}:{}",
            [key_text.clone(), updated_text, status.clone()]
                .into_iter()
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join(":")
        )
    } else {
        String::new()
    };
    json!({
        "id": id, "source": source, "kind": "ticket", "at": updated,
        "title": if !key_text.is_empty() && !text(Some(&summary)).is_empty() { format!("{key_text} {}", text(Some(&summary))) } else { text(Some(&summary)) },
        "status": status, "url": first_field(object, &["url", "browse_url", "link"]),
        "thread": if key_text.is_empty() { String::new() } else { format!("{source}:{key_text}") },
        "project": first_field(object, &["project"]), "to_me": truthy(field(object, "to_me")),
        "text": if description.is_string() { description } else { Value::Null }, "original": object,
    })
}

fn message(object: &Value, source: &str) -> Value {
    let ident = first_field(
        object,
        &[
            "id",
            "messageId",
            "message_id",
            "internetMessageId",
            "key",
            "uuid",
            "ts",
        ],
    );
    let body = field(object, "body");
    let body_text = match body {
        Some(Value::Object(body)) => body.get("content").cloned().unwrap_or(Value::Null),
        Some(value) => value.clone(),
        None => Value::Null,
    };
    let candidates = [
        field(object, "text").cloned().unwrap_or(Value::Null),
        body_text,
        field(object, "bodyPreview").cloned().unwrap_or(Value::Null),
        field(object, "snippet").cloned().unwrap_or(Value::Null),
    ];
    let body_text = candidates
        .iter()
        .find(|value| matches!(value, Value::String(value) if !value.trim().is_empty()))
        .cloned()
        .unwrap_or_else(|| json!(""));
    let mut title = first_field(object, &["title", "subject", "summary"]);
    if !nonempty(&title) {
        title = if text(Some(&body_text)).trim().is_empty() {
            json!("(no subject)")
        } else {
            json!(text(Some(&body_text))
                .trim()
                .lines()
                .next()
                .unwrap_or_default())
        };
    }
    let flag = field(object, "flag").cloned().unwrap_or_else(|| json!({}));
    let status = if text(field(&flag, "flagStatus")) == "flagged"
        || field(object, "flagged") == Some(&Value::Bool(true))
    {
        "flagged".to_owned()
    } else {
        name(field(object, "status").unwrap_or(&Value::Null))
    };
    let mut item = Map::new();
    for key in [
        "kind",
        "actor",
        "actor_id",
        "to_me",
        "channel",
        "importance",
        "personal",
        "tags",
        "project",
    ] {
        if let Some(value) = object.get(key) {
            item.insert(key.into(), value.clone());
        }
    }
    if !text(item.get("actor")).is_empty() && !ACTORS.contains(&text(item.get("actor")).as_str()) {
        item.remove("actor");
    }
    item.insert(
        "id".into(),
        json!(if nonempty(&ident) {
            format!("{source}:{}", text(Some(&ident)))
        } else {
            String::new()
        }),
    );
    item.insert("source".into(), json!(source));
    if text(item.get("kind")).is_empty() {
        item.insert("kind".into(), json!("mail"));
    }
    item.insert(
        "at".into(),
        epoch_or_value(&first_field(
            object,
            &[
                "at",
                "receivedDateTime",
                "received",
                "sentDateTime",
                "date",
                "timestamp",
                "created_at",
                "createdAt",
                "created",
                "time",
                "ts",
            ],
        )),
    );
    item.insert(
        "title".into(),
        json!(text(Some(&title)).lines().next().unwrap_or_default()),
    );
    item.insert(
        "url".into(),
        first_field(object, &["url", "webLink", "link"]),
    );
    item.insert(
        "thread".into(),
        first_field(object, &["thread", "conversationId", "threadId"]),
    );
    item.insert("status".into(), json!(status));
    item.insert("text".into(), json!(body_text));
    item.insert("original".into(), object.clone());
    item.insert(
        "from".into(),
        json!(name(field(object, "from").unwrap_or(&Value::Null))),
    );
    Value::Object(item)
}

fn meeting(object: &Value, source: &str) -> Value {
    let start = field(object, "start").cloned().unwrap_or(Value::Null);
    let at = if start.is_object() {
        first_field(&start, &["dateTime", "date"])
    } else {
        start
    };
    let ident = first_field(object, &["id", "eventId", "iCalUID"]);
    let id = if nonempty(&ident) && nonempty(&at) {
        format!("{source}:{}:{}", text(Some(&ident)), text(Some(&at)))
    } else {
        String::new()
    };
    let title = first_field(object, &["summary", "title", "subject"]);
    json!({
        "id": id, "source": source, "kind": "meeting", "at": at,
        "title": if nonempty(&title) { text(Some(&title)).lines().next().unwrap_or_default().to_owned() } else { "(no title)".into() },
        "url": first_field(object, &["htmlLink", "url", "webLink"]),
        "text": field(object, "description").filter(|value| value.is_string()).cloned().unwrap_or_else(|| json!("")),
        "original": object,
    })
}

/// Convert a raw ticket, mail or calendar reply without exposing its payload.
pub fn raw_events(data: &Value, source: &str) -> Result<Option<(Vec<Value>, usize, String)>> {
    if !data.is_object()
        || ["id", "at", "title"]
            .iter()
            .all(|key| truthy(data.get(*key)))
    {
        return Ok(None);
    }
    for (key, default) in [
        ("tickets", "jira"),
        ("messages", "mail"),
        ("events", "calendar"),
    ] {
        let Some(items) = data.get(key).and_then(Value::as_array) else {
            continue;
        };
        if key == "events"
            && !items.is_empty()
            && !items
                .iter()
                .any(|item| item.is_object() && item.get("start").is_some())
        {
            continue;
        }
        let source = source_name(if source.is_empty() { default } else { source })?;
        let mut out = Vec::new();
        let mut skipped = 0usize;
        for object in items {
            let event = if !object.is_object() {
                Value::Null
            } else {
                match key {
                    "tickets" => ticket(object, &source),
                    "events" => meeting(object, &source),
                    _ => message(object, &source),
                }
            };
            let valid = !text(field(&event, "id")).is_empty() && clean(&event, "event").is_ok();
            if valid {
                out.push(event);
            } else {
                skipped += 1;
            }
        }
        return Ok(Some((out, skipped, source)));
    }
    Ok(None)
}

pub fn note_failure(root: &Path, source: &str) -> Result<()> {
    if !valid_source(source) {
        return Ok(());
    }
    careerbrew_vault::lock::with_exact_lock(&feed_dir(root).join(".lock"), || -> Result<()> {
        let mut batch = read_json(root, "work/feed/feed-cycle.json", json!({}))?;
        let mut failed = field(&batch, "failed_sources")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|value| text(Some(value)))
            .collect::<BTreeSet<_>>();
        failed.insert(source.to_owned());
        batch["failed_sources"] = json!(failed.into_iter().collect::<Vec<_>>());
        write_json_if_changed(root, "work/feed/feed-cycle.json", &batch)?;
        Ok(())
    })?
}

pub fn add_lines(
    root: &Path,
    input: &str,
    plugin: &str,
    source: &str,
    components: &dyn ComponentProvider,
) -> Result<Value> {
    match add_lines_inner(root, input, plugin, source, components) {
        Err(error) => {
            note_failure(root, source)?;
            Err(error)
        }
        answer => answer,
    }
}

fn add_lines_inner(
    root: &Path,
    input: &str,
    plugin: &str,
    source: &str,
    components: &dyn ComponentProvider,
) -> Result<Value> {
    let origin = if plugin.is_empty() {
        "user".to_owned()
    } else {
        let installed = careerbrew_setup::plugins::installed()?;
        if !installed.iter().any(|item| item.name == plugin) {
            return Err(FeedError::feed(format!(
                "plugin {plugin} is not installed (see `careerbrew plugin list`)"
            )));
        }
        format!("plugin:{plugin}")
    };
    let raw = serde_json::from_str::<Value>(input)
        .ok()
        .and_then(|data| raw_events(&data, source).ok().flatten());
    if let Some((mut events, skipped, source)) = raw {
        if !plugin.is_empty() {
            for event in &mut events {
                let id = text(field(event, "id"));
                if !id.starts_with(&format!("{plugin}:")) {
                    event["id"] = json!(format!("{plugin}:{id}"));
                }
            }
        }
        let mut result = ingest(
            root,
            &events,
            &origin,
            if plugin.is_empty() { "user" } else { "plugin" },
            &[source],
            components,
        )?;
        result["skipped"] = json!(skipped);
        return Ok(result);
    }
    let mut items = Vec::new();
    for (index, line) in input.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let mut item: Value = serde_json::from_str(line).map_err(|_| {
            FeedError::feed(format!(
                "line {}: not JSON (fix: one JSON object per line with id, at, and title)",
                index + 1
            ))
        })?;
        let cleaned = clean(&item, &format!("line {}", index + 1))?;
        if !plugin.is_empty() && !text(field(&cleaned, "id")).starts_with(&format!("{plugin}:")) {
            item["id"] = json!(format!("{plugin}:{}", text(field(&cleaned, "id"))));
        }
        if item.get("original").is_none() {
            let mut original = item.as_object().cloned().unwrap_or_default();
            original.remove("original");
            item["original"] = Value::Object(original);
        }
        items.push(item);
    }
    if items.is_empty() {
        return Err(FeedError::feed(
            "no events in the input (fix: one JSON object per line with id, at, and title)",
        ));
    }
    let mut result = ingest(
        root,
        &items,
        &origin,
        if plugin.is_empty() { "user" } else { "plugin" },
        &[],
        components,
    )?;
    result["skipped"] = json!(0);
    Ok(result)
}

pub fn retriage(root: &Path) -> Result<Value> {
    let rule_list = rules(root)?;
    let team_rows = careerbrew_projects::team::load(root)?;
    let (seen, changed) = careerbrew_vault::lock::with_exact_lock(
        &feed_dir(root).join(".lock"),
        || -> Result<(usize, usize)> {
            let mut rows = load(root)?
                .into_iter()
                .map(|row| (text(field(&row, "id")), row))
                .collect::<BTreeMap<_, _>>();
            let originals = originals(root)?;
            let mut seen = 0usize;
            let mut changed = 0usize;
            for (id, original) in &originals {
                let Some(old) = rows.get(id).cloned() else {
                    continue;
                };
                seen += 1;
                let mut item = Map::new();
                for key in FACT_KEYS {
                    item.insert((*key).into(), old.get(*key).cloned().unwrap_or(Value::Null));
                }
                item.insert("text".into(), json!(text(field(original, "text"))));
                item.insert(
                    "personal".into(),
                    json!(text(field(&old, "personal_by")) == "source"),
                );
                let mut row = analyze(
                    &Value::Object(item),
                    &rule_list,
                    &text(field(&old, "origin")),
                    old.get("marked").and_then(Value::as_bool),
                    &team_rows,
                );
                keep_old(&mut row, Some(&old));
                if row != old {
                    rows.insert(id.clone(), row);
                    changed += 1;
                }
            }
            if changed > 0 {
                save(root, &rows, &originals)?;
            }
            Ok((seen, changed))
        },
    )??;
    emit(
        root,
        "work.feed.retriage",
        vec![
            ("status", Canon::Str("ok".into())),
            ("count", Canon::Int(seen.into())),
            ("changed", Canon::Int(changed.into())),
        ],
    )?;
    Ok(json!({"events": seen, "changed": changed}))
}

pub fn needs_research(row: &Value) -> Vec<String> {
    if text(field(row, "triage")) == "noise" || truthy(field(row, "personal")) {
        return Vec::new();
    }
    let done = field(row, "research")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| {
            !(text(field(entry, "by")) == "cycle"
                && field(entry, "opened") == Some(&Value::Bool(false)))
        })
        .map(|entry| text(field(entry, "url")))
        .collect::<BTreeSet<_>>();
    field(row, "links")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| text(Some(value)))
        .filter(|url| !done.contains(url))
        .collect()
}

pub fn research(root: &Path, event_id: &str, entries: &Value) -> Result<Value> {
    let Some(entries) = entries.as_array().filter(|entries| !entries.is_empty()) else {
        return Err(FeedError::feed(
            "research is a JSON list: one {url, summary} or {url, opened: false, reason} per link",
        ));
    };
    careerbrew_vault::lock::with_exact_lock(&feed_dir(root).join(".lock"), || -> Result<Value> {
        let mut rows = load(root)?
            .into_iter()
            .map(|row| (text(field(&row, "id")), row))
            .collect::<BTreeMap<_, _>>();
        let Some(mut row) = rows.get(event_id).cloned() else {
            return Err(FeedError::feed(format!(
                "no feed event {event_id} (see: careerbrew work feed links)"
            )));
        };
        if text(field(&row, "triage")) == "noise" || truthy(field(&row, "personal")) {
            return Err(FeedError::feed(format!(
                "{event_id} is {}: only kept events are researched",
                if truthy(field(&row, "personal")) {
                    "personal"
                } else {
                    "noise"
                }
            )));
        }
        let have = field(&row, "links")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let url = text(field(entry, "url"));
            if !have.iter().any(|known| text(Some(known)) == url) {
                return Err(FeedError::feed(format!(
                    "entry {}: url must be one of this event's links",
                    index + 1
                )));
            }
            if out.iter().any(|old| text(field(old, "url")) == url) {
                return Err(FeedError::feed(format!(
                    "entry {}: the same link twice; one entry per link",
                    index + 1
                )));
            }
            if field(entry, "opened") == Some(&Value::Bool(false)) {
                let reason = text(field(entry, "reason"))
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if reason.is_empty() || reason.chars().count() > 120 {
                    return Err(FeedError::feed(format!(
                        "entry {}: opened: false needs a reason",
                        index + 1
                    )));
                }
                out.push(json!({"url": url, "opened": false, "reason": reason}));
            } else {
                let summary = text(field(entry, "summary"));
                if summary.trim().is_empty()
                    || summary.trim().contains('\n')
                    || summary.chars().count() > 200
                {
                    return Err(FeedError::feed(format!(
                        "entry {}: summary is one line",
                        index + 1
                    )));
                }
                out.push(json!({"url": url, "opened": true, "summary": summary.split_whitespace().collect::<Vec<_>>().join(" ")}));
            }
        }
        let missing = have
            .iter()
            .enumerate()
            .filter(|(_, url)| {
                !out.iter()
                    .any(|entry| text(field(entry, "url")) == text(Some(url)))
            })
            .map(|(index, _)| index + 1)
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(FeedError::feed(format!(
                "refused: {} link(s) not researched (link {} of {})",
                missing.len(),
                missing
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                have.len()
            )));
        }
        row["research"] = json!(out);
        rows.insert(event_id.into(), row.clone());
        let mut values = rows.values().cloned().collect::<Vec<_>>();
        values.sort_by_key(|row| (text(field(row, "at")), text(field(row, "id"))));
        write_jsonl(root, "work/feed/events.jsonl", &values)?;
        Ok(row)
    })?
}

pub fn mark_ask(root: &Path, event_id: &str, value: &str) -> Result<Value> {
    if !["not-ask", "done", "ask"].contains(&value) {
        return Err(FeedError::feed(
            "an ask is marked not-ask, done, or ask to open it again",
        ));
    }
    careerbrew_vault::lock::with_exact_lock(&feed_dir(root).join(".lock"), || -> Result<Value> {
        let mut rows = load(root)?
            .into_iter()
            .map(|row| (text(field(&row, "id")), row))
            .collect::<BTreeMap<_, _>>();
        let Some(mut row) = rows.get(event_id).cloned() else {
            return Err(FeedError::feed(format!(
                "no feed event {event_id} (see: careerbrew work feed show --show)"
            )));
        };
        if value == "ask" {
            if let Some(row) = row.as_object_mut() {
                row.remove("ask_mark");
                row.remove("ask_marked_at");
            }
        } else {
            row["ask_mark"] = json!(value);
            row["ask_marked_at"] = json!(utc(&json!(now_iso()))?);
        }
        rows.insert(event_id.into(), row.clone());
        let mut values = rows.values().cloned().collect::<Vec<_>>();
        values.sort_by_key(|row| (text(field(row, "at")), text(field(row, "id"))));
        write_jsonl(root, "work/feed/events.jsonl", &values)?;
        Ok(row)
    })?
}

pub fn thread_key(row: &Value, entries: &[Value]) -> String {
    let thread = text(field(row, "thread"));
    if !thread.is_empty() {
        return thread;
    }
    let url = text(field(row, "url"));
    let Ok(parsed) = careerbrew_sources::http::parse_url(&url) else {
        return String::new();
    };
    let path = parsed.path.split(['?', '#']).next().unwrap_or_default();
    let item = Regex::new(r"^(/.+?/(?:pulls?|pull-requests|merge_requests|issues)/\d+)")
        .expect("item regex")
        .captures(path)
        .and_then(|capture| capture.get(1).map(|value| value.as_str()))
        .filter(|item| {
            path[item.len()..]
                .chars()
                .next()
                .is_none_or(|next| !next.is_ascii_digit())
        })
        .map(str::to_owned);
    if let Some(item) = item {
        return format!("link:{}{}", parsed.host, item);
    }
    let named = careerbrew_projects::project_priority::ticket_projects(entries);
    let standards = [
        "AES", "ANSI", "CVE", "CWE", "ECMA", "IEC", "IEEE", "ISO", "PEP", "RFC", "RSA", "SHA",
        "TLS", "UCS", "UTF",
    ];
    let ticket = Regex::new(r"^([A-Z][A-Z0-9]+)-\d+$").expect("ticket regex");
    for part in path.split('/') {
        if let Some(capture) = ticket.captures(part) {
            let prefix = capture
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or_default();
            if (named.is_empty() && !standards.contains(&prefix)) || named.contains(prefix) {
                return format!("ticket:{}:{part}", parsed.host);
            }
        }
    }
    String::new()
}

pub fn open_asks(
    root: &Path,
    rows: Option<&[Value]>,
    include_personal: bool,
) -> Result<Vec<Value>> {
    let owned;
    let rows = match rows {
        Some(rows) => rows,
        None => {
            owned = load(root)?;
            &owned
        }
    };
    let rows = rows
        .iter()
        .filter(|row| field(row, "at").is_some_and(Value::is_string))
        .cloned()
        .collect::<Vec<_>>();
    let entries = careerbrew_projects::project_priority::entries(root)?;
    let keys = rows
        .iter()
        .map(|row| (text(field(row, "id")), thread_key(row, &entries)))
        .collect::<BTreeMap<_, _>>();
    let mut answered = BTreeMap::<String, String>::new();
    for row in &rows {
        let key = keys
            .get(&text(field(row, "id")))
            .cloned()
            .unwrap_or_default();
        if text(field(row, "actor")) == "me" && !key.is_empty() {
            let at = text(field(row, "at"));
            answered
                .entry(key)
                .and_modify(|old| *old = old.clone().max(at.clone()))
                .or_insert(at);
        }
    }
    Ok(rows
        .into_iter()
        .filter(|row| {
            let id = text(field(row, "id"));
            let key = keys.get(&id).cloned().unwrap_or_default();
            let at = text(field(row, "at"));
            text(field(row, "triage")) == "act"
                && text(field(row, "ask_mark")).is_empty()
                && (include_personal || !truthy(field(row, "personal")))
                && !(!key.is_empty() && answered.get(&key).is_some_and(|answered| answered >= &at))
        })
        .collect())
}

pub fn summary(root: &Path, rows: &[Value]) -> Result<Value> {
    let work = rows
        .iter()
        .filter(|row| !truthy(field(row, "personal")))
        .collect::<Vec<_>>();
    let triage = TRIAGES
        .iter()
        .map(|triage| {
            (
                (*triage).to_owned(),
                work.iter()
                    .filter(|row| text(field(row, "triage")) == *triage)
                    .count(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut sources = BTreeMap::<String, usize>::new();
    for row in &work {
        *sources.entry(text(field(row, "source"))).or_default() += 1;
    }
    let asks = open_asks(root, Some(rows), false)?;
    let settings = read_yaml(root, "settings.yaml", json!({}))?;
    let hours = field(&settings, "feed_ask_overdue_hours")
        .and_then(Value::as_i64)
        .unwrap_or(8);
    let overdue = asks
        .iter()
        .filter(|row| work_minutes(row, now(), &settings) > hours * 60)
        .count();
    Ok(json!({
        "events": rows.len(), "personal": rows.len() - work.len(), "triage": triage,
        "sources": sources, "open_asks": asks.len(), "open_asks_overdue": overdue,
        "overdue_hours": hours, "evidence": work.iter().filter(|row| truthy(field(row, "evidence"))).count(),
    }))
}

pub fn work_minutes(row: &Value, at: DateTime<chrono::FixedOffset>, settings: &Value) -> i64 {
    let Some(lo) =
        DateTime::parse_from_rfc3339(&text(field(row, "at")).replace("Z", "+00:00")).ok()
    else {
        return 0;
    };
    let zone = text(field(settings, "timezone"))
        .parse::<chrono_tz::Tz>()
        .unwrap_or_else(|_| crate::local_zone());
    let lo = lo.with_timezone(&zone);
    let hi = at.with_timezone(&zone);
    let start = text(field(settings, "work_start"));
    let end = text(field(settings, "work_end"));
    let start = if start.is_empty() {
        "09:00".into()
    } else {
        start
    };
    let end = if end.is_empty() { "17:30".into() } else { end };
    let days = field(settings, "work_days")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| vec![json!(0), json!(1), json!(2), json!(3), json!(4)]);
    let days = days
        .iter()
        .filter_map(Value::as_i64)
        .collect::<BTreeSet<_>>();
    let mut seconds = 0i64;
    let mut day = lo.date_naive();
    while day <= hi.date_naive() {
        if days.contains(&(day.weekday().number_from_monday() as i64 - 1)) {
            let Some(start) = NaiveDateTime::parse_from_str(
                &format!("{}T{start}", day.format("%Y-%m-%d")),
                "%Y-%m-%dT%H:%M",
            )
            .ok() else {
                continue;
            };
            let Some(end) = NaiveDateTime::parse_from_str(
                &format!("{}T{end}", day.format("%Y-%m-%d")),
                "%Y-%m-%dT%H:%M",
            )
            .ok() else {
                continue;
            };
            let Some(a) = careerbrew_memory::memory_sync::local_naive(zone, start) else {
                continue;
            };
            let Some(b) = careerbrew_memory::memory_sync::local_naive(zone, end) else {
                continue;
            };
            let a = lo.max(a.with_timezone(&zone));
            let b = hi.min(b.with_timezone(&zone));
            seconds += (b - a).num_seconds().max(0);
        }
        day += TimeDelta::days(1);
    }
    seconds / 60
}

pub fn mark_cycle(
    root: &Path,
    required: Option<&[String]>,
    components: &dyn ComponentProvider,
) -> Result<Value> {
    let declaration = components.declaration(root)?;
    let wanted = declaration
        .required
        .iter()
        .cloned()
        .chain(required.unwrap_or(&[]).iter().cloned())
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    let required = wanted
        .into_iter()
        .filter(|source| seen.insert(source.clone()))
        .map(|source| source_name(&source))
        .collect::<Result<Vec<_>>>()?;
    if required.is_empty() {
        return Err(FeedError::feed("no required sources (fix: --require slack,jira,mail, or careerbrew config set feed_tool_sources)"));
    }
    careerbrew_vault::lock::with_exact_lock(
        &root.join("work/live/.cycle.lock"),
        || -> Result<Value> {
            let batch = careerbrew_vault::lock::with_exact_lock(
                &feed_dir(root).join(".lock"),
                || -> Result<Value> {
                    let batch = read_json(root, "work/feed/feed-cycle.json", json!({}))?;
                    if !batch.as_object().is_some_and(Map::is_empty) {
                        write_json_if_changed(
                            root,
                            "work/feed/feed-cycle.json",
                            &json!({"stored": []}),
                        )?;
                    }
                    Ok(batch)
                },
            )??;
            components.complete(root, &required, &batch)?;
            let gaps = components
                .required_gaps(root)?
                .into_iter()
                .collect::<BTreeSet<_>>();
            let stored = field(&batch, "stored")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|value| text(Some(value)))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let failed = field(&batch, "failed_sources")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|value| text(Some(value)))
                .collect::<BTreeSet<_>>();
            let errors = field(&batch, "errors").is_some_and(|value| {
                truthy(Some(value))
                    || value.as_object().is_some_and(|value| !value.is_empty())
                    || value.as_array().is_some_and(|value| !value.is_empty())
            });
            let not_stored = required
                .iter()
                .filter(|source| {
                    !stored.contains(source)
                        || (!declaration.required.is_empty()
                            && (failed.contains(*source) || errors))
                        || gaps.contains(*source)
                })
                .cloned()
                .collect::<Vec<_>>();
            emit(
                root,
                "work.feed.cycle",
                vec![
                    (
                        "status",
                        Canon::Str(if not_stored.is_empty() {
                            "ok".into()
                        } else {
                            "gap".into()
                        }),
                    ),
                    ("required", Canon::Int(required.len().into())),
                    ("stored", Canon::Int(stored.len().into())),
                    ("not_stored", Canon::Int(not_stored.len().into())),
                    ("story", Canon::Str("us-61".into())),
                ],
            )?;
            Ok(json!({"required": required, "stored": stored, "not_stored": not_stored}))
        },
    )?
}

pub fn cycle_json(result: &Value) -> Result<String> {
    Ok(format!(
        "{{\"required\": {}, \"stored\": {}, \"not_stored\": {}}}",
        compact(field(result, "required").unwrap_or(&json!([])))?,
        compact(field(result, "stored").unwrap_or(&json!([])))?,
        compact(field(result, "not_stored").unwrap_or(&json!([])))?,
    ))
}
