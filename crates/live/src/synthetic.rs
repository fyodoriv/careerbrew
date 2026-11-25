//! Synthetic rust-14 workload over the native live/review/notify components.

use crate::{
    cli, compact, field, health, listen, live, notify, read_json, read_text, review, root, text,
    truthy, typed, FeedError, Result,
};
use careerbrew_feed::feed;
use careerbrew_projects::{priorities, projects};
use careerbrew_sessions::drafts::{self, Backend as _, LocalBackend};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const CASES: &[&str] = &[
    "source-roster",
    "live-cycle",
    "acquisition-health",
    "direct-health",
    "listen-drafts",
    "link-review",
    "review-selection",
    "notify-dedup",
    "notify-time",
    "notify-project-quiet",
    "notify-suggestions",
    "cli-adapters",
];
const SHOP: &str = "https://github.com/example/shop/pull/7";
const TOOLS: &str = "https://github.com/example/tools/pull/3";
const MARKERS: &[&str] = &[
    "LIVEZEBRA",
    "LISTENZEBRA",
    "PROJECTZEBRA",
    "CANARYNOTIFY",
    "synthetic-not-reported",
];

struct Workspace {
    base: PathBuf,
    root: PathBuf,
}

impl Workspace {
    fn new(base: &Path, name: &str, notifier: bool) -> Result<Self> {
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
        std::env::set_var("CAREERBREW_NOW", "2026-10-25T16:00:00+00:00");
        std::env::set_var("CAREERBREW_TEST_OWNER", "1");
        std::env::set_var("CAREERBREW_NOTIFY", "off");
        std::env::set_var("CAREERBREW_HTTP_FIXTURES", fixture("http"));
        std::env::set_var("CB_T", "synthetic-not-reported");
        std::env::set_var("TZ", "UTC");
        std::env::remove_var("FAKE_NOTIFIER_CLICK");
        std::env::remove_var("FAKE_NOTIFIER_LOG");
        if notifier {
            let binary = base.join("bin");
            std::fs::create_dir_all(&binary)?;
            std::os::unix::fs::symlink(
                fixture("notifier/fake-notifier.sh"),
                binary.join("alerter"),
            )?;
            std::env::set_var(
                "PATH",
                format!("{}:/usr/bin:/bin", binary.to_string_lossy()),
            );
            std::env::set_var("FAKE_NOTIFIER_LOG", base.join("notifier.log"));
            std::env::set_var("CAREERBREW_NOTIFY", "on");
        }
        Ok(Self { base, root })
    }

    fn notifier_calls(&self) -> Result<Vec<Vec<String>>> {
        let text = match std::fs::read_to_string(self.base.join("notifier.log")) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut out = Vec::new();
        for line in text.lines() {
            if let Some(tool) = line.strip_prefix("--- ") {
                out.push(vec![tool.to_owned()]);
            } else if let Some(last) = out.last_mut() {
                last.push(line.to_owned());
            }
        }
        Ok(out)
    }

    fn use_notifier(&self, settings: &[(&str, &str)]) -> Result<()> {
        for (key, value) in [
            ("notify_events", "true"),
            ("notify_backend", "alerter"),
            ("notify_quiet_hours", "none"),
            ("notify_batch_minutes", "0"),
            ("notify_min_priority", "1"),
        ]
        .into_iter()
        .chain(settings.iter().copied())
        .collect::<BTreeMap<_, _>>()
        {
            crate::set_setting_text(&self.root, key, value)?;
        }
        Ok(())
    }
}

use std::os::unix::fs::PermissionsExt;

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(path)
        .canonicalize()
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures")
                .join(path)
        })
}

fn error(result: Result<()>) -> String {
    result
        .err()
        .map(|error| error.class)
        .unwrap_or("accepted")
        .to_owned()
}

fn fields(row: &Value, keys: &[&str]) -> Value {
    let mut out = Map::new();
    for key in keys {
        if let Some(value) = row.get(*key) {
            out.insert((*key).to_owned(), value.clone());
        }
    }
    Value::Object(out)
}

fn cycle_projection(rec: &Value) -> Value {
    let mut out = fields(
        rec,
        &[
            "n",
            "at",
            "status",
            "not_stored",
            "degraded",
            "verdicts",
            "counts",
            "actions",
            "goals",
            "review_sessions",
            "standup",
        ],
    );
    out["sources"] = json!(rec["sources"]
        .as_object()
        .into_iter()
        .flat_map(|rows| rows.iter())
        .map(|(name, row)| {
            (
                name.clone(),
                fields(
                    row,
                    &[
                        "pulled",
                        "stored",
                        "items",
                        "new",
                        "updated",
                        "skipped",
                        "error",
                        "error_kind",
                        "unchanged",
                        "validated_at",
                        "health",
                        "max_age_minutes",
                        "checked_at",
                        "acquired_at",
                        "age_seconds",
                    ],
                ),
            )
        })
        .collect::<Map<_, _>>());
    out["followups"] = json!(rec["followups"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| fields(
            row,
            &["id", "source", "class", "verdict", "actions", "project"]
        ))
        .collect::<Vec<_>>());
    out
}

fn state_projection(root: &Path) -> Result<Value> {
    let state = live::state(root)?;
    Ok(json!({
        "running": state["running"].clone(),
        "cursors": state["cursors"].clone(),
        "cycles": state["cycles"].clone(),
        "followed": state["followed"].clone(),
        "attached": state["attached"].clone(),
    }))
}

fn clean_text(_root: &Path) -> Result<Value> {
    let root = crate::root()?;
    std::fs::create_dir_all(root.join("day"))?;
    std::fs::create_dir_all(root.join("logs"))?;
    std::fs::create_dir_all(root.join("work/live"))?;
    let mut blobs = Vec::new();
    for path in [
        "work/live/state.json",
        "work/live/cycles.jsonl",
        "day/notifications.json",
    ] {
        let path = root.join(path);
        if path.exists() {
            blobs.push(read_text(
                &root,
                path.strip_prefix(root.as_path())
                    .expect("owned path")
                    .to_string_lossy()
                    .as_ref(),
            )?);
        }
    }
    let events = root.join("logs/events.jsonl");
    if events.exists() {
        blobs.push(std::fs::read_to_string(events)?);
    }
    let joined = blobs.join("\n");
    Ok(json!({"any": MARKERS.iter().any(|marker| joined.contains(marker))}))
}

