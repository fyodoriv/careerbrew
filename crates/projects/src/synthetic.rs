//! Synthetic rust-09 workload over the native component writers.

use crate::{
    cli, field, focus, priorities, project_priority, projects, promises, team, text, Result,
};
use chrono::NaiveDate;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const CASES: &[&str] = &[
    "project-lifecycle",
    "readiness-gates",
    "priorities-tickets",
    "promise-lifecycle",
    "lock-and-team",
    "cli-handlers",
];

struct Workspace {
    root: PathBuf,
}
impl Workspace {
    fn new(base: &Path, name: &str) -> Result<Self> {
        let base = base.join(name);
        let root = base.join("data");
        std::fs::create_dir_all(&root)?;
        std::fs::set_permissions(&root, Permissions::from_mode(0o700))?;
        std::fs::create_dir_all(base.join("home"))?;
        std::env::set_var("HOME", base.join("home"));
        std::env::set_var("CAREERBREW_HOME", &root);
        std::env::set_var("CAREERBREW_KEY_FILE", base.join("keys/backup.key"));
        std::env::set_var("CAREERBREW_EVENTS", "off");
        std::env::set_var("CAREERBREW_ENCRYPT", "off");
        std::env::set_var("CAREERBREW_NOW", "2026-11-02T09:00:00+00:00");
        std::env::set_var("CAREERBREW_TEST_OWNER", "1");
        std::env::set_var("TZ", "UTC");
        Ok(Self { root })
    }
}

fn error(result: Result<()>) -> String {
    result
        .err()
        .map(|error| error.class)
        .unwrap_or("accepted")
        .to_owned()
}

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/projects")
        .join(path)
}

fn selected(value: &Value, keys: &[&str]) -> Value {
    let mut out = Map::new();
    for key in keys {
        if let Some(item) = value.get(*key) {
            out.insert((*key).into(), item.clone());
        }
    }
    Value::Object(out)
}

fn identity_controls(root: &Path, name: &str) -> Result<Value> {
    projects::with_lock(root, || {
        let mut every = projects::load(root)?;
        every[name]
            .as_object_mut()
            .expect("project object")
            .remove("incarnation_id");
        projects::save(root, &every)
    })?;
    let (staged, migrated) = projects::session_record(root, name)?;
    let generated = text(field(&staged, "incarnation_id"));
    let persisted = text(field(&projects::load(root)?[name], "incarnation_id"));
    let (repeated, repeat_migrated) = projects::session_record(root, name)?;
    let mut invalid = Vec::new();
    for bad in ["A".repeat(32), "g".repeat(32), "abc".to_owned()] {
        projects::with_lock(root, || {
            let mut every = projects::load(root)?;
            every[name]["incarnation_id"] = json!(bad);
            projects::save(root, &every)
        })?;
        invalid.push(error(projects::session_record(root, name).map(|_| ())));
    }
    projects::with_lock(root, || {
        let mut every = projects::load(root)?;
        every[name]["incarnation_id"] = json!(12345678901234567890123456789012_u128);
        projects::save(root, &every)
    })?;
    invalid.push(error(projects::session_record(root, name).map(|_| ())));
    projects::with_lock(root, || {
        let mut every = projects::load(root)?;
        every[name]["incarnation_id"] = json!(generated);
        projects::save(root, &every)
    })?;
    Ok(json!({
        "migrated": migrated,
        "generated_shape": projects::valid_identity(&staged),
        "persisted": persisted == generated,
        "repeat_migrated": repeat_migrated,
        "repeat_stable": text(field(&repeated, "incarnation_id")) == generated,
        "invalid": invalid,
    }))
}

