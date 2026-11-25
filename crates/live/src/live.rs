//! Live source configuration, one-lock cycles, verdicts and acquisition receipts.

use crate::{
    chars, compact, emit, field, health, json_digest, listen, live_error, live_stamp, now_utc,
    read_json, read_jsonl, setting_i64, text, truthy, typed, write_json, write_jsonl, write_yaml,
    Result,
};
use careerbrew_feed::{feed, transcript, ComponentProvider, Declaration};
use careerbrew_vault::canon::Canon;
use chrono::{DateTime, SecondsFormat, Utc};
use regex::Regex;
use serde_json::{json, Map, Value};
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

pub const KINDS: &[&str] = &["chat", "tickets", "code", "mail", "calendar", "paging"];
pub const ADAPTERS: &[&str] = &["file", "connector"];
pub const VERDICTS: &[&str] = &[
    "answer", "done", "read", "skip", "evidence", "log", "personal",
];
pub const KEEP_CYCLES: usize = 200;
const SOURCE_KEYS: &[&str] = &[
    "name",
    "kind",
    "required",
    "adapter",
    "inbox",
    "connector",
    "pull",
    "max_age_minutes",
];

pub fn state_path(root: &Path) -> PathBuf {
    root.join("work/live/state.json")
}

pub fn cycles_path(root: &Path) -> PathBuf {
    root.join("work/live/cycles.jsonl")
}

pub fn config_path(root: &Path) -> PathBuf {
    root.join("work/live-sources.yaml")
}

pub fn check_source(value: &Value, where_: &str) -> Result<Value> {
    let Some(source) = value.as_object() else {
        return Err(live_error(format!(
            "{where_}: a source is a mapping with name, kind, and adapter"
        )));
    };
    if let Some(unknown) = source
        .keys()
        .find(|key| !SOURCE_KEYS.contains(&key.as_str()))
    {
        return Err(live_error(format!(
            "{where_}: unknown key {unknown} (allowed: {})",
            SOURCE_KEYS.join(", ")
        )));
    }
    let name = text(field(value, "name")).trim().to_owned();
    if !Regex::new(r"^[a-z0-9][a-z0-9._-]{0,39}$")
        .expect("source name")
        .is_match(&name)
    {
        return Err(live_error(format!(
            "{where_}: name {:?} must be a short lowercase name such as chat or tickets",
            chars(&name, 40)
        )));
    }
    let kind = text(field(value, "kind"));
    if !KINDS.contains(&kind.as_str()) {
        return Err(live_error(format!(
            "{where_} ({name}): kind must be one of {}",
            KINDS.join(", ")
        )));
    }
    let adapter = {
        let value = text(field(value, "adapter"));
        if value.is_empty() {
            "file".into()
        } else {
            value
        }
    };
    if !ADAPTERS.contains(&adapter.as_str()) {
        return Err(live_error(format!(
            "{where_} ({name}): adapter must be one of {}",
            ADAPTERS.join(", ")
        )));
    }
    if adapter == "file" && text(field(value, "inbox")).trim().is_empty() {
        return Err(live_error(format!("{where_} ({name}): a file adapter needs inbox: the file the agent saves the tool's reply to")));
    }
    if adapter == "connector" && text(field(value, "connector")).trim().is_empty() {
        return Err(live_error(format!("{where_} ({name}): a connector adapter needs connector: a kind such as slack (careerbrew connect <kind>)")));
    }
    let pull = field(value, "pull").cloned().unwrap_or_else(|| json!({}));
    if !pull.is_object() {
        return Err(live_error(format!(
            "{where_} ({name}): pull is a mapping such as {{hosts: [...], channels: [...]}}"
        )));
    }
    let age = field(value, "max_age_minutes");
    if age.is_some()
        && age
            .and_then(Value::as_i64)
            .is_none_or(|age| !(1..=10080).contains(&age))
    {
        return Err(live_error(format!(
            "{where_} ({name}): max_age_minutes must be an integer from 1 to 10080"
        )));
    }
    let mut out = Map::new();
    if let Some(age) = age.and_then(Value::as_i64) {
        out.insert("max_age_minutes".into(), json!(age));
    }
    out.extend([
        ("name".into(), json!(name)),
        ("kind".into(), json!(kind)),
        ("required".into(), json!(truthy(field(value, "required")))),
        ("adapter".into(), json!(adapter)),
        ("inbox".into(), json!(text(field(value, "inbox")))),
        ("connector".into(), json!(text(field(value, "connector")))),
        ("pull".into(), pull),
    ]);
    Ok(Value::Object(out))
}

