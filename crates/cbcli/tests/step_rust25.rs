//! Parity cases for the `rust-25` native handlers (`src/steps/rust25.rs`):
//! each served route key runs in fresh synthetic homes on both sides, plus
//! cases the handler must refuse and one negative control.
mod common;
use common::{assert_same, observe, prepare_setup, strict, Setup, Side};

const NOW: &str = "2026-11-20T09:00:00";
const CORPORATE: (&str, &str) = ("machine.yaml", "role: corporate\n");
const TERMS: (&str, &str) = ("sensitive-terms.txt", "zebrafruit\n");

/// A goal and three income rows, written by the Python CLI (sealed when
/// the vault is on): 2026-10 is 25.05% of the target, 2026-09 33.3%.
const INCOME: &[&[&str]] = &[
    &["indie", "goal", "--monthly", "1000", "--by", "2027-06-30"],
    &["indie", "income", "--month", "2026-10", "--amount", "250"],
    &["indie", "income", "--month", "2026-09", "--amount", "333"],
    &["indie", "income", "--month", "2026-10", "--amount", "0.5"],
];

/// Two synthetic bets: `alpha` is complete and has two PIVOT readings in a
/// row, `beta` is a proposed bet with nothing filled in.
const BETS: (&str, &str) = (
    "indie/bets.yaml",
    "bets:\n  beta:\n    path: /nonexistent/beta\n    origin: ''\n    kind: ''\n    hypothesis: ''\n    metric: ''\n    success: null\n    pivot: null\n    higher_is_better: true\n    measure: ''\n    state: proposed\n    decided_at: ''\n  alpha:\n    path: /nonexistent/alpha\n    origin: ''\n    kind: product\n    hypothesis: synthetic devs pay for a widget\n    metric: users\n    success: 10.0\n    pivot: 2.0\n    higher_is_better: true\n    measure: ''\n    state: active\n    decided_at: '2026-11-01T09:00:00+00:00'\n    decided_readings: 1\n",
);
const MEASURES: (&str, &str) = (
    "indie/measures.jsonl",
    "{\"at\": \"2026-11-02T09:00:00+00:00\", \"bet\": \"alpha\", \"status\": \"KEEP\", \"value\": 12.0}\n{\"at\": \"2026-11-09T09:00:00+00:00\", \"bet\": \"alpha\", \"status\": \"PIVOT\", \"value\": 1.0}\n{\"at\": \"2026-11-16T09:00:00+00:00\", \"bet\": \"alpha\", \"status\": \"PIVOT\", \"value\": 1.5}\n{\"at\": \"2026-11-16T09:00:00+00:00\", \"bet\": \"gone\", \"status\": \"WATCH\", \"value\": 4}\n",
);

/// Two trials: a closed indie one and an open work one (non-ASCII text).
const TRIALS: (&str, &str) = (
    "peer/experiments.jsonl",
    "{\"baseline\": 1.5, \"change\": \"batch the merges\", \"closed\": \"2026-11-10T09:00:00+00:00\", \"id\": \"x-indie-1\", \"lesson\": \"\", \"metric\": \"hours_per_outcome\", \"side\": \"indie\", \"started\": \"2026-11-01T09:00:00+00:00\", \"value\": 1.2, \"verdict\": \"KEEP\"}\n{\"baseline\": 0.25, \"change\": \"check the gate first, trié\", \"closed\": \"\", \"id\": \"x-work-1\", \"lesson\": \"L-indie-1\", \"metric\": \"step_fail_rate\", \"side\": \"work\", \"started\": \"2026-11-18T09:00:00+00:00\", \"value\": null, \"verdict\": \"\"}\n",
);

fn refuses(setup: &Setup, args: &[&str], label: &str) {
    let home = prepare_setup(setup);
    assert!(
        !assert_same(&home, args, false, label),
        "{label}: {args:?} served"
    );
}

