//! Parity cases for the `unowned` native handlers (`src/steps/unowned.rs`):
//! each served route key runs in fresh synthetic homes on both sides, plus
//! cases the handler must refuse and one negative control.
mod common;
use common::{assert_same, observe, prepare_setup, strict, Home, Observed, Setup, Side};
use std::path::Path;
use std::process::Command;

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

// ---------- report.public ----------

const GATE_SCRIPT: &str =
    "#!/bin/sh\nstep docs true\nstep unit true\nif true; then step lint true\nfi\n";
const GATE_EVENTS: &str = concat!(
    "{\"event\": \"check.step\", \"name\": \"docs\", \"seconds\": 1.5, \"ok\": true, \"ts\": \"2026-11-19T10:00:00Z\"}\n",
    "{\"event\": \"check.step\", \"name\": \"unit\", \"seconds\": 30, \"ok\": false, \"ts\": \"2026-11-19T10:01:00Z\"}\n",
    "{\"event\": \"check.step\", \"name\": \"unit\", \"seconds\": 12.25, \"ok\": true, \"ts\": \"2026-11-20T08:00:00+01:00\"}\n",
    "{\"event\": \"check.step\", \"name\": \"docs\", \"seconds\": 2, \"ok\": true, \"ts\": \"2026-11-20T08:30:00\"}\n",
    "{\"event\": \"check.step\", \"name\": \"lint\", \"seconds\": 0, \"ok\": true, \"cached\": true, \"ts\": \"2026-11-20T08:31:00\"}\n",
    "{\"event\": \"check.step\", \"name\": \"lint\", \"seconds\": 0.1, \"ok\": false, \"ts\": \"2026-11-20T08:32:00.250000\"}\n",
    "{\"event\": \"check.step\", \"name\": \"mystery\", \"seconds\": 4, \"ok\": false, \"ts\": \"2026-11-20T08:33:00\"}\n",
    "{\"event\": \"check.step\", \"name\": \"docs\", \"seconds\": 3, \"ok\": true, \"ts\": \"2026-10-01T00:00:00Z\"}\n",
    "{\"event\": \"check.run\", \"ok\": true}\n",
    "\n",
);
const E2E: &str = "{\"results\": {\"us-01\": \"ok\", \"us-02\": \"fail\", \"us-03\": \"ok\"}}\n";
const REPO_CHECKUP: (&str, &str) = (
    "out/reports/repo-checkup-20261120-080000.json",
    "{\"run\": \"repo-checkup\", \"status\": \"blocked\", \"at\": \"2026-11-20T08:00:00\", \"seq\": 3, \"done\": [], \"blockers\": [{\"label\": \"x\", \"why\": \"y\"}], \"next\": [{\"id\": \"pr-12\", \"label\": \"a\"}, {\"id\": \"task-1\", \"label\": \"b\"}, {\"id\": \"pr-40\", \"label\": \"c\"}]}\n",
);

/// A synthetic checkout for `CAREERBREW_REPO_DIR`: the gate script and its
/// `.metrics` files, so no run reads the real repo's metrics.
fn synthetic_repo(dir: &Path, events: &str) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::create_dir_all(dir.join(".metrics")).unwrap();
    std::fs::write(dir.join("scripts/check.sh"), GATE_SCRIPT).unwrap();
    std::fs::write(dir.join(".metrics/events.jsonl"), events).unwrap();
    std::fs::write(dir.join(".metrics/e2e-results.json"), E2E).unwrap();
}

