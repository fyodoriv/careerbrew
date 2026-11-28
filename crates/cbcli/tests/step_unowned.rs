//! Parity cases for the `unowned` native handlers (`src/steps/unowned.rs`):
//! each served route key runs in fresh synthetic homes on both sides, plus
//! cases the handler must refuse and one negative control.
mod common;
use common::{assert_same, observe, prepare_setup, strict, Setup, Side};

const NOW: &str = "2026-11-20T09:00:00";
const TERMS: (&str, &str) = ("sensitive-terms.txt", "zebrafruit\n");

const REVIEW: (&str, &str) = (
    "out/reports/review-20261119-080000.json",
    "{\"run\": \"review\", \"title\": \"Review\", \"status\": \"ok\", \"at\": \"2026-11-19T08:00:00\", \"seq\": 1, \"headline\": \"\", \"counts\": {\"metrics\": 3, \"ratio\": 0.25}, \"done\": [{\"label\": \"Read the metrics\", \"evidence\": \"3 rows\"}, {\"label\": \"Wrote the dashboard\"}], \"blockers\": [], \"next\": [{\"label\": \"Open the page\", \"command\": \"careerbrew open --report latest\"}], \"commands\": []}\n",
);
const CHECKUP: (&str, &str) = (
    "out/reports/checkup-20261120-090000.json",
    "{\"id\": \"stale\", \"run\": \"checkup\", \"title\": \"Checkup caf\\u00e9\", \"status\": \"blocked\", \"at\": \"2026-11-20T09:00:00\", \"seq\": 2, \"headline\": \"health: 4/5\", \"counts\": {\"failed\": 1, \"minutes\": 1.5, \"neg\": -2}, \"done\": [{\"label\": \"Ran doctor\", \"evidence\": \"5 checks\"}], \"blockers\": [{\"label\": \"A check failed\", \"why\": \"the synthetic tool is missing\", \"command\": \"scripts/setup.sh\"}, {\"label\": \"Second blocker\", \"why\": \"no fix yet\", \"command\": \"\"}], \"next\": [{\"label\": \"Fix the tool\"}, {\"label\": \"Rerun\", \"command\": \"scripts/checkup.sh\"}], \"commands\": [\"scripts/doctor.sh\"]}\n",
);
const NOT_A_PAGE: (&str, &str) = ("out/reports/notes.json", "{\"run\": \"x\"}\n");

fn refuses(setup: &Setup, args: &[&str], label: &str) {
    let home = prepare_setup(setup);
    assert!(
        !assert_same(&home, args, false, label),
        "{label}: {args:?} served"
    );
}

#[test]
fn report_latest_matches_python() {
    strict(NOW);
    // Route key: "report.latest".
    let home = prepare_setup(&Setup {
        id: "latest",
        cli: &[],
        files: &[REVIEW, CHECKUP, NOT_A_PAGE],
    });
    assert_same(&home, &["report", "latest"], true, "report.latest newest");
    assert_same(
        &home,
        &["report", "latest", "--json"],
        true,
        "report.latest json",
    );
    assert_same(
        &home,
        &["report", "latest", "--run", "review"],
        true,
        "report.latest one run",
    );
    assert_same(
        &home,
        &["report", "latest", "--run", "review", "--json"],
        true,
        "report.latest one run json",
    );
    assert_same(
        &home,
        &["report", "latest", "--run", "start"],
        true,
        "report.latest missing run",
    );
    let empty = prepare_setup(&Setup {
        id: "latest-empty",
        cli: &[],
        files: &[],
    });
    assert_same(
        &empty,
        &["report", "latest"],
        true,
        "report.latest no pages",
    );
    assert_same(
        &empty,
        &["report", "latest", "--run", "review"],
        true,
        "report.latest no pages one run",
    );
    let tie = Setup {
        id: "latest-tie",
        cli: &[],
        files: &[
            REVIEW,
            (
                "out/reports/start-20261119-080000.json",
                "{\"run\": \"start\", \"status\": \"ok\", \"at\": \"2026-11-19T08:00:00\", \"seq\": 1, \"done\": [], \"blockers\": [], \"next\": []}\n",
            ),
        ],
    };
    refuses(&tie, &["report", "latest"], "report.latest sort tie");
    let odd = Setup {
        id: "latest-odd-number",
        cli: &[],
        files: &[(
            "out/reports/start-20261119-080000.json",
            "{\"run\": \"start\", \"status\": \"ok\", \"at\": \"x\", \"seq\": 1, \"n\": 1e5, \"done\": [], \"blockers\": [], \"next\": []}\n",
        )],
    };
    refuses(
        &odd,
        &["report", "latest", "--json"],
        "report.latest exponent",
    );
    let redacted = Setup {
        id: "latest-terms",
        cli: &[],
        files: &[REVIEW, TERMS],
    };
    refuses(
        &redacted,
        &["report", "latest"],
        "report.latest redaction input",
    );
}