#[test]
fn web_rotate_token_matches_python() {
    strict(NOW);
    // Route key: "web.rotate-token".
    let home = prepare_setup(&Setup {
        id: "rotate",
        cli: &[],
        files: &[],
    });
    assert_same(
        &home,
        &["web", "rotate-token"],
        true,
        "web.rotate-token confirm",
    );
    let terms = Setup {
        id: "rotate-terms",
        cli: &[],
        files: &[TERMS],
    };
    refuses(
        &terms,
        &["web", "rotate-token"],
        "web.rotate-token redaction",
    );
}

#[test]
fn indie_goal_matches_python() {
    strict(NOW);
    // Route key: "indie.goal".
    let home = prepare_setup(&Setup {
        id: "goal",
        cli: INCOME,
        files: &[],
    });
    assert_same(&home, &["indie", "goal"], true, "indie.goal set");
    assert_same(&home, &["indie", "goal", "--json"], true, "indie.goal json");
    assert_same(
        &home,
        &["indie", "goal", "--monthly", "0"],
        true,
        "indie.goal zero target",
    );
    let empty = prepare_setup(&Setup {
        id: "goal-empty",
        cli: &[],
        files: &[],
    });
    assert_same(&empty, &["indie", "goal"], true, "indie.goal not set");
    assert_same(
        &empty,
        &["indie", "goal", "--json"],
        true,
        "indie.goal unset json",
    );
    let plain = prepare_setup(&Setup {
        id: "goal-plain",
        cli: &[],
        files: &[
            ("indie/goal.yaml", "monthly: 800\nby: null\n"),
            (
                "indie/income.jsonl",
                "{\"amount\": 100, \"bet\": \"\", \"month\": \"2026-09\"}\n",
            ),
        ],
    });
    assert_same(&plain, &["indie", "goal"], true, "indie.goal no record");
    assert_same(
        &plain,
        &["indie", "goal", "--json"],
        true,
        "indie.goal plain json",
    );
    // Writes goal.yaml through PyYAML: Python runs it.
    refuses(
        &Setup {
            id: "goal-write",
            cli: &[],
            files: &[],
        },
        &["indie", "goal", "--monthly", "1200"],
        "indie.goal write",
    );
    // 12.5 of 1000 is 1.25%: an exact tie for round(..., 1).
    refuses(
        &Setup {
            id: "goal-tie",
            cli: &[
                &["indie", "goal", "--monthly", "1000"],
                &["indie", "income", "--month", "2026-10", "--amount", "12.5"],
            ],
            files: &[],
        },
        &["indie", "goal"],
        "indie.goal tie",
    );
    refuses(
        &Setup {
            id: "goal-corp",
            cli: &[],
            files: &[CORPORATE],
        },
        &["indie", "goal"],
        "indie.goal corporate",
    );
    refuses(
        &Setup {
            id: "goal-bad",
            cli: &[],
            files: &[("indie/goal.yaml", "monthly: '900'\n")],
        },
        &["indie", "goal"],
        "indie.goal text target",
    );
}

#[test]
fn indie_income_matches_python() {
    strict(NOW);
    // Route key: "indie.income".
    let home = prepare_setup(&Setup {
        id: "income",
        cli: INCOME,
        files: &[],
    });
    assert_same(&home, &["indie", "income"], true, "indie.income recorded");
    assert_same(
        &home,
        &["indie", "income", "--json"],
        true,
        "indie.income json",
    );
    assert_same(
        &home,
        &["indie", "income", "--amount", "5"],
        true,
        "indie.income no month",
    );
    assert_same(
        &home,
        &["indie", "income", "--month", "2026-13", "--amount", "5"],
        true,
        "indie.income bad month",
    );
    assert_same(
        &home,
        &[
            "indie", "income", "--month", "2026-10", "--amount", "5", "--bet", "nobody",
        ],
        true,
        "indie.income unknown bet",
    );
    let empty = prepare_setup(&Setup {
        id: "income-empty",
        cli: &[],
        files: &[],
    });
    assert_same(&empty, &["indie", "income"], true, "indie.income none");
    refuses(
        &Setup {
            id: "income-write",
            cli: &[],
            files: &[],
        },
        &["indie", "income", "--month", "2026-10", "--amount", "5"],
        "indie.income write",
    );
    refuses(
        &Setup {
            id: "income-bad-row",
            cli: &[],
            files: &[(
                "indie/income.jsonl",
                "{\"amount\": \"7\", \"month\": \"2026-10\"}\n",
            )],
        },
        &["indie", "income"],
        "indie.income text amount",
    );
}

