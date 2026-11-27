//! Parity cases for the `rust-22` native handlers (`src/steps/rust22.rs`):
//! the served route key runs in fresh synthetic homes on both sides, plus
//! cases the handler must refuse and one negative control.
mod common;
use common::{assert_same, prepare_setup, strict, Setup};

const NOW: &str = "2026-11-20T09:00:00";
const TERMS: (&str, &str) = ("sensitive-terms.txt", "zebrafruit\n");

fn refuses(setup: &Setup, args: &[&str], label: &str) {
    let home = prepare_setup(setup);
    assert!(
        !assert_same(&home, args, false, label),
        "{label}: {args:?} served"
    );
}

const ATTEMPTS: &str = concat!(
    "{\"exercise\": \"two-sum\", \"score\": 0.5, \"correct\": false, \"seconds\": 40}\n",
    "{\"exercise\": \"two-sum\", \"score\": 1, \"correct\": false}\n",
    "{\"exercise\": \"valid-anagram\", \"score\": 1.0, \"correct\": true, \"seconds\": null}\n",
);

#[test]
fn exercises_list_matches_python() {
    strict(NOW);
    // Route key: "exercises.list".
    let home = prepare_setup(&Setup {
        id: "exercises",
        cli: &[],
        files: &[],
    });
    assert_same(&home, &["exercises", "list"], true, "exercises.list fresh");
    let progress = Setup {
        id: "exercises-progress",
        cli: &[],
        files: &[
            ("tutor/exercise-attempts.jsonl", ATTEMPTS),
            (
                "tutor/synced-solved.json",
                "{\"exercises\": [\"contains-duplicate\", \"no-such-one\"]}",
            ),
        ],
    };
    let home = prepare_setup(&progress);
    assert_same(
        &home,
        &["exercises", "list"],
        true,
        "exercises.list progress",
    );
    let bad = Setup {
        id: "exercises-bad",
        cli: &[],
        files: &[(
            "tutor/exercise-attempts.jsonl",
            "{\"exercise\": \"two-sum\", \"correct\": true}\n",
        )],
    };
    refuses(
        &bad,
        &["exercises", "list"],
        "exercises.list row without score",
    );
    let terms = Setup {
        id: "exercises-terms",
        cli: &[],
        files: &[TERMS],
    };
    refuses(&terms, &["exercises", "list"], "exercises.list redaction");
}