#[test]
fn report_list_matches_python() {
    strict(NOW);
    // Route key: "report.list".
    let home = prepare_setup(&Setup {
        id: "list",
        cli: &[],
        files: &[REVIEW, CHECKUP, NOT_A_PAGE],
    });
    assert_same(&home, &["report", "list"], true, "report.list default");
    assert_same(
        &home,
        &["report", "list", "--top", "1"],
        true,
        "report.list top 1",
    );
    assert_same(
        &home,
        &["report", "list", "--top", "-1"],
        // Refused natively on both hosted OSes, likely because the argument
        // parser reads `-1` as an option (task
        // `native-report-list-negative-top`); a served answer must match.
        false,
        "report.list negative top",
    );
    assert_same(
        &home,
        &["report", "list", "--json"],
        true,
        "report.list json",
    );
    assert_same(
        &home,
        &["report", "list", "--json", "--top", "0"],
        true,
        "report.list json empty",
    );
    let empty = prepare_setup(&Setup {
        id: "list-empty",
        cli: &[],
        files: &[],
    });
    assert_same(&empty, &["report", "list"], true, "report.list no pages");
    let bad = Setup {
        id: "list-bad-seq",
        cli: &[],
        files: &[(
            "out/reports/start-20261119-080000.json",
            "{\"run\": \"start\", \"status\": \"ok\", \"seq\": \"3\", \"done\": [], \"blockers\": [], \"next\": []}\n",
        )],
    };
    refuses(&bad, &["report", "list"], "report.list text seq");
    let home = prepare_setup(&Setup {
        id: "list-abbrev",
        cli: &[],
        files: &[REVIEW],
    });
    assert!(!assert_same(
        &home,
        &["report", "list", "--to", "1"],
        false,
        "report.list abbreviation"
    ));
}

const NOTES: &[(&str, &str)] = &[
    ("notes.txt", "a synthetic note\nsecond line\n"),
    ("jobs/leads.md", "# Leads\n- synthetic co\n"),
    ("work/plan.md", "synthetic plan\n"),
];

#[test]
fn data_decrypt_matches_python() {
    strict(NOW);
    // Route key: "data.decrypt".
    let sealed = prepare_setup(&Setup {
        id: "decrypt",
        cli: &[&["data", "encrypt"]],
        files: NOTES,
    });
    assert_same(&sealed, &["data", "decrypt"], true, "data.decrypt no yes");
    assert_same(
        &sealed,
        &["data", "decrypt", "--yes"],
        true,
        "data.decrypt sealed",
    );
    let plain = prepare_setup(&Setup {
        id: "decrypt-plain",
        cli: &[&["data", "encrypt"], &["data", "decrypt", "--yes"]],
        files: NOTES,
    });
    assert_same(
        &plain,
        &["data", "decrypt", "--yes"],
        true,
        "data.decrypt plaintext",
    );
    let no_key = Setup {
        id: "decrypt-no-key",
        cli: &[],
        files: NOTES,
    };
    refuses(
        &no_key,
        &["data", "decrypt", "--yes"],
        "data.decrypt no key",
    );
    let redacted = Setup {
        id: "decrypt-terms",
        cli: &[],
        files: &[TERMS],
    };
    refuses(
        &redacted,
        &["data", "decrypt"],
        "data.decrypt redaction input",
    );
}

