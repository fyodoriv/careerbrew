//! Synthetic rust-13 workload over native feed transactions.
//!
//! The provider below is a typed test double for the not-yet-native health/live
//! completion boundary. It is compiled into the synthetic candidate only.

use crate::{
    cli, feed, field, read_json, read_jsonl, read_yaml, sources, text, transcript, write_json,
    write_yaml, ComponentProvider, Declaration, FeedError, Result,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const CASES: &[&str] = &[
    "source-ingestion",
    "edited-upsert",
    "raw-replies",
    "claude-transcript",
    "codex-transcript",
    "containment-since",
    "asks-cycle",
    "team-feed",
    "local-time-and-nan",
    "source-failures",
    "source-inventory",
    "partial-transport",
];
const MAP: &str = "mcp__chat__*=chat, mcp__tickets__*=tickets, mcp__mail__*=mail";
const TOKEN: &str = "env:CB_TEST_TOKEN";

#[derive(Default)]
struct SyntheticComponents;

fn instant(value: Option<&Value>) -> Option<DateTime<Utc>> {
    value
        .and_then(|value| value.as_str())
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

impl ComponentProvider for SyntheticComponents {
    fn declaration(&self, root: &Path) -> Result<Declaration> {
        let settings = read_yaml(root, "settings.yaml", json!({}))?;
        let tools = field(&settings, "feed_tool_sources")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let required = tools
            .iter()
            .filter_map(|tool| {
                text(Some(tool))
                    .split_once('=')
                    .map(|(_, source)| source.to_owned())
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let binding =
            json!({"tools": tools, "sources": [], "required": required, "max_age_minutes": 15});
        let identity = format!(
            "{:x}",
            Sha256::digest(serde_json::to_string(&binding)?.as_bytes())
        );
        Ok(Declaration { required, identity })
    }

    fn complete(&self, root: &Path, required: &[String], batch: &Value) -> Result<()> {
        let declaration = self.declaration(root)?;
        let old = read_json(root, "work/feed/direct-cycle.json", json!({}))?;
        let binding_matches = text(field(batch, "identity")) == declaration.identity;
        let received = if binding_matches {
            field(batch, "acquired")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default()
        } else {
            Map::new()
        };
        let stored = field(batch, "stored")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|value| text(Some(value)))
            .collect::<BTreeSet<_>>();
        let missing = required
            .iter()
            .filter(|source| !stored.contains(*source))
            .collect::<Vec<_>>();
        let healthy = !declaration.required.is_empty()
            && declaration
                .required
                .iter()
                .all(|source| stored.contains(source))
            && missing.is_empty()
            && !crate::truthy(field(batch, "errors"))
            && !field(batch, "failed_sources")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|value| text(Some(value)))
                .any(|source| required.contains(&source));
        let at = crate::now_utc();
        let valid_clock = declaration
            .required
            .iter()
            .all(|source| instant(received.get(source)).is_some_and(|stamp| stamp <= at))
            && (old.as_object().is_none_or(Map::is_empty)
                || instant(field(&old, "checked_at")).is_some_and(|stamp| stamp <= at));
        let status = if !declaration.required.is_empty() && !binding_matches {
            "unknown"
        } else if !healthy {
            "failed"
        } else if !valid_clock {
            "unknown"
        } else {
            "ok"
        };
        let previous = instant(field(&old, "checked_at"));
        let checked = previous.map(|previous| previous.max(at)).unwrap_or(at);
        let acquired = declaration
            .required
            .iter()
            .map(|source| {
                (
                    source.clone(),
                    received.get(source).cloned().unwrap_or(Value::Null),
                )
            })
            .collect::<Map<_, _>>();
        let record = json!({
            "cycles": old.get("cycles").and_then(Value::as_i64).unwrap_or_default() + 1,
            "identity": declaration.identity,
            "required": declaration.required,
            "stored": stored.into_iter().collect::<Vec<_>>(),
            "status": status,
            "checked_at": checked.to_rfc3339_opts(SecondsFormat::Secs, false),
            "acquired": acquired,
            "errors": crate::truthy(field(batch, "errors")),
            "failed_sources": field(batch, "failed_sources").cloned().unwrap_or_else(|| json!([])),
        });
        write_json(root, "work/feed/direct-cycle.json", &record)?;
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
        write_json(root, "work/live/state.json", &state)
    }

    fn required_gaps(&self, root: &Path) -> Result<Vec<String>> {
        let declaration = self.declaration(root)?;
        if declaration.required.is_empty() {
            return Ok(Vec::new());
        }
        let direct = read_json(root, "work/feed/direct-cycle.json", json!({}))?;
        if text(field(&direct, "status")) == "ok"
            && field(&direct, "identity") == Some(&json!(declaration.identity))
        {
            return Ok(Vec::new());
        }
        Ok(declaration.required)
    }

    fn track_fresh(&self, _fresh: &[(Value, String)]) -> Result<()> {
        Ok(())
    }
}

struct Workspace {
    base: PathBuf,
    root: PathBuf,
}

impl Workspace {
    fn new(base: &Path, name: &str) -> Result<Self> {
        let base = base.join(name);
        let root = base.join("data");
        std::fs::create_dir_all(&root)?;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        std::fs::create_dir_all(base.join("home"))?;
        std::env::set_var("HOME", base.join("home"));
        std::env::set_var("CAREERBREW_HOME", &root);
        std::env::set_var("CAREERBREW_KEY_FILE", base.join("keys/backup.key"));
        std::env::set_var("CAREERBREW_EVENTS", "off");
        std::env::set_var("CAREERBREW_ENCRYPT", "on");
        std::env::set_var("CAREERBREW_NOW", "2026-10-21T16:00:00+00:00");
        std::env::set_var("CAREERBREW_TEST_OWNER", "1");
        std::env::set_var("CAREERBREW_HTTP_FIXTURES", fixture("http/feed"));
        std::env::set_var("CB_TEST_TOKEN", "synthetic-not-reported");
        std::env::set_var("CB_T", "synthetic");
        std::env::set_var("TZ", "UTC");
        Ok(Self { base, root })
    }
}

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(path)
}

fn error(result: Result<()>) -> String {
    result
        .err()
        .map(|error| error.class)
        .unwrap_or("accepted")
        .to_owned()
}

fn connectors(root: &Path, kinds: &[&str]) -> Result<()> {
    let rows = kinds
        .iter()
        .map(|kind| json!({"kind": kind, "token": TOKEN}))
        .collect::<Vec<_>>();
    write_json(root, "work/connectors.yaml", &json!({"connectors": rows}))
}

fn projection(row: &Value) -> Value {
    let mut out = Map::new();
    for key in [
        "id",
        "source",
        "kind",
        "at",
        "actor",
        "actor_id",
        "to_me",
        "channel",
        "from",
        "status",
        "importance",
        "thread",
        "triage",
        "rule",
        "personal",
        "personal_by",
        "team",
        "links",
        "signal",
        "evidence",
        "evidence_rule",
        "ask_mark",
    ] {
        if let Some(value) = row.get(key) {
            out.insert(key.into(), value.clone());
        }
    }
    Value::Object(out)
}

fn source_ingestion(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "source-ingestion")?;
    let root = &workspace.root;
    connectors(root, &["slack", "pagerduty", "outlook"])?;
    let components = SyntheticComponents;
    let first = sources::pull(root, &components)?;
    let repeated = sources::pull(root, &components)?;
    let rows = feed::load(root)?
        .into_iter()
        .map(|row| (text(field(&row, "id")), row))
        .collect::<BTreeMap<_, _>>();
    let dm = projection(&rows["slack:D0ALEX:1792591200.000100"]);
    let original = crate::read_text(root, "work/feed/originals.jsonl")?;
    let analysis = crate::read_text(root, "work/feed/events.jsonl")?;
    let sealed = std::fs::read(feed::originals_path(root))?.starts_with(b"CBV1\n")
        && std::fs::read(feed::events_path(root))?.starts_with(b"CBV1\n");
    Ok(json!({
        "first": first, "repeated": repeated, "count": rows.len(),
        "triages": rows.iter().map(|(id, row)| (id.clone(), row["triage"].clone())).collect::<Map<_, _>>(),
        "dm": dm,
        "open": feed::open_asks(root, None, false)?.iter().map(|row| text(field(row, "id"))).collect::<Vec<_>>(),
        "summary": feed::summary(root, &rows.values().cloned().collect::<Vec<_>>())?,
        "sealed": sealed,
        "original_kept": original.contains("can you review the rollout plan today?"),
        "analysis_separated": !analysis.contains("can you review the rollout plan today?") && !analysis.contains("PROJECTZEBRA"),
        "lock": feed::feed_dir(root).join(".lock").is_file(),
    }))
}