fn project_lifecycle(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "project-lifecycle")?;
    if std::env::var("CAREERBREW_RUST09_PLANT_WRITE").as_deref() == Ok("extra") {
        std::fs::write(workspace.root.join("planted-extra.txt"), "synthetic")?;
    }
    let root = &workspace.root;
    let cutline_raw = std::fs::read(fixture("cutline.json"))?;
    let goals_raw = std::fs::read(fixture("team-goals.csv"))?;
    let cutline: Value = serde_json::from_slice(&cutline_raw)?;
    let rows = cutline["projects"].as_array().expect("fixture projects");
    projects::add(
        root,
        "search",
        projects::AddOptions {
            title: text(field(&rows[0], "title")),
            group: text(field(&rows[0], "group")),
            ..Default::default()
        },
    )?;
    projects::add(
        root,
        "checkout",
        projects::AddOptions {
            title: text(field(&rows[2], "title")),
            ..Default::default()
        },
    )?;
    projects::add(
        root,
        "search-ranking",
        projects::AddOptions {
            parent: "search".into(),
            links: vec!["checkout".into()],
            ..Default::default()
        },
    )?;
    projects::add_source(root, "search", "tracker", "SRCH-1")?;
    projects::add_source(
        root,
        "search",
        "url",
        "https://docs.google.com/document/d/synthetic",
    )?;
    projects::check(root, "search", Some("SRCH-1"), "source checked")?;
    projects::record_update(root, "search", "approval", "SYNTHCANARY approval")?;
    let first = text(field(&projects::load(root)?["search"], "incarnation_id"));
    let (staged, migrated) =
        projects::with_lock(root, || projects::session_record(root, "search"))?;
    let second = text(field(&staged, "incarnation_id"));
    let every = projects::load(root)?;
    let restricted = projects::watch_list(root)?
        .into_iter()
        .find(|row| text(field(row, "name")) == "search")
        .expect("restricted project");
    let shown = projects::show(root, "search")?;
    let sources = shown["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            selected(
                row,
                &["kind", "ref", "checked_at", "status", "stale", "reference"],
            )
        })
        .collect::<Vec<_>>();
    let tree = projects::tree(root)?
        .into_iter()
        .map(|row| json!([text(field(&row, "name")), row["depth"]]))
        .collect::<Vec<_>>();
    Ok(json!({
        "fixture_sha256": {
            "cutline": format!("{:x}", Sha256::digest(&cutline_raw)),
            "team_goals": format!("{:x}", Sha256::digest(&goals_raw)),
            "goal_rows": goals_raw.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()).count() - 1,
        },
        "identity": {"shape": projects::valid_identity(&every["search"]), "stable": first == second, "migrated": migrated},
        "identity_controls": identity_controls(root, "checkout")?,
        "relations": {"parent": text(field(&every["search-ranking"], "parent")), "links": every["search-ranking"]["links"], "root": projects::root_name(&every, "search-ranking"), "notes_path": projects::notes_path(&every, "search-ranking")},
        "sources": sources,
        "approval": {"kind": text(field(&restricted["latest_update"], "kind")), "restricted_has_text": restricted["latest_update"].get("text").is_some(), "stored": text(field(every["search"]["updates"].as_array().and_then(|rows| rows.last()).expect("approval"), "text")) == "SYNTHCANARY approval"},
        "errors": {
            "duplicate": error(projects::add(root, "search", Default::default()).map(|_| ())),
            "kind": error(projects::record_update(root, "search", "invented", "example").map(|_| ())),
            "graph": error(projects::edit_parent(root, "search", "search-ranking")),
            "remove": error(projects::remove(root, "search")),
        },
        "tree": tree,
        "stale_count": projects::stale_sources(root)?.len(),
    }))
}

fn gate_rows(root: &Path, name: &str) -> Result<Vec<Value>> {
    let shown = projects::show(root, name)?;
    Ok(shown["prep"]["gates"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| selected(row, &["gate", "done", "at", "note", "fix"]))
        .collect())
}

