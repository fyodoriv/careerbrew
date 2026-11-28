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

// ---------- wave 4: work session open ----------

const OPEN_CMD: &str = "work.session.open";

const PROJECT: &[&[&str]] = &[&["work", "project", "add", "synth-proj"]];

#[test]
fn work_session_open_unknown_project_matches_python() {
    strict(NOW);
    for (id, cli) in [("open-no-projects", &[][..]), ("open-other", PROJECT)] {
        let home = prepare_setup(&Setup {
            id,
            cli,
            files: &[],
        });
        for argv in [
            &["work", "session", "open", "ghost-proj"][..],
            &["work", "session", "open", "ghost-proj", "--id", "sess-9"][..],
            &["work", "session", "open", "ghost-proj", "--json"][..],
        ] {
            assert_same(&home, argv, true, &format!("{OPEN_CMD} {id}"));
        }
    }
}

#[test]
fn work_session_open_refuses_a_known_project() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "open-known",
        cli: PROJECT,
        files: &[],
    });
    for argv in [
        &["work", "session", "open", "synth-proj"][..],
        &["work", "session", "open", " synth-proj"][..],
    ] {
        assert!(
            !assert_same(&home, argv, false, &format!("{OPEN_CMD} known")),
            "{argv:?} must refuse"
        );
    }
    let control = observe(
        &home,
        &["work", "session", "open", "ghost-proj"],
        &Side::Native,
    );
    assert_eq!(control.code, Some(2));
    assert!(control.stdout.starts_with("error: unknown project"));
}

// ---------- wave 6: work session ask ----------

const ASK: &str = "work.session.ask";

/// A plain-text data folder: a sealed write differs on every run, so the
/// handler refuses it (the hosted Linux runner seals homes without this).
const PLAIN: &[(&str, &str)] = &[(".plaintext", "")];

const ASKED_ONCE: &[&[&str]] = &[
    &["work", "project", "add", "synth-proj"],
    &["work", "session", "open", "synth-proj", "--id", "sess-1"],
    &["work", "session", "ask", "sess-1", "an earlier question"],
];

const QUIET: &[&[&str]] = &[
    &["work", "project", "add", "synth-proj"],
    &["work", "session", "open", "synth-proj", "--id", "sess-1"],
    &[
        "work",
        "project",
        "set",
        "synth-proj",
        "--quiet-until",
        "2026-12-01",
    ],
];

/// A session opened on an earlier day, written as `sessions._load` reads it.
const STALE: &[(&str, &str)] = &[
    (".plaintext", ""),
    (
        "work/sessions.json",
        "{\"claims\": [], \"sessions\": [{\"day\": \"2026-11-19\", \"ended\": \"\", \"id\": \"sess-old\", \"project\": \"synth-proj\", \"started\": \"2026-11-19T09:00:00+00:00\"}]}",
    ),
];

#[test]
fn work_session_ask_saves_a_new_question_like_python() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "ask-open",
        cli: OPEN,
        files: PLAIN,
    });
    for argv in [
        &[
            "work",
            "session",
            "ask",
            "sess-1",
            "Which branch should I use?",
        ][..],
        &["work", "session", "ask", "synth-proj", "Merge now?"][..],
        &[
            "work",
            "session",
            "ask",
            "sess-1",
            "Pick a base",
            "--option",
            "main",
            "--option",
            "dev",
            "--option",
            " main ",
            "--pick",
            "dev",
        ][..],
        &[
            "work",
            "session",
            "ask",
            "sess-1",
            "  spaced   question\tfolds  ",
            "--recheck",
            "https://example.com/pr/1",
        ][..],
        &[
            "work",
            "session",
            "ask",
            "sess-1",
            "Sign in again?",
            "--recheck",
            "careerbrew:signin",
        ][..],
        &["work", "session", "ask", "sess-1", "caf\u{e9} \u{2014} ok?"][..],
    ] {
        assert_same(&home, argv, true, &format!("{ASK} new"));
    }
    let again = prepare_setup(&Setup {
        id: "ask-second",
        cli: ASKED_ONCE,
        files: PLAIN,
    });
    assert_same(
        &again,
        &["work", "session", "ask", "sess-1", "a second question"],
        true,
        &format!("{ASK} second"),
    );
}