fn edited_upsert(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "edited-upsert")?;
    let root = &workspace.root;
    if std::env::var("CAREERBREW_RUST13_PLANT_WRITE").as_deref() == Ok("extra") {
        std::fs::write(root.join("planted-extra.txt"), "synthetic")?;
    }
    let components = SyntheticComponents;
    let item = json!({
        "id": "slack:C1:1", "source": "slack", "kind": "mention", "at": "2026-10-21T12:00:00Z",
        "title": "Review the rollout", "actor": "other", "to_me": true, "channel": "eng",
        "thread": "slack:C1:1", "text": "spec <https://docs.example.com/d/1|spec>",
    });
    let first = feed::add_lines(root, &serde_json::to_string(&item)?, "", "", &components)?;
    feed::mark_ask(root, "slack:C1:1", "done")?;
    feed::research(
        root,
        "slack:C1:1",
        &json!([{"url": "https://docs.example.com/d/1", "summary": "approved spec"}]),
    )?;
    let edited = json!({
        "id": "slack:C1:1", "source": "slack", "kind": "mention", "at": "2026-10-21T12:00:00Z",
        "title": "Review the edited rollout", "actor": "other", "to_me": true, "channel": "eng",
        "thread": "slack:C1:1", "text": "edited <https://docs.example.com/d/1|spec>", "original": {"edited": true},
    });
    let second = feed::add_lines(root, &serde_json::to_string(&edited)?, "", "", &components)?;
    let current = projection(&feed::load(root)?[0]);
    let originals = feed::originals(root)?;
    let original_edited = originals["slack:C1:1"]["original"] == json!({"edited": true});
    let repeated = feed::add_lines(root, &serde_json::to_string(&edited)?, "", "", &components)?;
    let retriaged = feed::retriage(root)?;
    let result = json!({
        "first": first, "second": second, "repeated": repeated, "retriaged": retriaged, "row": current,
        "title_edited": feed::load(root)?[0]["title"] == json!("Review the edited rollout"),
        "research_kept": feed::load(root)?[0].get("research") == Some(&json!([{"url": "https://docs.example.com/d/1", "opened": true, "summary": "approved spec"}])),
        "original_edited": original_edited, "one_row": feed::load(root)?.len() == 1,
        "open": feed::open_asks(root, None, false)?,
    });
    if std::env::var("CAREERBREW_RUST13_PLANT_WRITE").as_deref() == Ok("missing") {
        std::fs::remove_file(feed::originals_path(root))?;
    }
    if std::env::var("CAREERBREW_RUST13_PLANT_WRITE").as_deref() == Ok("operation-duplicate") {
        let rows = read_jsonl(root, "work/feed/events.jsonl")?;
        crate::write_jsonl(root, "work/feed/events.jsonl", &rows)?;
    }
    Ok(result)
}

