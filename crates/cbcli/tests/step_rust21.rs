//! Parity cases for the `rust-21` native handlers (`src/steps/rust21.rs`):
//! each served route key runs in fresh synthetic homes on both sides, plus
//! cases the handler must refuse and one negative control per command.
mod common;
use common::{assert_same, prepare_setup, strict, Setup};

const NOW: &str = "2026-11-20T09:00:00";
const CORPORATE: (&str, &str) = ("machine.yaml", "role: corporate\n");
const TERMS: (&str, &str) = ("sensitive-terms.txt", "zebrafruit\n");
const SEED: &[&[&str]] = &[&["teach", "feedback", "first synthetic note"]];

fn refuses(setup: &Setup, args: &[&str], label: &str) {
    let home = prepare_setup(setup);
    assert!(
        !assert_same(&home, args, false, label),
        "{label}: {args:?} served"
    );
}

#[test]
fn teach_feedback_matches_python() {
    strict(NOW);
    // Route key: "teach.feedback".
    let home = prepare_setup(&Setup {
        id: "feedback",
        cli: SEED,
        files: &[],
    });
    assert_same(
        &home,
        &["teach", "feedback", "  slower   lessons\tplease \n"],
        true,
        "teach.feedback spaces",
    );
    assert_same(
        &home,
        &[
            "teach",
            "feedback",
            "more examples",
            "--step",
            "a-very-long-step-kind-name-over-thirty",
        ],
        true,
        "teach.feedback step",
    );
    let fresh = prepare_setup(&Setup {
        id: "feedback-fresh",
        cli: &[],
        files: &[],
    });
    assert_same(
        &fresh,
        &["teach", "feedback", "first ever"],
        true,
        "teach.feedback fresh",
    );
    let odd = Setup {
        id: "feedback-odd",
        cli: SEED,
        files: &[],
    };
    refuses(
        &odd,
        &["teach", "feedback", "odd\u{1c}space"],
        "teach.feedback separator",
    );
    let terms = Setup {
        id: "feedback-terms",
        cli: &[],
        files: &[TERMS],
    };
    refuses(
        &terms,
        &["teach", "feedback", "a note"],
        "teach.feedback redaction",
    );
}

#[test]
fn teach_improvements_matches_python() {
    strict(NOW);
    // Route key: "teach.improvements".
    let home = prepare_setup(&Setup {
        id: "improvements",
        cli: &[],
        files: &[],
    });
    assert_same(
        &home,
        &["teach", "improvements"],
        true,
        "teach.improvements empty",
    );
    let written = Setup {
        id: "improvements-written",
        cli: SEED,
        files: &[],
    };
    refuses(
        &written,
        &["teach", "improvements"],
        "teach.improvements with feedback",
    );
    let corporate = Setup {
        id: "improvements-corp",
        cli: &[],
        files: &[CORPORATE],
    };
    refuses(
        &corporate,
        &["teach", "improvements"],
        "teach.improvements corporate",
    );
}

#[test]
fn goals_add_long_term_matches_python() {
    strict(NOW);
    // Route key: "goals.add-long-term". The tests run with no terminal, so
    // this is the agent's refusal.
    let home = prepare_setup(&Setup {
        id: "goals",
        cli: &[],
        files: &[],
    });
    assert_same(
        &home,
        &["goals", "add-long-term", "Lead a team"],
        true,
        "goals.add-long-term",
    );
    assert_same(
        &home,
        &["goals", "add-long-term", "Ship it", "--by", "2027-06-01"],
        true,
        "goals.add-long-term by",
    );
    let terms = Setup {
        id: "goals-terms",
        cli: &[],
        files: &[TERMS],
    };
    refuses(
        &terms,
        &["goals", "add-long-term", "Lead a team"],
        "goals.add-long-term redaction",
    );
    let corporate = Setup {
        id: "goals-corp",
        cli: &[],
        files: &[CORPORATE],
    };
    refuses(
        &corporate,
        &["goals", "add-long-term", "Lead a team"],
        "goals.add-long-term corporate",
    );
}
