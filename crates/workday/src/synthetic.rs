//! Synthetic rust-15 differential workload over native component functions.

use crate::{
    error, field, questions, read_json, reset_write_count, stages, status, steps, text, typed,
    work_session, write_count, write_json, Result,
};
use careerbrew_projects::projects::{self, AddOptions};
use chrono::NaiveDate;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const CASES: &[&str] = &[
    "status-schema",
    "question-records",
    "step-validation",
    "stage-readiness",
    "work-session",
    "legacy-receipts",
];
const CANARY: &str = "SYNTHCANARY-rust15-private";

struct Workspace {
    base: PathBuf,
    root: PathBuf,
}

impl Workspace {
    fn new(base: &Path, name: &str, project_names: &[&str]) -> Result<Self> {
        let base = base.join(name);
        let root = base.join("data");
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(base.join("home"))?;
        std::env::set_var("HOME", base.join("home"));
        std::env::set_var("CAREERBREW_HOME", &root);
        std::env::set_var("CAREERBREW_KEY_FILE", base.join("keys/backup.key"));
        std::env::set_var("CAREERBREW_EVENTS", "off");
        std::env::set_var("CAREERBREW_ENCRYPT", "on");
        std::env::set_var("CAREERBREW_NOW", "2026-11-02T09:00:00+00:00");
        std::env::set_var("CAREERBREW_TEST_OWNER", "1");
        std::env::set_var("CAREERBREW_RUST13_WRITE_TRACE", base.join("writes.log"));
        std::env::set_var("TZ", "UTC");
        std::env::remove_var("CAREERBREW_TEST_BUSINESS");
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../profile/examples");
        for relative in [
            "day/calendars.yaml",
            "jobs/criteria.yaml",
            "jobs/sources.yaml",
            "mail.yaml",
            "profile.yaml",
            "sensitive-terms.txt",
            "settings.yaml",
            "work/sources.yaml",
        ] {
            let text = std::fs::read_to_string(examples.join(relative))?;
            crate::write_text(&root, relative, &text)?;
        }
        for project in project_names {
            projects::add(&root, project, AddOptions::default())?;
        }
        reset_write_count(&base)?;
        Ok(Self { base, root })
    }
}

fn at(value: &str) {
    std::env::set_var("CAREERBREW_NOW", value);
}

fn day(value: &str) -> NaiveDate {
    crate::day(value).expect("fixture day")
}

fn error_of(result: Result<()>) -> String {
    result
        .err()
        .map(|error| error.class)
        .unwrap_or("accepted")
        .to_owned()
}