fn read_sources(data: &Value, where_: &str) -> Result<Vec<Value>> {
    let Some(rows) = field(data, "sources") else {
        return Ok(Vec::new());
    };
    let Some(rows) = rows.as_array() else {
        return Err(live_error(format!("{where_}: sources is a list")));
    };
    let mut out = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        out.push(check_source(
            row,
            &format!("{where_} source {}", index + 1),
        )?);
    }
    let names = out
        .iter()
        .map(|row| text(field(row, "name")))
        .collect::<BTreeSet<_>>();
    if names.len() != out.len() {
        let duplicate = out
            .iter()
            .map(|row| text(field(row, "name")))
            .find(|name| {
                out.iter()
                    .filter(|row| text(field(row, "name")) == *name)
                    .count()
                    > 1
            })
            .unwrap_or_default();
        return Err(live_error(format!(
            "{where_}: source {duplicate} is listed twice"
        )));
    }
    Ok(out)
}

fn yaml(path: &Path, where_: &str) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    crate::read_yaml(Path::new(""), &path.to_string_lossy(), json!({})).map_err(|_| {
        live_error(format!(
            "{where_} is not YAML (fix: sources: [{{name, kind, adapter, ...}}])"
        ))
    })
}

pub fn plugin_files(key: &str) -> Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    for plugin in careerbrew_setup::plugins::installed()? {
        if let Some(relative) = field(&plugin.manifest, "live")
            .and_then(|live| field(live, key))
            .and_then(Value::as_str)
        {
            out.push((
                text(field(&plugin.entry, "origin")),
                plugin.root.join(relative),
            ));
        }
    }
    Ok(out)
}

pub fn sources(root: &Path) -> Result<Vec<Value>> {
    let mut out = read_sources(
        &yaml(&config_path(root), "work/live-sources.yaml")?,
        "work/live-sources.yaml",
    )?
    .into_iter()
    .map(|mut row| {
        row["origin"] = json!("user");
        row
    })
    .collect::<Vec<_>>();
    let mut seen = out
        .iter()
        .map(|row| text(field(row, "name")))
        .collect::<BTreeSet<_>>();
    for (origin, path) in plugin_files("sources")? {
        for mut row in read_sources(
            &yaml(&path, &format!("{origin} live sources"))?,
            &format!("{origin} live sources"),
        )? {
            let name = text(field(&row, "name"));
            if seen.insert(name) {
                row["origin"] = json!(origin);
                out.push(row);
            }
        }
    }
    Ok(out)
}

pub fn roster(root: &Path) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for (_origin, path) in plugin_files("team")? {
        let data = yaml(&path, "plugin roster").unwrap_or_else(|_| json!({}));
        out.extend(
            field(&data, "team")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
    }
    let own = careerbrew_projects::team::load(root)?;
    out.extend(own);
    Ok(out)
}

pub fn save_sources(root: &Path, text_: &str) -> Result<Vec<Value>> {
    let data = careerbrew_vault::json_yaml::yaml_read(text_)
        .map(|value| careerbrew_setup::value::from_canon(&value))
        .map_err(|_| live_error("the source config is not YAML or JSON (fix: sources: [{name, kind, adapter, ...}])"))?;
    let rows = read_sources(&data, "the source config")?;
    if rows.is_empty() {
        return Err(live_error("the source config has no sources (fix: sources: [{name: chat, kind: chat, adapter: file, inbox: ...}])"));
    }
    write_yaml(
        root,
        "work/live-sources.yaml",
        &json!({"sources": rows.clone()}),
    )?;
    Ok(rows)
}

pub fn state(root: &Path) -> Result<Value> {
    let mut state = read_json(root, "work/live/state.json", json!({}))?;
    for (key, value) in [
        ("running", json!(false)),
        ("cursors", json!({})),
        ("seen", json!({})),
        ("cycles", json!(0)),
        ("followed", json!([])),
        ("attached", json!({})),
        ("validated", json!({})),
    ] {
        if state.get(key).is_none() {
            state[key] = value;
        }
    }
    Ok(state)
}

pub fn save_state(root: &Path, state: &Value) -> Result<()> {
    write_json(root, "work/live/state.json", state)
}

pub struct CycleLock {
    file: File,
}

impl CycleLock {
    pub fn new(root: &Path) -> Result<Self> {
        let path = root.join("work/live/.cycle.lock");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().append(true).create(true).open(path)?;
        file.try_lock().map_err(|_| {
            live_error(
                "another live cycle is running (wait for it to finish, then run the next one)",
            )
        })?;
        Ok(Self { file })
    }
}