fn file_facts(root: &Path) -> Result<BTreeMap<String, String>> {
    fn walk(root: &Path, folder: &Path, out: &mut BTreeMap<String, String>) -> Result<()> {
        for entry in std::fs::read_dir(folder)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else if path.is_file() {
                out.insert(
                    path.strip_prefix(root)
                        .expect("owned file")
                        .to_string_lossy()
                        .into_owned(),
                    format!("{:x}", Sha256::digest(std::fs::read(path)?)),
                );
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

fn put(path: &Path, text: &str, at: Option<&str>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    let at = at
        .map(str::to_owned)
        .unwrap_or_else(|| std::env::var("CAREERBREW_NOW").unwrap_or_default());
    let stamp =
        crate::parse_time(&at).ok_or_else(|| FeedError::value("invalid synthetic file time"))?;
    let seconds = stamp.timestamp_millis() as f64 / 1000.0;
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs_f64(seconds);
    let file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.set_times(
        std::fs::FileTimes::new()
            .set_accessed(time)
            .set_modified(time),
    )?;
    Ok(())
}

fn clock(text: &str) {
    std::env::set_var("CAREERBREW_NOW", text);
}

fn live_sources(workspace: &Workspace, listen_case: bool) -> Result<PathBuf> {
    let source = if listen_case {
        fixture("live/listen/sources.yaml")
    } else {
        fixture("live/sources.yaml")
    };
    let text = std::fs::read_to_string(source)?
        .replace("@DIR@", &workspace.base.join("inbox").to_string_lossy());
    let path = workspace.base.join("sources.yaml");
    std::fs::write(&path, text)?;
    Ok(path)
}

fn fill_live(workspace: &Workspace, listen_case: bool) -> Result<()> {
    let folder = if listen_case {
        fixture("live/listen")
    } else {
        fixture("live")
    };
    for entry in std::fs::read_dir(folder)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            let name = path.file_name().expect("fixture name");
            put(
                &workspace.base.join("inbox").join(name),
                &std::fs::read_to_string(path)?,
                None,
            )?;
        }
    }
    Ok(())
}

fn source_roster(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "source-roster", false)?;
    let root = &workspace.root;
    let plugin = workspace.base.join("plug");
    std::fs::create_dir_all(plugin.join("live"))?;
    std::fs::write(plugin.join("careerbrew-plugin.yaml"), "name: acme-live\nversion: 0.1.0\nlive: {sources: live/sources.yaml, team: live/team.yaml}\n")?;
    std::fs::write(plugin.join("live/sources.yaml"), format!("sources:\n  - {{name: paging, kind: paging, required: true, adapter: file, inbox: {}/inbox/paging.json}}\n  - {{name: chat, kind: chat, adapter: file, inbox: /nowhere.json}}\n", workspace.base.to_string_lossy()))?;
    std::fs::write(
        plugin.join("live/team.yaml"),
        "team:\n  - {name: Sam Lee, chat: U0SAM}\n",
    )?;
    careerbrew_setup::plugins::add(&plugin, false)?;
    live::save_sources(
        root,
        &serde_json::to_string(
            &json!({"sources": [{"name": "chat", "kind": "chat", "required": true, "inbox": workspace.base.join("inbox/chat.json").to_string_lossy()}]}),
        )?,
    )?;
    let mut bad = Vec::new();
    for source in [
        json!({"name": "Chat Room", "kind": "chat", "inbox": "x"}),
        json!({"name": "chat", "kind": "pager", "inbox": "x"}),
        json!({"name": "chat", "kind": "chat"}),
        json!({"name": "chat", "kind": "chat", "inbox": "x", "host": "y"}),
        json!({"name": "chat", "kind": "chat", "inbox": "x", "max_age_minutes": null}),
        json!({"name": "chat", "kind": "chat", "inbox": "x", "max_age_minutes": true}),
    ] {
        bad.push(error(live::check_source(&source, "synthetic").map(|_| ())));
    }
    let rows = live::sources(root)?
        .into_iter()
        .map(|row| {
            let mut row = fields(&row, &["name", "kind", "required", "adapter", "origin"]);
            row["since"] = json!("");
            row
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"sources": rows, "team": live::roster(root)?, "own_roster": careerbrew_projects::team::load(root)?, "bad": bad}),
    )
}

fn live_cycle(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "live-cycle", false)?;
    let root = &workspace.root;
    live::start(
        root,
        &std::fs::read_to_string(live_sources(&workspace, false)?)?,
    )?;
    if std::env::var("CAREERBREW_RUST14_PLANT_WRITE").as_deref() == Ok("extra") {
        std::fs::write(root.join("planted-extra.txt"), "synthetic")?;
    }
    fill_live(&workspace, false)?;
    crate::write_yaml(
        root,
        "work/feed-rules.yaml",
        &json!({"rules": [{"name": "shipped", "when": {"source": "tickets", "actor": "me"}, "evidence": true}]}),
    )?;
    let first = live::cycle(root, false)?;
    if std::env::var("CAREERBREW_RUST14_PLANT_WRITE").as_deref() == Ok("missing") {
        std::fs::remove_file(feed::originals_path(root))?;
    }
    if std::env::var("CAREERBREW_RUST14_PLANT_WRITE").as_deref() == Ok("operation-duplicate") {
        let rows = crate::read_jsonl(root, "work/feed/events.jsonl")?;
        crate::write_jsonl(root, "work/feed/events.jsonl", &rows)?;
    }
    let before = state_projection(root)?;
    let second = live::cycle(root, false)?;
    let after = state_projection(root)?;
    let stopped = live::stop(root)?;
    let stopped_error = error(live::cycle(root, false).map(|_| ()));
    let restarted = live::start(root, "")?;
    let lock = live::CycleLock::new(root)?;
    let lock_error = error(live::cycle(root, false).map(|_| ()));
    drop(lock);
    Ok(json!({
        "first": cycle_projection(&first),
        "second": cycle_projection(&second),
        "unchanged_state": {"seen": before["cursors"] == after["cursors"], "followed": before["followed"] == after["followed"]},
        "stopped": stopped,
        "stopped_error": stopped_error,
        "restarted": restarted,
        "lock_error": lock_error,
        "clean": clean_text(root)?,
    }))
}

fn healthy_workspace(base: &Path, name: &str) -> Result<Workspace> {
    let workspace = Workspace::new(base, name, false)?;
    let path = workspace.base.join("inbox.json");
    put(
        &path,
        "{\"tickets\": []}",
        Some("2026-11-05T12:00:00+00:00"),
    )?;
    clock("2026-11-05T12:00:00+00:00");
    live::start(
        &workspace.root,
        &serde_json::to_string(
            &json!({"sources": [{"name": "tickets", "kind": "tickets", "required": true, "inbox": path.to_string_lossy()}]}),
        )?,
    )?;
    Ok(workspace)
}