fn readiness_gates(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "readiness-gates")?;
    let root = &workspace.root;
    projects::add(
        root,
        "demo",
        projects::AddOptions {
            kind: "prep".into(),
            event_on: "2026-11-03".into(),
            ..Default::default()
        },
    )?;
    let initial = projects::show(root, "demo")?["prep"].clone();
    projects::add_source(root, "demo", "url", "https://example.com/rubric")?;
    projects::add_source(root, "demo", "doc", "pack.md")?;
    projects::check(root, "demo", None, "")?;
    let after_check = gate_rows(root, "demo")?;
    projects::gate(root, "demo", "tool", &[], false)?;
    projects::gate(root, "demo", "questions", &[], false)?;
    let first_quiz = projects::quiz(root, "demo", 10, 3)?;
    let short_requiz = error(projects::quiz(root, "demo", 2, 0).map(|_| ()));
    projects::quiz(root, "demo", 3, 1)?;
    projects::quiz(root, "demo", 1, 0)?;
    projects::gate(
        root,
        "demo",
        "links",
        &["https://example.com/rubric".into()],
        false,
    )?;
    let archive_refused = projects::archive(root, "demo", true, false)
        .err()
        .map(|error| error.message)
        .unwrap_or_else(|| "accepted".into());
    let blocks_before = projects::gate_blocks(
        root,
        NaiveDate::from_ymd_opt(2026, 11, 3).expect("fixture day"),
    )?
    .into_iter()
    .map(|row| selected(&row, &["id", "minutes", "gate", "project", "reason"]))
    .collect::<Vec<_>>();
    projects::gate(root, "demo", "links", &["pack.md".into()], false)?;
    let ready = projects::show(root, "demo")?["prep"].clone();
    projects::archive(root, "demo", true, false)?;
    let archived = projects::list_rows(root)?[0]["archived"].clone();
    let blocks_after = projects::gate_blocks(
        root,
        NaiveDate::from_ymd_opt(2026, 11, 3).expect("fixture day"),
    )?;
    let bad_counts = [(0, 0), (3, 4), (5, -1), (501, 0)]
        .into_iter()
        .map(|(asked, missed)| error(projects::quiz(root, "demo", asked, missed).map(|_| ())))
        .collect::<Vec<_>>();
    Ok(json!({
        "initial": selected(&initial, &["done", "total", "ready", "event_on"]),
        "after_check": after_check,
        "first_quiz": first_quiz["quiz"],
        "short_requiz": short_requiz,
        "archive_refused": archive_refused,
        "ready": selected(&ready, &["done", "ready", "quizzes", "quiz"]),
        "blocks_before": blocks_before,
        "archived": archived,
        "blocks_after": blocks_after,
        "bad_counts": bad_counts,
    }))
}

fn priority_view(row: &Value) -> Value {
    let mut positions = BTreeMap::new();
    positions.insert(text(field(row, "id")), 1);
    json!({"id": row["id"], "score": row["score"], "estimate": priorities::estimate(row), "parts": row["parts"], "why": focus::task_why(row, &positions, 1)})
}