impl Drop for CycleLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub fn start(root: &Path, text_: &str) -> Result<Value> {
    if !text_.is_empty() {
        save_sources(root, text_)?;
    }
    let sources = sources(root)?;
    if sources.is_empty() {
        return Err(live_error("no live sources (fix: careerbrew work live start --sources <file.yaml>, or install a plugin with live sources)"));
    }
    let mut state = state(root)?;
    state["running"] = json!(true);
    state["started_at"] = json!(live_stamp());
    save_state(root, &state)?;
    emit(
        root,
        "work.live.start",
        vec![
            ("status", Canon::Str("ok".into())),
            ("sources", Canon::Int(sources.len().into())),
            (
                "required",
                Canon::Int(
                    sources
                        .iter()
                        .filter(|row| truthy(field(row, "required")))
                        .count()
                        .into(),
                ),
            ),
        ],
    )?;
    let cursors = field(&state, "cursors")
        .cloned()
        .unwrap_or_else(|| json!({}));
    Ok(json!({"sources": sources.into_iter().map(|mut row| {
        let name = text(field(&row, "name"));
        row["since"] = json!(text(cursors.get(&name)));
        row
    }).collect::<Vec<_>>()}))
}

pub fn stop(root: &Path) -> Result<Value> {
    let mut state = state(root)?;
    let was = truthy(field(&state, "running"));
    state["running"] = json!(false);
    state["stopped_at"] = json!(live_stamp());
    save_state(root, &state)?;
    emit(
        root,
        "work.live.stop",
        vec![
            ("status", Canon::Str("ok".into())),
            ("was_running", Canon::Bool(was)),
        ],
    )?;
    Ok(json!({"was_running": was, "cycles": state["cycles"].clone()}))
}

pub fn identity(root: &Path, source: &Value) -> Result<String> {
    let mut bound = source.clone();
    bound["max_age_minutes"] = json!(source
        .get("max_age_minutes")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| setting_i64(root, "live_source_max_age_minutes", 15).unwrap_or(15)));
    if text(field(&bound, "adapter")) == "file" {
        let inbox = text(field(&bound, "inbox"));
        let path = PathBuf::from(shellexpand(&inbox));
        bound["inbox"] = json!(path.canonicalize().unwrap_or(path).to_string_lossy());
    }
    json_digest(&bound)
}

fn shellexpand(value: &str) -> String {
    if value == "~" || value.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}{}", PathBuf::from(home).to_string_lossy(), &value[1..]);
        }
    }
    value.to_owned()
}

fn observe_clock(state: &mut Value, at: DateTime<Utc>) -> bool {
    let previous = state
        .get("checked_at")
        .and_then(Value::as_str)
        .and_then(crate::parse_utc);
    if state.get("checked_at").is_some() && previous.is_none_or(|previous| at < previous) {
        return false;
    }
    state["checked_at"] = json!(at.to_rfc3339_opts(SecondsFormat::Secs, false));
    true
}

#[allow(clippy::too_many_arguments)]
fn source_health(
    root: &Path,
    state: &mut Value,
    source: &Value,
    row: &mut Value,
    acquired: Option<f64>,
    at: DateTime<Utc>,
    accept: bool,
    clock_valid: bool,
) -> Result<()> {
    let limit = source
        .get("max_age_minutes")
        .and_then(Value::as_i64)
        .unwrap_or(setting_i64(root, "live_source_max_age_minutes", 15)?);
    if !(1..=10080).contains(&limit) {
        return Err(live_error(
            "live_source_max_age_minutes must be an integer from 1 to 10080",
        ));
    }
    row["health"] = json!("unknown");
    row["max_age_minutes"] = json!(limit);
    row["checked_at"] = json!(at.to_rfc3339_opts(SecondsFormat::Secs, false));
    row["acquired_at"] = json!("");
    row["age_seconds"] = Value::Null;
    if !text(field(row, "error")).is_empty() {
        row["health"] = json!("error");
        return Ok(());
    }
    if text(field(source, "adapter")) == "file" && acquired.is_none() {
        row["health"] = json!("missing");
        return Ok(());
    }
    let acquisition = acquired
        .and_then(|seconds| {
            DateTime::from_timestamp(
                seconds.trunc() as i64,
                (seconds.fract().abs() * 1_000_000_000.0) as u32,
            )
        })
        .unwrap_or(at);
    let age = (at - acquisition).num_milliseconds() as f64 / 1000.0;
    row["acquired_at"] = json!(acquisition.to_rfc3339_opts(SecondsFormat::Secs, false));
    row["age_seconds"] = json!(age);
    if age < 0.0 || !clock_valid {
        return Ok(());
    }
    let name = text(field(source, "name"));
    let identity = identity(root, source)?;
    if accept && truthy(field(row, "stored")) {
        state["validated"][&name] = json!({"identity": identity, "fingerprint": field(row, "fingerprint").cloned().unwrap_or(Value::Null), "acquired_at": acquisition.to_rfc3339_opts(SecondsFormat::Secs, false), "accepted_at": at.to_rfc3339_opts(SecondsFormat::Secs, false)});
    }
    let receipt = field(&state["validated"], &name)
        .cloned()
        .unwrap_or_else(|| json!({}));
    if text(field(&receipt, "identity")) != identity
        || field(&receipt, "fingerprint") != field(row, "fingerprint")
        || text(field(&receipt, "acquired_at"))
            != acquisition.to_rfc3339_opts(SecondsFormat::Secs, false)
    {
        return Ok(());
    }
    row["health"] = json!(if age <= (limit * 60) as f64 {
        "healthy"
    } else {
        "stale"
    });
    Ok(())
}