fn merged(base: &Value, extra: Value) -> Value {
    let mut out = base.clone();
    if let (Some(target), Some(source)) = (out.as_object_mut(), extra.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
    out
}

fn set_setting(root: &Path, key: &str, value: i64) -> Result<()> {
    let path = root.join("settings.yaml");
    careerbrew_vault::lock::with_lock(&path, || {
        let mut settings = crate::read_yaml(root, "settings.yaml", json!({}))?;
        settings[key] = json!(value);
        careerbrew_projects::write_yaml(root, "settings.yaml", &settings)
    })?
}

fn status_case(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "status-schema", &[])?;
    let root = &ws.root;
    let day = day("2026-11-02");
    let blank = status::blank("/careerbrew");
    write_json(
        root,
        "day/work-2026-11-02.json",
        &json!({
            "rows": [{"step": "/careerbrew", "status": "waiting"}],
            "handled": {"pr:example/tools#1": "/careerbrew"},
            "start": {"preflight": [{"check": "synthetic", "ok": true}]},
            "questions": [],
        }),
    )?;
    let legacy = status::stored(root, day)?["/careerbrew"].clone();
    let names = vec!["/careerbrew".to_owned(), "/standup".to_owned()];
    let accepted = format!("  Approve\u{2003}{}  ", "é".repeat(192));
    let running = status::update(
        root,
        "/careerbrew",
        &names,
        &status::Update {
            status: Some("running".into()),
            summary: Some(accepted),
            needs_you: Some("Need\u{2003}a decision".into()),
            ..Default::default()
        },
        day,
    )?;
    let repeated = status::update(
        root,
        "/careerbrew",
        &names,
        &status::Update {
            status: Some("running".into()),
            ..Default::default()
        },
        day,
    )?;
    let summary_only = status::update(
        root,
        "/careerbrew",
        &names,
        &status::Update {
            summary: Some("Finished \u{2003}now".into()),
            ..Default::default()
        },
        day,
    )?;
    let errors = vec![
        error_of(
            status::update(
                root,
                "/careerbrew",
                &names,
                &status::Update {
                    summary: Some("x".repeat(201)),
                    ..Default::default()
                },
                day,
            )
            .map(|_| ()),
        ),
        error_of(
            status::update(
                root,
                "/careerbrew",
                &names,
                &status::Update {
                    link: Some("javascript:alert(1)".into()),
                    ..Default::default()
                },
                day,
            )
            .map(|_| ()),
        ),
        error_of(
            status::update(
                root,
                "/careerbrew",
                &names,
                &status::Update {
                    status: Some("failed".into()),
                    error_class: Some("other".into()),
                    ..Default::default()
                },
                day,
            )
            .map(|_| ()),
        ),
        error_of(
            status::update(
                root,
                "/careerbrew",
                &names,
                &status::Update {
                    status: Some("blocked".into()),
                    needs_you: Some("".into()),
                    ..Default::default()
                },
                day,
            )
            .map(|_| ()),
        ),
    ];
    let claim = status::update(
        root,
        "/standup",
        &names,
        &status::Update {
            status: Some("done".into()),
            handled_items: vec!["checkout:~/synthetic".into()],
            ..Default::default()
        },
        day,
    )?;
    let mut errors = errors;
    errors.push(error_of(
        status::update(
            root,
            "/careerbrew",
            &names,
            &status::Update {
                handled_items: vec!["checkout:~/synthetic".into()],
                ..Default::default()
            },
            day,
        )
        .map(|_| ()),
    ));
    reset_write_count(&ws.base)?;
    let stale = status::timed_out(root, "/careerbrew", &names, "not-the-current-start", day)?;
    let noop_writes = write_count(&ws.base);
    let blocked = status::update(
        root,
        "/careerbrew",
        &names,
        &status::Update {
            status: Some("blocked".into()),
            needs_you: Some("Owner picks".into()),
            ..Default::default()
        },
        day,
    )?;
    let retry_running = status::update(
        root,
        "/standup",
        &names,
        &status::Update {
            status: Some("running".into()),
            ..Default::default()
        },
        day,
    )?;
    let timed = status::timed_out(
        root,
        "/standup",
        &names,
        &text(field(&retry_running, "started")),
        day,
    )?;
    if std::env::var("CAREERBREW_RUST15_PLANT_WRITE").as_deref() == Ok("extra") {
        std::fs::write(root.join("planted-extra.txt"), "synthetic")?;
    }
    Ok(json!({
        "blank": blank,
        "legacy": legacy,
        "running": running,
        "repeated_attempt": repeated["attempt"],
        "summary_only": summary_only,
        "errors": errors,
        "claim": claim,
        "stale_timeout": stale,
        "noop_writes": noop_writes,
        "blocked": blocked,
        "timed": timed,
        "handled": status::handled(root, day)?,
        "counts": status::counts(&status::rows(root, &names, day)?),
        "row_order": crate::read_json(root, "day/work-2026-11-02.json", json!({}))?["rows"].as_array().into_iter().flatten().map(|row| text(field(row, "step"))).collect::<Vec<_>>(),
        "start_kept": status::start_record(root, day)?,
        "sealed": std::fs::read(status::path_for(root, day))?.starts_with(b"CBV1\n"),
        "plain_absent": !std::fs::read(status::path_for(root, day))?.windows(CANARY.len()).any(|part| part == CANARY.as_bytes()),
    }))
}