#[test]
fn indie_bets_matches_python() {
    strict(NOW);
    // Route key: "indie.bets".
    let home = prepare_setup(&Setup {
        id: "bets",
        cli: &[],
        files: &[BETS, MEASURES],
    });
    assert_same(&home, &["indie", "bets"], true, "indie.bets text");
    assert_same(&home, &["indie", "bets", "--json"], true, "indie.bets json");
    let empty = prepare_setup(&Setup {
        id: "bets-empty",
        cli: &[],
        files: &[],
    });
    assert_same(&empty, &["indie", "bets"], true, "indie.bets none");
    assert_same(
        &empty,
        &["indie", "bets", "--json"],
        true,
        "indie.bets none json",
    );
    refuses(
        &Setup {
            id: "bets-no-state",
            cli: &[],
            files: &[(
                "indie/bets.yaml",
                "bets:\n  gamma:\n    path: /nonexistent/gamma\n",
            )],
        },
        &["indie", "bets"],
        "indie.bets no state",
    );
    refuses(
        &Setup {
            id: "bets-terms",
            cli: &[],
            files: &[BETS, TERMS],
        },
        &["indie", "bets"],
        "indie.bets redaction",
    );
    refuses(
        &Setup {
            id: "bets-corp",
            cli: &[],
            files: &[BETS, CORPORATE],
        },
        &["indie", "bets"],
        "indie.bets corporate",
    );
}

#[test]
fn indie_bet_matches_python() {
    strict(NOW);
    // Route key: "indie.bet".
    let home = prepare_setup(&Setup {
        id: "bet",
        cli: &[],
        files: &[BETS],
    });
    assert_same(
        &home,
        &["indie", "bet", "nobody"],
        true,
        "indie.bet unknown",
    );
    assert_same(
        &home,
        &["indie", "bet", "nobody", "--kind", "product", "--json"],
        true,
        "indie.bet unknown with fields",
    );
    // Showing a bet depends on may_show_business: Python runs it.
    refuses(
        &Setup {
            id: "bet-show",
            cli: &[],
            files: &[BETS],
        },
        &["indie", "bet", "alpha", "--json"],
        "indie.bet show",
    );
}

#[test]
fn indie_decide_matches_python() {
    strict(NOW);
    // Route key: "indie.decide".
    let home = prepare_setup(&Setup {
        id: "decide",
        cli: &[],
        files: &[BETS, MEASURES],
    });
    assert_same(
        &home,
        &["indie", "decide", "alpha", "pause"],
        true,
        "indie.decide no yes",
    );
    assert_same(
        &home,
        &["indie", "decide", "nobody", "kill", "--yes"],
        true,
        "indie.decide unknown",
    );
    assert_same(
        &home,
        &["indie", "decide", "beta", "activate", "--yes"],
        true,
        "indie.decide lacks",
    );
    refuses(
        &Setup {
            id: "decide-write",
            cli: &[],
            files: &[BETS, MEASURES],
        },
        &["indie", "decide", "alpha", "pause", "--yes"],
        "indie.decide write",
    );
}

#[test]
fn indie_measure_matches_python() {
    strict(NOW);
    // Route key: "indie.measure".
    let home = prepare_setup(&Setup {
        id: "measure",
        cli: &[],
        files: &[BETS],
    });
    assert_same(
        &home,
        &["indie", "measure", "nobody"],
        true,
        "indie.measure unknown",
    );
    assert_same(
        &home,
        &["indie", "measure", "beta", "--json"],
        true,
        "indie.measure no command",
    );
    refuses(
        &Setup {
            id: "measure-write",
            cli: &[],
            files: &[BETS],
        },
        &["indie", "measure", "alpha", "--value", "3"],
        "indie.measure write",
    );
}

