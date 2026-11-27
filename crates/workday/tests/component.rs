//! Direct native controls for rust-15's synthetic component boundary.

use careerbrew_workday::{questions, stages, status, steps, work_session};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn setup(root: &Path) {
    std::fs::create_dir_all(root.join("home")).unwrap();
    std::env::set_var("HOME", root.join("home"));
    std::env::set_var("CAREERBREW_HOME", root.join("data"));
    std::env::set_var("CAREERBREW_KEY_FILE", root.join("keys/backup.key"));
    std::env::set_var("CAREERBREW_EVENTS", "off");
    std::env::set_var("CAREERBREW_ENCRYPT", "on");
    std::env::set_var("CAREERBREW_NOW", "2026-11-02T09:00:00+00:00");
    std::env::set_var("TZ", "UTC");
}

#[test]
fn canonical_unicode_write_readiness_privacy_and_lock_controls() {
    let _guard = ENV_LOCK.lock().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    setup(scratch.path());
    let root = scratch.path().join("data");
    std::fs::create_dir_all(&root).unwrap();
    let day = careerbrew_workday::day("2026-11-02").unwrap();
    let names = vec!["/careerbrew".to_owned(), "/standup".to_owned()];
    let accepted = format!("Approve\u{2003}{}", "é".repeat(192));
    assert_eq!(accepted.chars().count(), 200);
    let running = status::update(
        &root,
        "/careerbrew",
        &names,
        &status::Update {
            status: Some("running".into()),
            summary: Some(accepted),
            ..Default::default()
        },
        day,
    )
    .unwrap();
    assert_eq!(running["attempt"], 1);
    let error = status::update(
        &root,
        "/careerbrew",
        &names,
        &status::Update {
            summary: Some("x".repeat(201)),
            ..Default::default()
        },
        day,
    )
    .unwrap_err();
    assert_eq!(error.class, "ValueError");
    assert!(std::fs::read(status::path_for(&root, day))
        .unwrap()
        .starts_with(b"CBV1\n"));

    let before = std::fs::read(status::path_for(&root, day)).unwrap();
    assert!(
        status::timed_out(&root, "/careerbrew", &names, "stale-start", day)
            .unwrap()
            .is_none()
    );
    assert_eq!(before, std::fs::read(status::path_for(&root, day)).unwrap());

    let question = questions::add(
        &root,
        "/careerbrew",
        "Approve synthetic?",
        Some(&["yes".into(), "no".into()]),
        "yes",
        "true",
        "agent",
        day,
    )
    .unwrap();
    let before = std::fs::read(status::path_for(&root, day)).unwrap();
    let repeated = questions::add(
        &root,
        "/careerbrew",
        "Approve synthetic?",
        Some(&["yes".into(), "no".into()]),
        "yes",
        "true",
        "agent",
        day,
    )
    .unwrap();
    assert_eq!(question, repeated);
    assert_eq!(
        before,
        std::fs::read(status::path_for(&root, day)).unwrap(),
        "an identical open-question save writes nothing"
    );
    let public = questions::public(&question);
    assert!(public.get("text").is_none() && public.get("pick").is_none());
    assert_eq!(public["options"], 2);
    assert_eq!(public["recheck"], "command");

    let mut changed = question.clone();
    changed["text"] = json!("stale");
    assert!(!questions::close(
        &root,
        question["id"].as_str().unwrap(),
        "stale",
        day,
        Some(&changed)
    )
    .unwrap());
    assert!(questions::close(
        &root,
        question["id"].as_str().unwrap(),
        "answered",
        day,
        Some(&question)
    )
    .unwrap());

    let declared = [
        json!({"step": "/a", "scope": "repo"}),
        json!({"step": "/b", "scope": "repo/web", "after": "/a"}),
        json!({"step": "/c", "uses": "browser:sso"}),
        json!({"step": "/d", "uses": "browser:sso"}),
    ]
    .into_iter()
    .map(|raw| steps::entry(&raw, "settings").unwrap())
    .collect::<Vec<_>>();
    let (found, _) = stages::build(&declared, &BTreeSet::new()).unwrap();
    assert_eq!(
        stages::ready(&found, &[json!({"step": "/a", "status": "running"})]),
        vec!["/c"]
    );
    assert_eq!(
        stages::ready(&found, &[json!({"step": "/a", "status": "failed"})]),
        vec!["/c"]
    );

    let raw = json!({
        "id": "sess-a", "project": "billing", "day": "2026-11-02", "started": "2026-11-02T09:00:00+00:00",
        "ended": "", "state": "live", "questions": 1, "has_handoff": false, "wrap_requested_at": "", "handoff_at": "",
        "activity": {"status": "running", "at": "2026-11-02T09:00:00+00:00", "events": 1, "title": "SYNTHCANARY"},
        "claims": [{"resource": "slack-draft:C0SYN/1.0"}],
    });
    let projected = work_session::public(&raw);
    assert_eq!(projected["project"], "0c95c7ec");
    assert_eq!(projected["claims"], 1);
    assert!(projected["activity"].get("title").is_none());
    assert!(!projected.to_string().contains("SYNTHCANARY"));

    let plan = (0..8).map(|n| format!("/step-{n}")).collect::<Vec<_>>();
    std::thread::scope(|scope| {
        for step in &plan {
            let root = &root;
            let plan = &plan;
            scope.spawn(move || {
                status::update(
                    root,
                    step,
                    plan,
                    &status::Update {
                        status: Some("done".into()),
                        ..Default::default()
                    },
                    day,
                )
                .unwrap();
            });
        }
    });
    let rows = status::rows(&root, &plan, day).unwrap();
    assert_eq!(rows.len(), 8);
    assert!(
        rows.iter().all(|row| row["status"] == "done"),
        "the shared day lock preserves every concurrent row"
    );
}

#[test]
fn canonical_agentfile_inventory_drives_workflow_resolution() {
    let _guard = ENV_LOCK.lock().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    setup(scratch.path());
    let resolved = steps::resolve_origin("/work", "careerbrew").unwrap();
    assert!(resolved["path"].is_string());
    assert!(resolved["description"]
        .as_str()
        .unwrap()
        .starts_with("Run the work day with careerbrew"));

    let repo = scratch.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(repo.join("Agentfile.yaml"), "commands: not-a-list\n").unwrap();
    let inventory = careerbrew_setup::workflow::repository(&repo).unwrap();
    assert!(!inventory.problems.is_empty());
}

#[test]
fn typed_rust14_review_adapter_boundary_is_used() {
    let _guard = ENV_LOCK.lock().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    setup(scratch.path());
    let root = scratch.path().join("data");
    let question = questions::record(
        "/careerbrew",
        "review?",
        None,
        "",
        "careerbrew:review-pr https://github.com/example/tools/pull/7",
        "agent",
        "",
        None,
    )
    .unwrap();
    let expected_root = root.clone();
    let adapter = questions::ReviewRechecks::new(
        move |actual_root: &Path, url: &str, deadline: Option<f64>| {
            assert_eq!(actual_root, expected_root);
            assert_eq!(url, "https://github.com/example/tools/pull/7");
            assert_eq!(deadline, Some(12.5));
            Ok(("closed".into(), "review merged".into()))
        },
    );
    let result = questions::check_one(&root, &question, &adapter, Some(12.5)).unwrap();
    assert_eq!(result, ("closed".into(), "review merged".into()));
}