fn acquisition_health(base: &Path) -> Result<Value> {
    if std::env::var("CAREERBREW_RUST14_PLANT_WRITE").as_deref() == Ok("duplicate") {
        let path = base.join("live-cycle/data/work/feed/events.jsonl");
        let bytes = std::fs::read(&path)?;
        std::thread::sleep(std::time::Duration::from_millis(2));
        std::fs::write(path, bytes)?;
    }
    let workspace = healthy_workspace(base, "acquisition-health")?;
    let root = &workspace.root;
    let path = workspace.base.join("inbox.json");
    let first = live::cycle(root, false)?;
    let unchanged = live::cycle(root, false)?;
    clock("2026-11-05T12:15:00+00:00");
    let inclusive = live::cycle(root, false)?;
    clock("2026-11-05T12:15:01+00:00");
    let expired = live::cycle(root, false)?;
    clock("2026-11-05T12:16:00+00:00");
    let older = live::cycle(root, false)?;
    let stamp = std::fs::metadata(&path)?.modified()?;
    std::fs::write(&path, "not JSON")?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)?
        .set_times(
            std::fs::FileTimes::new()
                .set_accessed(stamp)
                .set_modified(stamp),
        )?;
    let malformed = live::cycle(root, false)?;

    let future = healthy_workspace(base, "future-clock")?;
    put(
        &future.base.join("inbox.json"),
        "{\"tickets\": []}",
        Some("2026-11-05T12:01:00+00:00"),
    )?;
    let future_first = live::cycle(&future.root, false)?;
    let future_second = live::cycle(&future.root, false)?;

    let backwards = healthy_workspace(base, "backwards-clock")?;
    clock("2026-11-05T12:16:00+00:00");
    live::cycle(&backwards.root, false)?;
    clock("2026-11-05T12:14:00+00:00");
    let reversed = live::cycle(&backwards.root, false)?;

    let optional = healthy_workspace(base, "optional-gap")?;
    live::save_sources(
        &optional.root,
        &serde_json::to_string(
            &json!({"sources": [{"name": "tickets", "kind": "tickets", "required": true, "inbox": optional.base.join("inbox.json").to_string_lossy()}, {"name": "optional", "kind": "chat", "inbox": optional.base.join("missing.json").to_string_lossy()}]}),
        )?,
    )?;
    let optional_row = live::cycle(&optional.root, false)?;
    Ok(json!({
        "first": cycle_projection(&first), "unchanged": cycle_projection(&unchanged), "inclusive": cycle_projection(&inclusive), "expired": cycle_projection(&expired), "older": cycle_projection(&older), "malformed": cycle_projection(&malformed),
        "future_first": cycle_projection(&future_first), "future_second": cycle_projection(&future_second), "reversed": cycle_projection(&reversed), "optional": cycle_projection(&optional_row),
        "invalid_limit": error(live::save_sources(&optional.root, &serde_json::to_string(&json!({"sources": [{"name": "bad", "kind": "chat", "inbox": "synthetic", "max_age_minutes": 1.5}]}))?).map(|_| ())),
    }))
}

fn direct_health(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "direct-health", false)?;
    let root = &workspace.root;
    clock("2026-11-05T12:00:00+00:00");
    crate::set_setting_text(
        root,
        "feed_tool_sources",
        "mcp__tickets__*=tickets,mcp__chat__*=chat",
    )?;
    feed::ingest(
        root,
        &[],
        "user",
        "feed",
        &["tickets".into(), "chat".into()],
        &live::Provider,
    )?;
    let completed = feed::mark_cycle(root, None, &live::Provider)?;
    let healthy = health::snapshot(root)?;
    let before = file_facts(root)?;
    clock("2026-11-05T12:15:00+00:00");
    let inclusive = health::snapshot(root)?;
    clock("2026-11-05T12:15:01+00:00");
    let stale = health::snapshot(root)?;
    let stale_again = health::snapshot(root)?;
    let after = file_facts(root)?;
    crate::set_setting_text(
        root,
        "feed_tool_sources",
        "mcp__other__*=tickets,mcp__chat__*=chat",
    )?;
    let changed = health::snapshot(root)?;
    crate::set_setting_text(
        root,
        "feed_tool_sources",
        "mcp__tickets__*=tickets,mcp__chat__*=chat",
    )?;
    feed::ingest(
        root,
        &[],
        "user",
        "feed",
        &["tickets".into()],
        &live::Provider,
    )?;
    feed::mark_cycle(root, None, &live::Provider)?;
    let failed = health::snapshot(root)?;
    let exact = health::result_health(&json!({"feed_source": "chat"}), &healthy);
    let unknown = health::result_health(&json!({"id": "chat:ambiguous"}), &healthy);
    let invalid_workspace = Workspace::new(base, "invalid-health", false)?;
    crate::write_yaml(
        &invalid_workspace.root,
        "work/live-sources.yaml",
        &json!({"sources": [{"name": "invalid-canary", "inbox": "/synthetic/private/input"}]}),
    )?;
    let invalid = health::snapshot(&invalid_workspace.root)?;
    Ok(json!({
        "completed": completed,
        "healthy": healthy,
        "inclusive": inclusive,
        "stale": stale,
        "stale_again": stale_again,
        "read_only": before == after,
        "changed": changed,
        "failed": failed,
        "exact": exact,
        "unknown": unknown,
        "invalid": invalid,
        "invalid_has_path": invalid.to_string().contains("/synthetic/private/input"),
    }))
}

fn goals(root: &Path) -> Result<()> {
    crate::write_yaml(
        root,
        "goals.yaml",
        &json!({"long_term": [{"id": "reviewer", "text": "Grow as a code reviewer", "by": null}], "queue": [{"id": "api-rollout", "kind": "custom", "title": "Ship the API rollout", "date": "2026-11-20", "topics": ["rollout"], "minutes_per_day": 30}], "history": []}),
    )
}