#[test]
fn indie_end_matches_python() {
    strict(NOW);
    // Route key: "indie.end".
    let home = prepare_setup(&Setup {
        id: "end",
        cli: &[],
        files: &[BETS],
    });
    assert_same(
        &home,
        &["indie", "end", "--hours", "alpha=x"],
        true,
        "indie.end bad hours",
    );
    assert_same(
        &home,
        &["indie", "end", "--hours", "alpha=1.5", "--hours", "beta"],
        true,
        "indie.end no value",
    );
    // A value that could be a float ("nan") and a valid one: Python runs it.
    refuses(
        &Setup {
            id: "end-nan",
            cli: &[],
            files: &[BETS],
        },
        &["indie", "end", "--hours", "alpha=nan"],
        "indie.end nan",
    );
    refuses(
        &Setup {
            id: "end-write",
            cli: &[],
            files: &[BETS],
        },
        &["indie", "end", "--hours", "alpha=1.5"],
        "indie.end write",
    );
}

#[test]
fn peer_experiment_matches_python() {
    strict(NOW);
    // Route key: "peer.experiment".
    let home = prepare_setup(&Setup {
        id: "peer",
        cli: &[],
        files: &[TRIALS],
    });
    for (args, label) in [
        (&["peer", "experiment"][..], "peer.experiment list"),
        (
            &["peer", "experiment", "--json"][..],
            "peer.experiment json",
        ),
        (
            &["peer", "experiment", "--side", "work"][..],
            "peer.experiment work",
        ),
        (
            &["peer", "experiment", "--side", "indie", "--json"][..],
            "peer.experiment indie json",
        ),
        (
            &["peer", "experiment", "--start", "try the batch order"][..],
            "peer.experiment start no side",
        ),
        (
            &[
                "peer",
                "experiment",
                "--side",
                "work",
                "--start",
                "short",
                "--metric",
                "step_minutes",
            ][..],
            "peer.experiment short change",
        ),
        (
            &[
                "peer",
                "experiment",
                "--side",
                "indie",
                "--start",
                "try a new order",
                "--metric",
                "hours_per_outcome",
                "--lesson",
                "L-indie-2",
            ][..],
            "peer.experiment own lesson",
        ),
        (
            &[
                "peer",
                "experiment",
                "--side",
                "work",
                "--start",
                "try the batch order",
                "--metric",
                "step_minutes",
            ][..],
            "peer.experiment open trial",
        ),
        (
            &[
                "peer",
                "experiment",
                "--side",
                "indie",
                "--start",
                "try the batch order",
                "--metric",
                "bogus",
            ][..],
            "peer.experiment bad metric",
        ),
        (
            &["peer", "experiment", "--close"][..],
            "peer.experiment close no side",
        ),
        (
            &["peer", "experiment", "--side", "indie", "--close"][..],
            "peer.experiment close none open",
        ),
        (
            &["peer", "experiment", "--side", "work", "--close"][..],
            "peer.experiment close too soon",
        ),
    ] {
        assert_same(&home, args, true, label);
    }
    let empty = prepare_setup(&Setup {
        id: "peer-empty",
        cli: &[],
        files: &[],
    });
    assert_same(
        &empty,
        &["peer", "experiment"],
        true,
        "peer.experiment empty",
    );
    // A known metric measures and appends a trial: Python runs it.
    refuses(
        &Setup {
            id: "peer-start",
            cli: &[],
            files: &[TRIALS],
        },
        &[
            "peer",
            "experiment",
            "--side",
            "indie",
            "--start",
            "try the batch order",
            "--metric",
            "hours_per_outcome",
        ],
        "peer.experiment start",
    );
    refuses(
        &Setup {
            id: "peer-bad-row",
            cli: &[],
            files: &[(
                "peer/experiments.jsonl",
                "{\"id\": 7, \"side\": \"indie\"}\n",
            )],
        },
        &["peer", "experiment"],
        "peer.experiment bad row",
    );
}

/// Negative control: two sides list different trials, so the comparison
/// sees a planted difference.
#[test]
fn the_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "control",
        cli: &[],
        files: &[TRIALS],
    });
    let a = observe(
        &home,
        &["peer", "experiment", "--side", "indie"],
        &Side::Native,
    );
    let b = observe(
        &home,
        &["peer", "experiment", "--side", "work"],
        &Side::Python,
    );
    assert_eq!(a.code, Some(0));
    assert_ne!(a, b);
    assert!(a.stdout.starts_with("  x-indie-1"));
}

