use careerbrew_live::{listen, live, notify, review};
use serde_json::json;

#[test]
fn verdict_preserves_personal_and_evidence_rules() {
    let personal = json!({"id": "x", "triage": "act", "personal": true});
    assert_eq!(live::verdict(&personal, &Default::default()), "personal");
    let evidence = json!({"id": "y", "triage": "mine", "evidence": true, "personal": false});
    assert!(live::reaches_pm(&evidence));
    assert!(!live::reaches_pm(
        &json!({"id": "y", "triage": "mine", "evidence": true, "personal": true})
    ));
}

#[test]
fn codeowners_last_rule_and_reviewer_mentions() {
    let rules =
        review::parse_codeowners("* @all-dev\n/src/ @src-dev @org/team owner@example.com\n");
    assert_eq!(
        review::owners_of("src/a.py", &rules),
        ["src-dev", "org/team"]
    );
    assert_eq!(review::owners_of("README.md", &rules), ["all-dev"]);
}

#[test]
fn quiet_hours_include_start_and_exclude_end() {
    let at = |time| careerbrew_live::parse_time(&format!("2026-10-21T{time}:00+00:00")).unwrap();
    assert!(notify::in_quiet(at("19:00"), "19:00-08:00"));
    assert!(notify::in_quiet(at("07:59"), "19:00-08:00"));
    assert!(!notify::in_quiet(at("08:00"), "19:00-08:00"));
    assert!(!notify::in_quiet(at("18:59"), "19:00-08:00"));
}

#[test]
fn review_sessions_use_one_digest_per_request() {
    assert_eq!(
        listen::review_key("https://github.com/example/shop/pull/7"),
        listen::review_key("https://github.com/example/shop/pull/7")
    );
    assert_ne!(
        listen::review_key("https://github.com/example/shop/pull/7"),
        listen::review_key("https://github.com/example/shop/pull/8")
    );
}