fn priorities_tickets(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "priorities-tickets")?;
    let root = &workspace.root;
    projects::add(root, "billing", Default::default())?;
    projects::add(root, "search", Default::default())?;
    project_priority::set_project(
        root,
        "billing",
        None,
        Some(vec!["BILL-*".into(), "api-*".into()]),
    )?;
    project_priority::set_project(root, "search", None, Some(vec!["SRCH-*".into()]))?;
    let entries = project_priority::entries(root)?;
    let configured =
        project_priority::keys("Fix UTF-8 and ISO-8601; BILL-42", "", "", "", &entries);
    let generic = project_priority::keys("Fix UTF-8, ISO-8601 and ABC-123", "", "", "", &[]);
    let linked = project_priority::keys(
        "See the ticket",
        "",
        "https://jira.example.com/browse/OPS-3",
        "",
        &[],
    );
    let bad_hours = ["nan", "inf", "-inf", "0.01", "201"]
        .into_iter()
        .map(|value| error(priorities::hours_text(value).map(|_| ())))
        .collect::<Vec<_>>();
    let estimates = ["nan", "inf", "-1", "abc"]
        .into_iter()
        .map(|value| priorities::estimate(&json!({"estimate_h": value})))
        .chain([
            priorities::estimate(&json!({"estimate_h": null})),
            priorities::estimate(&json!({"estimate_h": 0.05})),
            priorities::estimate(&json!({"estimate_h": 200})),
        ])
        .collect::<Vec<_>>();
    let first = priorities::add(
        root,
        "Fix UTF-8 and BILL-42",
        4,
        4,
        json!(2),
        Some("2026-11-03"),
        &["ticket=https://jira.example.com/browse/BILL-42".into()],
    )?;
    let _second = priorities::add(root, "SRCH-7 ranking", 3, 3, json!(1), None, &[])?;
    projects::link_task(root, "billing", &text(field(&first, "id")))?;
    let ranked = priorities::rank(
        root,
        NaiveDate::from_ymd_opt(2026, 11, 2).expect("fixture day"),
    )?;
    let regraded = priorities::regrade(root, "t002", Some(5), Some(2), Some(json!(1.5)))?;
    let impacted = priorities::record_impact(
        root,
        "t001",
        priorities::ImpactInput {
            expected: "Fewer synthetic failures",
            who: "support",
            metric: "failures per day",
            ..Default::default()
        },
    )?;
    let done = priorities::done(root, "t001", json!(1))?;
    let already_done = error(priorities::done(root, "t001", json!(1)).map(|_| ()));
    let old_link = "https://evidence.example.com/old".to_owned();
    let new_link = "https://evidence.example.com/new".to_owned();
    priorities::record_impact(
        root,
        "t001",
        priorities::ImpactInput {
            evidence: &[old_link.clone(), old_link.clone()],
            ..Default::default()
        },
    )?;
    std::env::set_var("CAREERBREW_ENCRYPT", "on");
    let measured = priorities::record_impact(
        root,
        "t001",
        priorities::ImpactInput {
            result: "  Fewer   failures ",
            evidence: &[old_link.clone(), new_link.clone()],
            ..Default::default()
        },
    )?;
    let evidence_path = root.join("work/evidence.jsonl");
    let evidence_lock = root.join("work/.evidence.jsonl.lock");
    let notes = crate::read_jsonl(root, "work/evidence.jsonl")?
        .iter()
        .filter(|row| text(field(row, "kind")) == "note")
        .map(|row| {
            selected(
                row,
                &[
                    "id", "kind", "title", "impact", "at", "tags", "origin", "links",
                ],
            )
        })
        .collect::<Vec<_>>();
    let rounding = [1.125, -1.125, 1.135, 1.124999999, 1.125000001, -1.125000001]
        .into_iter()
        .map(|value| crate::python_round(value, 2))
        .collect::<Vec<_>>();
    Ok(json!({
        "entries": entries,
        "configured_keys": configured,
        "generic_keys": generic,
        "linked_keys": linked,
        "ticket_projects": project_priority::ticket_projects(&entries).into_iter().collect::<Vec<_>>(),
        "repos": [project_priority::repo_of("pr:acme/api#4", ""), project_priority::repo_of("git:local:abcdef", ""), project_priority::repo_of("", "https://github.com/acme/web/pull/2")],
        "bonus": ([0, 1, 2, 3].into_iter().map(|rank| project_priority::bonus(rank, 2)).collect::<Vec<_>>()),
        "bad_hours": bad_hours,
        "estimates": estimates,
        "ranked": ranked.iter().map(priority_view).collect::<Vec<_>>(),
        "regraded": selected(&regraded, &["id", "impact", "urgency", "estimate_h"]),
        "impact": impacted["expected_impact"],
        "measured_result": {
            "task": measured["measured_impact"],
            "notes": notes,
            "sealed": std::fs::read(&evidence_path)?.starts_with(b"CBV1\n"),
            "mode": std::fs::metadata(&evidence_path)?.permissions().mode() & 0o777,
            "lock": {"exists": evidence_lock.exists(), "mode": std::fs::metadata(&evidence_lock)?.permissions().mode() & 0o777},
        },
        "rounding": rounding,
        "done": selected(&done, &["id", "status", "actual_h", "estimate_h"]),
        "already_done": already_done,
        "focus_blocks": focus::blocks(),
        "due_text": ([-2, -1, 0, 1, 2].into_iter().map(focus::due_text).collect::<Vec<_>>()),
    }))
}