fn question_records(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "question-records", &[])?;
    let root = &ws.root;
    at("2026-11-01T07:00:00+00:00");
    let first = questions::add(
        root,
        "/careerbrew",
        &format!("  Approve\u{2003}{CANARY}?  "),
        Some(&["merge".into(), "merge".into(), "wait".into(), "".into()]),
        "merge",
        "true",
        "agent",
        day("2026-11-01"),
    )?;
    at("2026-11-02T09:00:00+00:00");
    let second = questions::add(
        root,
        "/careerbrew",
        &format!("Approve {CANARY}?"),
        Some(&["merge".into(), "wait".into()]),
        "wait",
        "true",
        "agent",
        day("2026-11-02"),
    )?;
    reset_write_count(&ws.base)?;
    let third = questions::add(
        root,
        "/careerbrew",
        &format!("Approve {CANARY}?"),
        Some(&["merge".into(), "wait".into()]),
        "wait",
        "true",
        "agent",
        day("2026-11-02"),
    )?;
    let repeat_writes = write_count(&ws.base);
    let invalid = vec![
        error_of(
            questions::record(
                "/careerbrew",
                &"x".repeat(201),
                None,
                "",
                "",
                "agent",
                "",
                None,
            )
            .map(|_| ()),
        ),
        error_of(
            questions::record(
                "/careerbrew",
                "x",
                Some(&["x".repeat(81)]),
                "",
                "",
                "agent",
                "",
                None,
            )
            .map(|_| ()),
        ),
        error_of(
            questions::record(
                "/careerbrew",
                "x",
                Some(&(0..7).map(|n| n.to_string()).collect::<Vec<_>>()),
                "",
                "",
                "agent",
                "",
                None,
            )
            .map(|_| ()),
        ),
        error_of(
            questions::record(
                "/careerbrew",
                "x",
                Some(&["a".into()]),
                "b",
                "",
                "agent",
                "",
                None,
            )
            .map(|_| ()),
        ),
        error_of(
            questions::record("/careerbrew", "x", None, "", "", "unknown", "", None).map(|_| ()),
        ),
        error_of(questions::check_recheck("sh -c 'open").map(|_| ())),
        error_of(questions::check_recheck(&"x".repeat(501)).map(|_| ())),
        error_of(questions::check_recheck("https://example.com/a b").map(|_| ())),
    ];
    let public = questions::public(&merged(
        &second,
        json!({"age": "26 h", "state": "open", "why": "still open"}),
    ));
    let plugin_public = questions::public(&merged(
        &second,
        json!({"source": "plugin:private-name", "recheck": "careerbrew:review-pr https://github.com/example/tools/pull/7"}),
    ));
    let stale_expected = merged(
        &second,
        json!({"text": "stale writer", "day": "2026-11-01", "age": "26 h"}),
    );
    let stale_close = questions::close(
        root,
        &text(field(&second, "id")),
        "stale writer",
        day("2026-11-01"),
        Some(&stale_expected),
    )?;
    let changed = questions::records(root, day("2026-11-01"))?[0].clone();
    let expected = merged(&changed, json!({"day": "2026-11-01", "age": "26 h"}));
    let actual_close = questions::close(
        root,
        &text(field(&second, "id")),
        "answered",
        day("2026-11-01"),
        Some(&expected),
    )?;
    at("2026-11-01T06:00:00+00:00");
    questions::add(
        root,
        "/careerbrew",
        "open command",
        None,
        "",
        "true",
        "agent",
        day("2026-11-01"),
    )?;
    questions::add(
        root,
        "/careerbrew",
        "closed command",
        None,
        "",
        "false",
        "agent",
        day("2026-11-01"),
    )?;
    at("2026-11-01T08:30:00+00:00");
    questions::add(
        root,
        "/careerbrew",
        "agent link",
        None,
        "",
        "https://example.com/pr/1",
        "agent",
        day("2026-11-01"),
    )?;
    questions::add(
        root,
        "/careerbrew",
        "manual close",
        None,
        "",
        "",
        "agent",
        day("2026-11-01"),
    )?;
    at("2026-11-02T09:00:00+00:00");
    let plant = std::env::var("CAREERBREW_RUST15_PLANT_WRITE").unwrap_or_default();
    if plant == "event-duplicate" {
        std::env::set_var("CAREERBREW_EVENTS", "on");
    }
    let batch = questions::recheck(root, day("2026-11-02"), &questions::LocalRechecks)?;
    let signal_receipt = questions::run_verify("sh -c 'kill -TERM $$'")?;
    let old_records = questions::records(root, day("2026-11-01"))?;
    let current_records = questions::records(root, day("2026-11-02"))?;
    if plant == "operation-duplicate" {
        let document = read_json(root, "day/work-2026-11-02.json", json!({}))?;
        write_json(root, "day/work-2026-11-02.json", &document)?;
    }
    if plant == "event-duplicate" {
        let logged = std::fs::read_to_string(root.join("logs/events.jsonl"))?;
        let row = logged
            .lines()
            .last()
            .and_then(|line| serde_json::from_str::<Value>(line).ok())
            .map(|row| careerbrew_setup::value::to_canon(&row))
            .ok_or_else(|| error("ValueError", "event plant has no source row"))?;
        careerbrew_vault::events::write_row(root, &row)?;
    }
    Ok(json!({
        "first": questions::public(&first),
        "second": questions::public(&second),
        "third_same": third == second,
        "repeat_writes": repeat_writes,
        "invalid": invalid,
        "public": public,
        "plugin_public": plugin_public,
        "close": [stale_close, actual_close],
        "batch": batch,
        "signal_receipt": signal_receipt,
        "old_records": old_records.iter().map(questions::public).collect::<Vec<_>>(),
        "current_records": current_records.iter().map(questions::public).collect::<Vec<_>>(),
        "ages": [questions::age("bad", crate::now()), questions::age("2026-11-01T08:45:00+00:00", crate::now()), questions::age("2026-10-30T08:00:00+00:00", crate::now())],
        "sealed": std::fs::read(status::path_for(root, day("2026-11-01")))?.starts_with(b"CBV1\n"),
        "plain_absent": !std::fs::read(status::path_for(root, day("2026-11-01")))?.windows(CANARY.len()).any(|part| part == CANARY.as_bytes()),
    }))
}