fn reply_data(text_: &str) -> Result<Value> {
    let mut text_ = text_.to_owned();
    for _ in 0..8 {
        let data = match serde_json::from_str::<Value>(&text_) {
            Ok(data) => data,
            Err(_) => {
                let rows = text_.lines().filter(|line| !line.trim().is_empty()).map(serde_json::from_str::<Value>).collect::<std::result::Result<Vec<_>, _>>().map_err(|_| live_error("the inbox is not JSON (fix: save the tool's reply exactly as returned)"))?;
                if rows.is_empty() {
                    return Err(live_error(
                        "the inbox is empty (fix: save a supported quiet reply)",
                    ));
                }
                Value::Array(rows)
            }
        };
        if data.is_object()
            && (field(&data, "ok") == Some(&json!(false))
                || field(&data, "isError") == Some(&json!(true))
                || truthy(field(&data, "error")))
        {
            return Err(live_error(
                "the inbox reports a tool error (fix: pull the source successfully again)",
            ));
        }
        if let Some(blocks) = data.as_array().filter(|rows| {
            !rows.is_empty()
                && rows
                    .iter()
                    .all(|row| matches!(text(field(row, "type")).as_str(), "text" | "input_text"))
        }) {
            text_ = blocks
                .iter()
                .map(|row| text(field(row, "text")))
                .collect::<Vec<_>>()
                .join("\n");
            continue;
        }
        return Ok(data);
    }
    Err(live_error(
        "the inbox has too many nested text wrappers (fix: save the tool reply directly)",
    ))
}

fn candidates(data: &Value) -> Vec<Value> {
    match data {
        Value::Array(rows) => rows.iter().filter(|row| row.is_object()).cloned().collect(),
        Value::Object(object) => {
            let lists = object
                .values()
                .filter_map(Value::as_array)
                .filter(|rows| rows.iter().any(Value::is_object))
                .collect::<Vec<_>>();
            if lists.is_empty() {
                vec![data.clone()]
            } else {
                lists
                    .into_iter()
                    .flatten()
                    .filter(|value| value.is_object())
                    .cloned()
                    .collect()
            }
        }
        _ => Vec::new(),
    }
}

fn file_stamp(path: &Path) -> Result<Vec<u64>> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path)?;
    Ok(vec![
        metadata.dev(),
        metadata.ino(),
        metadata.size(),
        (metadata.mtime() as u64) * 1_000_000_000 + metadata.mtime_nsec() as u64,
        (metadata.ctime() as u64) * 1_000_000_000 + metadata.ctime_nsec() as u64,
    ])
}

#[allow(clippy::type_complexity)]
fn read_inbox(
    source: &Value,
    seen: Option<&str>,
) -> Result<(Option<Vec<Value>>, Option<String>, usize, Option<f64>)> {
    let path = PathBuf::from(shellexpand(&text(field(source, "inbox"))));
    if !path.is_file() {
        return Ok((None, None, 0, None));
    }
    let stamp = file_stamp(&path)?;
    let payload = std::fs::read(&path)?;
    if file_stamp(&path)? != stamp {
        return Err(live_error(
            "the inbox changed while reading (fix: finish saving the reply, then run the cycle)",
        ));
    }
    let mut fingerprint_input = serde_json::to_vec(&stamp)?;
    fingerprint_input.extend(&payload);
    let fingerprint = format!("{:x}", sha2::Sha256::digest(&fingerprint_input));
    let data = reply_data(
        std::str::from_utf8(&payload).map_err(|_| live_error("the inbox is not UTF-8"))?,
    )?;
    let source_name = text(field(source, "name"));
    let raw = feed::raw_events(&data, &source_name)?;
    let (mut items, skipped) = if let Some((items, skipped, _)) =
        raw.filter(|(items, skipped, _)| !items.is_empty() || *skipped == 0)
    {
        (items, skipped)
    } else {
        let mut items = Vec::new();
        let mut skipped = 0;
        for object in candidates(&data) {
            if let Some(event) = transcript::event(&object, &source_name)? {
                items.push(event);
            } else {
                skipped += 1;
            }
        }
        if items.is_empty() && data != json!([]) {
            return Err(live_error(
                "the inbox has no usable events (fix: save a supported quiet reply)",
            ));
        }
        (items, skipped)
    };
    let acquired = Some(stamp[3] as f64 / 1_000_000_000.0);
    if seen == Some(fingerprint.as_str()) {
        return Ok((None, Some(fingerprint), skipped, acquired));
    }
    Ok((
        Some(std::mem::take(&mut items)),
        Some(fingerprint),
        skipped,
        acquired,
    ))
}

