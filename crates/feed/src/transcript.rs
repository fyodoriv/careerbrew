//! Claude/Codex transcript normalization into the feed component.

use crate::{
    emit, feed, field, now_utc, read_yaml, text, truthy, ComponentProvider, FeedError, Result,
};
use careerbrew_vault::canon::Canon;
use chrono::{DateTime, TimeDelta};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const COPY: &[&str] = &[
    "kind",
    "actor",
    "actor_id",
    "to_me",
    "url",
    "thread",
    "tags",
    "personal",
    "channel",
    "from",
    "status",
    "importance",
    "project",
];

pub fn tool_map(root: &Path) -> Result<Vec<(String, String)>> {
    let settings = read_yaml(root, "settings.yaml", json!({}))?;
    let values = field(&settings, "feed_tool_sources")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    values
        .iter()
        .map(|value| {
            let value = text(Some(value));
            value
                .split_once('=')
                .map(|(glob, source)| (glob.to_owned(), source.to_owned()))
                .ok_or_else(|| FeedError::value("feed_tool_sources entries use glob=source"))
        })
        .collect()
}

pub fn mapped_sources(pairs: &[(String, String)]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    pairs
        .iter()
        .map(|(_, source)| source.clone())
        .filter(|source| seen.insert(source.clone()))
        .collect()
}

fn source(tool: &str, pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .find(|(glob, _)| {
            glob::Pattern::new(glob)
                .map(|glob| glob.matches(tool))
                .unwrap_or(false)
        })
        .map(|(_, source)| source.clone())
        .unwrap_or_default()
}

fn since(_root: &Path, value: &str) -> Result<String> {
    let value = value.trim();
    if let Some(capture) = Regex::new(r"^(\d+)([mhd])$")
        .expect("since regex")
        .captures(value)
    {
        let count = capture
            .get(1)
            .and_then(|value| value.as_str().parse::<i64>().ok())
            .unwrap_or_default();
        let delta = match capture.get(2).map(|value| value.as_str()) {
            Some("m") => TimeDelta::minutes(count),
            Some("h") => TimeDelta::hours(count),
            _ => TimeDelta::days(count),
        };
        return Ok((now_utc() - delta).format("%Y-%m-%dT%H:%M:%SZ").to_string());
    }
    feed::utc(&json!(value)).map_err(|_| {
        FeedError::feed(format!(
            "--since {:?}: use an ISO time or a duration such as 30m, 2h, 1d",
            value.chars().take(40).collect::<String>()
        ))
    })
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(value) => value.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|block| matches!(text(field(block, "type")).as_str(), "text" | "input_text"))
            .map(|block| text(field(block, "text")))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn parse(value: &str) -> Value {
    if let Ok(data) = serde_json::from_str(value) {
        return data;
    }
    Value::Array(
        value
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .collect(),
    )
}

fn unwrap(data: &Value) -> Value {
    if let Value::Array(blocks) = data {
        if !blocks.is_empty()
            && blocks.iter().all(|block| {
                block.is_object()
                    && matches!(text(field(block, "type")).as_str(), "text" | "input_text")
            })
        {
            return parse(&content_text(data));
        }
    }
    data.clone()
}

fn candidates(data: &Value) -> Vec<Value> {
    match unwrap(data) {
        Value::Array(items) => items.into_iter().filter(|item| item.is_object()).collect(),
        Value::Object(object) => {
            let lists = object
                .values()
                .filter_map(Value::as_array)
                .filter(|items| items.iter().any(|item| item.is_object()))
                .cloned()
                .collect::<Vec<_>>();
            if lists.is_empty() {
                vec![Value::Object(object)]
            } else {
                lists
                    .into_iter()
                    .flatten()
                    .filter(|item| item.is_object())
                    .collect()
            }
        }
        _ => Vec::new(),
    }
}