fn step_validation(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "step-validation", &[])?;
    let rich = steps::entry(
        &json!({
            "step": "standup",
            "scope": ["repo", "repo", "repo/web"],
            "after": "/refresh",
            "longRunning": false,
            "operatorTurn": true,
            "uses": ["browser:sso", "file:result"],
            "verify": ".venv/bin/python -m unittest",
            "cycle": "careerbrew work live cycle",
            "review_ask_cycle": "careerbrew work review-ask https://github.com/example/tools/pull/7",
            "watch": ["repo:tools", "feed:chat"],
            "about": "  Draft today's standup from synthetic work  ",
        }),
        "settings",
    )?;
    let defaults = steps::entry(&json!("/standup"), "settings")?;
    let verify_errors = [
        "python3 -c 'pass'",
        "env FOO=1 python3.12 -c 'pass'",
        "sh -c 'python3 -c pass'",
        "sh -c 'open",
        "",
        &"x".repeat(501),
    ]
    .into_iter()
    .map(|value| {
        error_of(steps::entry(&json!({"step": "/x", "verify": value}), "settings").map(|_| ()))
    })
    .collect::<Vec<_>>();
    let other_errors = vec![
        error_of(steps::normalize("/a:..:x").map(|_| ())),
        error_of(steps::entry(&json!({"step": "/x", "use": "file:x"}), "settings").map(|_| ())),
        error_of(
            steps::entry(&json!({"step": "/x", "uses": "printer:one"}), "settings").map(|_| ()),
        ),
        error_of(
            steps::entry(&json!({"step": "/x", "scope": "x".repeat(201)}), "settings").map(|_| ()),
        ),
        error_of(
            steps::entry(
                &json!({"step": "/x", "operatorTurn": true, "longRunning": true}),
                "settings",
            )
            .map(|_| ()),
        ),
        error_of(steps::entry(&json!({"step": "/x", "about": "one\ntwo"}), "settings").map(|_| ())),
        error_of(
            steps::entry(
                &json!({"step": "/x", "about": vec!["word"; 16].join(" ")}),
                "settings",
            )
            .map(|_| ()),
        ),
        error_of(
            steps::entry(
                &json!({"step": "/x", "watch": "url:https://example.com"}),
                "settings",
            )
            .map(|_| ()),
        ),
        error_of(steps::validate(&json!({"step": "/x"}), "settings").map(|_| ())),
        error_of(steps::validate(&json!(vec!["/x"; 31]), "settings").map(|_| ())),
    ];
    let commands = ws.base.join("commands");
    std::fs::create_dir(&commands)?;
    std::fs::write(commands.join("standup.md"), "---\ndescription: \"Draft the standup. It reads yesterday.\"\ndisable-model-invocation: true\n---\nbody\n")?;
    std::fs::write(commands.join("plain.md"), "no front matter\n")?;
    let resolved_raw = steps::resolve("/standup", std::slice::from_ref(&ws.base))?;
    let resolved = json!({"description": resolved_raw["description"], "user_only": resolved_raw["user_only"], "present": !resolved_raw["path"].is_null(), "source_error": resolved_raw.get("source_error").cloned().unwrap_or_else(|| json!(""))});
    let canonical_raw = steps::resolve_origin("/work", "careerbrew")?;
    let canonical = json!({"description": canonical_raw["description"], "user_only": canonical_raw["user_only"], "present": !canonical_raw["path"].is_null(), "source_error": canonical_raw.get("source_error").cloned().unwrap_or_else(|| json!(""))});
    let plain_raw = steps::resolve("/plain", std::slice::from_ref(&ws.base))?;
    let plain = json!({"description": plain_raw["description"], "user_only": plain_raw["user_only"], "present": !plain_raw["path"].is_null(), "source_error": plain_raw.get("source_error").cloned().unwrap_or_else(|| json!(""))});
    Ok(json!({
        "rich": rich,
        "defaults": defaults,
        "verify_errors": verify_errors,
        "other_errors": other_errors,
        "resolved": resolved,
        "canonical": canonical,
        "plain": plain,
        "about": [json!(steps::about(&json!({"about": "Own words"}), &resolved_raw)), json!(steps::about(&json!({"about": ""}), &resolved_raw)), json!(steps::missing_about(&[json!({"step": "/a", "about": ""}), json!({"step": "/b", "about": "ok"})]))],
        "first_sentences": [steps::first_sentence(" 'One sentence. Next sentence.' "), steps::first_sentence("No punctuation"), steps::first_sentence(&"x".repeat(200))],
    }))
}