#[test]
fn work_session_ask_errors_match_python() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "ask-errors",
        cli: OPEN,
        files: PLAIN,
    });
    let long = "x".repeat(201);
    let long_option = "y".repeat(81);
    for (label, argv) in [
        (
            "empty question",
            &["work", "session", "ask", "sess-1", "   "][..],
        ),
        (
            "long question",
            &["work", "session", "ask", "sess-1", long.as_str()][..],
        ),
        (
            "long option",
            &[
                "work",
                "session",
                "ask",
                "sess-1",
                "q",
                "--option",
                long_option.as_str(),
            ][..],
        ),
        (
            "seven options",
            &[
                "work", "session", "ask", "sess-1", "q", "--option", "a", "--option", "b",
                "--option", "c", "--option", "d", "--option", "e", "--option", "f", "--option",
                "g",
            ][..],
        ),
        (
            "pick not an option",
            &[
                "work", "session", "ask", "sess-1", "q", "--option", "a", "--pick", "b",
            ][..],
        ),
        (
            "unknown session",
            &["work", "session", "ask", "sess-9", "q"][..],
        ),
    ] {
        assert_same(&home, argv, true, &format!("{ASK} {label}"));
    }
    let none = prepare_setup(&Setup {
        id: "ask-no-store",
        cli: &[],
        files: PLAIN,
    });
    assert_same(
        &none,
        &["work", "session", "ask", "sess-1", "q"],
        true,
        &format!("{ASK} no store"),
    );
    let stale = prepare_setup(&Setup {
        id: "ask-stale",
        cli: PROJECT,
        files: STALE,
    });
    assert_same(
        &stale,
        &["work", "session", "ask", "sess-old", "q"],
        true,
        &format!("{ASK} stale"),
    );
}

#[test]
fn work_session_ask_refuses_what_it_cannot_copy() {
    strict(NOW);
    let again = prepare_setup(&Setup {
        id: "ask-refuse-repeat",
        cli: ASKED_ONCE,
        files: PLAIN,
    });
    let home = prepare_setup(&Setup {
        id: "ask-refuse",
        cli: OPEN,
        files: PLAIN,
    });
    let sealed = prepare_setup(&Setup {
        id: "ask-refuse-sealed",
        cli: OPEN,
        files: &[],
    });
    let quiet = prepare_setup(&Setup {
        id: "ask-refuse-quiet",
        cli: QUIET,
        files: PLAIN,
    });
    let redact = prepare_setup(&Setup {
        id: "ask-refuse-redaction",
        cli: OPEN,
        files: &[
            (".plaintext", ""),
            ("profile.yaml", "name: Synthetic Person\n"),
        ],
    });
    for (label, at, argv) in [
        (
            "same question again",
            &again,
            &["work", "session", "ask", "sess-1", "an earlier question"][..],
        ),
        (
            "command recheck",
            &home,
            &["work", "session", "ask", "sess-1", "q", "--recheck", "true"][..],
        ),
        (
            "sealed folder",
            &sealed,
            &["work", "session", "ask", "sess-1", "q"][..],
        ),
        (
            "quiet project",
            &quiet,
            &["work", "session", "ask", "sess-1", "q"][..],
        ),
        (
            "redaction input",
            &redact,
            &["work", "session", "ask", "sess-1", "q"][..],
        ),
    ] {
        assert!(
            !assert_same(at, argv, false, &format!("{ASK} {label}")),
            "{label} must refuse"
        );
    }
}

/// Negative control: two questions leave different day files, so the
/// comparison would see a wrong write.
#[test]
fn the_ask_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "ask-control",
        cli: OPEN,
        files: PLAIN,
    });
    let one = observe(
        &home,
        &["work", "session", "ask", "sess-1", "first"],
        &Side::Native,
    );
    let two = observe(
        &home,
        &["work", "session", "ask", "sess-1", "second"],
        &Side::Python,
    );
    assert_eq!(one.code, Some(0));
    assert_eq!(two.code, Some(0));
    assert_ne!(one.stdout, two.stdout);
    assert_ne!(one.files, two.files);
}

// ---------- wave 8: day check, day update, day question add ----------

const CHECK: &str = "day.check";
const UPDATE: &str = "day.update";
const QUESTION_ADD: &str = "day.question.add";

/// Two `work_steps` from the settings, one after the other.
const STEPS: &[(&str, &str)] = &[
    (".plaintext", ""),
    (
        "settings.yaml",
        "work_steps:\n- /synth-review\n- step: /synth-ship\n  after: /synth-review\n  timeout: 5\n",
    ),
];

/// A row started at 01:00 UTC: past its 30 minutes at 09:00.
const RUNNING_ONCE: &[(&str, &str)] = &[
    (".plaintext", ""),
    (
        "day/work-2026-11-20.json",
        "{\"day\": \"2026-11-20\", \"handled\": {}, \"rows\": [{\"at\": \"2026-11-20T01:00:00+00:00\", \"attempt\": 1, \"ended\": \"\", \"error_class\": \"\", \"link\": \"\", \"needs_you\": \"\", \"next\": \"\", \"retry\": false, \"seconds\": null, \"started\": \"2026-11-20T01:00:00+00:00\", \"status\": \"running\", \"step\": \"/careerbrew\", \"summary\": \"\", \"timed_out\": false, \"verify\": \"\"}]}",
    ),
];