fn raw_replies(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "raw-replies")?;
    let root = &workspace.root;
    if std::env::var("CAREERBREW_RUST13_PLANT_WRITE").as_deref() == Ok("duplicate") {
        // Rewrite the prior phase's file: a before/after trace cannot see a
        // second same-byte rewrite inside the phase that created the file.
        let path = base.join("edited-upsert/data/work/feed/events.jsonl");
        let bytes = std::fs::read(&path)?;
        std::thread::sleep(std::time::Duration::from_millis(2));
        std::fs::write(path, bytes)?;
    }
    let components = SyntheticComponents;
    let tickets = json!({"total": 3, "tickets": [
        {"key": "PROJ-7", "summary": "Fix the login bug", "description": "Steps CANARYTICKET", "status": "In Progress", "updated": "2026-10-21T12:00:00Z", "url": "https://tickets.example.com/browse/PROJ-7"},
        {"key": "PROJ-8", "fields": {"summary": "Write the API docs", "status": {"name": "To Do"}, "updated": "2026-10-21T12:30:00Z", "project": {"name": "Docs"}}},
        {"key": "PROJ-9", "summary": "no time on this one"},
    ]});
    let mail = json!({"messages": [
        {"id": "AAMk-1", "subject": "Quarterly plan", "receivedDateTime": "2026-10-21T11:00:00Z", "from": {"emailAddress": {"name": "Sam Lee", "address": "sam@example.com"}}, "importance": "high", "bodyPreview": "CANARYMAIL body"},
        {"id": "AAMk-2", "subject": "", "receivedDateTime": "2026-10-21T11:30:00Z", "flag": {"flagStatus": "flagged"}},
    ]});
    let calendar = json!({"events": [
        {"id": "abc", "start": {"dateTime": "2026-10-29T15:00:00Z"}, "summary": "Design review", "htmlLink": "https://cal.example.com/abc"},
        {"start": {"dateTime": "2026-10-29T16:00:00Z"}, "summary": "No id"},
        {"id": "nostart", "summary": "No start"},
        {"id": "allday", "start": {"date": "2026-10-30"}, "summary": "Offsite"},
    ]});
    let added_tickets = feed::add_lines(
        root,
        &serde_json::to_string(&tickets)?,
        "",
        "jira",
        &components,
    )?;
    let added_mail = feed::add_lines(root, &serde_json::to_string(&mail)?, "", "", &components)?;
    let added_calendar = feed::add_lines(
        root,
        &serde_json::to_string(&calendar)?,
        "",
        "calendar",
        &components,
    )?;
    let quiet = feed::add_lines(
        root,
        &serde_json::to_string(&json!({"messages": []}))?,
        "",
        "chat",
        &components,
    )?;
    let rows = feed::load(root)?
        .iter()
        .map(|row| (text(field(row, "id")), projection(row)))
        .collect::<Map<_, _>>();
    let originals = feed::originals(root)?;
    let not_calendar = feed::raw_events(
        &json!({"events": [{"id": "pg-1", "at": "2026-10-29T15:00:00Z", "title": "Disk full"}]}),
        "pager",
    )?;
    Ok(json!({
        "tickets": added_tickets, "mail": added_mail, "calendar": added_calendar, "quiet": quiet, "rows": rows,
        "ticket_original": originals["jira:PROJ-7:2026-10-21T12:00:00Z:In Progress"]["original"]["description"].as_str().is_some_and(|value| value.ends_with("CANARYTICKET")),
        "mail_original": originals["mail:AAMk-1"]["text"] == json!("CANARYMAIL body"),
        "analysis_separated": feed::load(root)?.iter().all(|row| !row.to_string().contains("CANARY")),
        "not_calendar": not_calendar.is_none(),
        "bad_source": error(feed::add_lines(root, &serde_json::to_string(&tickets)?, "", "Bad Source", &components).map(|_| ())),
    }))
}