fn stage_readiness(base: &Path) -> Result<Value> {
    Workspace::new(base, "stage-readiness", &[])?;
    let declared = vec![
        steps::entry(&json!("/careerbrew"), "settings")?,
        steps::entry(
            &json!({"step": "/z-operator", "operatorTurn": true, "uses": "browser:z"}),
            "settings",
        )?,
        steps::entry(
            &json!({"step": "/a-operator", "operatorTurn": true, "uses": "browser:a"}),
            "settings",
        )?,
        steps::entry(
            &json!({"step": "/dispatch", "operatorTurn": true, "uses": "browser:sso"}),
            "settings",
        )?,
        steps::entry(
            &json!({"step": "/feed", "longRunning": true, "uses": "browser:feed"}),
            "settings",
        )?,
        steps::entry(
            &json!({"step": "/check", "uses": "browser:feed"}),
            "settings",
        )?,
        steps::entry(
            &json!({"step": "/report", "scope": "report", "after": "/check"}),
            "settings",
        )?,
        steps::entry(&json!({"step": "/other", "scope": "other"}), "settings")?,
    ];
    let (found, view) = stages::build(&declared, &BTreeSet::from(["/dispatch".to_owned()]))?;
    let initial = stages::ready(&found, &[json!({"step": "/careerbrew", "status": "done"})]);
    let running = stages::ready(
        &found,
        &[
            json!({"step": "/careerbrew", "status": "done"}),
            json!({"step": "/check", "status": "running"}),
        ],
    );
    let checked = stages::ready(
        &found,
        &[
            json!({"step": "/careerbrew", "status": "done"}),
            json!({"step": "/check", "status": "done"}),
        ],
    );
    let failed = stages::ready(
        &found,
        &[
            json!({"step": "/careerbrew", "status": "done"}),
            json!({"step": "/check", "status": "failed"}),
        ],
    );
    let blocked = stages::ready(
        &found,
        &[
            json!({"step": "/careerbrew", "status": "done"}),
            json!({"step": "/check", "status": "blocked"}),
        ],
    );
    let errors = vec![
        error_of(
            stages::build(
                &[
                    steps::entry(&json!({"step": "/a", "after": "/b"}), "settings")?,
                    steps::entry(&json!({"step": "/b", "after": "/a"}), "settings")?,
                ],
                &BTreeSet::new(),
            )
            .map(|_| ()),
        ),
        error_of(
            stages::build(
                &[
                    steps::entry(&json!({"step": "/feed", "longRunning": true}), "settings")?,
                    steps::entry(&json!({"step": "/a", "after": "/feed"}), "settings")?,
                ],
                &BTreeSet::new(),
            )
            .map(|_| ()),
        ),
        error_of(
            stages::build(
                &[
                    steps::entry(
                        &json!({"step": "/f1", "longRunning": true, "uses": "browser:sso"}),
                        "settings",
                    )?,
                    steps::entry(
                        &json!({"step": "/f2", "longRunning": true, "uses": "browser:sso"}),
                        "settings",
                    )?,
                ],
                &BTreeSet::new(),
            )
            .map(|_| ()),
        ),
        error_of(
            stages::build(
                &[
                    steps::entry(&json!({"step": "/check", "scope": "check"}), "settings")?,
                    steps::entry(
                        &json!({"step": "/dispatch", "operatorTurn": true, "after": "/check"}),
                        "settings",
                    )?,
                ],
                &BTreeSet::new(),
            )
            .map(|_| ()),
        ),
    ];
    Ok(
        json!({"found": found, "view": view, "ready": {"initial": initial, "running": running, "checked": checked, "failed": failed, "blocked": blocked}, "errors": errors}),
    )
}

