//! rust-05 native handlers (`src/steps/rust05.rs`): parity with the Python
//! CLI in fresh synthetic homes for "config.show", "config.set",
//! "config.unset", "approvals.list",
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

/// Settings with a list of strings, an unknown key and both time forms
/// PyYAML writes (`08:30` plain, `'19:15'` quoted).
const MIXED: &str = "backup_targets:\n- syn-a\n- syn-b\nsynthetic_extra: 5\nwork_start: 08:30\nwork_end: '19:15'\nbackup_icloud: true\n";

#[test]
fn config_show_matches_python() {
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
            id: "mixed-settings",
            cli: &[],
            files: files(vec![PLAIN, ("settings.yaml", MIXED)]),
        }),
        prepare_setup(&Setup {
            id: "sealed-settings",
            cli: &[
                &["machine", "set", "personal"],
                &["config", "set", "serve_port", "9001"],
                &["data", "encrypt"],
            ],
            files: &[],
        }),
    ];
    for home in &homes {
        assert_same(home, &["config", "show"], true, "config.show");
    }
}

#[test]
fn config_show_refuses_what_it_cannot_match() {
    strict(NOW);
    let cases = [
        ("redaction-input", "serve_port: 9000\n", true),
        ("a-float", "synthetic_ratio: 1.5\n", false),
        ("a-map", "synthetic_extra:\n  a: 1\n", false),
        ("not-a-map", "- a\n- b\n", false),
        ("malformed", "serve_port: [\n", false),
        ("non-ascii", "pm_report_title: Synthetic caf\u{e9}\n", false),
    ];
    for (id, settings, profile) in cases {
        let mut list = vec![PLAIN, ("settings.yaml", settings)];
        if profile {
            list.push(("profile.yaml", "name: Synthetic Person\n"));
        }
        let home = prepare_setup(&Setup {
            id,
            cli: &[],
            files: files(list),
        });
        assert!(refused(&home, &["config", "show"], &[]), "{id} must refuse");
        assert_same(&home, &["config", "show"], false, "config.show");
    }
    // `--help` is the compiled-in help page, served for a native command;
    // an extra word is argparse's.
    let empty = prepare_setup(&Setup {
        id: "empty",
        cli: &[],
        files: &[],
    });
    assert_same(&empty, &["config", "show", "--help"], true, "config.show");
    let extra = &["config", "show", "extra"][..];
    assert!(refused(&empty, extra, &[]), "{extra:?} must refuse");
    assert_same(&empty, extra, false, "config.show");
}

#[test]
fn config_set_matches_python() {
    strict(NOW);
    let plain = prepare_setup(&Setup {
        id: "plain-settings",
        cli: &[],
        files: files(vec![PLAIN, ("settings.yaml", SETTINGS)]),
    });
    let mixed = prepare_setup(&Setup {
        id: "mixed-settings",
        cli: &[],
        files: files(vec![PLAIN, ("settings.yaml", MIXED)]),
    });
    let bare = prepare_setup(&Setup {
        id: "plain-no-file",
        cli: &[],
        files: files(vec![PLAIN]),
    });
    let sets: &[&[&str]] = &[
        &["work_start", "08:30"],
        &["work_end", "19:15"],
        &["work_end", "7:15"],
        &["serve_port", "8800"],
        &["serve_port", "70000"],
        &["serve_port", "abc"],
        &["serve_port", "88.5"],
        &["work_days", "4,0,2,2"],
        &["work_days", "9"],
        &["backup_icloud", "yes"],
        &["backup_icloud", "maybe"],
        &["ladder", "staff-x"],
        &["pm_slack", ""],
        &["timezone", "UTC"],
        &["web_server", "native"],
        &["web_server", "other"],
        &["time_baseline_standup", "12"],
        &["backup_targets", "syn-c, syn-d"],
        &["nosuch", "1"],
    ];
    for home in [&plain, &mixed, &bare] {
        for kv in sets {
            let args = ["config", "set", kv[0], kv[1]];
            assert_same(home, &args, true, "config.set");
        }
    }
    // Errors read nothing, so a sealed home serves them too.
    let sealed = prepare_setup(&Setup {
        id: "sealed-settings",
        cli: &[
            &["machine", "set", "personal"],
            &["config", "set", "serve_port", "9001"],
            &["data", "encrypt"],
        ],
        files: &[],
    });
    for kv in [["nosuch", "1"], ["serve_port", "70000"]] {
        let args = ["config", "set", kv[0], kv[1]];
        assert_same(&sealed, &args, true, "config.set");
    }
}