fn read_connector(root: &Path, source: &Value) -> Result<Vec<Value>> {
    let connector = text(field(source, "connector"));
    if !["slack", "pagerduty", "outlook"].contains(&connector.as_str()) {
        return Err(live_error(format!(
            "connector {connector} has no feed reader (feed readers: slack, pagerduty, outlook)"
        )));
    }
    let fixtures = std::env::var_os("CAREERBREW_HTTP_FIXTURES")
        .map(PathBuf::from)
        .ok_or_else(|| {
            live_error("recorded HTTP fixtures are required for the synthetic connector adapter")
        })?;
    let cfg = careerbrew_feed::sources::inventory(root)?
        .into_iter()
        .find(|row| text(field(row, "kind")) == connector)
        .ok_or_else(|| {
            live_error(format!(
                "connector {connector} is not connected (fix: careerbrew connect {connector})"
            ))
        })?;
    Ok(careerbrew_feed::sources::read(&fixtures, &cfg)?
        .into_iter()
        .map(|mut row| {
            row["source"] = json!(text(field(source, "name")));
            row
        })
        .collect())
}

pub fn reaches_pm(row: &Value) -> bool {
    truthy(field(row, "evidence")) && !truthy(field(row, "personal"))
}

pub fn verdict(row: &Value, open_ids: &BTreeSet<String>) -> String {
    if truthy(field(row, "personal")) {
        return "personal".into();
    }
    match text(field(row, "triage")).as_str() {
        "act" => {
            if open_ids.contains(&text(field(row, "id"))) {
                "answer".into()
            } else {
                "done".into()
            }
        }
        "fyi" => "read".into(),
        "noise" => "skip".into(),
        _ => {
            if reaches_pm(row) {
                "evidence".into()
            } else {
                "log".into()
            }
        }
    }
}

fn actions(
    not_stored: &[String],
    counts: &Value,
    research: usize,
    followups: &[Value],
) -> Vec<String> {
    let mut out = not_stored
        .iter()
        .map(|source| format!("pull {source} again, then run: careerbrew work live cycle"))
        .collect::<Vec<_>>();
    let acts = followups
        .iter()
        .flat_map(|row| {
            row["actions"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|value| text(Some(value)))
        })
        .collect::<Vec<_>>();
    let replies = acts
        .iter()
        .filter(|action| action.starts_with("reply draft saved"))
        .count();
    let reviews = acts
        .iter()
        .filter(|action| action.starts_with("review notes drafted"))
        .count();
    if replies > 0 || reviews > 0 {
        out.push(format!("{replies} reply draft(s) and {reviews} review note(s) saved, never sent: read each in your terminal (careerbrew draft show <target>), rewrite it (careerbrew draft replace <target> --file <file>), then send it yourself"));
    }
    let opened = acts
        .iter()
        .filter(|action| action.starts_with("review session opened"))
        .count();
    if opened > 0 {
        out.push(format!("{opened} new review session(s): start a fresh reviewer for each (skill live-feed, Review sessions); it writes the review to a file and saves a reply draft, and must never post or send"));
    }
    if counts["answer"].as_i64().unwrap_or_default() > 0 {
        out.push(format!(
            "answer {} open ask(s): careerbrew work feed show --open --show (to your terminal)",
            counts["answer"].as_i64().unwrap_or_default()
        ));
    }
    if research > 0 {
        out.push(format!(
            "research {research} link(s): careerbrew work feed links --pending"
        ));
    }
    if counts["evidence"].as_i64().unwrap_or_default() > 0 {
        out.push(format!("{} item(s) reach the PM draft: careerbrew day send --via email (review before you send)", counts["evidence"].as_i64().unwrap_or_default()));
    }
    out
}

pub struct Provider;

impl ComponentProvider for Provider {
    fn declaration(&self, root: &Path) -> Result<Declaration> {
        let (_sources, required, identity) = health::declared(root)?;
        Ok(Declaration { required, identity })
    }

    fn complete(&self, root: &Path, required: &[String], batch: &Value) -> Result<()> {
        health::complete(root, required, batch)
    }

    fn required_gaps(&self, root: &Path) -> Result<Vec<String>> {
        let snapshot = health::snapshot(root)?;
        Ok(snapshot["sources"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| truthy(field(row, "required")) && text(field(row, "health")) != "healthy")
            .map(|row| text(field(row, "name")))
            .collect())
    }