fn copy_dir(source: &Path, target: &Path) -> Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn session(workspace: &Workspace, name: &str) -> Result<PathBuf> {
    let root = workspace.base.join(name);
    copy_dir(&fixture("transcripts/feed-cycle"), &root.join("feed-cycle"))?;
    let path = root.join("feed-cycle.jsonl");
    let text = std::fs::read_to_string(fixture("transcripts/feed-cycle.jsonl"))?
        .replace("@DIR@", &root.to_string_lossy());
    std::fs::write(&path, text)?;
    careerbrew_vault::lock::with_lock(&workspace.root.join("settings.yaml"), || {
        write_yaml(
            &workspace.root,
            "settings.yaml",
            &json!({"feed_tool_sources": MAP.split(", ").collect::<Vec<_>>()}),
        )
    })??;
    Ok(path)
}

fn claude_transcript(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "claude-transcript")?;
    let root = &workspace.root;
    let session = session(&workspace, "session")?;
    let components = SyntheticComponents;
    let first = transcript::store(root, &session, "", &components)?;
    let repeated = transcript::store(root, &session, "", &components)?;
    let cli_result = cli::transcript_command(root, &session, "", &components);
    let rows = feed::load(root)?
        .iter()
        .map(|item| (text(field(item, "id")), projection(item)))
        .collect::<Map<_, _>>();
    let originals = feed::originals(root)?;
    Ok(json!({
        "first": first, "repeated": repeated, "cli": cli_result, "rows": rows,
        "original_padded": originals["tickets:PROJ-7"]["original"]["pad"] == json!("x".repeat(2000)),
        "sealed": std::fs::read(feed::originals_path(root))?.starts_with(b"CBV1\n"),
        "analysis_separated": !crate::read_text(root, "work/feed/events.jsonl")?.contains("CANARYCHAT"),
    }))
}