#[test]
fn config_set_refuses_before_any_write() {
    strict(NOW);
    let plain = prepare_setup(&Setup {
        id: "plain-settings",
        cli: &[],
        files: files(vec![PLAIN, ("settings.yaml", SETTINGS)]),
    });
    let spaced = prepare_setup(&Setup {
        id: "spaced-value",
        cli: &[],
        files: files(vec![
            PLAIN,
            ("settings.yaml", "pm_report_title: Synthetic update\n"),
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
    let cases: [(&Home, &[&str]); 7] = [
        // Validators that call workday modules or tzdata stay in Python.
        (&plain, &["config", "set", "work_steps", "synthetic"]),
        (&plain, &["config", "set", "standup_tz", "UTC"]),
        // A value outside the YAML subset written byte-equal to PyYAML.
        (&plain, &["config", "set", "pm_report_title", "Two words"]),
        (&plain, &["config", "set", "timezone", "Europe/Berlin"]),
        // The file already holds such a value.
        (&spaced, &["config", "set", "serve_port", "8800"]),
        // A sealed write differs on every run.
        (&sealed, &["config", "set", "serve_port", "8800"]),
        // A missing value: argparse's usage error.
        (&plain, &["config", "set", "work_start"]),
    ];
    for (home, args) in cases {
        let before = observe_env(home, &["approvals", "list"], &Side::Python, &[]).files;
        let direct = observe_env(home, args, &Side::Native, &[]);
        assert_eq!(direct.code, Some(64), "{args:?} must refuse");
        assert_eq!(direct.files, before, "{args:?} wrote before refusing");
        assert!(direct.stdout.is_empty() && direct.events.is_empty());
        // Python's own sealed write has a new IV each run.
        if !std::ptr::eq(home, &sealed) {
            assert_same(home, args, false, "config.set");
        }
    }
}

#[test]
fn telemetry_off_matches_python() {
    strict(NOW);
    let plain = prepare_setup(&Setup {
        id: "plain-settings",
        cli: &[],
        files: files(vec![PLAIN, ("settings.yaml", SETTINGS)]),
    });
    let consented = prepare_setup(&Setup {
        id: "consent-yes",
        cli: &[],
        files: files(vec![PLAIN, ("settings.yaml", "telemetry_consent: 'yes'\n")]),
    });
    let bare = prepare_setup(&Setup {
        id: "plain-no-file",
        cli: &[],
        files: files(vec![PLAIN]),
    });
    for home in [&plain, &consented, &bare] {
        assert_same(home, &["telemetry", "off"], true, "telemetry.off");
    }
}

#[test]
fn telemetry_off_refuses_before_any_write() {
    strict(NOW);
    let spaced = prepare_setup(&Setup {
        id: "spaced-value",
        cli: &[],
        files: files(vec![
            PLAIN,
            ("settings.yaml", "pm_report_title: Synthetic update\n"),
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
    for home in [&spaced, &sealed] {
        let args = ["telemetry", "off"];
        let before = observe_env(home, &["approvals", "list"], &Side::Python, &[]).files;
        let direct = observe_env(home, &args, &Side::Native, &[]);
        assert_eq!(direct.code, Some(64), "telemetry off must refuse");
        assert_eq!(direct.files, before, "telemetry off wrote before refusing");
        assert!(direct.stdout.is_empty() && direct.events.is_empty());
    }
    assert_same(&spaced, &["telemetry", "off"], false, "telemetry.off");
}

/// Negative control for the config commands: two different values give a
/// different file and line, and an error writes no file.
#[test]
fn the_config_comparison_detects_a_planted_difference() {
    strict(NOW);
    let plain = prepare_setup(&Setup {
        id: "plain-settings",
        cli: &[],
        files: files(vec![PLAIN, ("settings.yaml", SETTINGS)]),
    });
    let a = observe(
        &plain,
        &["config", "set", "serve_port", "8800"],
        &Side::Native,
    );
    let b = observe(
        &plain,
        &["config", "set", "serve_port", "8801"],
        &Side::Python,
    );
    assert_ne!(a.files, b.files);
    assert_ne!(a.stdout, b.stdout);
    let error = observe(&plain, &["config", "set", "serve_port", "1"], &Side::Native);
    assert_eq!(error.code, Some(2));
    assert!(!error
        .files
        .iter()
        .any(|f| f.starts_with(".settings.yaml.lock")));
    let shown = observe(&plain, &["config", "show"], &Side::Native);
    let mixed = prepare_setup(&Setup {
        id: "mixed-settings",
        cli: &[],
        files: files(vec![PLAIN, ("settings.yaml", MIXED)]),
    });
    let other = observe(&mixed, &["config", "show"], &Side::Python);
    assert_ne!(shown.stdout, other.stdout);
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