fn promise_lifecycle(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "promise-lifecycle")?;
    let root = &workspace.root;
    projects::add(
        root,
        "billing",
        projects::AddOptions {
            title: "Billing service".into(),
            ..Default::default()
        },
    )?;
    let rows = vec![
        json!({"id": "slack:D1:1", "source": "slack", "kind": "dm", "at": "2026-11-02T08:00:00Z", "title": "DM from Sam", "actor": "other", "to_me": true, "from": "Sam", "thread": "slack:D1"}),
        json!({"id": "slack:D1:2", "source": "slack", "kind": "dm", "at": "2026-11-02T08:05:00Z", "title": "You wrote in a DM", "actor": "me", "thread": "slack:D1", "url": "https://slack.example.com/archives/D1/p2", "project": "Billing service"}),
    ];
    let original = "Thanks! I'll send the rollout notes tomorrow".to_owned();
    let made = promises::track(root, &rows, &[(rows[1].clone(), original.clone())])?;
    assert!(!made.is_empty());
    let initial = promises::open_list(root, true, false)?;
    let name = text(field(&initial[0], "name"));
    let restricted = promises::open_list(root, false, false)?;
    promises::step(root, &name, "research", "read the synthetic log")?;
    let after_research = text(field(&promises::open_list(root, true, false)?[0], "next"));
    promises::step(root, &name, "draft", "synthetic follow-up drafted")?;
    let after_draft = text(field(&promises::open_list(root, true, false)?[0], "next"));
    let bad_step = error(promises::step(root, &name, "send", "not allowed").map(|_| ()));
    promises::close(root, &name, "done")?;
    let closed = promises::open_list(root, true, true)?;
    let mut originals = Map::new();
    originals.insert(
        "slack:D1:2".into(),
        json!({"id": "slack:D1:2", "at": "2026-11-02T08:05:00Z", "text": original}),
    );
    let rescan = promises::scan(root, &rows, &originals)?;
    let after_rescan = promises::open_list(root, true, true)?;
    let manual_a = promises::add(
        root,
        "Send the synthetic notes",
        "Kim",
        "2026-10-30",
        "",
        "",
    )?;
    let manual_b = promises::add(
        root,
        "Send the synthetic notes",
        "Kim",
        "2026-10-30",
        "",
        "",
    )?;
    projects::archive(root, &manual_a, true, false)?;
    projects::archive(root, &manual_a, false, false)?;
    let manual_reopened = promises::open_list(root, true, false)?
        .into_iter()
        .find(|row| text(field(row, "name")) == manual_a)
        .is_some_and(|row| text(field(&row, "closed")).is_empty());
    let positives = [
        "Thanks! I'll look into the timeout.",
        "I will send it tomorrow",
        "ok, I'm going to fix the flaky test by Friday!",
        "Let me check the logs today",
        "i'll write the doc next week",
        "I can review it by 2026-11-04",
        "Sure <@U1>, I'll ping <https://x.example.com/a|the owners> in 2 days",
    ]
    .into_iter()
    .map(|value| {
        promises::detect(
            value,
            NaiveDate::from_ymd_opt(2026, 11, 2).expect("fixture day"),
        )
    })
    .collect::<Vec<_>>();
    let negatives = [
        "Let me know when it is done",
        "I'll be out tomorrow",
        "I will not do that",
        "I'll check.",
        "I can review it",
        "I can do that by Friday?",
        "The timeout fired again",
    ]
    .into_iter()
    .map(|value| {
        promises::detect(
            value,
            NaiveDate::from_ymd_opt(2026, 11, 2).expect("fixture day"),
        )
    })
    .collect::<Vec<_>>();
    Ok(json!({
        "initial": initial, "restricted": restricted, "after_research": after_research, "after_draft": after_draft,
        "bad_step": bad_step, "closed": closed, "rescan": rescan, "after_rescan": after_rescan,
        "manual_distinct": manual_a != manual_b, "manual_reopened": manual_reopened,
        "positive": positives, "negative": negatives, "stale_sources": projects::stale_sources(root)?,
        "handoff": promises::handoff_items(root, false)?,
        "bad_close": error(promises::close(root, &name, "invented")),
        "bad_due": error(promises::add(root, "Send it", "", "next friday", "", "").map(|_| ())),
        "bad_link": error(promises::add(root, "Send it", "", "", "javascript:alert(1)", "").map(|_| ())),
    }))
}