fn invocation_text(result: (i32, String)) -> Value {
    json!({"exit": result.0, "stdout": result.1})
}

fn work_session_case(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "work-session", &["billing", "ledger", "search"])?;
    let root = &ws.root;
    set_setting(root, "work_session_cap", 2)?;
    reset_write_count(&ws.base)?;
    let opened = work_session::open_cli(root, "billing", "sess-a", true, false);
    let ledger = work_session::open_cli(root, "ledger", "sess-b", true, false);
    let cap = work_session::open_cli(root, "search", "sess-c", false, false);
    reset_write_count(&ws.base)?;
    let reused = work_session::open_cli(root, "billing", "sess-a", true, false);
    let reuse_writes = write_count(&ws.base);
    let claim = careerbrew_sessions::sessions::take(root, "slack-draft:C0SYN/1.0", "sess-a", None)
        .map_err(|error| crate::error(error.class, error.message))?;
    let asked = work_session::ask_cli(
        root,
        "sess-a",
        CANARY,
        &["send".into(), "hold".into()],
        "hold",
        "true",
    );
    let activity = invocation_text(careerbrew_sessions::cli::work_agent_event(
        root,
        "billing-bot",
        CANARY,
        "running",
        "",
        "",
        "",
        "sess-a",
    ));
    let listing = work_session::listing(root, false)?;
    let public = work_session::public(
        listing
            .iter()
            .find(|row| text(field(row, "id")) == "sess-a")
            .expect("session"),
    );
    let listed = work_session::list_cli(root, false, true, false);
    let listed_text = work_session::list_cli(root, false, false, false);
    let missing = work_session::end_cli(root, "", true, true);
    let handoff = work_session::handoff_cli(root, "sess-a", CANARY, "");
    let handoff_b = work_session::handoff_cli(root, "sess-b", "ledger complete", "");
    let ended = work_session::end_cli(root, "", true, true);
    let reopened = work_session::open_cli(root, "billing", "sess-a", true, false);
    std::env::set_var("CAREERBREW_EVENTS", "on");
    let event_session = work_session::open_cli(root, "ledger", "sess-events", true, false);
    let event_names = std::fs::read_to_string(root.join("logs/events.jsonl"))?
        .lines()
        .rev()
        .take(2)
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .map(|row| text(field(&row, "event")))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    std::env::set_var("CAREERBREW_EVENTS", "off");
    let session_path = root.join("work/sessions.json");
    let raw = std::fs::read(&session_path)?;
    match std::env::var("CAREERBREW_RUST15_PLANT_WRITE").as_deref() {
        Ok("missing") => std::fs::remove_file(&session_path)?,
        Ok("duplicate") => {
            let document = read_json(root, "work/sessions.json", json!({}))?;
            write_json(root, "work/sessions.json", &document)?;
        }
        _ => {}
    }
    Ok(json!({
        "opened": opened,
        "ledger": ledger,
        "cap": cap,
        "reused": reused,
        "reuse_writes": reuse_writes,
        "raw_claim": claim,
        "asked": asked,
        "activity": activity,
        "public": public,
        "listed": listed,
        "listed_text": listed_text,
        "missing": missing,
        "handoff": handoff,
        "handoff_b": handoff_b,
        "ended": ended,
        "reopened": reopened,
        "event_session": event_session,
        "event_names": event_names,
        "claims_after_end": careerbrew_sessions::sessions::claims(root).map_err(|error| crate::error(error.class, error.message))?,
        "sealed": raw.starts_with(b"CBV1\n"),
        "plain_absent": !raw.windows(CANARY.len()).any(|part| part == CANARY.as_bytes()),
    }))
}