fn pick(object: &Value, key: &str) -> Value {
    let aliases: &[&str] = match key {
        "id" => &["id", "key", "uuid", "ts"],
        "at" => &[
            "at",
            "timestamp",
            "created_at",
            "createdAt",
            "created",
            "time",
            "date",
            "ts",
        ],
        _ => &["title", "subject", "summary", "name", "text"],
    };
    aliases
        .iter()
        .map(|alias| field(object, alias).cloned().unwrap_or(Value::Null))
        .find(|value| !value.is_null() && !text(Some(value)).is_empty())
        .unwrap_or(Value::Null)
}

fn at(value: &Value) -> Result<Value> {
    let seconds = match value {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.parse::<f64>().ok(),
        _ => None,
    };
    if seconds.is_some_and(|seconds| seconds.is_infinite()) {
        return Err(FeedError::new("OverflowError", "timestamp out of range"));
    }
    Ok(
        match seconds
            .filter(|seconds| seconds.is_finite())
            .and_then(|seconds| DateTime::from_timestamp(seconds as i64, 0))
        {
            Some(value) => json!(value.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            None => value.clone(),
        },
    )
}

pub fn event(object: &Value, source: &str) -> Result<Option<Value>> {
    let ident = pick(object, "id");
    let timestamp = pick(object, "at");
    let title = pick(object, "title");
    if ident.is_null() || timestamp.is_null() || title.is_null() {
        return Ok(None);
    }
    let mut item = Map::new();
    for key in COPY {
        if let Some(value) = object.get(*key) {
            item.insert((*key).into(), value.clone());
        }
    }
    if item.get("actor").is_some() && !feed::ACTORS.contains(&text(item.get("actor")).as_str()) {
        item.remove("actor");
    }
    let body = field(object, "text")
        .filter(|value| value.is_string())
        .or_else(|| field(object, "body").filter(|value| value.is_string()))
        .cloned()
        .unwrap_or_else(|| json!(""));
    item.insert(
        "id".into(),
        json!(format!("{source}:{}", text(Some(&ident)))),
    );
    item.insert("source".into(), json!(source));
    item.insert("at".into(), at(&timestamp)?);
    item.insert(
        "title".into(),
        json!(text(Some(&title)).lines().next().unwrap_or_default()),
    );
    item.insert("text".into(), body);
    item.insert("original".into(), object.clone());
    let item = Value::Object(item);
    Ok(feed::clean(&item, "event").ok().map(|_| item))
}

fn read_row(line: &str) -> Option<Value> {
    serde_json::from_str::<Value>(line)
        .ok()
        .filter(|row| row.is_object())
}

fn blocks(row: &Value, kind: &str) -> Vec<Value> {
    field(row, "message")
        .and_then(|message| field(message, "content"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| text(field(block, "type")) == kind)
        .cloned()
        .collect()
}

/// Normalize one Codex response/event row to the Claude block shape.
pub fn normalized(row: &Value) -> Value {
    let payload = field(row, "payload").cloned().unwrap_or(Value::Null);
    if text(field(row, "type")) == "event_msg"
        && text(field(&payload, "type")) == "mcp_tool_call_end"
    {
        let invocation = field(&payload, "invocation")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let result = field(&payload, "result")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let value = field(&result, "Ok").cloned().unwrap_or_else(|| json!({}));
        let error = field(&result, "Err").is_some() || truthy(field(&value, "isError"));
        let mut content = field(&value, "content")
            .cloned()
            .unwrap_or_else(|| json!([]));
        if content_text(&content).is_empty()
            && field(&value, "structuredContent").is_some_and(Value::is_object)
        {
            content = json!(serde_json::to_string(
                field(&value, "structuredContent").unwrap_or(&Value::Null)
            )
            .unwrap_or_default());
        }
        return json!({"timestamp": field(row, "timestamp").cloned().unwrap_or(Value::Null), "message": {"content": [
            {"type": "tool_use", "id": field(&payload, "call_id").cloned().unwrap_or(Value::Null), "name": format!("mcp__{}__{}", text(field(&invocation, "server")), text(field(&invocation, "tool")))},
            {"type": "tool_result", "tool_use_id": field(&payload, "call_id").cloned().unwrap_or(Value::Null), "content": content, "is_error": error},
        ]}});
    }
    if text(field(row, "type")) != "response_item" || !payload.is_object() {
        return row.clone();
    }
    let kind = text(field(&payload, "type"));
    let block = if kind == "function_call" {
        let mut name = text(field(&payload, "name"));
        let namespace = text(field(&payload, "namespace"))
            .trim_end_matches('_')
            .to_owned();
        if namespace.starts_with("mcp__") && !name.starts_with("mcp__") {
            name = format!("{namespace}__{name}");
        }
        json!({"type": "tool_use", "id": field(&payload, "call_id").cloned().unwrap_or(Value::Null), "name": name})
    } else if kind == "function_call_output" {
        let output = field(&payload, "output")
            .cloned()
            .unwrap_or_else(|| json!(""));
        let output = if output.is_object() {
            json!(serde_json::to_string(&output).unwrap_or_default())
        } else {
            output
        };
        json!({"type": "tool_result", "tool_use_id": field(&payload, "call_id").cloned().unwrap_or(Value::Null), "content": output, "is_error": truthy(field(&payload, "is_error"))})
    } else {
        return row.clone();
    };
    json!({"timestamp": field(row, "timestamp").cloned().unwrap_or(Value::Null), "message": {"content": [block]}})
}

fn stamp(row: &Value) -> String {
    feed::utc(field(row, "timestamp").unwrap_or(&Value::Null)).unwrap_or_default()
}

fn expand(path: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    if value == "~" || value.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(value.trim_start_matches("~/"));
        }
    }
    path.to_path_buf()
}

