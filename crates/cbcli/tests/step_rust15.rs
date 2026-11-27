//! rust-15 native handler against Python: `work session handoff`, in
//! synthetic homes (`common`) with one live project session.
mod common;

use common::*;

const HANDOFF: &str = "work.session.handoff";

/// The clock both sides read: the handler serves only a UTC clock.
const NOW: &str = "2026-11-20T09:00:00+00:00";

const OPEN: &[&[&str]] = &[
    &["work", "project", "add", "synth-proj"],
    &["work", "session", "open", "synth-proj", "--id", "sess-1"],
];

const OPEN_TWICE: &[&[&str]] = &[
    &["work", "project", "add", "synth-proj"],
    &["work", "session", "open", "synth-proj", "--id", "sess-1"],
    &[
        "work",
        "session",
        "handoff",
        "sess-1",
        "first status; next: review the draft",
    ],
];

#[test]
fn work_session_handoff_matches_python() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "handoff-open",
        cli: OPEN,
        files: &[],
    });
    for argv in [
        &[
            "work",
            "session",
            "handoff",
            "sess-1",
            "draft is ready; next: ask for review",
        ][..],
        &[
            "work",
            "session",
            "handoff",
            "synth-proj",
            "tests pass; next: merge",
        ][..],
        &[
            "work",
            "session",
            "handoff",
            "sess-1",
            "  spaced   text\tis folded  ",
        ][..],
        &[
            "work",
            "session",
            "handoff",
            "sess-1",
            "see the report",
            "--link",
            "https://example.com/x",
        ][..],
    ] {
        assert_same(&home, argv, true, &format!("{HANDOFF} open"));
    }
    let again = prepare_setup(&Setup {
        id: "handoff-replace",
        cli: OPEN_TWICE,
        files: &[],
    });
    assert_same(
        &again,
        &[
            "work",
            "session",
            "handoff",
            "sess-1",
            "second status; next: ship it",
        ],
        true,
        &format!("{HANDOFF} replace"),
    );
}

#[test]
fn work_session_handoff_refuses_what_it_cannot_copy() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "handoff-refuse",
        cli: OPEN,
        files: &[],
    });
    let long = "x".repeat(220);
    for (label, argv) in [
        // Python's own errors stay with Python.
        (
            "empty text",
            &["work", "session", "handoff", "sess-1", "   "][..],
        ),
        (
            "ftp link",
            &[
                "work", "session", "handoff", "sess-1", "status", "--link", "ftp://x",
            ][..],
        ),
        (
            "unknown session",
            &["work", "session", "handoff", "sess-9", "status"][..],
        ),
        (
            "long text",
            &["work", "session", "handoff", "sess-1", long.as_str()][..],
        ),
    ] {
        assert!(
            !assert_same(&home, argv, false, &format!("{HANDOFF} {label}")),
            "{label} must refuse"
        );
    }
    let none = prepare_setup(&Setup {
        id: "handoff-no-store",
        cli: &[],
        files: &[],
    });
    assert!(!assert_same(
        &none,
        &["work", "session", "handoff", "sess-1", "status"],
        false,
        HANDOFF
    ));
    let redact = prepare_setup(&Setup {
        id: "handoff-redaction",
        cli: OPEN,
        files: &[("profile.yaml", "name: Synthetic Person\n")],
    });
    assert!(!assert_same(
        &redact,
        &["work", "session", "handoff", "sess-1", "status"],
        false,
        HANDOFF
    ));
}

/// Negative control: two texts of different length leave different files,
/// so the comparison would see a wrong write.
#[test]
fn the_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "handoff-control",
        cli: OPEN,
        files: &[],
    });
    let short = observe(
        &home,
        &["work", "session", "handoff", "sess-1", "ok"],
        &Side::Native,
    );
    let long = observe(
        &home,
        &[
            "work",
            "session",
            "handoff",
            "sess-1",
            "a much longer status line; next: something else",
        ],
        &Side::Python,
    );
    assert_eq!(short.code, Some(0));
    assert_eq!(short.stdout, long.stdout);
    assert_ne!(short.files, long.files);
}
