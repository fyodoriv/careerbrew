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

const QUEUE: &str = "long_term:
- id: staff
  text: Reach staff engineer
  by: '2027-06-01'
- id: talk
  text: Give a talk
  by: null
queue:
- id: old
  kind: learn
  title: Old goal
  date: '2026-11-01'
  topics: [graphs]
  minutes_per_day: 60
- id: big
  kind: promotion
  title: Promo case
  date: '2026-12-15'
  minutes_per_day: 45
- id: near
  kind: learn
  title: Learn graphs
  date: '2026-11-25'
  topics: [graphs, dp]
  minutes_per_day: 90
- id: open
  kind: custom
  title: Undated thing
  minutes_per_day: 30
";
const LEGACY: &str = "upcoming:
  id: iv
  kind: interview
  title: Interview prep
  date: '2026-11-20'
  company: null
  minutes_per_day: 120
";
const UNDATED: &str = "queue:
- id: js
  kind: job-search
  title: Find a job \u{2713}
  minutes_per_day: 20
- id: past
  kind: custom
  title: Other
  date: '2026-10-01'
  minutes_per_day: 20
";
const COMPANY: &str = "queue:
- id: iv
  kind: interview
  title: Interview at Synthco
  company: Synthco
  minutes_per_day: 120
";
const BARE_DATE: &str = "queue:
- id: a
  kind: learn
  title: A
  date: 2026-12-01
  minutes_per_day: 20
";
const WEEK_DATE: &str = "queue:
- id: a
  kind: learn
  title: A
  date: '2026-W48-1'
  minutes_per_day: 20
";
const BAD_ZONE: (&str, &str) = ("settings.yaml", "timezone: Nowhere/Synthetic\n");