/// Bets for `indie scan`: two whose folders do not exist and one whose
/// empty path is the current folder (`Path("")`).
const SCAN_BETS: (&str, &str) = (
    "indie/bets.yaml",
    "bets:\n  beta:\n    path: /nonexistent/beta\n    state: proposed\n  alpha:\n    path: /nonexistent/alpha\n    state: active\n  gamma:\n    path: ''\n    state: proposed\n",
);
/// Roots under the synthetic data folder (HOME is its sibling).
const SCAN_ROOTS: (&str, &str) = (
    "settings.yaml",
    "indie_roots: ' ~/../data/roots , ~/nothere,'\n",
);
/// Folders `found_repos` skips: a nested folder with no repo, a hidden
/// repo, a repo inside a `.noindex` group, and a linked worktree's child.
const SCAN_TREE: &[(&str, &str)] = &[
    SCAN_BETS,
    SCAN_ROOTS,
    ("roots/group/sub/notes.txt", "synthetic\n"),
    ("roots/.hidden/.git/HEAD", "ref: refs/heads/main\n"),
    (
        "roots/old.noindex/inner/.git/HEAD",
        "ref: refs/heads/main\n",
    ),
    ("roots/wt/.git", "gitdir: /nonexistent/wt\n"),
    ("roots/wt/child/.git/HEAD", "ref: refs/heads/main\n"),
];

#[test]
fn indie_scan_matches_python() {
    strict(NOW);
    // Route key: "indie.scan".
    let empty = prepare_setup(&Setup {
        id: "scan-empty",
        cli: &[],
        files: &[],
    });
    assert_same(&empty, &["indie", "scan"], true, "indie.scan empty");
    let bets = prepare_setup(&Setup {
        id: "scan-bets",
        cli: &[],
        files: &[SCAN_BETS],
    });
    assert_same(&bets, &["indie", "scan"], true, "indie.scan default roots");
    assert_same(
        &bets,
        &["indie", "scan", "--json"],
        true,
        "indie.scan default roots json",
    );
    let tree = prepare_setup(&Setup {
        id: "scan-tree",
        cli: &[],
        files: SCAN_TREE,
    });
    assert_same(
        &tree,
        &["indie", "scan"],
        true,
        "indie.scan skipped folders",
    );
    assert_same(
        &tree,
        &["indie", "scan", "--json"],
        true,
        "indie.scan skipped folders json",
    );
    // A repo under the roots: Python asks git for its origin and saves it.
    refuses(
        &Setup {
            id: "scan-repo",
            cli: &[],
            files: &[
                SCAN_BETS,
                SCAN_ROOTS,
                ("roots/group/proj/.git/HEAD", "ref: refs/heads/main\n"),
            ],
        },
        &["indie", "scan"],
        "indie.scan repo found",
    );
    refuses(
        &Setup {
            id: "scan-odd-roots",
            cli: &[],
            files: &[("settings.yaml", "indie_roots: 5\n")],
        },
        &["indie", "scan"],
        "indie.scan roots not text",
    );
    refuses(
        &Setup {
            id: "scan-corporate",
            cli: &[],
            files: &[CORPORATE],
        },
        &["indie", "scan"],
        "indie.scan corporate",
    );
    refuses(
        &Setup {
            id: "scan-terms",
            cli: &[],
            files: &[SCAN_BETS, TERMS],
        },
        &["indie", "scan"],
        "indie.scan redaction",
    );
}

// ---------- changes ----------

const PLAIN: (&str, &str) = (".plaintext", "");

/// A synthetic user ladder: a date, a float, non-ASCII text and an
/// `origin` key that the hash leaves out.
const LADDER: (&str, &str) = (
    "ladders/synthetic.yaml",
    "id: synthetic-ladder\nname: Synthetic ladder é\norigin: elsewhere\nsince: 2026-01-02\nlevels: [one, two]\ncompetencies:\n  - id: craft\n    name: Craft ✓\n    weight: 0.5\n    signals: {tags: [quality], keywords: [widget*]}\n    expect: {one: 1, two: 3}\n    descriptions: {two: Builds synthetic widgets}\n",
);