#[test]
fn data_encrypt_matches_python() {
    strict(NOW);
    // Route key: "data.encrypt".
    let plain = prepare_setup(&Setup {
        id: "encrypt",
        cli: &[&["data", "encrypt"], &["data", "decrypt", "--yes"]],
        files: NOTES,
    });
    assert_same(&plain, &["data", "encrypt"], true, "data.encrypt plaintext");
    let sealed = prepare_setup(&Setup {
        id: "encrypt-again",
        cli: &[&["data", "encrypt"]],
        files: NOTES,
    });
    assert_same(
        &sealed,
        &["data", "encrypt"],
        true,
        "data.encrypt already on",
    );
    let no_key = Setup {
        id: "encrypt-no-key",
        cli: &[],
        files: NOTES,
    };
    refuses(&no_key, &["data", "encrypt"], "data.encrypt no key yet");
    let bundle = Setup {
        id: "encrypt-bundle",
        // Python's own `data encrypt` refuses a folder that holds a bundle,
        // so this home has no setup command.
        cli: &[],
        files: &[
            ("notes.txt", "synthetic\n"),
            ("old.cbk", "synthetic bundle\n"),
        ],
    };
    refuses(&bundle, &["data", "encrypt"], "data.encrypt bundle");
    let token_like = Setup {
        id: "encrypt-token-like",
        cli: &[&["data", "encrypt"], &["data", "decrypt", "--yes"]],
        files: &[("notes.txt", "gAAAA synthetic\n")],
    };
    refuses(
        &token_like,
        &["data", "encrypt"],
        "data.encrypt token-like file",
    );
}

#[test]
fn data_show_matches_python() {
    strict(NOW);
    // Route key: "data.show".
    let plain = prepare_setup(&Setup {
        id: "show",
        cli: &[],
        files: NOTES,
    });
    assert_same(
        &plain,
        &["data", "show", "notes.txt"],
        true,
        "data.show top file",
    );
    assert_same(
        &plain,
        &["data", "show", "jobs/leads.md"],
        true,
        "data.show jobs file",
    );
    assert_same(
        &plain,
        &["data", "show", "private/notes.txt"],
        true,
        "data.show private prefix",
    );
    assert_same(
        &plain,
        &["data", "show", "work/plan.md"],
        true,
        "data.show business hidden",
    );
    assert_same(
        &plain,
        &["data", "show", "missing.txt"],
        true,
        "data.show missing",
    );
    assert_same(&plain, &["data", "show", "work"], true, "data.show folder");
    let sealed = prepare_setup(&Setup {
        id: "show-sealed",
        cli: &[&["data", "encrypt"]],
        files: NOTES,
    });
    assert_same(
        &sealed,
        &["data", "show", "notes.txt"],
        true,
        "data.show sealed",
    );
    assert_same(
        &sealed,
        &["data", "show", "work/plan.md"],
        true,
        "data.show sealed hidden",
    );
    refuses(
        &Setup {
            id: "show-keys",
            cli: &[],
            files: NOTES,
        },
        &["data", "show", "notes.txt", "--keys"],
        "data.show keys",
    );
    refuses(
        &Setup {
            id: "show-dots",
            cli: &[],
            files: NOTES,
        },
        &["data", "show", "jobs/../notes.txt"],
        "data.show dot dot",
    );
    refuses(
        &Setup {
            id: "show-terms",
            cli: &[],
            files: &[("notes.txt", "zebrafruit here\n"), TERMS],
        },
        &["data", "show", "notes.txt"],
        "data.show redaction input",
    );
}

/// Negative control: the comparison sees a real difference between homes.
#[test]
fn the_comparison_detects_a_planted_difference() {
    strict(NOW);
    let one = prepare_setup(&Setup {
        id: "control-one",
        cli: &[],
        files: &[REVIEW],
    });
    let two = prepare_setup(&Setup {
        id: "control-two",
        cli: &[],
        files: &[REVIEW, CHECKUP],
    });
    let native = observe(&one, &["report", "latest"], &Side::Native);
    let python = observe(&two, &["report", "latest"], &Side::Python);
    assert_ne!(native, python);
    assert!(native.stdout.contains("review: ok at"));
    assert!(python.stdout.contains("checkup: blocked at"));
}
