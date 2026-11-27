//! rust-05 native handlers (`src/steps/rust05.rs`): parity with the Python
//! CLI in fresh synthetic homes for "config.unset", "approvals.list",
//! "approvals.guidance", "approvals.show" and "approvals.decide", plus the
//! cases the native binary must refuse and a negative control.
mod common;

use common::*;
use std::process::Command;

const NOW: &str = "2026-11-20T09:00:00";

/// Four synthetic receipts: pending, approved with a note, rejected with a
/// note, approved without one.
const RECORDS: &str = r#"[
  {"at": "2026-11-19T08:00:00+00:00", "by": "agent", "detail": "", "id": "aaaa0001", "note": "", "status": "pending", "title": "Synthetic pending"},
  {"at": "2026-11-19T08:01:00+00:00", "by": "agent", "decided_at": "2026-11-19T09:00:00+00:00", "detail": "", "id": "aaaa0002", "note": "only on weekdays", "status": "approved", "title": "Synthetic approved"},
  {"at": "2026-11-19T08:02:00+00:00", "by": "agent", "decided_at": "2026-11-19T09:00:00+00:00", "detail": "d", "id": "aaaa0003", "note": "ask first", "status": "rejected", "title": "Synthetic rejected"},
  {"at": "2026-11-19T08:03:00+00:00", "by": "agent", "decided_at": "2026-11-19T10:00:00+00:00", "detail": "", "id": "aaaa0004", "note": "", "status": "approved", "title": "Synthetic four"}
]"#;

/// Settings in the YAML subset the native writer covers.
const SETTINGS: &str = "ladder: generic-swe\nserve_port: 9000\nwork_days:\n- 0\n- 2\nbackup_targets: []\npm_slack: ''\nstart_web: false\ntimezone: null\nweb_server: native\n";

const PLAIN: (&str, &str) = (".plaintext", "");

/// 25 receipts, so `approvals list` shows only the last 20 and guidance
/// keeps 10 of its noted decisions.
fn many_records() -> &'static str {
    let mut rows = Vec::new();
    for i in 0..25 {
        let (status, extra) = match i % 3 {
            0 => ("pending", String::new()),
            1 => (
                "approved",
                format!(", \"decided_at\": \"2026-11-19T{:02}:00:00+00:00\"", i % 24),
            ),
            _ => (
                "rejected",
                // Ties on decided_at keep the file order (a stable sort).
                ", \"decided_at\": \"2026-11-19T07:00:00+00:00\"".to_owned(),
            ),
        };
        let note = if i % 3 == 0 { "" } else { "synthetic note" };
        rows.push(format!(
            "{{\"at\": \"2026-11-18T08:00:00+00:00\", \"id\": \"bbbb{i:04}\", \"note\": \"{note}\", \"status\": \"{status}\", \"title\": \"Synthetic {i}\"{extra}}}"
        ));
    }
    Box::leak(format!("[{}]", rows.join(", ")).into_boxed_str())
}

fn files(list: Vec<(&'static str, &'static str)>) -> &'static [(&'static str, &'static str)] {
    Box::leak(list.into_boxed_slice())
}