fn lock_and_team(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "lock-and-team")?;
    let root = &workspace.root;
    projects::with_lock(root, || {
        projects::add(root, "billing", Default::default())?;
        projects::record_update(root, "billing", "team", "nested update").map(|_| ())
    })?;
    projects::add_source(root, "billing", "tracker", "BILL-1")?;
    projects::add(root, "search", Default::default())?;
    projects::add_source(root, "search", "tracker", "SRCH-1")?;
    let handles = ["billing", "search"].into_iter().map(|name| {
        let root = root.clone();
        let name = name.to_owned();
        std::thread::spawn(move || projects::with_lock(&root, || {
            let mut every = projects::load(&root)?;
            std::thread::sleep(Duration::from_millis(50));
            let mut updates = every[&name]["updates"].as_array().cloned().unwrap_or_default();
            updates.push(json!({"at": "2026-11-02T09:00:00+00:00", "kind": "project", "text": format!("update {name}")}));
            every[&name]["updates"] = Value::Array(updates);
            every[&name]["sources"][0]["status"] = json!(format!("seen {name}"));
            projects::save(&root, &every)
        }))
    }).collect::<Vec<_>>();
    for handle in handles {
        handle.join().expect("writer thread")?;
    }
    let stored = projects::load(root)?;
    let mut lock_rows = Map::new();
    for name in ["billing", "search"] {
        lock_rows.insert(name.into(), json!({
            "updates": stored[name]["updates"].as_array().into_iter().flatten().map(|row| text(field(row, "text"))).collect::<Vec<_>>(),
            "status": text(field(&stored[name]["sources"][0], "status")),
        }));
    }
    let promise_name = promises::add(root, "Close the synthetic promise", "", "", "", "")?;
    let close_root = root.clone();
    let close_name = promise_name.clone();
    let closer = projects::with_lock(root, || {
        let mut every = projects::load(root)?;
        every[&promise_name]["promise"]["owner_note"] = json!("owner edit kept");
        let closer = std::thread::spawn(move || promises::close(&close_root, &close_name, "done"));
        std::thread::sleep(Duration::from_millis(50));
        projects::save(root, &every)?;
        Ok(closer)
    })?;
    closer.join().expect("closer thread")?;
    let closed = projects::load(root)?[&promise_name].clone();
    let added = team::add(root, "Sam Lee", "U0SAM", "SAM@example.com", "@samlee")?;
    let unchanged = team::add(root, "sam lee", "U0SAM", "sam@example.com", "samlee")?;
    let roster = team::load(root)?;
    let matches = vec![
        team::matches(&roster, "U0SAM", "", ""),
        team::matches(&roster, "", "sam@example.com", ""),
        team::matches(&roster, "", "", "Sam Lee owns this"),
        team::matches(&roster, "", "", "<@u0sam> and @SAMLEE"),
        team::matches(&roster, "", "", "max retries"),
    ];
    let bad_chat = error(team::add(root, "Bad Chat", "not-an-id", "", "").map(|_| ()));
    let unknown_remove = error(team::remove(root, "Nobody"));
    team::remove(root, "Sam Lee")?;
    Ok(json!({
        "nested": stored["billing"]["updates"][0]["text"] == "nested update",
        "lock_rows": lock_rows,
        "owner_closure": {"closed": text(field(&closed["promise"], "closed")), "owner_kept": text(field(&closed["promise"], "owner_note")) == "owner edit kept", "archived": !text(field(&closed, "archived_at")).is_empty()},
        "team_added": added, "team_unchanged": unchanged, "matches": matches,
        "bad_chat": bad_chat, "unknown_remove": unknown_remove, "team_after_remove": team::load(root)?,
    }))
}

