//! Native rust-12 learning-rule and document-validator controls.

use careerbrew_learn::{cards, explain, prd, project};
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
fn short_explanation_keeps_every_cited_source_registered() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let example = std::fs::read_to_string(
        root.join("skills/careerbrew-project-explainer/references/example-careerbrew.md"),
    )
    .unwrap();
    let headings = regex::Regex::new(r"(?m)^## ").unwrap();
    let starts = headings
        .find_iter(&example)
        .map(|found| found.start())
        .collect::<Vec<_>>();
    let mut short = String::new();
    for (index, start) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(example.len());
        let section = &example[*start..end];
        let heading = section[3..].split('\n').next().unwrap_or("").trim();
        if explain::SHORT.contains(&explain::base(heading).as_str()) {
            short.push_str(section);
        }
    }
    assert!(explain::check(&short, true, false).is_empty());
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
fn learning_identity_uses_python_full_unicode_casefold() {
    assert_eq!(project::slug("Straße"), "strasse");
    assert_eq!(cards::topic_name("Straße"), "strasse");
    assert_eq!(cards::topic_name("\u{A7D0}"), "\u{A7D1}");
    assert_eq!(careerbrew_learn::casefold("\u{A7D0}"), "\u{A7D1}");
}

#[test]
fn retention_excludes_reviews_of_removed_concepts() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("data");
    std::env::set_var("HOME", scratch.path());
    std::env::set_var("CAREERBREW_HOME", &root);
    std::env::set_var("CAREERBREW_KEY_FILE", scratch.path().join("backup.key"));
    std::env::set_var("CAREERBREW_EVENTS", "off");
    std::env::set_var("CAREERBREW_ENCRYPT", "off");
    let folder = project::folder(&root, "Toy");
    let keep = json!({"id": "keep", "question": "Keep?", "answer": "Yes", "tags": ["core"]});
    let removed =
        json!({"id": "removed", "question": "Removed?", "answer": "No", "tags": ["core"]});
    cards::write_deck(&root, &folder, &[keep.clone(), removed.clone()]).unwrap();
    let at = chrono::DateTime::parse_from_rfc3339("2026-10-26T09:00:00+00:00").unwrap();
    for (id, ratings) in [("keep", ["good", "good"]), ("removed", ["good", "again"])] {
        for rating in ratings {
            cards::review(&root, &folder, id, rating, at, None).unwrap();
        }
    }
    cards::write_deck(&root, &folder, &[keep]).unwrap();
    let stats = cards::stats(&root, &folder, at).unwrap();
    assert_eq!(stats["retention_7d"], json!(1.0));
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
    // The CLI step module rust12 serves these six through careerbrew-native
    // (`crates/cbcli/src/steps/rust12.rs`); every other learn command stays on Python.
    const NATIVE: [&str; 6] = [
        "learn.list",
        "learn.note",
        "learn.status",
        "learn.next",
        "learn.tour.approve",
        "learn.tour.export",
    ];
    for record in &learn {
        let expected = if NATIVE.contains(&record[0]) {
            "native"
        } else {
            "python"
        };
        assert_eq!(record[1], expected, "{}", record[0]);
    }
    // rust24 serves the slot-range error of `work project prd` and the small
    // cases of `work project explain` natively
    // (`crates/cbcli/src/steps/rust24.rs`); this crate still has no route.
    for (action, expected) in [
        ("work.project.explain", "native"),
        ("work.project.prd", "native"),
    ] {
        let route = records.iter().find(|record| record[0] == action).unwrap();
        assert_eq!(route[1], expected);
    }
    let shipped = std::fs::read_to_string(root.join("scripts/rust-shipped-crates.txt")).unwrap();
    assert!(!shipped.lines().any(|line| line.trim() == "crates/learn"));
}