/// The same row on its second attempt: the next timeout fails it.
const RUNNING_TWICE: &[(&str, &str)] = &[
    (".plaintext", ""),
    (
        "day/work-2026-11-20.json",
        "{\"day\": \"2026-11-20\", \"handled\": {}, \"rows\": [{\"at\": \"2026-11-20T01:00:00+00:00\", \"attempt\": 2, \"ended\": \"\", \"error_class\": \"\", \"link\": \"\", \"needs_you\": \"\", \"next\": \"\", \"retry\": false, \"seconds\": null, \"started\": \"2026-11-20T01:00:00+00:00\", \"status\": \"running\", \"step\": \"/careerbrew\", \"summary\": \"\", \"timed_out\": false, \"verify\": \"\"}]}",
    ),
];

const HELD: &[&[&str]] = &[&["day", "wrap", "--hold"]];

const BLOCKED: &[&[&str]] = &[&[
    "day",
    "update",
    "/careerbrew",
    "--status",
    "blocked",
    "--needs-you",
    "pick one",
]];

const ASKED: &[&[&str]] = &[&[
    "day",
    "question",
    "add",
    "/careerbrew",
    "pick a reviewer",
    "--option",
    "synth-a",
    "--option",
    "synth-b",
]];

#[test]
fn day_check_matches_python() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "check-empty",
        cli: &[],
        files: PLAIN,
    });
    let steps = prepare_setup(&Setup {
        id: "check-steps",
        cli: &[],
        files: STEPS,
    });
    let once = prepare_setup(&Setup {
        id: "check-retry",
        cli: &[],
        files: RUNNING_ONCE,
    });
    let twice = prepare_setup(&Setup {
        id: "check-failed",
        cli: &[],
        files: RUNNING_TWICE,
    });
    let held = prepare_setup(&Setup {
        id: "check-held",
        cli: HELD,
        files: PLAIN,
    });
    for (label, at) in [
        ("empty", &empty),
        ("steps", &steps),
        ("retry", &once),
        ("failed", &twice),
        ("held", &held),
    ] {
        for argv in [&["day", "check"][..], &["day", "check", "--json"][..]] {
            assert_same(at, argv, true, &format!("{CHECK} {label}"));
        }
    }
}

#[test]
fn day_update_matches_python() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "update-empty",
        cli: &[],
        files: PLAIN,
    });
    let steps = prepare_setup(&Setup {
        id: "update-steps",
        cli: &[],
        files: STEPS,
    });
    let asked = prepare_setup(&Setup {
        id: "update-asked",
        cli: BLOCKED,
        files: PLAIN,
    });
    let empty_cases: &[&[&str]] = &[
        &["day", "update", "/careerbrew", "--status", "running"],
        &[
            "day",
            "update",
            "careerbrew",
            "--status",
            "done",
            "--summary",
            "all good",
        ],
        &[
            "day",
            "update",
            "/careerbrew",
            "--status",
            "blocked",
            "--needs-you",
            "pick a reviewer",
        ],
        &["day", "update", "/careerbrew", "--status", "blocked"],
        &[
            "day",
            "update",
            "/careerbrew",
            "--status",
            "failed",
            "--error-class",
            "tool",
        ],
        &["day", "update", "/careerbrew", "--status", "failed"],
        &[
            "day",
            "update",
            "/careerbrew",
            "--status",
            "running",
            "--error-class",
            "env",
        ],
        &["day", "update", "/careerbrew"],
        &["day", "update", "/synth-missing", "--status", "running"],
        &[
            "day",
            "update",
            "/careerbrew",
            "--link",
            "https://example.com/x",
        ],
        &[
            "day",
            "update",
            "/careerbrew",
            "--handled",
            "checkout:synth-repo",
            "--handled",
            "pr:synth-repo#7",
        ],
    ];
    for argv in empty_cases {
        assert_same(&empty, argv, true, &format!("{UPDATE} empty"));
    }
    let steps_cases: &[&[&str]] = &[
        &["day", "update", "/synth-review", "--status", "running"],
        &["day", "update", "/synth-ship", "--status", "done"],
        &["day", "update", "/synth-review", "--status", "done"],
    ];
    for argv in steps_cases {
        assert_same(&steps, argv, true, &format!("{UPDATE} steps"));
    }
    let asked_cases: &[&[&str]] = &[
        &["day", "update", "/careerbrew", "--status", "done"],
        &[
            "day",
            "update",
            "/careerbrew",
            "--status",
            "blocked",
            "--needs-you",
            "pick one",
        ],
        &["day", "update", "/careerbrew", "--summary", "still waiting"],
    ];
    for argv in asked_cases {
        assert_same(&asked, argv, true, &format!("{UPDATE} asked"));
    }
}