fn codex_transcript(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "codex-transcript")?;
    let root = &workspace.root;
    let session = session(&workspace, "source")?;
    let components = SyntheticComponents;
    let expected = transcript::store(root, &session, "", &components)?;
    let mut rows = Vec::new();
    for line in std::fs::read_to_string(&session)?.lines() {
        let raw: Value = serde_json::from_str(line)?;
        let content = field(&raw, "message")
            .and_then(|message| field(message, "content"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for block in content {
            let payload = match text(field(&block, "type")).as_str() {
                "tool_use" => {
                    json!({"type": "function_call", "name": field(&block, "name").cloned().unwrap_or(Value::Null), "call_id": field(&block, "id").cloned().unwrap_or(Value::Null), "arguments": "{}"})
                }
                "tool_result" => {
                    json!({"type": "function_call_output", "call_id": field(&block, "tool_use_id").cloned().unwrap_or(Value::Null), "output": field(&block, "content").cloned().unwrap_or(Value::Null), "is_error": field(&block, "is_error").cloned().unwrap_or(json!(false))})
                }
                _ => continue,
            };
            rows.push(json!({"type": "response_item", "timestamp": field(&raw, "timestamp").cloned().unwrap_or(Value::Null), "payload": payload}));
        }
    }
    let native = session.with_file_name("native.jsonl");
    std::fs::write(
        &native,
        rows.iter()
            .map(|row| row.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )?;
    let response_items = transcript::store(root, &native, "", &components)?;
    let quiet = workspace.base.join("events.jsonl");
    let event_rows = [
        json!({"type": "event_msg", "timestamp": "2026-10-21T15:59:00Z", "payload": {"type": "mcp_tool_call_end", "call_id": "q", "invocation": {"server": "mail", "tool": "search", "arguments": {}}, "result": {"Ok": {"content": [{"type": "text", "text": "{\"messages\": []}"}]}}}}),
        json!({"type": "event_msg", "timestamp": "2026-10-21T15:59:00Z", "payload": {"type": "mcp_tool_call_end", "call_id": "e", "invocation": {"server": "tickets", "tool": "search", "arguments": {}}, "result": {"Err": "CANARY-NATIVE-ERROR"}}}),
    ];
    std::fs::write(
        &quiet,
        event_rows
            .iter()
            .map(|row| row.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )?;
    let end_events = transcript::store(root, &quiet, "", &components)?;
    let malformed = workspace.base.join("malformed.jsonl");
    std::fs::write(&malformed, "{\"type\":\"future_format\",\"payload\":{}}\n")?;
    Ok(json!({
        "expected": expected, "response_items": response_items,
        "same_facts": response_items["stored"] == expected["stored"] && response_items["replies"] == expected["replies"] && response_items["errors"] == expected["errors"],
        "end_events": end_events,
        "unsupported": error(transcript::store(root, &malformed, "", &components).map(|_| ())),
    }))
}

fn containment_since(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "containment-since")?;
    let root = &workspace.root;
    let session = session(&workspace, "session")?;
    let outside = workspace.base.join("elsewhere.json");
    std::fs::write(
        &outside,
        serde_json::to_string(
            &json!([{"id": "X-1", "at": "2026-10-21T12:00:00Z", "title": "outside"}]),
        )?,
    )?;
    let pointer = session
        .parent()
        .unwrap_or(root)
        .join("feed-cycle/tool-results/big.txt")
        .to_string_lossy()
        .into_owned();
    let original = std::fs::read_to_string(&session)?;
    std::fs::write(
        &session,
        original.replace(&pointer, &outside.to_string_lossy()),
    )?;
    let components = SyntheticComponents;
    let refused = transcript::store(root, &session, "", &components)?;
    std::fs::write(&session, original)?;
    let since = transcript::store(root, &session, "2026-10-21T13:13:30Z", &components)?;
    let duration = transcript::store(root, &session, "30m", &components)?;
    let bad = error(transcript::store(root, &session, "yesterday", &components).map(|_| ()));
    Ok(json!({
        "refused": refused, "outside_stored": feed::originals(root)?.contains_key("tickets:X-1"),
        "since": since, "duration": duration, "bad": bad,
    }))
}

fn asks_cycle(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "asks-cycle")?;
    let root = &workspace.root;
    let components = SyntheticComponents;
    let ask = json!({"id": "rr-12", "source": "github", "kind": "review-request", "to_me": true, "at": "2026-10-21T13:00:00Z", "title": "Review requested", "url": "https://github.example.com/acme/api/pull/12"});
    let first = feed::ingest(
        root,
        std::slice::from_ref(&ask),
        "user",
        "feed",
        &[],
        &components,
    )?;
    let open_first = feed::open_asks(root, None, false)?
        .iter()
        .map(|row| text(field(row, "id")))
        .collect::<Vec<_>>();
    let idle = feed::ingest(root, &[ask], "user", "feed", &[], &components)?;
    feed::mark_ask(root, "rr-12", "done")?;
    let marked_closed = feed::open_asks(root, None, false)?.is_empty();
    feed::mark_ask(root, "rr-12", "ask")?;
    let reply = json!({"id": "rv-12", "source": "github", "kind": "review", "actor": "me", "at": "2026-10-21T14:00:00Z", "title": "Reviewed", "url": "https://github.example.com/acme/api/pull/12/files#pullrequestreview-9"});
    feed::ingest(root, &[reply], "user", "feed", &[], &components)?;
    let reply_closed = feed::open_asks(root, None, false)?.is_empty();
    let legacy = json!({"id": "legacy", "thread": "synthetic-thread", "triage": "act", "at": null});
    let valid = json!({"id": "valid", "thread": "synthetic-thread", "triage": "act", "at": "2026-10-19T15:00:00-04:00"});
    let legacy_sibling = feed::open_asks(root, Some(&[legacy, valid]), false)?
        .iter()
        .map(|row| text(field(row, "id")))
        .collect::<Vec<_>>();
    let required = ["github".to_owned(), "jira".to_owned(), "mail".to_owned()];
    let cycle = feed::mark_cycle(root, Some(&required), &components)?;
    let next_cycle = feed::mark_cycle(root, Some(&required), &components)?;
    let cli_cycle = cli::cycle_command(root, &required, false, &components);
    Ok(json!({
        "first": first, "idle": idle, "open_first": open_first, "marked_closed": marked_closed,
        "reply_closed": reply_closed, "legacy_sibling": legacy_sibling, "cycle": cycle,
        "next_cycle": next_cycle, "cli_cycle": cli_cycle, "lock": feed::feed_dir(root).join(".lock").is_file(),
    }))
}

fn team_feed(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "team-feed")?;
    let root = &workspace.root;
    std::env::set_var("CAREERBREW_HTTP_FIXTURES", fixture("http/team-feed"));
    connectors(root, &["slack"])?;
    careerbrew_projects::team::add(root, "Sam Lee", "U0SAM", "Sam@Example.com", "samlee")?;
    let components = SyntheticComponents;
    let pulled = sources::pull(root, &components)?;
    let rows = feed::load(root)?
        .iter()
        .map(|item| (text(field(item, "id")), projection(item)))
        .collect::<Map<_, _>>();
    feed::research(
        root,
        "slack:C0ENG:1792591260.000300",
        &json!([{"url": "https://tickets.example.com/PROJ-7", "summary": "the flag rollout ticket"}]),
    )?;
    let kept = cli::team_feed_rows(root, 7)?.iter().map(|row| json!({"id": field(row, "id").cloned().unwrap_or(Value::Null), "projects": field(row, "projects").cloned().unwrap_or(Value::Null), "to_research": field(row, "to_research").cloned().unwrap_or(Value::Null)})).collect::<Vec<_>>();
    let cli_list = cli::team_list_command(root);
    let cli_feed = cli::team_feed_command(root, 7);
    std::env::set_var(
        "CAREERBREW_HTTP_FIXTURES",
        fixture("http/team-feed-noscope"),
    );
    let without_scope = sources::slack(
        &fixture("http/team-feed-noscope"),
        &json!({"kind": "slack", "token": TOKEN}),
    )?
    .iter()
    .map(|item| text(field(item, "id")))
    .collect::<Vec<_>>();
    careerbrew_projects::projects::add(root, "export", Default::default())?;
    careerbrew_projects::projects::add_source(root, "export", "chat", "#eng")?;
    careerbrew_projects::projects::add(root, "archived", Default::default())?;
    careerbrew_projects::projects::add_source(root, "archived", "chat", "#eng")?;
    careerbrew_projects::projects::archive(root, "archived", true, false)?;
    let linked = cli::team_feed_rows(root, 7)?
        .iter()
        .map(|row| (text(field(row, "id")), row["projects"].clone()))
        .collect::<Map<_, _>>();
    let linked_cli = cli::team_feed_command(root, 7);
    Ok(json!({
        "pulled": pulled, "rows": rows, "kept": kept, "cli_list": cli_list, "cli_feed": cli_feed,
        "without_scope": without_scope, "linked": linked, "linked_cli": linked_cli,
    }))
}