fn listen_drafts(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "listen-drafts", false)?;
    let root = &workspace.root;
    live::start(
        root,
        &std::fs::read_to_string(live_sources(&workspace, true)?)?,
    )?;
    fill_live(&workspace, true)?;
    projects::add(root, "shop-api", Default::default())?;
    projects::add_source(root, "shop-api", "repo", "example-shop/api")?;
    goals(root)?;
    let first = live::cycle(root, false)?;
    let holding = LocalBackend::new(root).read("chat:a1")?;
    let review_row = feed::load(root)?
        .into_iter()
        .find(|row| text(field(row, "id")) == "chat:1792936860.000200")
        .expect("review row");
    let review_id = listen::review_target(&review_row);
    let notes = LocalBackend::new(root)
        .read(&review_id)?
        .unwrap_or_default();
    let updates = projects::load(root)?["shop-api"]["updates"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let sessions = listen::review_sessions(root)?;
    let second = live::cycle(root, false)?;
    let goal_lines = first["goals"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|goal| listen::goal_line(goal, false))
        .collect::<Vec<_>>();
    let kept_workspace = Workspace::new(base, "existing-draft", false)?;
    drafts::replace(
        &kept_workspace.root,
        "chat:a1",
        "my own words, written earlier",
        false,
        "",
    )?;
    live::start(
        &kept_workspace.root,
        &std::fs::read_to_string(live_sources(&kept_workspace, true)?)?,
    )?;
    fill_live(&kept_workspace, true)?;
    let kept = live::cycle(&kept_workspace.root, false)?;
    Ok(json!({
        "first": cycle_projection(&first),
        "holding": holding.as_deref() == Some(listen::HOLDING),
        "review_notes": {"link": notes.contains("https://github.com/example-shop/api/pull/9"), "draft": notes.contains("not posted"), "verdict": notes.contains("Verdict:")},
        "updates": updates,
        "sessions": sessions,
        "second": cycle_projection(&second),
        "goal_lines": goal_lines,
        "kept": cycle_projection(&kept),
        "kept_text": LocalBackend::new(&kept_workspace.root).read("chat:a1")?,
        "clean": clean_text(&kept_workspace.root)?,
    }))
}

fn range_url(value: u128) -> String {
    let address = std::net::Ipv6Addr::from(value);
    if value <= u32::MAX as u128 {
        format!("http://{}/x", std::net::Ipv4Addr::from(value as u32))
    } else {
        format!("http://[{address}]/x")
    }
}

fn link_review(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "link-review", false)?;
    let root = &workspace.root;
    live::start(
        root,
        &std::fs::read_to_string(live_sources(&workspace, true)?)?,
    )?;
    fill_live(&workspace, true)?;
    std::env::set_var("CAREERBREW_RUST14_ADDRESS_STUB", "1");
    crate::set_setting_text(root, "live_open_links", "true")?;
    let rec = live::cycle(root, true)?;
    let rows = feed::load(root)?
        .into_iter()
        .map(|row| (text(field(&row, "id")), row))
        .collect::<BTreeMap<_, _>>();
    let research_rows = [
        "chat:1792936800.000100",
        "chat:1792936860.000200",
        "chat:1792936920.000300",
    ]
    .into_iter()
    .map(|id| (id.to_owned(), rows[id]["research"].clone()))
    .collect::<Map<_, _>>();
    let mut private = Vec::new();
    for url in [
        "http://localhost:8080/x",
        "http://private.example/y",
        "http://localhost/z",
        "https://[draft",
    ] {
        match listen::private_reason(url) {
            Ok(reason) => private.push(json!({"url": url, "reason": reason})),
            Err(error) => private.push(json!({"url": url, "error": error.class})),
        }
    }
    std::env::remove_var("CAREERBREW_RUST14_ADDRESS_STUB");
    let mut range_controls = Vec::new();
    for url in [
        0xC0000201,
        0x64000001,
        0xC6120001,
        0xF0000001,
        0xCB007101,
        0x20010DB8000000000000000000000001,
        0x26064700470000000000000000001111,
    ]
    .into_iter()
    .map(range_url)
    .collect::<Vec<_>>()
    {
        range_controls.push(json!([url, listen::private_reason(&url)?]));
    }
    projects::add(root, "ops", Default::default())?;
    projects::add_source(root, "ops", "url", "https://status.example.com/budget")?;
    set_reference(root, "ops", "https://status.example.com/budget")?;
    let mut unresearched = rows["chat:1792936800.000100"].clone();
    unresearched
        .as_object_mut()
        .expect("feed row")
        .remove("research");
    let reference = listen::research_links(root, &unresearched, true)?;
    let ping = json!({"id": "chat:ping", "source": "chat", "kind": "mention", "at": "2026-10-25T16:01:00Z", "title": "re-review", "actor": "other", "to_me": true, "url": "https://github.com/example-shop/api/pull/9"});
    feed::ingest(
        root,
        std::slice::from_ref(&ping),
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    let row = feed::load(root)?.last().cloned().expect("ping row");
    let open = feed::open_asks(root, None, false)?
        .iter()
        .map(|row| text(field(row, "id")))
        .collect::<BTreeSet<_>>();
    let verdict = live::verdict(&row, &open);
    let (follow, _) = listen::follow_up(
        root,
        &BTreeMap::from([(text(field(&ping, "id")), verdict)]),
        &BTreeSet::new(),
        false,
    )?;
    let partial = json!({"id": "chat:partial", "source": "chat", "kind": "dm", "at": "2026-10-25T16:02:00Z", "title": "partial research", "actor": "other", "to_me": true, "text": "see https://public.example/first"});
    feed::ingest(
        root,
        std::slice::from_ref(&partial),
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    feed::research(
        root,
        "chat:partial",
        &json!([{"url": "https://public.example/first", "summary": "agent original"}]),
    )?;
    let mut second = partial.clone();
    second["text"] = json!("see https://public.example/first and https://public.example/second");
    feed::ingest(root, &[second], "user", "feed", &[], &live::Provider)?;
    listen::cycle_research(
        root,
        "chat:partial",
        &[json!({"url": "https://public.example/second", "opened": false, "reason": "offline"})],
    )?;
    let mut updated = partial.clone();
    updated["text"] = json!("see https://public.example/first and https://public.example/second and https://public.example/third");
    feed::ingest(root, &[updated], "user", "feed", &[], &live::Provider)?;
    let research_merge = feed::load(root)?
        .into_iter()
        .find(|row| text(field(row, "id")) == "chat:partial")
        .expect("partial row")["research"]
        .clone();
    let public_summary = listen::read_url("https://public.example/budget")?;
    Ok(
        json!({"cycle": cycle_projection(&rec), "research": research_rows, "private": private, "range_controls": range_controls, "research_merge": research_merge, "public_summary": public_summary, "reference": reference, "re_review": follow, "sessions": listen::review_sessions(root)?, "clean": clean_text(root)?}),
    )
}

fn github(root: &Path) -> Result<()> {
    crate::write_yaml(
        root,
        "work/connectors.yaml",
        &json!({"connectors": [{"kind": "github", "token": "env:CB_T", "user": "me"}]}),
    )
}

fn review_selection(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "review-selection", false)?;
    let root = &workspace.root;
    github(root)?;
    let shop = review::build(root, SHOP, &[])?;
    let tools = review::build(root, TOOLS, &[])?;
    let extra = review::build(
        root,
        TOOLS,
        &["@eli-dev".into(), "example/maintainers".into()],
    )?;
    let checks = vec![
        review::check(root, "Please review @ana-dev and @example/backend", 2, "")?,
        review::check_pr(root, SHOP, "@author-dev @ana-dev")?,
        review::check_pr(root, SHOP, "@ana-dev and @ben-dev, please")?,
        review::check(root, "@Ana-Dev and @ana-dev", 2, "")?,
        review::check(root, "@here please review @ana-dev", 2, "")?,
    ];
    let mut patterns = Vec::new();
    for (pattern, path) in [
        ("*", "a/b.py"),
        ("/src/api/", "src/api/x/y.py"),
        ("/src/api/", "lib/src/api/x.py"),
        ("docs/*", "docs/sub/a.md"),
        ("**/logs", "a/b/logs/x"),
    ] {
        patterns.push(
            !review::owners_of(
                path,
                &review::parse_codeowners(&format!("{pattern} @someone")),
            )
            .is_empty(),
        );
    }
    let last = review::parse_codeowners("* @all-dev\n/src/ @src-dev @org/team owner@example.com\n");
    let mismatch =
        error(review::build(root, "https://evil.example.com/example/shop/pull/7", &[]).map(|_| ()));
    let links = vec![
        review::recheck_pr_link(SHOP)?,
        match review::recheck_pr_link("http://github.com/example/shop/pull/7") {
            Ok(_) => json!("accepted"),
            Err(error) => json!(error.class),
        },
    ];
    let unknown = review::recheck_pr(root, SHOP);
    Ok(json!({
        "shop": shop,
        "tools": tools,
        "extra": extra,
        "checks": checks,
        "patterns": patterns,
        "last": review::owners_of("src/a.py", &last),
        "mismatch": mismatch,
        "links": links,
        "recheck_unknown": [unknown.0, unknown.1],
    }))
}