/// One run with `CAREERBREW_REPO_DIR` set, plus every file in `extra`
/// (a folder outside the data root, reset with the home) and its content.
fn observe_repo(home: &Home, repo: &Path, args: &[&str], native: bool) -> Observed {
    common::restore(home);
    let mut cmd = if native {
        let mut cmd = Command::new(common::BIN);
        cmd.args(args);
        common::base(&mut cmd, home);
        cmd
    } else {
        let mut cmd = common::python(home, args);
        cmd.env("CAREERBREW_NATIVE", "off");
        cmd
    };
    let output = cmd.env("CAREERBREW_REPO_DIR", repo).output().unwrap();
    let mut files = Vec::new();
    common::walk(&home.data, &home.data, &mut files);
    if let Ok(entries) = std::fs::read_dir(public_dir(home)) {
        for entry in entries {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            files.push(format!("{}: {text}", path.display()));
        }
    }
    files.sort();
    let events = std::fs::read_to_string(home.data.join("logs/events.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(common::normalize)
        .collect();
    Observed {
        code: output.status.code(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        files,
        events,
    }
}

fn public_dir(home: &Home) -> std::path::PathBuf {
    home.home.parent().unwrap().join("public")
}

fn public_same(home: &Home, repo: &Path, args: &[&str], label: &str) {
    let want = observe_repo(home, repo, args, false);
    let got = observe_repo(home, repo, args, true);
    assert_ne!(got.code, Some(64), "{label}: {args:?} refused natively");
    assert_eq!(got, want, "{label}: {args:?}");
}

#[test]
fn report_public_matches_python() {
    strict(NOW);
    // Route key: "report.public".
    let empty = prepare_setup(&Setup {
        id: "public-days",
        cli: &[],
        files: &[],
    });
    // The window check refuses before any repo file is read.
    assert_same(
        &empty,
        &["report", "public", "--days", "0"],
        true,
        "report.public days 0",
    );
    assert_same(
        &empty,
        &["report", "public", "--days", "367", "--json"],
        true,
        "report.public days 367",
    );
    refuses(
        &Setup {
            id: "public-terms",
            cli: &[],
            files: &[TERMS],
        },
        &["report", "public", "--days", "0"],
        "report.public redaction input",
    );

    let repo_dir = tempfile::tempdir().unwrap();
    let repo = repo_dir.path().join("checkout");
    synthetic_repo(&repo, GATE_EVENTS);
    let home = prepare_setup(&Setup {
        id: "public",
        cli: &[],
        files: &[REPO_CHECKUP, REVIEW],
    });
    public_same(&home, &repo, &["report", "public"], "report.public text");
    public_same(
        &home,
        &repo,
        &["report", "public", "--json"],
        "report.public json",
    );
    public_same(
        &home,
        &repo,
        &["report", "public", "--days", "1"],
        "report.public one day",
    );
    let out = public_dir(&home);
    let out = out.to_str().unwrap();
    public_same(
        &home,
        &repo,
        &["report", "public", "--out", out],
        "report.public out",
    );
    public_same(
        &home,
        &repo,
        &["report", "public", "--json", "--days", "7", "--out", out],
        "report.public json out",
    );
    let inside = home.data.join("exports");
    public_same(
        &home,
        &repo,
        &["report", "public", "--out", inside.to_str().unwrap()],
        "report.public out inside the data root",
    );
    let bare = prepare_setup(&Setup {
        id: "public-bare",
        cli: &[],
        files: &[],
    });
    let none = repo_dir.path().join("no-metrics");
    std::fs::create_dir_all(none.join("scripts")).unwrap();
    public_same(
        &bare,
        &none,
        &["report", "public"],
        "report.public no metrics",
    );

    // A torn event line goes to Python, which skips it.
    let torn = repo_dir.path().join("torn");
    synthetic_repo(
        &torn,
        &format!("{GATE_EVENTS}{{\"event\": \"check.step\", \"name\": \"do"),
    );
    let native = observe_repo(&home, &torn, &["report", "public"], true);
    assert_eq!(native.code, Some(64), "report.public torn line served");
    assert!(native.stdout.is_empty() && native.events.is_empty());
    let python = observe_repo(&home, &torn, &["report", "public"], false);
    assert_eq!(python.code, Some(0), "report.public torn line in Python");
}

// ---------- data.set ----------

const PLAIN: (&str, &str) = (".plaintext", "");
const PROFILE: (&str, &str) = (
    "profile.yaml",
    "# synthetic profile\nname: \"Old Name\"\nemployer: Example Shop  # current\nrole: engineer\nskills:\n- rust\n- python\n",
);
const PROFILE_NULL: (&str, &str) = ("profile.yaml", "name:\nrole: engineer\n");
const PROFILE_ANCHOR: (&str, &str) = ("profile.yaml", "name: &a Old\nrole: *a\n");
const PROFILE_TAB: (&str, &str) = ("profile.yaml", "name:\tOld\n");

#[test]
fn data_set_matches_python() {
    strict(NOW);
    // Route key: "data.set".
    let both = [
        "data",
        "set",
        "profile.yaml",
        "--employer",
        "Sample Works",
        "--name",
        "Pat Example",
    ];
    let home = prepare_setup(&Setup {
        id: "set-profile",
        cli: &[],
        files: &[PLAIN, PROFILE],
    });
    assert_same(&home, &both, true, "data.set both fields");
    assert_same(
        &home,
        &["data", "set", "profile.yaml", "--name", "Pat O'Example"],
        true,
        "data.set name only",
    );
    assert_same(
        &home,
        &["data", "set", "other.yaml", "--name", "Pat"],
        true,
        "data.set other file",
    );
    assert_same(
        &home,
        &["data", "set", "profile.yaml"],
        true,
        "data.set no fields",
    );
    assert_same(
        &home,
        &["data", "set", "profile.yaml", "--name", ""],
        true,
        "data.set empty name",
    );
    let null = prepare_setup(&Setup {
        id: "set-null",
        cli: &[],
        files: &[PLAIN, PROFILE_NULL],
    });
    assert_same(&null, &both, true, "data.set null name, appended employer");
    let missing = prepare_setup(&Setup {
        id: "set-missing",
        cli: &[],
        files: &[PLAIN],
    });
    assert_same(&missing, &both, true, "data.set missing profile");

    let unicode = ["data", "set", "profile.yaml", "--name", "Caf\u{e9} Example"];
    refuses(
        &Setup {
            id: "set-unicode",
            cli: &[],
            files: &[PLAIN, PROFILE],
        },
        &unicode,
        "data.set non-ASCII value",
    );
    refuses(
        &Setup {
            id: "set-encrypted",
            cli: &[],
            files: &[PROFILE],
        },
        &both,
        "data.set encrypted folder",
    );
    refuses(
        &Setup {
            id: "set-anchor",
            cli: &[],
            files: &[PLAIN, PROFILE_ANCHOR],
        },
        &both,
        "data.set anchor",
    );
    refuses(
        &Setup {
            id: "set-tab",
            cli: &[],
            files: &[PLAIN, PROFILE_TAB],
        },
        &both,
        "data.set tab",
    );
}

// ---------- review ----------

const PAGES_OFF: (&str, &str) = ("settings.yaml", "run_pages: false\n");
const UTC_ZONE: (&str, &str) = ("settings.yaml", "timezone: Etc/UTC\nrun_pages: true\n");

#[test]
fn review_matches_python_in_a_folder_with_no_outcome_data() {
    strict(NOW);
    // Route key: "review".
    let fresh = prepare_setup(&Setup {
        id: "review-fresh",
        cli: &[],
        files: &[PLAIN],
    });
    assert_same(&fresh, &["review"], true, "review fresh, pages on");
    let off = prepare_setup(&Setup {
        id: "review-pages-off",
        cli: &[],
        files: &[PLAIN, PAGES_OFF],
    });
    assert_same(&off, &["review"], true, "review fresh, pages off");
    let zone = prepare_setup(&Setup {
        id: "review-utc",
        cli: &[],
        files: &[PLAIN, UTC_ZONE],
    });
    assert_same(&zone, &["review"], true, "review fresh, UTC zone");
    // A second run appends to the review log Python wrote.
    let again = prepare_setup(&Setup {
        id: "review-again",
        cli: &[&["review"]],
        files: &[PLAIN, PAGES_OFF],
    });
    assert_same(&again, &["review"], true, "review again, pages off");
}

#[test]
fn review_refuses_what_it_cannot_prove() {
    strict(NOW);
    let fresh: &[(&str, &str)] = &[PLAIN];
    refuses(
        &Setup {
            id: "review-html",
            cli: &[],
            files: fresh,
        },
        &["review", "--html"],
        "review --html",
    );
    refuses(
        &Setup {
            id: "review-encrypted",
            cli: &[],
            files: &[("settings.yaml", "run_pages: false\n")],
        },
        &["review"],
        "review encrypted folder",
    );
    refuses(
        &Setup {
            id: "review-data",
            cli: &[],
            files: &[
                PLAIN,
                (
                    "metrics.jsonl",
                    "{\"metric\": \"promo_coverage\", \"value\": 0.5, \"at\": \"2026-11-19T08:00:00+00:00\"}\n",
                ),
            ],
        },
        &["review"],
        "review with outcome data",
    );
    refuses(
        &Setup {
            id: "review-pages",
            cli: &[&["review"]],
            files: fresh,
        },
        &["review"],
        "review with earlier pages",
    );
    refuses(
        &Setup {
            id: "review-setting",
            cli: &[],
            files: &[
                PLAIN,
                ("settings.yaml", "run_pages: false\nwork_start: '09:00'\n"),
            ],
        },
        &["review"],
        "review other setting",
    );
    refuses(
        &Setup {
            id: "review-goals",
            cli: &[],
            files: &[
                PLAIN,
                ("goals.yaml", "metrics:\n  readiness:\n    success: 0.9\n"),
            ],
        },
        &["review"],
        "review own goals",
    );
    refuses(
        &Setup {
            id: "review-terms",
            cli: &[],
            files: &[PLAIN, PAGES_OFF, TERMS],
        },
        &["review"],
        "review redaction input",
    );
}

/// Negative control: a review in a home with outcome data differs from the
/// fresh run the native handler serves.
#[test]
fn review_comparison_detects_outcome_data() {
    strict(NOW);
    let fresh = prepare_setup(&Setup {
        id: "review-control-fresh",
        cli: &[],
        files: &[PLAIN, PAGES_OFF],
    });
    let data = prepare_setup(&Setup {
        id: "review-control-data",
        cli: &[],
        files: &[
            PLAIN,
            PAGES_OFF,
            (
                "metrics.jsonl",
                "{\"metric\": \"promo_coverage\", \"value\": 0.5, \"at\": \"2026-11-19T08:00:00+00:00\"}\n",
            ),
        ],
    });
    let native = observe(&fresh, &["review"], &Side::Native);
    let python = observe(&data, &["review"], &Side::Python);
    assert_ne!(native, python);
    assert!(native
        .stdout
        .contains("no-data  promo_coverage           None"));
    assert!(
        python.stdout.contains("promo_coverage           0.5"),
        "{}",
        python.stdout
    );
}