fn local_time_and_nan(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "local-time-and-nan")?;
    let root = &workspace.root;
    careerbrew_vault::lock::with_lock(&root.join("settings.yaml"), || {
        write_yaml(
            root,
            "settings.yaml",
            &json!({"timezone": "America/New_York"}),
        )
    })??;
    std::env::set_var("CAREERBREW_NOW", "2026-10-21T16:00:00");
    let naive = feed::clean(
        &json!({"id": "naive", "at": "2026-10-21T12:00:00", "title": "naive"}),
        "event",
    )?;
    let aware = feed::clean(
        &json!({"id": "aware", "at": "2026-10-21T12:00:00-04:00", "title": "aware"}),
        "event",
    )?;
    let row = json!({"at": "2026-10-19T16:30:00-04:00"});
    let at = DateTime::parse_from_rfc3339("2026-10-19T17:30:00-04:00")
        .map_err(|_| FeedError::value("invalid fixture time"))?;
    let minutes = feed::work_minutes(&row, at, &read_yaml(root, "settings.yaml", json!({}))?);
    let mut raw = Vec::new();
    for stamp in ["NaN", "inf"] {
        let (events, skipped, source) = feed::raw_events(
            &json!({"messages": [{"id": stamp, "title": "bad", "ts": stamp}]}),
            "chat",
        )?
        .expect("raw reply");
        raw.push(json!({"events": events.len(), "skipped": skipped, "source": source}));
    }
    let transcript_results = ["NaN", "inf"]
        .iter()
        .map(|stamp| {
            match transcript::event(&json!({"id": stamp, "at": stamp, "title": "bad"}), "chat") {
                Ok(Some(_)) => "accepted",
                Ok(None) => "skipped",
                Err(error) => error.class,
            }
        })
        .collect::<Vec<_>>();
    careerbrew_vault::lock::with_lock(&root.join("settings.yaml"), || {
        write_yaml(root, "settings.yaml", &json!({"timezone": ""}))
    })??;
    std::env::set_var("TZ", ":America/New_York");
    let colon = crate::now_iso();
    std::env::remove_var("TZ");
    let system = crate::now_iso();
    careerbrew_vault::lock::with_lock(&root.join("settings.yaml"), || {
        write_yaml(
            root,
            "settings.yaml",
            &json!({"timezone": "America/New_York"}),
        )
    })??;
    let gap = match feed::utc(&json!("2026-03-08T02:30:00")) {
        Ok(value) => value,
        Err(error) => error.class.into(),
    };
    Ok(json!({
        "now": crate::now_iso(), "naive": naive["at"], "aware": aware["at"],
        "work_minutes": minutes, "raw": raw, "transcript": transcript_results,
        "colon": colon, "system": system, "gap": gap,
    }))
}

