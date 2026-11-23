use careerbrew_projects::{priorities, project_priority, projects, promises, python_round};
use chrono::NaiveDate;
use serde_json::json;

#[test]
fn finite_hours_and_legacy_estimates_fail_closed() {
    for value in ["nan", "inf", "-inf", "0.01", "201"] {
        assert!(priorities::hours_text(value).is_err(), "accepted {value}");
    }
    assert_eq!(priorities::hours_text("0.05").unwrap(), 0.05);
    assert_eq!(priorities::hours_text("200").unwrap(), 200.0);
    for value in [
        json!("nan"),
        json!("inf"),
        json!(-1),
        json!("abc"),
        json!(null),
    ] {
        assert_eq!(priorities::estimate(&json!({"estimate_h": value})), 1.0);
    }
}

#[test]
fn configured_ticket_identity_excludes_utf_and_iso() {
    let entries = vec![json!({"name": "billing", "rank": 1, "match": ["BILL-*"]})];
    assert_eq!(
        project_priority::keys("Fix UTF-8 and ISO-8601; BILL-42", "", "", "", &entries),
        ["bill-42", "bill", "fix utf-8 and iso-8601; bill-42"]
    );
    assert_eq!(
        project_priority::keys("Fix UTF-8, ISO-8601 and ABC-123", "", "", "", &[]),
        ["abc-123", "abc", "fix utf-8, iso-8601 and abc-123"]
    );
    assert_eq!(
        project_priority::keys(
            "See the ticket",
            "",
            "https://jira.example.com/browse/OPS-3",
            "",
            &[]
        ),
        ["ops-3", "ops", "see the ticket"]
    );
}

#[test]
fn promise_ids_detection_and_due_days_are_stable() {
    assert_eq!(promises::name_for("slack:D1:2"), "promise-4fde10e7");
    assert_eq!(
        promises::name_for("slack:D1:2"),
        promises::name_for("slack:D1:2")
    );
    let said = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
    assert_eq!(
        promises::detect("I will send it tomorrow", said).unwrap(),
        json!({"what": "Send it tomorrow", "due": "2026-11-03"})
    );
    assert_eq!(promises::detect("I can review it", said), None);
    assert_eq!(promises::detect("I can do that by Friday?", said), None);
}

#[test]
fn project_bonus_keeps_rank_order_and_bounds() {
    assert_eq!(project_priority::bonus(0, 2), 0.0);
    assert_eq!(project_priority::bonus(1, 2), 3.0);
    assert_eq!(project_priority::bonus(2, 2), 1.5);
    assert_eq!(project_priority::bonus(3, 2), 0.0);
}

#[test]
fn a_task_matching_two_projects_counts_only_at_the_first_match() {
    let entries = vec![
        json!({"name": "billing", "rank": 1, "match": ["BILL-*"]}),
        json!({"name": "api", "rank": 2, "match": ["api-*"]}),
    ];
    let tasks = vec![json!({
        "id": "t001", "title": "BILL-1", "status": "open",
        "links": [{"label": "", "url": "https://github.com/acme/api/pull/1"}]
    })];
    let counts = project_priority::open_task_counts(&tasks, &Default::default(), &entries);
    assert_eq!(
        counts.get("billing").and_then(|value| value.as_u64()),
        Some(1)
    );
    assert_eq!(counts.get("api").and_then(|value| value.as_u64()), Some(0));
}

#[test]
fn rounding_is_python_ties_even_including_negative_and_near_ties() {
    for (value, expected) in [
        (1.125, 1.12),
        (-1.125, -1.12),
        (1.135, 1.14),
        (1.124999999, 1.12),
        (1.125000001, 1.13),
        (-1.125000001, -1.13),
    ] {
        assert_eq!(python_round(value, 2), expected, "rounding {value}");
    }
}

#[test]
fn saved_project_identity_requires_lowercase_hex_32() {
    assert!(projects::valid_identity(
        &json!({"incarnation_id": "0123456789abcdef0123456789abcdef"})
    ));
    for bad in ["A".repeat(32), "g".repeat(32), "abc".to_owned()] {
        assert!(!projects::valid_identity(&json!({"incarnation_id": bad})));
    }
}

#[test]
fn daily_results_use_real_rows_and_only_the_first_project_match() {
    let entries = vec![
        json!({"name": "billing", "rank": 1, "match": ["BILL-*"]}),
        json!({"name": "api", "rank": 2, "match": ["API-*"]}),
    ];
    let rows = vec![
        json!({"id": "result-1", "title": "BILL-100"}),
        json!({"id": "result-2", "title": "BILL-101 API-2"}),
        json!({"id": "result-3", "title": "unmatched"}),
    ];
    let counts = project_priority::day_result_counts(&rows, &Default::default(), &entries);
    assert_eq!(
        counts.get("billing").and_then(|value| value.as_u64()),
        Some(2)
    );
    assert_eq!(counts.get("api").and_then(|value| value.as_u64()), Some(0));
}