struct ChangedReview;
impl questions::RecheckProvider for ChangedReview {
    fn signin(&self, root: &Path, command: &str) -> Result<(bool, String)> {
        questions::LocalRechecks.signin(root, command)
    }
    fn may_run(&self, root: &Path, source: &str) -> Result<bool> {
        questions::LocalRechecks.may_run(root, source)
    }
    fn review_pr(
        &self,
        root: &Path,
        _url: &str,
        _deadline: Option<f64>,
    ) -> Result<(String, String)> {
        let day = day("2026-11-02");
        crate::status::with_day_lock(root, day, || {
            let mut document = crate::status::data(root, day)?;
            if let Some(question) = document
                .get_mut("questions")
                .and_then(Value::as_array_mut)
                .and_then(|rows| rows.last_mut())
            {
                if text(field(question, "id")) == "review" {
                    question["text"] = json!("changed underneath");
                }
            }
            write_json(root, "day/work-2026-11-02.json", &document)
        })?;
        Ok(("closed".into(), "review merged".into()))
    }
}

fn legacy_receipts(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "legacy-receipts", &[])?;
    let root = &ws.root;
    let day = day("2026-11-02");
    write_json(
        root,
        "day/work-2026-11-02.json",
        &json!({"rows": "not-a-list", "questions": "not-a-list"}),
    )?;
    let malformed_status = status::stored(root, day)?;
    let malformed_questions = questions::records(root, day)?;
    write_json(
        root,
        "day/work-2026-11-02.json",
        &json!({
            "rows": [],
            "questions": [
                {"id": "same", "step": "/careerbrew", "text": "old", "createdAt": "2026-11-01T08:00:00+00:00", "recheck": "", "closed": ""},
                {"id": "same", "step": "/careerbrew", "text": "new", "createdAt": "2026-11-02T08:00:00+00:00", "recheck": "", "closed": "answered"},
            ],
        }),
    )?;
    let open_rows = questions::open_records(root, day)?;
    at("2026-11-02T09:00:00+00:00");
    let record = questions::record(
        "/careerbrew",
        "review?",
        None,
        "",
        "careerbrew:review-pr https://github.com/example/tools/pull/7",
        "agent",
        "review",
        None,
    )?;
    questions::save(root, &record, day, false)?;
    let review_batch = questions::recheck(root, day, &ChangedReview)?;
    let review_open = questions::records(root, day)?
        .into_iter()
        .filter(|question| {
            text(field(question, "id")) == "review" && text(field(question, "closed")).is_empty()
        })
        .collect::<Vec<_>>();
    projects::add(root, "billing", AddOptions::default())?;
    careerbrew_sessions::sessions::open_session(root, "billing", "sess-old")
        .map_err(|error| crate::error(error.class, error.message))?;
    let mut raw = read_json(root, "work/sessions.json", json!({}))?;
    raw["claims"] = json!([{"resource": "browser:sso", "session": "sess-old", "until": "bad"}]);
    write_json(root, "work/sessions.json", &raw)?;
    let before = std::fs::read(root.join("work/sessions.json"))?;
    let invalid_claim = error_of(work_session::listing(root, false).map(|_| ()));
    let unchanged = before == std::fs::read(root.join("work/sessions.json"))?;

    let brief = Workspace::new(base, "legacy-session-brief", &["billing"])?;
    at("2026-11-01T09:00:00+00:00");
    careerbrew_sessions::sessions::open_session(&brief.root, "billing", "legacy")
        .map_err(|error| crate::error(error.class, error.message))?;
    careerbrew_sessions::sessions::write_handoff(
        &brief.root,
        "legacy",
        "prior private handoff",
        "",
    )
    .map_err(|error| crate::error(error.class, error.message))?;
    careerbrew_sessions::sessions::end(&brief.root, "legacy", false)
        .map_err(|error| crate::error(error.class, error.message))?;
    let mut saved = read_json(&brief.root, "work/sessions.json", json!({}))?;
    saved["sessions"]
        .as_array_mut()
        .and_then(|rows| rows.last_mut())
        .expect("legacy session")
        .as_object_mut()
        .expect("session object")
        .retain(|key, _| !["record_id", "project_incarnation"].contains(&key.as_str()));
    write_json(&brief.root, "work/sessions.json", &saved)?;
    at("2026-11-02T09:00:00+00:00");
    let legacy_open = work_session::open_cli(&brief.root, "billing", "new", true, false);
    careerbrew_sessions::sessions::write_handoff(&brief.root, "new", "current private handoff", "")
        .map_err(|error| crate::error(error.class, error.message))?;
    let mut saved = read_json(&brief.root, "work/sessions.json", json!({}))?;
    saved["sessions"]
        .as_array_mut()
        .and_then(|rows| rows.last_mut())
        .expect("new session")
        .as_object_mut()
        .expect("session object")
        .retain(|key, _| !["record_id", "project_incarnation"].contains(&key.as_str()));
    write_json(&brief.root, "work/sessions.json", &saved)?;
    let legacy_end = work_session::end_cli(&brief.root, "new", false, true);
    let loops = read_json(&brief.root, "day/loops.json", json!({}))?;
    let legacy_loops = loops
        .get("loops")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| text(field(row, "stopped")).is_empty())
        .map(|row| text(field(row, "id")))
        .collect::<Vec<_>>();

    let quiet = Workspace::new(base, "quiet-session-receipt", &["billing"])?;
    at("2026-10-30T09:00:00+00:00");
    careerbrew_sessions::sessions::open_session(&quiet.root, "billing", "old")
        .map_err(|error| crate::error(error.class, error.message))?;
    careerbrew_sessions::sessions::write_handoff(&quiet.root, "old", "prior private handoff", "")
        .map_err(|error| crate::error(error.class, error.message))?;
    careerbrew_sessions::sessions::end(&quiet.root, "old", false)
        .map_err(|error| crate::error(error.class, error.message))?;
    let generation = "a".repeat(32);
    let mut project_data = projects::load(&quiet.root)?;
    project_data["billing"]["quiet_until"] = json!("2026-11-01");
    project_data["billing"]["quiet_generation"] = json!(generation);
    careerbrew_projects::write_yaml(
        &quiet.root,
        "work/projects.yaml",
        &json!({"projects": project_data}),
    )?;
    at("2026-11-02T09:00:00+00:00");
    let quiet_open = work_session::open(&quiet.root, "billing", "resumed")?;
    reset_write_count(&quiet.base)?;
    let quiet_reused = work_session::open(&quiet.root, "billing", "resumed")?;
    let quiet_reuse_writes = write_count(&quiet.base);
    let quiet_state = read_json(&quiet.root, "work/sessions.json", json!({}))?;
    let quiet_receipt = quiet_state
        .get("quiet_receipts")
        .and_then(Value::as_object)
        .is_some_and(|receipts| receipts.contains_key(&generation));
    Ok(json!({
        "malformed_status": malformed_status,
        "malformed_questions": malformed_questions,
        "open_rows": open_rows,
        "review_batch": review_batch,
        "review_still_open": review_open,
        "invalid_claim": invalid_claim,
        "unchanged": unchanged,
        "legacy_open": legacy_open,
        "legacy_end": legacy_end,
        "legacy_loops": legacy_loops,
        "quiet_open": {"id": quiet_open["id"], "brief": quiet_open["brief"], "brief_status": quiet_open.get("brief_status").cloned().unwrap_or_else(|| json!(""))},
        "quiet_reused": quiet_reused["brief"],
        "quiet_receipt": quiet_receipt,
        "quiet_reuse_writes": quiet_reuse_writes,
    }))
}

fn run(case: &str, base: &Path) -> Result<Value> {
    match case {
        "status-schema" => status_case(base),
        "question-records" => question_records(base),
        "step-validation" => step_validation(base),
        "stage-readiness" => stage_readiness(base),
        "work-session" => work_session_case(base),
        "legacy-receipts" => legacy_receipts(base),
        _ => Err(error("ValueError", "unknown rust-15 case")),
    }
}

pub fn run_all(selected: Option<&str>) -> Result<String> {
    let base = crate::root()?.join("rust15-workload");
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
    Ok(crate::compact(&typed(&json!({"cases": cases})))? + "\n")
}