fn event(eid: &str, kind: &str, at: &str, extra: &[(&str, Value)]) -> Value {
    let mut row = json!({"id": eid, "at": at, "title": format!("title of {eid}"), "source": "slack", "kind": kind, "actor": "other", "to_me": true, "thread": format!("t-{eid}")});
    for (key, value) in extra {
        row[key] = value.clone();
    }
    row
}

fn notification_view(root: &Path) -> Result<Value> {
    let state = notify::load(root)?;
    Ok(json!({
        "items": state["items"].as_array().into_iter().flatten().map(|row| fields(row, &["id", "key", "ref", "refs", "pending_refs", "held_refs", "snoozed_until_by_ref", "kind", "sub", "headline", "why", "next", "priority", "priority_why", "count", "at", "status", "popup", "popup_id", "created_at", "updated_at", "acted_at", "snooze_until", "resolved", "snoozes"])).collect::<Vec<_>>(),
        "popups": state["popups"].clone(),
    }))
}

fn notify_dedup(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "notify-dedup", true)?;
    let root = &workspace.root;
    clock("2026-10-21T10:00:00+00:00");
    workspace.use_notifier(&[("notify_batch_minutes", "10")])?;
    if std::env::var("CAREERBREW_RUST14_PLANT_WRITE").as_deref() == Ok("event-duplicate") {
        std::env::set_var("CAREERBREW_EVENTS", "on");
        std::fs::create_dir_all(root.join("logs"))?;
        std::fs::write(root.join("logs/events.jsonl"), "")?;
    }
    crate::set_setting_text(root, "project_priority", "billing=*invoice*")?;
    crate::write_text(root, "sensitive-terms.txt", "CANARYNOTIFY\n")?;
    feed::ingest(
        root,
        &[
            event(
                "pagerduty:P1",
                "page",
                "2026-10-21T13:30:00Z",
                &[
                    ("source", json!("pagerduty")),
                    ("status", json!("triggered")),
                ],
            ),
            event("slack:C1:1.0", "dm", "2026-10-21T13:30:00Z", &[]),
            event(
                "github:pr-9",
                "review-request",
                "2026-10-21T13:30:00Z",
                &[("source", json!("CANARYNOTIFY"))],
            ),
            event(
                "slack:C3:3.0",
                "mention",
                "2026-10-21T13:30:00Z",
                &[("title", json!("Mention about the invoice export"))],
            ),
            event(
                "slack:C4:4.0",
                "message",
                "2026-10-21T13:30:00Z",
                &[("actor", json!("bot"))],
            ),
            event("slack:old", "dm", "2026-10-19T13:30:00Z", &[]),
            event(
                "slack:personal",
                "dm",
                "2026-10-21T13:30:00Z",
                &[("personal", json!(true))],
            ),
        ],
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    let priorities = notify::candidates(root, None)?
        .into_iter()
        .map(|row| (text(field(&row, "ref")), row["priority"].clone()))
        .collect::<Map<_, _>>();
    let first = notify::run(root, true, None)?;
    if std::env::var("CAREERBREW_RUST14_PLANT_WRITE").as_deref() == Ok("event-duplicate") {
        let logged = crate::read_jsonl(root, "logs/events.jsonl")?;
        if let Some(row) = logged.last() {
            let row = careerbrew_setup::value::to_canon(row);
            careerbrew_vault::events::write_row(root, &row)?;
        }
    }
    let first_view = notification_view(root)?;
    let repeated = notify::run(root, true, None)?;
    clock("2026-10-21T10:11:00+00:00");
    feed::ingest(
        root,
        &[event(
            "slack:C1:2.0",
            "dm",
            "2026-10-21T14:10:00Z",
            &[("thread", json!("t-slack:C1:1.0"))],
        )],
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    let merged = notify::run(root, true, None)?;
    feed::ingest(
        root,
        &[event(
            "slack:C1:reply",
            "message",
            "2026-10-21T14:11:00Z",
            &[
                ("actor", json!("me")),
                ("to_me", json!(false)),
                ("thread", json!("t-slack:C1:1.0")),
            ],
        )],
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    let closed = notify::run(root, true, None)?;
    clock("2026-10-21T10:30:00+00:00");
    feed::ingest(
        root,
        &[event(
            "slack:C1:new",
            "dm",
            "2026-10-21T14:20:00Z",
            &[("thread", json!("t-slack:C1:1.0"))],
        )],
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    let reopened = notify::run(root, true, None)?;
    Ok(json!({
        "priorities": priorities,
        "first": first,
        "first_view": first_view,
        "first_calls": workspace.notifier_calls()?,
        "repeated": repeated,
        "merged": merged,
        "merged_view": notification_view(root)?,
        "closed": closed,
        "closed_view": notification_view(root)?,
        "reopened": reopened,
        "reopened_view": notification_view(root)?,
        "calls": workspace.notifier_calls()?,
        "notifier_clean": !std::fs::read_to_string(workspace.base.join("notifier.log")).unwrap_or_default().contains("CANARYNOTIFY"),
        "clean": clean_text(root)?,
        "sealed": std::fs::read(root.join("day/notifications.json"))?.starts_with(b"CBV1\n"),
    }))
}

fn notify_time(base: &Path) -> Result<Value> {
    let quiet = Workspace::new(base, "quiet-hours", true)?;
    quiet.use_notifier(&[("notify_quiet_hours", "19:00-08:00")])?;
    clock("2026-10-21T22:00:00+00:00");
    feed::ingest(
        &quiet.root,
        &[
            event("slack:C1:1.0", "dm", "2026-10-22T01:30:00Z", &[]),
            event(
                "pagerduty:P1",
                "page",
                "2026-10-22T01:45:00Z",
                &[
                    ("source", json!("pagerduty")),
                    ("status", json!("triggered")),
                ],
            ),
        ],
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    let held = notify::run(&quiet.root, true, None)?;
    let held_view = notification_view(&quiet.root)?;
    clock("2026-10-22T08:30:00+00:00");
    let released = notify::run(&quiet.root, true, None)?;
    let away_calls = quiet.notifier_calls()?;

    let cap = Workspace::new(base, "daily-cap", true)?;
    cap.use_notifier(&[("notify_daily_cap", "1")])?;
    clock("2026-10-21T10:00:00+00:00");
    feed::ingest(
        &cap.root,
        &[event("slack:C1:1.0", "dm", "2026-10-21T13:30:00Z", &[])],
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    notify::run(&cap.root, true, None)?;
    clock("2026-10-21T11:00:00+00:00");
    feed::ingest(
        &cap.root,
        &[event("slack:C2:2.0", "dm", "2026-10-21T14:50:00Z", &[])],
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    let capped = notify::run(&cap.root, true, None)?;
    clock("2026-10-22T10:00:00+00:00");
    feed::ingest(
        &cap.root,
        &[event("slack:C3:3.0", "dm", "2026-10-22T13:50:00Z", &[])],
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    let next_day = notify::run(&cap.root, true, None)?;

    let snooze = Workspace::new(base, "snooze", true)?;
    snooze.use_notifier(&[("notify_snooze_minutes", "60")])?;
    clock("2026-10-21T10:00:00+00:00");
    feed::ingest(
        &snooze.root,
        &[event("slack:C1:1.0", "dm", "2026-10-21T13:30:00Z", &[])],
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    std::env::set_var("FAKE_NOTIFIER_CLICK", "Snooze");
    notify::run(&snooze.root, true, None)?;
    std::env::remove_var("FAKE_NOTIFIER_CLICK");
    let snoozed = notification_view(&snooze.root)?;
    clock("2026-10-21T10:30:00+00:00");
    let early = notify::run(&snooze.root, true, None)?;
    clock("2026-10-21T11:01:00+00:00");
    let due = notify::run(&snooze.root, true, None)?;
    let quiet_boundaries = ["07:59", "08:00", "18:59", "22:00"]
        .into_iter()
        .map(|stamp| {
            notify::in_quiet(
                crate::parse_time(&format!("2026-10-21T{stamp}:00+00:00")).expect("quiet fixture"),
                "19:00-08:00",
            )
        })
        .collect::<Vec<_>>();
    let cap_view = notification_view(&snooze.root)?;
    let snooze_view = notification_view(&snooze.root)?;
    Ok(json!({
        "held": held, "held_view": held_view, "released": released, "away_calls": away_calls,
        "capped": capped, "next_day": next_day, "cap_calls": cap.notifier_calls()?, "cap_view": cap_view,
        "snoozed": snoozed, "early": early, "due": due, "snooze_calls": snooze.notifier_calls()?, "snooze_view": snooze_view,
        "quiet_boundaries": quiet_boundaries,
    }))
}

fn edit_quiet(root: &Path, name: &str, until: &str) -> Result<()> {
    Ok(projects::with_lock(root, || {
        let mut every = projects::load(root)?;
        every[name]["quiet_until"] = json!(until);
        projects::save(root, &every)
    })?)
}

fn set_reference(root: &Path, name: &str, reference: &str) -> Result<()> {
    Ok(projects::with_lock(root, || {
        let mut every = projects::load(root)?;
        for source in every[name]["sources"].as_array_mut().into_iter().flatten() {
            if text(field(source, "ref")) == reference {
                source["reference"] = json!(true);
                source
                    .as_object_mut()
                    .expect("source")
                    .remove("stale_reason");
            }
        }
        projects::save(root, &every)
    })?)
}

fn notify_project_quiet(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "notify-project-quiet", true)?;
    let root = &workspace.root;
    clock("2026-10-21T10:00:00+00:00");
    workspace.use_notifier(&[("notify_snooze_minutes", "15")])?;
    projects::add(root, "billing", Default::default())?;
    projects::add(root, "ledger", Default::default())?;
    crate::set_setting_text(
        root,
        "project_priority",
        "billing=*invoice*, ledger=*ledger*",
    )?;
    edit_quiet(root, "billing", "2026-10-24")?;
    feed::ingest(
        root,
        &[
            event(
                "slack:C1:1.0",
                "dm",
                "2026-10-21T09:30:00Z",
                &[
                    ("title", json!("invoice export")),
                    ("thread", json!("shared")),
                ],
            ),
            event(
                "slack:C1:2.0",
                "dm",
                "2026-10-21T09:31:00Z",
                &[("title", json!("unrelated B")), ("thread", json!("shared"))],
            ),
        ],
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    let candidates = notify::candidates(root, None)?
        .into_iter()
        .map(|row| {
            json!([
                text(field(&row, "ref")),
                truthy(field(&row, "project_quiet"))
            ])
        })
        .collect::<Vec<_>>();
    std::env::set_var("FAKE_NOTIFIER_CLICK", "Snooze");
    notify::run(root, true, None)?;
    std::env::remove_var("FAKE_NOTIFIER_CLICK");
    edit_quiet(root, "ledger", "2026-10-23")?;
    clock("2026-10-21T10:05:00+00:00");
    feed::ingest(
        root,
        &[event(
            "slack:C1:3.0",
            "dm",
            "2026-10-21T10:04:00Z",
            &[("title", json!("unrelated C")), ("thread", json!("shared"))],
        )],
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    notify::run(root, true, None)?;
    clock("2026-10-21T10:16:00+00:00");
    notify::run(root, true, None)?;
    let second_refs = popup_refs(root)?;
    clock("2026-10-21T10:21:00+00:00");
    notify::run(root, true, None)?;
    let third_refs = popup_refs(root)?;
    clock("2026-10-23T09:00:00+00:00");
    notify::run(root, true, None)?;
    let fourth_refs = popup_refs(root)?;
    clock("2026-10-24T09:00:00+00:00");
    notify::run(root, true, None)?;
    notify::run(root, true, None)?;
    let final_refs = popup_refs(root)?;

    let reserved = Workspace::new(base, "reserved-quiet", true)?;
    clock("2026-10-21T10:00:00+00:00");
    reserved.use_notifier(&[])?;
    projects::add(&reserved.root, "billing", Default::default())?;
    crate::set_setting_text(&reserved.root, "project_priority", "billing=*invoice*")?;
    feed::ingest(
        &reserved.root,
        &[event(
            "slack:C1:1.0",
            "dm",
            "2026-10-21T13:30:00Z",
            &[("title", json!("invoice export"))],
        )],
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    let (_stats, popup) = notify::process(&reserved.root, None)?;
    let popup = popup.expect("reserved popup");
    edit_quiet(&reserved.root, "billing", "2026-10-23")?;
    let presented = notify::present(&reserved.root, &text(field(&popup, "id")))?;
    let held = notification_view(&reserved.root)?;
    clock("2026-10-23T09:00:00+00:00");
    let returned = notify::run(&reserved.root, true, None)?;
    let returned_view = notification_view(&reserved.root)?;
    Ok(json!({
        "candidates": candidates,
        "second_refs": second_refs, "third_refs": third_refs, "fourth_refs": fourth_refs, "final_refs": final_refs,
        "calls": workspace.notifier_calls()?, "view": notification_view(&reserved.root)?,
        "presented": presented, "held": held, "reserved_calls": reserved.notifier_calls()?, "returned": returned, "returned_view": returned_view, "returned_calls": reserved.notifier_calls()?,
    }))
}

fn popup_refs(root: &Path) -> Result<Vec<String>> {
    let state = notify::load(root)?;
    Ok(state["popups"]
        .as_array()
        .and_then(|popups| popups.last())
        .and_then(|popup| popup["refs"].as_object())
        .into_iter()
        .flat_map(|refs| refs.values())
        .flat_map(|refs| refs.as_array().into_iter().flatten())
        .map(|value| text(Some(value)))
        .collect())
}

fn agent_task(root: &Path, title: &str) -> Result<Value> {
    let task = notify::task_record(
        root,
        title,
        "agent",
        "the release is Thursday",
        "proposed",
        &[],
    )?;
    notify::inbox_offer(root, &task)?;
    Ok(task)
}

fn seed_suggestion(root: &Path, kind: &str, data: Value) -> Result<String> {
    let item_id = format!("{kind}-kind");
    let item =
        json!({"id": item_id, "type": "proposal", "kind": kind, "status": "open", "data": data});
    crate::write_json(root, "day/inbox.json", &json!({"items": [item]}))?;
    let notification = json!({
        "id": format!("n-{kind}"),
        "key": format!("inbox:{item_id}"),
        "item": item_id,
        "refs": [item_id],
        "count": 1,
        "kind": "suggestion",
        "sub": kind,
        "source": "careerbrew",
        "headline": "careerbrew: a suggestion",
        "why": "a decision waits",
        "next": "accept or dismiss it",
        "priority": 3,
        "priority_why": "3 for a suggestion",
        "at": "2026-10-21T10:00:00+00:00",
        "status": "new",
        "popup": "sent",
        "created_at": "2026-10-21T10:00:00+00:00",
        "updated_at": "2026-10-21T10:00:00+00:00",
        "acted_at": "",
        "snooze_until": "",
        "resolved": "",
    });
    crate::write_json(
        root,
        "day/notifications.json",
        &json!({"items": [notification], "popups": []}),
    )?;
    Ok(format!("n-{kind}"))
}

fn suggestion_kinds(base: &Path) -> Result<Value> {
    let mut out = Map::new();
    for kind in [
        "task-done",
        "new-task",
        "source",
        "feed-rule",
        "plugin",
        "goal",
        "goal-cli",
    ] {
        let workspace = Workspace::new(base, &format!("kind-{kind}"), false)?;
        let root = &workspace.root;
        let data = match kind {
            "task-done" => json!({"task": "t001"}),
            "new-task" => json!({"title": "Ticket work"}),
            "source" => json!({"project": "billing", "kind": "repo", "ref": "example/added"}),
            "feed-rule" => {
                json!({"rule": {"name": "quiet", "when": {"source": "chat"}, "triage": "fyi"}})
            }
            "plugin" => json!({"plugin": "acme", "task": "Document the rollout", "link": ""}),
            _ => {
                json!({"id": "goal-test", "title": "Grow", "kind": "custom", "date": "2026-12-01"})
            }
        };
        if kind == "task-done" {
            priorities::add(root, "Finish me", 3, 3, json!(1.0), None, &[])?;
        }
        if kind == "source" {
            projects::add(root, "billing", Default::default())?;
        }
        let nid = seed_suggestion(
            root,
            if kind.starts_with("goal") {
                "goal"
            } else {
                kind
            },
            data,
        )?;
        let accepted: Value = if kind == "goal-cli" {
            json!(error(
                notify::act(root, &nid, "accept", "cli", None).map(|_| ())
            ))
        } else {
            notify::act(
                root,
                &nid,
                "accept",
                if kind == "goal" { "ui" } else { "cli" },
                None,
            )?
        };
        let tasks = priorities::load(root)?;
        let every = projects::load(root)?;
        let goals = notify::goals_load(root)?;
        let rules = feed::rules(root)?;
        out.insert(
            kind.into(),
            json!({
                "accepted": accepted,
                "tasks": tasks,
                "projects": every,
                "goals": goals,
                "rules": rules,
            }),
        );
    }
    let workspace = Workspace::new(base, "goal-existing", false)?;
    let root = &workspace.root;
    let existing_plan = json!({
        "calendar_connected": true,
        "calendar_errors": [],
        "calibration": 1.25,
        "day": "2026-10-25",
        "focus_minutes": 120,
        "free_minutes": 300,
        "meetings": [],
        "schedule": [{"carry_over": true, "end": "2026-10-25T08:30+00:00", "goal": false, "reason": "existing work", "start": "2026-10-25T08:00+00:00", "task": "owner-task", "title": "Existing work"}],
        "unplaced": [],
    });
    crate::write_json(root, "day/plan-2026-10-25.json", &existing_plan)?;
    let nid = seed_suggestion(
        root,
        "goal",
        json!({"id": "goal-test", "title": "Grow", "kind": "custom", "date": "2026-12-01"}),
    )?;
    let accepted = notify::act(root, &nid, "accept", "ui", None)?;
    out.insert("goal-existing".into(), json!({"accepted": accepted, "plan": read_json(root, "day/plan-2026-10-25.json", json!({}))?}));
    Ok(Value::Object(out))
}

fn notify_suggestions(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "notify-suggestions", true)?;
    let root = &workspace.root;
    clock("2026-10-21T10:00:00+00:00");
    workspace.use_notifier(&[])?;
    let suggestion = agent_task(root, "Review the rollout checklist")?;
    let first = notify::run(root, true, None)?;
    let first_view = notification_view(root)?;
    let first_item = notify::load(root)?["items"][0].clone();
    notify::inbox_dismiss(root, &text(field(&first_item, "item")))?;
    let popup = notify::load(root)?["popups"]
        .as_array()
        .and_then(|popups| popups.last())
        .cloned()
        .expect("first popup");
    let late = apply_click_public(root, &popup, "Accept")?;
    let late_view = notification_view(root)?;
    clock("2026-10-21T10:30:00+00:00");
    agent_task(root, "Update the runbook")?;
    let second = notify::run(root, true, None)?;
    let nid = notify::load(root)?["items"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| text(field(row, "status")) == "new" && text(field(row, "kind")) == "suggestion")
        .map(|row| text(field(row, "id")))
        .expect("new suggestion");
    let accepted = notify::act(root, &nid, "accept", "cli", None)?;
    let accepted_view = notification_view(root)?;
    let inbox_state = notify::inbox_load(root)?
        .into_iter()
        .map(|row| json!([text(field(&row, "id")), text(field(&row, "status"))]))
        .collect::<Vec<_>>();
    let repeated_offer = notify::inbox_offer(root, &suggestion)?;
    let kinds = suggestion_kinds(base)?;
    let clean = clean_text(root)?;
    let sealed = std::fs::read(root.join("day/notifications.json"))?.starts_with(b"CBV1\n");
    Ok(json!({
        "suggestion": suggestion,
        "first": first, "first_view": first_view, "first_calls": workspace.notifier_calls()?,
        "late": late, "late_view": late_view,
        "second": second, "accepted": accepted, "accepted_view": accepted_view,
        "inbox": inbox_state, "repeated_offer": repeated_offer, "kinds": kinds, "calls": workspace.notifier_calls()?, "clean": clean, "sealed": sealed,
    }))
}

fn apply_click_public(root: &Path, popup: &Value, action: &str) -> Result<String> {
    let target = text(field(popup, "target"));
    let refs = popup["refs"]
        .as_object()
        .and_then(|refs| refs.get(&target))
        .and_then(Value::as_array)
        .map(|refs| {
            refs.iter()
                .map(|value| text(Some(value)))
                .collect::<Vec<_>>()
        });
    match notify::act(
        root,
        &target,
        &action.to_lowercase(),
        "popup",
        refs.as_deref(),
    ) {
        Ok(_) => Ok(action.into()),
        Err(_) => Ok(String::new()),
    }
}

fn cli_adapters(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "cli-adapters", true)?;
    let root = &workspace.root;
    let started = cli::live_start(root, &live_sources(&workspace, false)?);
    fill_live(&workspace, false)?;
    let cycled = cli::live_cycle(root);
    let status = cli::live_status(root, true);
    let stopped = cli::live_stop(root);
    github(root)?;
    let review = cli::review_ask(root, SHOP, &[], None);
    let short = cli::review_ask(root, TOOLS, &[], None);
    workspace.use_notifier(&[])?;
    clock("2026-10-21T10:00:00+00:00");
    feed::ingest(
        root,
        &[event("slack:C1:1.0", "dm", "2026-10-21T13:30:00Z", &[])],
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    let checked = cli::notify_check(root, true);
    let listed = cli::notify_list(root, false, false);
    let notify_status = cli::notify_status(root);
    let nid = notify::load(root)?["items"][0]["id"]
        .as_str()
        .expect("notification id")
        .to_owned();
    let opened = cli::notify_open(root, &nid);
    let os_bin = workspace.base.join("os-bin");
    std::fs::create_dir_all(&os_bin)?;
    std::os::unix::fs::symlink(
        fixture("notifier/fake-notifier.sh"),
        os_bin.join("osascript"),
    )?;
    std::env::set_var(
        "PATH",
        format!("{}:/usr/bin:/bin", os_bin.to_string_lossy()),
    );
    std::env::set_var("FAKE_NOTIFIER_LOG", workspace.base.join("osascript.log"));
    std::env::set_var("CAREERBREW_RUST14_PLATFORM", "darwin");
    crate::set_setting_text(root, "notify_backend", "osascript")?;
    feed::ingest(
        root,
        &[event("slack:os:1", "dm", "2026-10-21T13:30:00Z", &[])],
        "user",
        "feed",
        &[],
        &live::Provider,
    )?;
    let osascript_ok = cli::notify_check(root, true);
    let osascript_log =
        std::fs::read_to_string(workspace.base.join("osascript.log")).unwrap_or_default();
    std::fs::remove_file(os_bin.join("osascript"))?;
    let osascript_missing = cli::notify_check(root, true);
    Ok(
        json!({"started": started, "cycled": cycled, "status": status, "stopped": stopped, "review": review, "short": short, "checked": checked, "listed": listed, "notify_status": notify_status, "opened": opened, "osascript_ok": osascript_ok, "osascript_log": osascript_log, "osascript_missing": osascript_missing, "clean": clean_text(root)?}),
    )
}

fn run(case: &str, base: &Path) -> Result<Value> {
    match case {
        "source-roster" => source_roster(base),
        "live-cycle" => live_cycle(base),
        "acquisition-health" => acquisition_health(base),
        "direct-health" => direct_health(base),
        "listen-drafts" => listen_drafts(base),
        "link-review" => link_review(base),
        "review-selection" => review_selection(base),
        "notify-dedup" => notify_dedup(base),
        "notify-time" => notify_time(base),
        "notify-project-quiet" => notify_project_quiet(base),
        "notify-suggestions" => notify_suggestions(base),
        "cli-adapters" => cli_adapters(base),
        _ => Err(FeedError::value("unknown rust-14 case")),
    }
}

pub fn run_all(selected: Option<&str>) -> Result<String> {
    let base = root()?.join("rust14-workload");
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
    Ok(compact(&typed(&json!({"cases": cases})))? + "\n")
}