pub fn store(
    root: &Path,
    path: &Path,
    since_value: &str,
    components: &dyn ComponentProvider,
) -> Result<Value> {
    let pairs = tool_map(root)?;
    if pairs.is_empty() {
        return Err(FeedError::feed("no tool map (fix: careerbrew config set feed_tool_sources 'mcp__chat__*=chat, mcp__tickets__*=tickets')"));
    }
    let path = expand(path);
    if !path.is_file() {
        return Err(FeedError::feed(
            "the transcript file does not exist (fix: pass the session's .jsonl file)",
        ));
    }
    let folder = path.canonicalize()?.parent().unwrap_or(&path).to_path_buf();
    let cut = if since_value.is_empty() {
        String::new()
    } else {
        since(root, since_value)?
    };
    let sources = mapped_sources(&pairs);
    let mut replies = sources
        .iter()
        .map(|source| (source.clone(), 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut stored = replies.clone();
    let mut followed = 0usize;
    let mut refused = 0usize;
    let mut tools = BTreeMap::<String, String>::new();
    let mut items = BTreeMap::<String, Value>::new();
    let mut quiet = BTreeSet::new();
    let mut errors = 0usize;
    let mut skipped = 0usize;
    let mut failed_sources = BTreeSet::new();
    let mut recognized = false;
    let bytes = std::fs::read(&path)?;
    let pointer_pattern = Regex::new(r#"(?i)saved to:?\s+([^\s<>\"'`]+)"#).expect("pointer regex");
    for line in String::from_utf8_lossy(&bytes).lines() {
        let Some(raw) = read_row(line) else {
            continue;
        };
        recognized |= field(&raw, "message").is_some_and(Value::is_object)
            || ["response_item", "session_meta", "event_msg", "turn_context"]
                .contains(&text(field(&raw, "type")).as_str());
        let row = normalized(&raw);
        for block in blocks(&row, "tool_use") {
            let source = source(&text(field(&block, "name")), &pairs);
            let id = text(field(&block, "id"));
            if !source.is_empty() && !id.is_empty() {
                tools.insert(id, source);
            }
        }
        for block in blocks(&row, "tool_result") {
            let source = tools
                .get(&text(field(&block, "tool_use_id")))
                .cloned()
                .unwrap_or_default();
            if source.is_empty() || (!cut.is_empty() && stamp(&row) < cut) {
                continue;
            }
            *replies.entry(source.clone()).or_default() += 1;
            if truthy(field(&block, "is_error")) {
                errors += 1;
                failed_sources.insert(source);
                continue;
            }
            let mut value = content_text(field(&block, "content").unwrap_or(&Value::Null));
            let pointer = if value.len() < 2000 {
                pointer_pattern.captures(&value).and_then(|capture| {
                    capture.get(1).map(|value| {
                        value
                            .as_str()
                            .trim_end_matches(['.', ',', ';', ':', ')'])
                            .to_owned()
                    })
                })
            } else {
                None
            };
            if let Some(pointer) = pointer {
                let target = folder.join(pointer);
                let target = target.canonicalize().ok();
                if let Some(target) =
                    target.filter(|target| target.starts_with(&folder) && target.is_file())
                {
                    followed += 1;
                    value = String::from_utf8_lossy(&std::fs::read(target)?).into_owned();
                } else {
                    refused += 1;
                    failed_sources.insert(source);
                    continue;
                }
            }
            let data = unwrap(&parse(&value));
            if let Some((events, count, _)) = feed::raw_events(&data, &source)? {
                skipped += count;
                for event in events {
                    items.insert(text(field(&event, "id")), event);
                }
                if count == 0
                    && ["messages", "tickets", "events"].iter().any(|key| {
                        field(&data, key)
                            .and_then(Value::as_array)
                            .is_some_and(Vec::is_empty)
                    })
                {
                    quiet.insert(source);
                }
                continue;
            }
            for object in candidates(&data) {
                if let Some(event) = event(&object, &source)? {
                    items.insert(text(field(&event, "id")), event);
                } else {
                    skipped += 1;
                }
            }
        }
    }
    if !recognized {
        return Err(FeedError::feed(
            "unsupported transcript format (use Claude/native JSONL, or careerbrew work feed add)",
        ));
    }
    let values = items.values().cloned().collect::<Vec<_>>();
    let result = if !values.is_empty() || !quiet.is_empty() {
        feed::ingest(
            root,
            &values,
            "user",
            "transcript",
            &quiet.iter().cloned().collect::<Vec<_>>(),
            components,
        )?
    } else {
        json!({"new": 0, "updated": 0})
    };
    for event in items.values() {
        *stored.entry(text(field(event, "source"))).or_default() += 1;
    }
    let gaps = sources
        .iter()
        .filter(|source| {
            stored.get(*source).copied().unwrap_or_default() == 0 && !quiet.contains(*source)
        })
        .cloned()
        .collect::<Vec<_>>();
    for source in failed_sources.into_iter().chain(
        gaps.iter()
            .filter(|source| replies.get(*source).copied().unwrap_or_default() > 0)
            .cloned(),
    ) {
        feed::note_failure(root, &source)?;
    }
    let new = result["new"].as_i64().unwrap_or_default();
    let updated = result["updated"].as_i64().unwrap_or_default();
    emit(
        root,
        "work.feed.transcript",
        vec![
            (
                "status",
                Canon::Str(if gaps.is_empty() {
                    "ok".into()
                } else {
                    "gap".into()
                }),
            ),
            ("new", Canon::Int(new.into())),
            ("updated", Canon::Int(updated.into())),
            ("skipped", Canon::Int(skipped.into())),
            ("errors", Canon::Int(errors.into())),
            ("gaps", Canon::Int(gaps.len().into())),
            ("story", Canon::Str("us-61".into())),
        ],
    )?;
    Ok(json!({
        "new": new, "updated": updated, "already": items.len() as i64 - new - updated,
        "skipped": skipped, "errors": errors, "replies": replies, "stored": stored,
        "pointers": {"followed": followed, "refused": refused}, "gaps": gaps,
    }))
}
