//! Native rust-12 learning-rule and document-validator controls.

use careerbrew_learn::{explain, prd, project};
use serde_json::json;
use std::path::Path;

#[test]
fn mastery_requires_two_counted_answers_and_one_apply_level() {
    let row = |mode, result, confidence, level| json!({"mode": mode, "result": result, "confidence": confidence, "level": level});
    assert!(!project::mastered(&[
        row("pretest", "correct", "high", "apply"),
        row("quiz", "correct", "high", "apply")
    ]));
    assert!(!project::mastered(&[
        row("quiz", "correct", "low", "apply"),
        row("quiz", "correct", "high", "apply")
    ]));
    assert!(!project::mastered(&[
        row("quiz", "correct", "high", "understand"),
        row("quiz", "correct", "high", "understand")
    ]));
    assert!(project::mastered(&[
        row("quiz", "correct", "high", "understand"),
        row("quiz", "correct", "medium", "apply")
    ]));
}

#[test]
fn a_delayed_miss_clears_old_mastery_evidence() {
    let row = |mode, result| json!({"mode": mode, "result": result, "confidence": "high", "level": "apply"});
    assert!(project::mastered(&[
        row("quiz", "correct"),
        row("quiz", "correct")
    ]));
    assert!(!project::mastered(&[
        row("quiz", "correct"),
        row("quiz", "correct"),
        row("retest", "wrong")
    ]));
    assert!(project::mastered(&[
        row("quiz", "correct"),
        row("quiz", "correct"),
        row("retest", "wrong"),
        row("quiz", "correct"),
        row("quiz", "correct")
    ]));
}

#[test]
fn reference_explanations_pass_at_full_and_deep_depths() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let example = std::fs::read_to_string(
        root.join("skills/careerbrew-project-explainer/references/example-careerbrew.md"),
    )
    .unwrap();
    assert!(explain::check(&example, false, false).is_empty());
    assert!(!explain::check(&example, false, true).is_empty());
    let deep = example.replace(
        "## Invariants and boundaries",
        "## How it runs\n\n- Start with `uv run careerbrew` [S1].\n\n## Invariants and boundaries",
    );
    assert!(explain::check(&deep, false, true).is_empty());
}

#[test]
fn prd_requires_pushback_only_when_estimates_exceed_the_slot() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let example = std::fs::read_to_string(
        root.join("skills/careerbrew-project-explainer/references/example-prd.md"),
    )
    .unwrap();
    assert!(prd::check(&example).is_empty());
    let start = example.find("## Pushback").unwrap();
    let end = example[start..]
        .find("\n## Open questions")
        .map(|end| start + end + 1)
        .unwrap();
    let without = format!("{}{}", &example[..start], &example[end..]);
    assert!(prd::check(&without)
        .iter()
        .any(|problem| problem.contains("requirements need 2.5 dev weeks")));
    assert!(
        prd::check(&without.replace("Plan slot: 1 dev week", "Plan slot: 3 dev weeks")).is_empty()
    );
}

#[test]
fn production_route_and_shipped_inventory_stay_untouched_by_the_component() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let routes = std::fs::read_to_string(root.join("config/native-routes.txt")).unwrap();
    let records = routes
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    assert!(records.iter().all(|record| record.len() == 2));
    let learn = records
        .iter()
        .filter(|record| record[0].starts_with("learn."))
        .collect::<Vec<_>>();
    assert_eq!(learn.len(), 14);
    assert!(learn.iter().all(|record| record[1] == "python"));
    for action in ["work.project.explain", "work.project.prd"] {
        let route = records.iter().find(|record| record[0] == action).unwrap();
        assert_eq!(route[1], "python");
    }
    let shipped = std::fs::read_to_string(root.join("scripts/rust-shipped-crates.txt")).unwrap();
    assert!(!shipped.lines().any(|line| line.trim() == "crates/learn"));
}