fn cli_handlers(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "cli-handlers")?;
    let root = &workspace.root;
    priorities::add(root, "Synthetic task", 3, 3, json!(2), None, &[])?;
    let (exit, stdout) =
        cli::work_priority(root, "set", "billing", Some(1), Some(vec!["BILL-*".into()]));
    let priority_set = cli::invocation(exit, stdout);
    let (exit, stdout) = cli::work_priority(root, "set", "api", None, Some(vec!["api".into()]));
    let priority_set_api = cli::invocation(exit, stdout);
    let rows = [
        json!({"id": "result-1", "at": "2026-11-02T08:00:00Z", "kind": "ticket", "title": "BILL-100", "source": "synthetic", "origin": "user"}),
        json!({"id": "result-2", "at": "2026-11-02T08:01:00Z", "kind": "ticket", "title": "BILL-101 API-2", "source": "synthetic", "origin": "user"}),
        json!({"id": "result-3", "at": "2026-11-02T08:02:00Z", "kind": "other", "title": "unmatched", "source": "synthetic", "origin": "user"}),
        json!({"id": "pr:acme/api#4", "at": "2026-11-02T08:03:00Z", "kind": "pr", "title": "unmatched", "source": "synthetic", "origin": "user"}),
    ];
    let context = crate::context()?;
    let store = context.store();
    store.upsert_jsonl(
        &root.join("day/2026-11-02.jsonl"),
        &rows
            .iter()
            .map(careerbrew_setup::value::to_canon)
            .collect::<Vec<_>>(),
        &[
            "kind",
            "title",
            "draft",
            "state",
            "merged_at",
            "feed_source",
        ],
    )?;
    std::env::set_var("CAREERBREW_ENCRYPT", "on");
    let (exit, stdout) = cli::work_priority(root, "show", "", None, None);
    let priority_show = cli::invocation(exit, stdout);
    let (exit, stdout) = cli::work_task_set(root, "t001", Some(4), Some(5), Some(json!(1.5)));
    let task_set = cli::invocation(exit, stdout);
    let (exit, stdout) = cli::work_impact(
        root,
        "t001",
        priorities::ImpactInput {
            expected: "Fewer synthetic failures",
            who: "support",
            metric: "failures per day",
            ..Default::default()
        },
    );
    let impact = cli::invocation(exit, stdout);
    let (exit, stdout) = cli::work_done(root, "t001", json!(1));
    let done = cli::invocation(exit, stdout);
    let evidence = vec![
        "https://evidence.example.com/old".into(),
        "https://evidence.example.com/new".into(),
    ];
    let (exit, stdout) = cli::work_impact(
        root,
        "t001",
        priorities::ImpactInput {
            result: "  CLI   result ",
            evidence: &evidence,
            ..Default::default()
        },
    );
    let impact_result = cli::invocation(exit, stdout);
    let (exit, stdout) = cli::work_impact(
        root,
        "t001",
        priorities::ImpactInput {
            result: "uncertain",
            unsure: true,
            ..Default::default()
        },
    );
    let impact_unsure = cli::invocation(exit, stdout);
    let (exit, stdout) = cli::work_task_set(root, "t001", None, None, None);
    let task_set_empty = cli::invocation(exit, stdout);
    let (exit, stdout) = cli::work_impact(root, "t001", priorities::ImpactInput::default());
    let impact_empty = cli::invocation(exit, stdout);
    let evidence_path = root.join("work/evidence.jsonl");
    std::fs::write(&evidence_path, "not json\n")?;
    let (exit, stdout) = cli::work_priority(root, "show", "", None, None);
    let parse_fallback = cli::invocation(exit, stdout);
    std::fs::write(&evidence_path, b"CBV1\ninvalid\n")?;
    let (exit, stdout) = cli::work_priority(root, "show", "", None, None);
    let decrypt_fallback = cli::invocation(exit, stdout);
    std::fs::remove_file(&evidence_path)?;
    std::fs::create_dir(&evidence_path)?;
    let (exit, stdout) = cli::work_priority(root, "show", "", None, None);
    let read_fallback = cli::invocation(exit, stdout);
    match std::env::var("CAREERBREW_RUST09_PLANT_WRITE").as_deref() {
        Ok("missing") => std::fs::remove_file(root.join("work/tasks.yaml"))?,
        Ok("duplicate") => {
            let path = base.join("lock-and-team/data/work/projects.yaml");
            let bytes = std::fs::read(&path)?;
            std::fs::write(path, bytes)?;
        }
        _ => {}
    }
    Ok(json!({
        "priority_set": priority_set, "priority_set_api": priority_set_api, "priority_show": priority_show,
        "task_set": task_set, "impact": impact, "done": done, "impact_result": impact_result,
        "impact_unsure": impact_unsure, "task_set_empty": task_set_empty, "impact_empty": impact_empty,
        "evidence_fallback": {"parse": parse_fallback, "decrypt": decrypt_fallback, "read": read_fallback}
    }))
}

fn run(case: &str, base: &Path) -> Result<Value> {
    match case {
        "project-lifecycle" => project_lifecycle(base),
        "readiness-gates" => readiness_gates(base),
        "priorities-tickets" => priorities_tickets(base),
        "promise-lifecycle" => promise_lifecycle(base),
        "lock-and-team" => lock_and_team(base),
        "cli-handlers" => cli_handlers(base),
        _ => Err(crate::ProjectError::value("unknown rust-09 case")),
    }
}

pub fn run_all(selected: Option<&str>) -> Result<String> {
    let base = crate::root()?.join("rust09-workload");
    std::fs::create_dir_all(&base)?;
    let mut cases = Vec::new();
    for case in CASES {
        if selected.is_none_or(|selected| selected == *case) {
            cases.push(json!({"id": case, "result": run(case, &base)?}));
        }
    }
    let report = crate::typed(&json!({"cases": cases}));
    Ok(serde_json::to_string(&report)? + "\n")
}