/// A user ladder `promo.validate_ladder` rejects (no expectation for `two`).
const BAD_LADDER: (&str, &str) = (
    "ladders/bad.yaml",
    "id: bad-ladder\nlevels: [one, two]\ncompetencies:\n  - id: craft\n    name: Craft\n    weight: 1\n    signals: {}\n    expect: {one: 1}\n",
);

/// A stale snapshot: every ladder and company counts as added.
const STALE: (&str, &str) = (
    "changes/snapshot.json",
    "{\"companies\": {}, \"ladders\": {}, \"pages\": {}, \"plugins\": {}}",
);

/// A snapshot that holds no section yet (`null`): a first run.
const NULL_SNAPSHOT: (&str, &str) = ("changes/snapshot.json", "null");

const BASELINE: &[&[&str]] = &[&["changes"]];

#[test]
fn changes_matches_python() {
    strict(NOW);
    // Route key: "changes".
    for (id, cli, files, label) in [
        (
            "changes-first",
            &[] as &[&[&str]],
            &[PLAIN] as &[(&str, &str)],
            "changes first run",
        ),
        ("changes-again", BASELINE, &[PLAIN], "changes unchanged"),
        (
            "changes-sealed-again",
            BASELINE,
            &[],
            "changes unchanged sealed",
        ),
        (
            "changes-ladder-first",
            &[],
            &[PLAIN, LADDER],
            "changes user ladder first run",
        ),
        (
            "changes-ladder-again",
            BASELINE,
            &[PLAIN, LADDER],
            "changes user ladder unchanged",
        ),
        (
            "changes-null",
            &[],
            &[PLAIN, NULL_SNAPSHOT],
            "changes null snapshot",
        ),
    ] {
        let home = prepare_setup(&Setup { id, cli, files });
        assert_same(&home, &["changes"], true, label);
    }
}

/// One refusal case: setup id, setup commands, files, argv, label.
type Case<'a> = (
    &'a str,
    &'a [&'a [&'a str]],
    &'a [(&'a str, &'a str)],
    &'a [&'a str],
    &'a str,
);

#[test]
fn changes_refuses_what_python_must_run() {
    strict(NOW);
    // Route key: "changes".
    let cases: &[Case] = &[
        (
            "changes-fetch",
            BASELINE,
            &[PLAIN],
            &["changes", "--fetch"],
            "changes --fetch",
        ),
        (
            "changes-stale",
            &[],
            &[PLAIN, STALE],
            &["changes"],
            "changes found a change",
        ),
        (
            "changes-sealed-first",
            &[],
            &[],
            &["changes"],
            "changes sealed first run",
        ),
        (
            "changes-bad-ladder",
            &[],
            &[PLAIN, BAD_LADDER],
            &["changes"],
            "changes invalid ladder",
        ),
        (
            "changes-plugin",
            &[],
            &[PLAIN, ("plugins.lock.json", "{\"plugins\": {}}")],
            &["changes"],
            "changes plugin lock",
        ),
        (
            "changes-terms",
            BASELINE,
            &[PLAIN, TERMS],
            &["changes"],
            "changes redaction",
        ),
    ];
    for (id, cli, files, args, label) in cases {
        refuses(&Setup { id, cli, files }, args, label);
    }
}

/// Negative control: a ladder added after the baseline is a change, so the
/// native side must not answer "no changes".
#[test]
fn changes_negative_control_new_ladder_is_a_change() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "changes-control",
        cli: BASELINE,
        files: &[PLAIN],
    });
    // `observe` restores the home from its snapshot copy: add the ladder there.
    let snap = home
        .home
        .parent()
        .unwrap()
        .with_file_name("snap")
        .join("data");
    std::fs::create_dir_all(snap.join("ladders")).unwrap();
    std::fs::write(snap.join(LADDER.0), LADDER.1).unwrap();
    let native = observe(&home, &["changes"], &Side::Native);
    assert_eq!(native.code, Some(64), "a new ladder must refuse natively");
}