/// `observe` with extra environment variables on every side (the owner
/// switch `CAREERBREW_TEST_OWNER`, which the shared harness never sets).
fn observe_env(home: &Home, args: &[&str], side: &Side, extra: &[(&str, &str)]) -> Observed {
    restore(home);
    let mut cmd = match side {
        Side::Python => {
            let mut cmd = python(home, args);
            cmd.env("CAREERBREW_NATIVE", "off");
            cmd
        }
        Side::Console => {
            let mut cmd = python(home, args);
            cmd.env("CAREERBREW_NATIVE_BIN", BIN);
            cmd
        }
        Side::Native => {
            let mut cmd = Command::new(BIN);
            cmd.args(args);
            base(&mut cmd, home);
            cmd
        }
    };
    for (key, value) in extra {
        cmd.env(key, value);
    }
    let output = cmd.output().unwrap();
    let mut files = Vec::new();
    walk(&home.data, &home.data, &mut files);
    files.sort();
    let events = std::fs::read_to_string(home.data.join("logs/events.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(normalize)
        .collect();
    Observed {
        code: output.status.code(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        files,
        events,
    }
}

/// `assert_same` with extra environment variables.
fn assert_same_env(
    home: &Home,
    args: &[&str],
    extra: &[(&str, &str)],
    must_serve: bool,
    label: &str,
) -> bool {
    let want = observe_env(home, args, &Side::Python, extra);
    let direct = observe_env(home, args, &Side::Native, extra);
    let served = direct.code != Some(64);
    if must_serve {
        assert!(served, "{label}: {args:?} refused natively");
    }
    if served {
        assert_eq!(direct, want, "{label}: {args:?} natively");
    } else {
        assert!(
            direct.stdout.is_empty() && direct.events.is_empty(),
            "{label}: {args:?}"
        );
    }
    let mut console = observe_env(home, args, &Side::Console, extra);
    if !served {
        if let Some(at) = console
            .events
            .iter()
            .position(|row| row.contains("cli.native_unsupported"))
        {
            console.events.remove(at);
        }
    }
    assert_eq!(console, want, "{label}: {args:?} via the console script");
    served
}

fn refused(home: &Home, args: &[&str], extra: &[(&str, &str)]) -> bool {
    observe_env(home, args, &Side::Native, extra).code == Some(64)
}

const OWNER: &[(&str, &str)] = &[("CAREERBREW_TEST_OWNER", "1")];

#[test]
fn approvals_readers_match_python() {
    strict(NOW);
    let homes = [
        prepare_setup(&Setup {
            id: "empty",
            cli: &[],
            files: &[],
        }),
        prepare_setup(&Setup {
            id: "plain-records",
            cli: &[],
            files: files(vec![PLAIN, ("approvals.json", RECORDS)]),
        }),
        prepare_setup(&Setup {
            id: "sealed-records",
            cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
            files: files(vec![("approvals.json", RECORDS)]),
        }),
        prepare_setup(&Setup {
            id: "many-records",
            cli: &[],
            files: files(vec![PLAIN, ("approvals.json", many_records())]),
        }),
        prepare_setup(&Setup {
            id: "requested",
            cli: &[&["approvals", "request", "Synthetic deploy step"]],
            files: &[],
        }),
    ];
    for home in &homes {
        assert_same(home, &["approvals", "list"], true, "approvals.list");
        assert_same(home, &["approvals", "guidance"], true, "approvals.guidance");
        for id in ["aaaa0001", "aaaa0002", "aaaa0003", "bbbb0004", "nosuch"] {
            assert_same(home, &["approvals", "show", id], true, "approvals.show");
        }
    }
}

#[test]
fn approvals_readers_refuse_what_they_cannot_match() {
    strict(NOW);
    let malformed = prepare_setup(&Setup {
        id: "malformed",
        cli: &[],
        files: files(vec![PLAIN, ("approvals.json", "[{\"id\": \"x\"}]")]),
    });
    let redaction = prepare_setup(&Setup {
        id: "redaction-input",
        cli: &[],
        files: files(vec![
            PLAIN,
            ("approvals.json", RECORDS),
            ("profile.yaml", "name: Synthetic Person\n"),
        ]),
    });
    for home in [&malformed, &redaction] {
        for args in [
            &["approvals", "list"][..],
            &["approvals", "guidance"],
            &["approvals", "show", "aaaa0002"],
        ] {
            assert!(refused(home, args, &[]), "{args:?} must refuse");
            assert_same(home, args, false, "approvals.show");
        }
    }
    // An id whose `repr` needs other quotes goes to Python.
    let plain = prepare_setup(&Setup {
        id: "plain-records",
        cli: &[],
        files: files(vec![PLAIN, ("approvals.json", RECORDS)]),
    });
    assert!(refused(&plain, &["approvals", "show", "no'such"], &[]));
    assert_same(
        &plain,
        &["approvals", "show", "no'such"],
        false,
        "approvals.show",
    );
}

#[test]
fn approvals_decide_matches_python() {
    strict(NOW);
    let plain = prepare_setup(&Setup {
        id: "plain-records",
        cli: &[],
        files: files(vec![PLAIN, ("approvals.json", RECORDS)]),
    });
    let zoned = prepare_setup(&Setup {
        id: "plain-zoned",
        cli: &[],
        files: files(vec![
            PLAIN,
            ("approvals.json", RECORDS),
            ("settings.yaml", "timezone: Asia/Tokyo\n"),
        ]),
    });
    for home in [&plain, &zoned] {
        let decide = ["approvals", "decide", "aaaa0001", "approved"];
        assert_same_env(home, &decide, OWNER, true, "approvals.decide");
        let noted = [
            "approvals",
            "decide",
            "aaaa0001",
            "rejected",
            "--note",
            "  synthetic note  ",
        ];
        assert_same_env(home, &noted, OWNER, true, "approvals.decide");
        // Without a terminal or the owner switch: `only you set goals`, exit 1.
        assert_same(home, &decide, true, "approvals.decide");
    }
}

#[test]
fn approvals_decide_refuses_before_any_write() {
    strict(NOW);
    let plain = prepare_setup(&Setup {
        id: "plain-records",
        cli: &[],
        files: files(vec![PLAIN, ("approvals.json", RECORDS)]),
    });
    let sealed = prepare_setup(&Setup {
        id: "sealed-records",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: files(vec![("approvals.json", RECORDS)]),
    });
    let cases: [(&Home, &[&str]); 4] = [
        // Already decided, and an unknown id: Python's errors.
        (&plain, &["approvals", "decide", "aaaa0002", "approved"]),
        (&plain, &["approvals", "decide", "nosuch", "rejected"]),
        // A sealed write differs on every run.
        (&sealed, &["approvals", "decide", "aaaa0001", "approved"]),
        // Not a listed choice: argparse's usage error.
        (&plain, &["approvals", "decide", "aaaa0001", "maybe"]),
    ];
    for (home, args) in cases {
        let before = observe_env(home, &["approvals", "list"], &Side::Python, &[]).files;
        let direct = observe_env(home, args, &Side::Native, OWNER);
        assert_eq!(direct.code, Some(64), "{args:?} must refuse");
        assert_eq!(direct.files, before, "{args:?} wrote before refusing");
        assert!(direct.stdout.is_empty() && direct.events.is_empty());
        // Python's own sealed write has a new IV each run, so only the
        // plaintext home compares whole runs.
        if std::ptr::eq(home, &plain) {
            assert_same_env(home, args, OWNER, false, "approvals.decide");
        }
    }
}

#[test]
fn config_unset_matches_python() {
    strict(NOW);
    let homes = [
        prepare_setup(&Setup {
            id: "empty",
            cli: &[],
            files: &[],
        }),
        prepare_setup(&Setup {
            id: "plain-settings",
            cli: &[],
            files: files(vec![PLAIN, ("settings.yaml", SETTINGS)]),
        }),
        prepare_setup(&Setup {
            id: "set-by-cli",
            cli: &[
                &["config", "set", "serve_port", "9100"],
                &["config", "set", "work_days", "0,1,2"],
                &["config", "set", "ladder", "staff-x"],
            ],
            files: files(vec![PLAIN]),
        }),
    ];
    for home in &homes {
        for key in ["serve_port", "work_days", "ladder", "timezone", "nosuch"] {
            assert_same(home, &["config", "unset", key], true, "config.unset");
        }
    }
}

#[test]
fn config_unset_refuses_writes_it_cannot_match() {
    strict(NOW);
    let spaced = prepare_setup(&Setup {
        id: "spaced-value",
        cli: &[],
        files: files(vec![
            PLAIN,
            (
                "settings.yaml",
                "pm_report_title: Synthetic update\nserve_port: 9000\n",
            ),
        ]),
    });
    let sealed = prepare_setup(&Setup {
        id: "sealed-settings",
        cli: &[
            &["machine", "set", "personal"],
            &["config", "set", "serve_port", "9001"],
            &["data", "encrypt"],
        ],
        files: &[],
    });
    let listed = prepare_setup(&Setup {
        id: "not-a-map",
        cli: &[],
        files: files(vec![PLAIN, ("settings.yaml", "- a\n- b\n")]),
    });
    for (home, key) in [
        (&spaced, "serve_port"),
        (&sealed, "serve_port"),
        (&listed, "ladder"),
    ] {
        let args = ["config", "unset", key];
        let before = observe_env(home, &["approvals", "list"], &Side::Python, &[]).files;
        let direct = observe_env(home, &args, &Side::Native, &[]);
        assert_eq!(direct.code, Some(64), "{key} must refuse");
        assert_eq!(direct.files, before, "{key} wrote before refusing");
        assert!(direct.stdout.is_empty() && direct.events.is_empty());
        // Python's own sealed write has a new IV each run.
        if !std::ptr::eq(home, &sealed) {
            assert_same(home, &args, false, "config.unset");
        }
    }
    // The same homes still serve an unset that writes nothing.
    assert_same(
        &spaced,
        &["config", "unset", "nosuch"],
        true,
        "config.unset",
    );
    assert_same(
        &sealed,
        &["config", "unset", "nosuch"],
        true,
        "config.unset",
    );
}

/// Negative control: the comparison sees a real difference. A decision
/// changes the saved receipts and the event rows; a different receipt
/// prints a different line and exit code.
#[test]
fn the_comparison_detects_a_planted_difference() {
    strict(NOW);
    let plain = prepare_setup(&Setup {
        id: "plain-records",
        cli: &[],
        files: files(vec![PLAIN, ("approvals.json", RECORDS)]),
    });
    let approved = observe_env(
        &plain,
        &["approvals", "decide", "aaaa0001", "approved"],
        &Side::Native,
        OWNER,
    );
    let rejected = observe_env(
        &plain,
        &["approvals", "decide", "aaaa0001", "rejected"],
        &Side::Python,
        OWNER,
    );
    assert_ne!(approved.files, rejected.files);
    assert_ne!(approved.events, rejected.events);
    let shown = observe(&plain, &["approvals", "show", "aaaa0002"], &Side::Native);
    let other = observe(&plain, &["approvals", "show", "aaaa0003"], &Side::Python);
    assert_ne!(shown, other);
    assert_eq!((shown.code, other.code), (Some(0), Some(1)));
}
