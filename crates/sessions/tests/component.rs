//! Native rust-10 controls, including 1,000 locked draft interleavings.

use careerbrew_sessions::{drafts, prior_selection, sessions, synthetic};
use serde_json::json;
use std::time::Duration;

#[test]
fn newest_thread_and_origin_session_controls() {
    let thread = drafts::messages(&json!({"messages": [
        {"ts": "2", "text": "Deploy started."},
        {"ts": "1", "text": "Can you review this?"},
    ]}))
    .unwrap();
    let answer = drafts::fit(
        &thread,
        Some("Could you change this?"),
        Some(&json!("1")),
        None,
    )
    .unwrap();
    assert_eq!(answer["kind"], "ack");
    assert_eq!(answer["newer"], 1);
    assert_eq!(answer["newest_ts"], "2");
    assert!(!drafts::deploy_started("We will deploy tomorrow."));
    assert!(!drafts::deploy_started("Did the deploy start?"));
    assert!(drafts::deploy_started(
        "Deploy started, will update when done."
    ));

    let record = "a".repeat(32);
    let other = "b".repeat(32);
    let project = "1".repeat(32);
    let rows = vec![
        json!({"day": "2026-11-06", "record_id": record.clone(), "project_incarnation": project.clone(), "status": "done"}),
        json!({"day": "2026-11-06", "record_id": record, "project_incarnation": project.clone()}),
        json!({"day": "2026-11-06", "record_id": other, "project_incarnation": "2".repeat(32)}),
    ];
    let selected = prior_selection::select(
        chrono::NaiveDate::from_ymd_opt(2026, 11, 7).unwrap(),
        &rows,
        &[project],
    )
    .unwrap();
    assert_eq!(selected[0]["reason"], "duplicate_record_id");
    assert_eq!(selected[1]["reason"], "duplicate_record_id");
    assert_eq!(selected[2]["reason"], "unknown_project_id");
}

#[test]
fn clear_save_readback_lock_covers_1000_interleavings() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("data");
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("HOME", scratch.path().join("home"));
    std::env::set_var("CAREERBREW_HOME", &root);
    std::env::set_var("CAREERBREW_KEY_FILE", scratch.path().join("backup.key"));
    std::env::set_var("CAREERBREW_EVENTS", "off");
    std::env::set_var("CAREERBREW_ENCRYPT", "off");
    std::env::set_var("CAREERBREW_NOW", "2026-11-02T09:00:00+00:00");

    let lock = drafts::locked("C1/busy.1", Duration::ZERO, &root.join("drafts")).unwrap();
    let busy = drafts::replace(&root, "C1/busy.1", "text", false, "").unwrap_err();
    assert_eq!(busy.class, "DraftBusy");
    drop(lock);
    assert_eq!(
        drafts::replace(&root, "C1/busy.1", "text", false, "").unwrap()["same"],
        true
    );

    for project in ["billing", "ledger"] {
        careerbrew_projects::projects::add(
            &root,
            project,
            careerbrew_projects::projects::AddOptions::default(),
        )
        .unwrap();
    }
    sessions::open_session(&root, "billing", "sess-a").unwrap();
    sessions::open_session(&root, "ledger", "sess-b").unwrap();
    let observed = synthetic::interleaved_transactions(&root, false).unwrap();
    assert_eq!(observed["operations"], 1000);
    assert_eq!(observed["contentions"], 250);
    assert_eq!(observed["draft_saves"], 250);
    assert_eq!(observed["cross_session_writes"], 0);
    assert_eq!(observed["stale_claims"], 0);
    assert_eq!(observed["conflicting_target_saves"], 0);

    let negative = synthetic::interleaved_transactions(&root, true).unwrap();
    assert_eq!(negative["operations"], 1000);
    assert_eq!(negative["cross_session_writes"], 1);
    assert_eq!(negative["conflicting_target_saves"], 1);
}