fn source_failures(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "source-failures")?;
    let root = &workspace.root;
    connectors(root, &["slack", "pagerduty", "outlook"])?;
    crate::write_text(root, "sensitive-terms.txt", "CANARYSOURCE\n")?;
    let plugin_root = root.join("plugins/feedplug");
    write_yaml(
        &plugin_root,
        "careerbrew-plugin.yaml",
        &json!({"name": "feedplug", "version": "0.1.0", "sensitive_terms": "sensitive-terms.txt"}),
    )?;
    crate::write_text(&plugin_root, "sensitive-terms.txt", "CANARYPLUGIN\n")?;
    write_json(
        root,
        "plugins.lock.json",
        &json!({"plugins": {"feedplug": {"origin": "plugin:feedplug"}}}),
    )?;
    std::env::set_var("CAREERBREW_RUST13_SOURCE_FAILURE", "1");
    let components = SyntheticComponents;
    let result = sources::pull(root, &components)?;
    std::env::remove_var("CAREERBREW_RUST13_SOURCE_FAILURE");
    let lengths = result["reasons"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|reason| text(Some(reason)).len())
        .collect::<Vec<_>>();
    let absent_workspace = Workspace::new(base, "absent-plugin-term")?;
    let absent_root = &absent_workspace.root;
    connectors(absent_root, &["slack", "pagerduty", "outlook"])?;
    crate::write_text(absent_root, "sensitive-terms.txt", "CANARYSOURCE\n")?;
    let absent_plugin = absent_root.join("plugins/feedplug");
    write_yaml(
        &absent_plugin,
        "careerbrew-plugin.yaml",
        &json!({"name": "feedplug", "version": "0.1.0", "sensitive_terms": "missing.txt"}),
    )?;
    write_json(
        absent_root,
        "plugins.lock.json",
        &json!({"plugins": {"feedplug": {"origin": "plugin:feedplug"}}}),
    )?;
    std::env::set_var("CAREERBREW_RUST13_SOURCE_FAILURE", "root-only");
    let absent = sources::pull(absent_root, &components)?;
    std::env::remove_var("CAREERBREW_RUST13_SOURCE_FAILURE");
    Ok(json!({"result": result, "lengths": lengths, "absent": absent}))
}