    fn track_fresh(&self, _fresh: &[(Value, String)]) -> Result<()> {
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub fn mark_cycle(
    root: &Path,
    state: &mut Value,
    sources: &[Value],
    results: &BTreeMap<String, Value>,
    verdicts: &BTreeMap<String, String>,
    research: usize,
    followups: &[Value],
    goal_updates: &[Value],
    clock_valid: bool,
) -> Result<Value> {
    let checked_at = now_utc();
    let clock_valid = observe_clock(state, checked_at) && clock_valid;
    let mut final_results = BTreeMap::new();
    for source in sources {
        let name = text(field(source, "name"));
        let mut row = results[&name].clone();
        let acquired = row.get("_acquired").and_then(Value::as_f64);
        source_health(
            root,
            state,
            source,
            &mut row,
            acquired,
            checked_at,
            false,
            clock_valid,
        )?;
        final_results.insert(name, row);
    }
    finish_cycle(
        root,
        state,
        sources,
        &final_results,
        verdicts,
        research,
        followups,
        goal_updates,
        checked_at,
        clock_valid,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish_cycle(
    root: &Path,
    state: &mut Value,
    sources: &[Value],
    results: &BTreeMap<String, Value>,
    verdicts: &BTreeMap<String, String>,
    research: usize,
    followups: &[Value],
    goal_updates: &[Value],
    checked_at: DateTime<Utc>,
    _clock_valid: bool,
) -> Result<Value> {
    let not_stored = sources
        .iter()
        .filter(|source| {
            truthy(field(source, "required"))
                && text(field(&results[&text(field(source, "name"))], "health")) != "healthy"
        })
        .map(|source| text(field(source, "name")))
        .collect::<Vec<_>>();
    let degraded = sources
        .iter()
        .filter(|source| {
            !truthy(field(source, "required"))
                && text(field(&results[&text(field(source, "name"))], "health")) != "healthy"
        })
        .map(|source| text(field(source, "name")))
        .collect::<Vec<_>>();
    let counts = json!(VERDICTS
        .iter()
        .map(|verdict| (
            (*verdict).to_owned(),
            verdicts.values().filter(|value| *value == verdict).count()
        ))
        .collect::<BTreeMap<_, _>>());
    let by_name = sources
        .iter()
        .map(|source| (text(field(source, "name")), source))
        .collect::<BTreeMap<_, _>>();
    for (name, row) in results {
        if truthy(field(row, "stored")) {
            let cursor = text(field(row, "cursor"));
            if !cursor.is_empty() {
                let previous = text(state["cursors"].get(name));
                state["cursors"][name] = json!(previous.max(cursor));
            }
            if let Some(fingerprint) = field(row, "fingerprint").and_then(Value::as_str) {
                state["seen"][name] =
                    json!({"identity": identity(root, by_name[name])?, "fingerprint": fingerprint});
            }
        }
    }
    state["cycles"] = json!(state["cycles"].as_i64().unwrap_or_default() + 1);
    let source_records = results
        .iter()
        .map(|(name, row)| {
            let mut out = row.as_object().cloned().unwrap_or_default();
            out.remove("fingerprint");
            out.remove("_acquired");
            out.insert(
                "source_identity".into(),
                json!(identity(root, by_name[name]).unwrap_or_default()),
            );
            (name.clone(), Value::Object(out))
        })
        .collect::<Map<_, _>>();
    let rec = json!({
        "n": state["cycles"].clone(),
        "at": checked_at.to_rfc3339_opts(SecondsFormat::Secs, false),
        "status": if not_stored.is_empty() { "ok" } else { "failed" },
        "not_stored": not_stored,
        "degraded": degraded,
        "sources": source_records,
        "verdicts": verdicts,
        "counts": counts,
        "followups": followups,
        "goals": goal_updates,
        "actions": actions(&not_stored, &counts, research, followups),
        "review_sessions": followups.iter().flat_map(|row| row["actions"].as_array().into_iter().flatten()).filter(|action| text(Some(action)).starts_with("review session opened")).count(),
    });
    state["last"] = json!({"n": rec["n"].clone(), "at": rec["at"].clone(), "status": rec["status"].clone(), "not_stored": rec["not_stored"].clone(), "counts": rec["counts"].clone()});
    let fresh = crate::live::state(root)?;
    for key in ["running", "started_at", "stopped_at"] {
        if let Some(value) = fresh.get(key) {
            state[key] = value.clone();
        }
    }
    save_state(root, state)?;
    let mut rows = read_jsonl(root, "work/live/cycles.jsonl")?;
    rows.push(rec.clone());
    let start = rows.len().saturating_sub(KEEP_CYCLES);
    write_jsonl(root, "work/live/cycles.jsonl", &rows[start..])?;
    let stored = results
        .values()
        .map(|row| {
            row["new"].as_i64().unwrap_or_default() + row["updated"].as_i64().unwrap_or_default()
        })
        .sum::<i64>();
    emit(
        root,
        "work.live.cycle",
        vec![
            ("status", Canon::Str(text(field(&rec, "status")))),
            ("sources", Canon::Int(sources.len().into())),
            ("not_stored", Canon::Int(not_stored.len().into())),
            ("stored", Canon::Int(stored.into())),
            ("story", Canon::Str("us-97".into())),
        ],
    )?;
    Ok(rec)
}

pub fn standup(root: &Path, _state: &mut Value, _new_rows: &[Value]) -> Value {
    let saved = read_json(root, "work/standup.json", json!({})).unwrap_or_else(|_| json!({}));
    if field(&saved, "rows")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return json!({"status": "skipped", "reason": "no rows"});
    }
    // Standup due/publish belongs to rust-18. Its typed provider reports the
    // current fresh/no-op result without duplicating that pipeline here.
    json!({"status": "skipped", "reason": "fresh"})
}

pub fn standup_line(row: &Value) -> String {
    match text(field(row, "status")).as_str() {
        "published" => format!(
            "standup: {} rows published ({})",
            row["rows"].as_i64().unwrap_or_default(),
            text(field(row, "reason"))
        ),
        "failed" => format!(
            "standup: failed ({}); the cycle goes on",
            text(field(row, "error"))
        ),
        _ => format!("standup: skipped, {}", text(field(row, "reason"))),
    }
}

pub fn cycle(root: &Path, read_allowed: bool) -> Result<Value> {
    let _lock = CycleLock::new(root)?;
    let mut state = state(root)?;
    if !truthy(field(&state, "running")) {
        return Err(live_error(
            "the live feed is stopped (fix: careerbrew work live start)",
        ));
    }
    let sources = sources(root)?;
    let mut results = BTreeMap::new();
    let mut stored_ids = Vec::new();
    let mut clock_valid = observe_clock(&mut state, now_utc());
    for source in &sources {
        let name = text(field(source, "name"));
        let mut row = json!({"pulled": false, "stored": false, "items": 0, "new": 0, "updated": 0, "skipped": 0, "error": "", "unchanged": false, "validated_at": ""});
        let mut acquired = None;
        let mut validation_at = None;
        let attempt = (|| -> Result<()> {
            let mut items = if text(field(source, "adapter")) == "file" {
                let binding = state["seen"]
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let seen = (text(field(&binding, "identity")) == identity(root, source)?)
                    .then(|| text(field(&binding, "fingerprint")));
                let (items, fingerprint, skipped, stamp) = read_inbox(source, seen.as_deref())?;
                row["fingerprint"] = fingerprint.clone().map(Value::from).unwrap_or(Value::Null);
                row["unchanged"] = json!(items.is_none() && fingerprint.is_some());
                row["skipped"] = json!(skipped);
                acquired = stamp;
                items
            } else {
                Some(read_connector(root, source)?)
            };
            validation_at = Some(now_utc());
            clock_valid =
                observe_clock(&mut state, validation_at.expect("validation time")) && clock_valid;
            if items.is_some() || truthy(field(&row, "unchanged")) {
                row["validated_at"] = json!(validation_at
                    .expect("validation time")
                    .to_rfc3339_opts(SecondsFormat::Secs, false));
            }
            if text(field(source, "adapter")) == "connector" {
                acquired = validation_at.map(|at| at.timestamp_millis() as f64 / 1000.0);
            }
            if let Some(items) = items.take() {
                row["pulled"] = json!(true);
                row["items"] = json!(items.len());
                let got = if items.is_empty() {
                    json!({"new": 0, "updated": 0})
                } else {
                    feed::ingest(
                        root,
                        &items,
                        &text(field(source, "origin")),
                        &text(field(source, "kind")),
                        &[],
                        &Provider,
                    )?
                };
                row["new"] = got["new"].clone();
                row["updated"] = got["updated"].clone();
                row["stored"] = json!(true);
                stored_ids.extend(
                    items
                        .iter()
                        .map(|item| feed::clean(item, "event").map(|row| text(field(&row, "id"))))
                        .collect::<Result<Vec<_>>>()?,
                );
                row["cursor"] = json!(items
                    .iter()
                    .map(|item| feed::clean(item, "event").map(|row| text(field(&row, "at"))))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .max()
                    .unwrap_or_default());
            }
            Ok(())
        })();
        if let Err(error) = attempt {
            row["error"] = json!(careerbrew_policy::redact(&error.message, &[]));
            row["error_kind"] = json!(error.class);
            emit(
                root,
                "work.live.error",
                vec![
                    ("source_kind", Canon::Str(text(field(source, "kind")))),
                    ("error", Canon::Str(error.class.into())),
                    ("story", Canon::Str("us-97".into())),
                ],
            )?;
        }
        let validation_at = validation_at.unwrap_or_else(|| {
            let at = now_utc();
            clock_valid = observe_clock(&mut state, at) && clock_valid;
            at
        });
        row["_acquired"] = acquired.map(Value::from).unwrap_or(Value::Null);
        source_health(
            root,
            &mut state,
            source,
            &mut row,
            acquired,
            validation_at,
            true,
            clock_valid,
        )?;
        results.insert(name, row);
    }
    let rows = feed::load(root)?
        .into_iter()
        .map(|row| (text(field(&row, "id")), row))
        .collect::<BTreeMap<_, _>>();
    let mine = stored_ids
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|id| rows.get(&id).cloned())
        .collect::<Vec<_>>();
    let open_ids = feed::open_asks(
        root,
        Some(&rows.values().cloned().collect::<Vec<_>>()),
        true,
    )?
    .iter()
    .map(|row| text(field(row, "id")))
    .collect::<BTreeSet<_>>();
    let verdicts = mine
        .iter()
        .map(|row| (text(field(row, "id")), verdict(row, &open_ids)))
        .collect::<BTreeMap<_, _>>();
    let (followups, attached) = match listen::follow_up(
        root,
        &verdicts,
        &state["followed"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|value| text(Some(value)))
            .collect::<BTreeSet<_>>(),
        read_allowed,
    ) {
        Ok(result) => result,
        Err(error) => {
            emit(
                root,
                "work.live.error",
                vec![
                    ("source_kind", Canon::Str("listen".into())),
                    ("error", Canon::Str(error.class.into())),
                    ("story", Canon::Str("us-101".into())),
                ],
            )?;
            (Vec::new(), BTreeMap::new())
        }
    };
    state["followed"] = json!(state["followed"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(
            followups
                .iter()
                .map(|row| field(row, "id").unwrap_or(&Value::Null))
        )
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(listen::KEEP_FOLLOWED)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>());
    let old_attached = state["attached"].as_object().cloned().unwrap_or_default();
    let attached = old_attached
        .into_iter()
        .chain(
            attached
                .into_iter()
                .map(|(key, value)| (key, Value::String(value))),
        )
        .collect::<Vec<_>>();
    state["attached"] = json!(attached
        .into_iter()
        .rev()
        .take(listen::KEEP_FOLLOWED)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Map<_, _>>());
    let goal_updates = listen::goal_updates(root, &state["attached"]).unwrap_or_default();
    let fresh = feed::load(root)?
        .into_iter()
        .map(|row| (text(field(&row, "id")), row))
        .collect::<BTreeMap<_, _>>();
    let research = mine
        .iter()
        .map(|row| {
            let current = fresh.get(&text(field(row, "id"))).unwrap_or(row);
            if text(field(row, "triage")) == "noise" || truthy(field(row, "personal")) {
                0
            } else {
                feed::needs_research(current).len()
            }
        })
        .sum::<usize>();
    let published = standup(root, &mut state, &mine);
    let mut rec = mark_cycle(
        root,
        &mut state,
        &sources,
        &results,
        &verdicts,
        research,
        &followups,
        &goal_updates,
        clock_valid,
    )?;
    rec["standup"] = published;
    Ok(rec)
}

pub fn status(root: &Path) -> Result<Value> {
    let state = state(root)?;
    let sources = sources(root)?;
    let last = read_jsonl(root, "work/live/cycles.jsonl")?
        .last()
        .cloned()
        .unwrap_or_else(|| json!({}));
    let per = field(&last, "sources")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let cursors = field(&state, "cursors")
        .cloned()
        .unwrap_or_else(|| json!({}));
    Ok(json!({
        "running": state["running"].clone(),
        "started_at": text(field(&state, "started_at")),
        "cycles": state["cycles"].clone(),
        "last": field(&state, "last").cloned().unwrap_or_else(|| json!({})),
        "actions": field(&last, "actions").cloned().unwrap_or_else(|| json!([])),
        "health": health::snapshot(root)?,
        "sources": sources.into_iter().map(|source| {
            let name = text(field(&source, "name"));
            json!({"name": name.clone(), "kind": field(&source, "kind").cloned().unwrap_or(Value::Null), "required": field(&source, "required").cloned().unwrap_or(Value::Null), "adapter": field(&source, "adapter").cloned().unwrap_or(Value::Null), "origin": field(&source, "origin").cloned().unwrap_or(Value::Null), "since": text(cursors.get(&name)), "last_stored": truthy(per.get(&name).and_then(|row| field(row, "stored")))})
        }).collect::<Vec<_>>(),
    }))
}

pub fn cycle_text(rec: &Value) -> Result<String> {
    compact(&typed(rec))
}