#[test]
fn day_question_add_matches_python() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "question-empty",
        cli: &[],
        files: PLAIN,
    });
    let asked = prepare_setup(&Setup {
        id: "question-asked",
        cli: ASKED,
        files: PLAIN,
    });
    let empty_cases: &[&[&str]] = &[
        &[
            "day",
            "question",
            "add",
            "/careerbrew",
            "pick a reviewer",
            "--option",
            "synth-a",
            "--option",
            "synth-b",
            "--pick",
            "synth-a",
        ],
        &[
            "day",
            "question",
            "add",
            "careerbrew",
            "is the page up?",
            "--recheck",
            "https://example.com/x",
        ],
        &["day", "question", "add", "/careerbrew", "  spaced   ask  "],
        &["day", "question", "add", "/synth-missing", "q"],
        &[
            "day",
            "question",
            "add",
            "/careerbrew",
            "q",
            "--plugin",
            "synth-plugin",
        ],
        &["day", "question", "add", "/careerbrew", "   "],
        &[
            "day",
            "question",
            "add",
            "/careerbrew",
            "q",
            "--option",
            "a",
            "--pick",
            "b",
        ],
    ];
    for argv in empty_cases {
        assert_same(&empty, argv, true, &format!("{QUESTION_ADD} empty"));
    }
    let asked_cases: &[&[&str]] = &[
        &[
            "day",
            "question",
            "add",
            "/careerbrew",
            "pick a reviewer",
            "--option",
            "synth-a",
            "--option",
            "synth-b",
        ],
        &["day", "question", "add", "/careerbrew", "another ask"],
    ];
    for argv in asked_cases {
        assert_same(&asked, argv, true, &format!("{QUESTION_ADD} asked"));
    }
}

#[test]
fn the_work_day_plan_refuses_what_it_cannot_copy() {
    strict(NOW);
    let sealed = prepare_setup(&Setup {
        id: "plan-refuse-sealed",
        cli: &[],
        files: &[],
    });
    let verify = prepare_setup(&Setup {
        id: "plan-refuse-verify",
        cli: &[],
        files: &[
            (".plaintext", ""),
            (
                "settings.yaml",
                "work_steps:\n- step: /synth-test\n  verify: uv run python -m unittest\n",
            ),
        ],
    });
    let other = prepare_setup(&Setup {
        id: "plan-refuse-project",
        cli: &[&["work", "project", "add", "synth-proj"]],
        files: PLAIN,
    });
    let home = prepare_setup(&Setup {
        id: "plan-refuse",
        cli: &[],
        files: PLAIN,
    });
    let cases: &[(&str, &Home, &[&str])] = &[
        ("sealed check", &sealed, &["day", "check"]),
        (
            "sealed update",
            &sealed,
            &["day", "update", "/careerbrew", "--status", "running"],
        ),
        (
            "sealed question",
            &sealed,
            &["day", "question", "add", "/careerbrew", "q"],
        ),
        (
            "verify step",
            &verify,
            &["day", "update", "/synth-test", "--status", "done"],
        ),
        ("verify check", &verify, &["day", "check"]),
        (
            "a handled item it does not check",
            &home,
            &["day", "update", "/careerbrew", "--handled", "branch:synth"],
        ),
        ("a project file", &other, &["day", "check"]),
        (
            "command recheck",
            &home,
            &[
                "day",
                "question",
                "add",
                "/careerbrew",
                "q",
                "--recheck",
                "true",
            ],
        ),
    ];
    for (label, at, argv) in cases {
        assert!(
            !assert_same(at, argv, false, &format!("work day plan {label}")),
            "{label} must refuse"
        );
    }
}

/// Negative control: two statuses leave different day files and lines, so
/// the comparison would see a wrong write.
#[test]
fn the_work_day_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "plan-control",
        cli: &[],
        files: PLAIN,
    });
    let one = observe(
        &home,
        &["day", "update", "/careerbrew", "--status", "running"],
        &Side::Native,
    );
    let two = observe(
        &home,
        &["day", "update", "/careerbrew", "--status", "done"],
        &Side::Python,
    );
    assert_eq!(one.code, Some(0));
    assert_eq!(two.code, Some(0));
    assert_ne!(one.stdout, two.stdout);
    assert_ne!(one.files, two.files);
}