fn source_inventory(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "source-inventory")?;
    let root = &workspace.root;
    connectors(root, &["slack"])?;
    let slack = json!({"kind": "slack", "token": TOKEN});
    write_yaml(
        root,
        "work/sources.yaml",
        &json!({"sources": [slack.clone()]}),
    )?;
    let plugin_root = root.join("plugins/feedplug");
    write_yaml(
        &plugin_root,
        "careerbrew-plugin.yaml",
        &json!({"name": "feedplug", "version": "0.1.0", "provides": {"evidence_sources": "evidence.yaml"}}),
    )?;
    write_yaml(
        &plugin_root,
        "evidence.yaml",
        &json!({"connectors": [slack]}),
    )?;
    write_json(
        root,
        "plugins.lock.json",
        &json!({"plugins": {"feedplug": {"origin": "plugin:feedplug"}}}),
    )?;
    let components = SyntheticComponents;
    let result = sources::pull(root, &components)?;
    let inventory = sources::inventory(root)?.iter().map(|row| json!({"kind": field(row, "kind").cloned().unwrap_or(Value::Null), "origin": field(row, "origin").cloned().unwrap_or(Value::Null)})).collect::<Vec<_>>();
    let origins = feed::load(root)?
        .iter()
        .map(|row| (text(field(row, "id")), row["origin"].clone()))
        .collect::<Map<_, _>>();
    Ok(json!({"result": result, "inventory": inventory, "origins": origins}))
}

fn slack_ids(result: Result<Vec<Value>>) -> Value {
    match result {
        Ok(rows) => json!(rows
            .iter()
            .map(|row| text(field(row, "id")))
            .collect::<Vec<_>>()),
        Err(error) => json!(error.class),
    }
}

fn partial_transport(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "partial-transport")?;
    let root = &workspace.root;
    std::env::set_var("CAREERBREW_HTTP_FIXTURES", fixture("http/team-feed"));
    std::env::set_var("CAREERBREW_EVENTS", "on");
    careerbrew_projects::team::add(root, "Sam Lee", "U0SAM", "", "")?;
    // Compare feed diagnostics, not the roster setup event or its receipt.
    std::fs::create_dir_all(root.join("logs"))?;
    std::fs::write(root.join("logs/events.jsonl"), "")?;
    if let Some(trace) = std::env::var_os("CAREERBREW_RUST13_WRITE_TRACE") {
        std::fs::write(trace, "")?;
    }
    std::env::set_var("CAREERBREW_RUST13_TRANSPORT_ERROR", "team-search");
    let team_search = slack_ids(sources::slack(
        &fixture("http/team-feed"),
        &json!({"kind": "slack", "token": TOKEN}),
    ));
    std::env::set_var("CAREERBREW_RUST13_TRANSPORT_ERROR", "replies");
    let replies = slack_ids(sources::slack(
        &fixture("http/team-feed"),
        &json!({"kind": "slack", "token": TOKEN}),
    ));
    std::env::remove_var("CAREERBREW_RUST13_TRANSPORT_ERROR");
    let logged = read_jsonl(root, "logs/events.jsonl")?;
    let diagnostics = logged
        .iter()
        .map(|row| text(field(row, "event")))
        .filter(|event| ["work.feed.team_search", "work.feed.replies"].contains(&event.as_str()))
        .collect::<Vec<_>>();
    if std::env::var("CAREERBREW_RUST13_PLANT_WRITE").as_deref() == Ok("event-duplicate") {
        let row = careerbrew_setup::value::to_canon(logged.last().expect("diagnostic row"));
        careerbrew_vault::events::write_row(root, &row)?;
    }
    Ok(json!({"team_search": team_search, "replies": replies, "diagnostics": diagnostics}))
}

fn run(case: &str, base: &Path) -> Result<Value> {
    match case {
        "source-ingestion" => source_ingestion(base),
        "edited-upsert" => edited_upsert(base),
        "raw-replies" => raw_replies(base),
        "claude-transcript" => claude_transcript(base),
        "codex-transcript" => codex_transcript(base),
        "containment-since" => containment_since(base),
        "asks-cycle" => asks_cycle(base),
        "team-feed" => team_feed(base),
        "local-time-and-nan" => local_time_and_nan(base),
        "source-failures" => source_failures(base),
        "source-inventory" => source_inventory(base),
        "partial-transport" => partial_transport(base),
        _ => Err(FeedError::value("unknown rust-13 case")),
    }
}

pub fn run_all(selected: Option<&str>) -> Result<String> {
    let base = crate::root()?.join("rust13-workload");
    std::fs::create_dir_all(&base)?;
    let mut cases = Vec::new();
    for case in CASES {
        if selected.is_none_or(|selected| selected == *case) {
            let result = match run(case, &base) {
                Ok(result) => result,
                Err(error) => json!({"error": error.class, "message": error.message}),
            };
            cases.push(json!({"id": case, "result": result}));
        }
    }
    let report = crate::typed(&json!({"cases": cases}));
    Ok(crate::compact(&report)? + "\n")
}