#[test]
fn goals_show_matches_python() {
    strict(NOW);
    // Route key: "goals.show".
    let empty = prepare_setup(&Setup {
        id: "show-empty",
        cli: &[],
        files: &[],
    });
    assert_same(&empty, &["goals", "show"], true, "goals.show empty");
    let served: [(&str, &'static [(&str, &str)]); 3] = [
        ("show-queue", &[("goals.yaml", QUEUE)]),
        ("show-legacy", &[("goals.yaml", LEGACY)]),
        ("show-undated", &[("goals.yaml", UNDATED)]),
    ];
    for (id, files) in served {
        let home = prepare_setup(&Setup {
            id,
            cli: &[],
            files,
        });
        assert_same(&home, &["goals", "show"], true, id);
    }
    // Python reads no machine role for `goals`: a corporate home is served.
    let corporate = prepare_setup(&Setup {
        id: "show-corp",
        cli: &[],
        files: &[CORPORATE, ("goals.yaml", UNDATED)],
    });
    assert_same(&corporate, &["goals", "show"], true, "goals.show corporate");
    let encrypted = prepare_setup(&Setup {
        id: "show-encrypted",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[("goals.yaml", QUEUE)],
    });
    assert_same(&encrypted, &["goals", "show"], true, "goals.show encrypted");
    let zone = prepare_setup(&Setup {
        id: "show-zone",
        cli: &[],
        files: &[
            ("settings.yaml", "timezone: Asia/Tokyo\n"),
            ("goals.yaml", QUEUE),
        ],
    });
    assert_same(&zone, &["goals", "show"], true, "goals.show zone");
    let refused: [(&str, &'static [(&str, &str)]); 6] = [
        ("show-company", &[("goals.yaml", COMPANY)]),
        ("show-bare-date", &[("goals.yaml", BARE_DATE)]),
        ("show-week-date", &[("goals.yaml", WEEK_DATE)]),
        ("show-list", &[("goals.yaml", "- a\n- b\n")]),
        ("show-bad-zone", &[BAD_ZONE, ("goals.yaml", QUEUE)]),
        ("show-terms", &[TERMS, ("goals.yaml", QUEUE)]),
    ];
    for (id, files) in refused {
        refuses(
            &Setup {
                id,
                cli: &[],
                files,
            },
            &["goals", "show"],
            id,
        );
    }
}

/// Negative control: the comparison sees the goal file, and the current
/// goal follows the pinned date.
#[test]
fn goals_show_output_follows_the_goal_file() {
    strict(NOW);
    let queue = prepare_setup(&Setup {
        id: "control-queue",
        cli: &[],
        files: &[("goals.yaml", QUEUE)],
    });
    let legacy = prepare_setup(&Setup {
        id: "control-legacy",
        cli: &[],
        files: &[("goals.yaml", LEGACY)],
    });
    let a = common::observe(&queue, &["goals", "show"], &common::Side::Native);
    let b = common::observe(&legacy, &["goals", "show"], &common::Side::Native);
    assert_ne!(a, b);
    assert!(a
        .stdout
        .contains("upcoming: Learn graphs [learn] \u{b7} 5 day(s) left \u{b7} 90 min/day\n"));
    assert!(b.stdout.contains("focus: the interview loop\n"));
}

const APPROVALS: &str = r#"[{"id": "a0", "title": "Old synthetic ask", "detail": "", "by": "agent", "at": "2026-11-19T08:00:00+00:00", "status": "approved", "note": "", "decided_at": "2026-11-19T09:00:00+00:00"}, {"id": "a1", "title": "Merge synthetic change ✓", "detail": "", "by": "agent", "at": "2026-11-20T08:00:00+00:00", "status": "pending", "note": ""}, {"id": "a2", "title": "Second ask", "detail": "", "by": "agent", "at": "2026-11-20T08:30:00+00:00", "status": "pending", "note": ""}]"#;
const BAD_APPROVALS: &str = r#"[{"id": "a1", "title": "No stamp", "status": "pending"}]"#;
const SHORT_GOAL: &str = "queue:
- id: g
  kind: learn
  title: Learn graphs
  topics: [graphs]
  minutes_per_day: 10
";
const COMPANY_FREE: &str = "queue:
- id: iv
  kind: interview
  title: Interview prep
  company: null
  minutes_per_day: 120
";
const STEPS: &str = "{\"at\": \"2026-11-19T08:00:00+00:00\", \"kind\": \"card\"}
{\"at\": \"2026-11-20T08:00:00+00:00\", \"kind\": \"card\", \"outcome\": \"good\", \"ref\": \"c1\", \"rules\": \"x\", \"seconds\": 400}
{\"at\": \"2026-11-20T08:30:00+00:00\", \"kind\": \"exercise\", \"outcome\": \"pass\", \"ref\": \"e1\", \"rules\": \"x\", \"seconds\": 230}
";
const TOKYO_STEPS: &str =
    "{\"at\": \"2026-11-19T20:00:00+00:00\", \"kind\": \"card\", \"seconds\": 650}
{\"at\": \"2026-11-19T14:00:00+00:00\", \"kind\": \"card\", \"seconds\": 5000}
";
const UTC: (&str, &str) = ("settings.yaml", "timezone: UTC\n");
const BAD_MINUTES: &str = "queue:
- id: g
  kind: learn
  title: Learn graphs
  minutes_per_day: '10'
";

#[test]
fn teach_next_matches_python() {
    strict(NOW);
    // Route key: "teach.next".
    let served: [(&str, &'static [(&str, &str)]); 6] = [
        ("next-empty", &[]),
        ("next-approval", &[("approvals.json", APPROVALS)]),
        (
            "next-approval-goal",
            &[("approvals.json", APPROVALS), ("goals.yaml", COMPANY_FREE)],
        ),
        ("next-none-pending", &[("approvals.json", "[]")]),
        (
            "next-done",
            &[
                UTC,
                ("goals.yaml", SHORT_GOAL),
                ("teach/steps.jsonl", STEPS),
            ],
        ),
        (
            "next-done-tokyo",
            &[
                ("settings.yaml", "timezone: Asia/Tokyo\n"),
                ("goals.yaml", SHORT_GOAL),
                ("teach/steps.jsonl", TOKYO_STEPS),
            ],
        ),
    ];
    for (id, files) in served {
        let home = prepare_setup(&Setup {
            id,
            cli: &[],
            files,
        });
        assert_same(&home, &["teach", "next"], true, id);
    }
    let encrypted = prepare_setup(&Setup {
        id: "next-encrypted",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[("approvals.json", APPROVALS)],
    });
    assert_same(&encrypted, &["teach", "next"], true, "next-encrypted");
    let json = Setup {
        id: "next-json",
        cli: &[],
        files: &[],
    };
    // Python's JSON holds the wall clock (`started`), so two Python runs may
    // differ: check only the native refusal here.
    let json = prepare_setup(&json);
    let native = common::observe(&json, &["teach", "next", "--json"], &common::Side::Native);
    assert_eq!(native.code, Some(64), "teach.next --json served");
    assert!(native.stdout.is_empty() && native.events.is_empty());
    let refused: [(&str, &'static [(&str, &str)]); 9] = [
        ("next-corp", &[CORPORATE]),
        ("next-terms", &[TERMS, ("approvals.json", APPROVALS)]),
        ("next-bad-approvals", &[("approvals.json", BAD_APPROVALS)]),
        ("next-company", &[("goals.yaml", COMPANY)]),
        ("next-bad-minutes", &[("goals.yaml", BAD_MINUTES)]),
        ("next-below-budget", &[UTC, ("goals.yaml", SHORT_GOAL)]),
        (
            "next-mistakes",
            &[
                UTC,
                ("goals.yaml", SHORT_GOAL),
                ("teach/steps.jsonl", STEPS),
                ("tutor/mistakes.jsonl", ""),
            ],
        ),
        (
            "next-naive-step",
            &[
                UTC,
                ("goals.yaml", SHORT_GOAL),
                (
                    "teach/steps.jsonl",
                    "{\"at\": \"2026-11-20T08:00:00\", \"kind\": \"card\", \"seconds\": 900}\n",
                ),
            ],
        ),
        ("next-bad-zone", &[BAD_ZONE, ("goals.yaml", SHORT_GOAL)]),
    ];
    for (id, files) in refused {
        refuses(
            &Setup {
                id,
                cli: &[],
                files,
            },
            &["teach", "next"],
            id,
        );
    }
}

/// Negative control: the comparison sees the approvals and step files.
#[test]
fn teach_next_output_follows_the_data() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "next-control-empty",
        cli: &[],
        files: &[],
    });
    let asked = prepare_setup(&Setup {
        id: "next-control-asked",
        cli: &[],
        files: &[("approvals.json", APPROVALS)],
    });
    let done = prepare_setup(&Setup {
        id: "next-control-done",
        cli: &[],
        files: &[
            UTC,
            ("goals.yaml", SHORT_GOAL),
            ("teach/steps.jsonl", STEPS),
        ],
    });
    let a = common::observe(&empty, &["teach", "next"], &common::Side::Native);
    let b = common::observe(&asked, &["teach", "next"], &common::Side::Native);
    let c = common::observe(&done, &["teach", "next"], &common::Side::Native);
    assert_eq!(
        a.stdout,
        "next: Set a goal first\nwhy: Lessons and exercises follow your upcoming goal, and none is set yet.\n"
    );
    assert!(b
        .stdout
        .starts_with("next: Approve or reject: Merge synthetic change \u{2713}\n"));
    assert_eq!(
        c.stdout,
        "next: That is today's study time. Well done.\nwhy: You used your 10 minutes for Learn graphs.\n"
    );
}
