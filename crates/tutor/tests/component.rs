//! Native rust-20 FSRS, identity, and privacy controls.

use careerbrew_tutor::{fsrs, notion, public};
use chrono::{TimeZone, Utc};
use serde_json::json;

#[test]
fn all_registered_depvec_fsrs_vectors_are_present() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../xtask/tests/fixtures/depvec/fsrs.json"
    ))
    .unwrap();
    let report = careerbrew_depvec::run("fsrs", &fixture).unwrap();
    let rows = report["results"].as_array().unwrap();
    assert_eq!(rows.len(), 18);
    assert_eq!(rows[0]["id"], "parameters");
    assert!(rows[..16]
        .iter()
        .all(|row| row["output"].get("error").is_none()));
    assert_eq!(rows[16]["output"], json!({"error": true}));
    assert_eq!(rows[17]["output"], json!({"error": true}));
}

#[test]
fn tutor_scheduler_matches_learning_and_lapse_anchors_without_normalization() {
    let start = Utc
        .with_ymd_and_hms(2026, 1, 5, 9, 0, 0)
        .unwrap()
        .fixed_offset();
    let reviews = [
        (0, 3),
        (10, 3),
        (7_200, 3),
        (43_200, 1),
        (43_210, 3),
        (46_080, 3),
    ];
    let expected = [
        (1, Some(1), 600, "2.31", "2.12"),
        (2, None, 173_400, "2.31", "2.11"),
        (2, None, 1_814_400, "16.19", "2.10"),
        (3, Some(0), 2_592_600, "1.96", "7.39"),
        (2, None, 2_765_400, "1.97", "7.38"),
        (2, None, 3_110_400, "4.01", "7.37"),
    ];
    let mut old = fsrs::CardState::new(7, start);
    for ((minutes, rating), (phase, step, due, stability, difficulty)) in
        reviews.into_iter().zip(expected)
    {
        let at = start + chrono::Duration::minutes(minutes);
        let next = fsrs::next_state(Some(&old), rating, at).unwrap();
        assert_eq!(next.phase.code(), phase);
        assert_eq!(next.step, step);
        assert_eq!(next.due.timestamp() - start.timestamp(), due);
        assert_eq!(format!("{:.2}", next.stability.unwrap()), stability);
        assert_eq!(format!("{:.2}", next.difficulty.unwrap()), difficulty);
        assert_ne!(
            next.due.timestamp() - start.timestamp(),
            due + 1,
            "planted one-second due mismatch must be visible"
        );
        old = next;
    }
}

#[test]
fn old_state_roundtrip_retains_exact_numbers_and_fractional_due() {
    let old = json!({
        "card_id": 424242,
        "state": 2,
        "step": null,
        "stability": 2.3065,
        "difficulty": 2.111214235785395,
        "due": "2026-10-21T09:10:00.123456+00:00",
        "last_review": "2026-10-19T09:10:00+00:00",
    });
    let state = fsrs::CardState::from_json(&old).unwrap();
    assert_eq!(state.to_json(), old);
    assert_eq!(state.difficulty, Some(2.111214235785395));
    assert_eq!(state.to_json()["due"], "2026-10-21T09:10:00.123456+00:00");
}

#[test]
fn public_lint_and_leak_controls_reject_remote_http_and_canaries() {
    let explanation = public::SECTIONS
        .iter()
        .map(|section| format!("{section}\n{}", vec!["word"; 40].join(" ")))
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut card = json!({
        "id": "kb-test",
        "question": "What is a heap?",
        "answer": "A tree where each parent is ordered before its children.",
        "topic": "computer science",
        "explanation": format!("{explanation}\n`proxy_pass http://localhost:8080;`"),
    });
    assert!(public::lint(&card).is_empty());
    card["explanation"] = json!(format!("{explanation} http://evil.test/x"));
    assert!(public::lint(&card)
        .iter()
        .any(|problem| problem.contains("plain http")));
    card["explanation"] = json!(format!(
        "{explanation}\n`<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">`"
    ));
    assert!(public::lint(&card).is_empty());
    assert!(!careerbrew_policy::find_leaks("At zzcorporation", &["Zzcorp".into()]).is_empty());
}

#[test]
fn notion_plain_text_and_rating_boundaries_match_the_contract() {
    assert_eq!(notion::plain("**What is a \\^heap?**"), "What is a ^heap?");
    assert_eq!(notion::plain("__Why__ `useMemo`?"), "Why useMemo?");
    let names = [0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]
        .into_iter()
        .map(fsrs::score_to_rating)
        .collect::<Vec<_>>();
    assert_eq!(names, [1, 2, 2, 3, 3, 4, 4]);
}
