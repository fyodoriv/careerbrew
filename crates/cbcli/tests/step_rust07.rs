//! Parity cases for the rust-07 native handlers: `"init"`,
//! `"machine.set"`, `"plugin.refresh"` and
//! `"machine.split-plugin-evidence"`.
//!
//! Each case builds a synthetic home with the Python CLI, then compares the
//! native binary and the console script with Python (stdout, exit code,
//! data-root files with content digests, event rows).
mod common;

use common::*;

const NOW: &str = "2026-11-20T09:00:00+00:00";

#[test]
fn init_matches_python_in_an_empty_home() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "init-empty",
        cli: &[],
        files: &[],
    });
    assert!(assert_same(&home, &["init"], true, "init"));
}

#[test]
fn init_skips_files_that_are_already_there() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "init-partial",
        cli: &[],
        files: &[
            ("jobs/criteria.yaml", "synthetic: kept\n"),
            ("mail.yaml", "synthetic: kept\n"),
        ],
    });
    assert!(assert_same(&home, &["init"], true, "init"));
}

#[test]
fn init_matches_python_on_a_corporate_machine() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "init-corporate",
        cli: &[&["machine", "set", "corporate"]],
        files: &[],
    });
    assert!(assert_same(&home, &["init"], true, "init"));
}

#[test]
fn init_seals_new_files_in_an_encrypted_home() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "init-encrypted",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[],
    });
    assert!(assert_same(&home, &["init"], true, "init"));
}

#[test]
fn init_writes_plaintext_when_encryption_is_off() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "init-plaintext",
        cli: &[],
        files: &[(".plaintext", "")],
    });
    assert!(assert_same(&home, &["init"], true, "init"));
}

#[test]
fn init_after_init_goes_to_python() {
    strict(NOW);
    // A first `init` leaves profile.yaml behind, which Python's redaction
    // reads: the second run must go to Python.
    let home = prepare_setup(&Setup {
        id: "init-twice",
        cli: &[&["init"]],
        files: &[],
    });
    assert!(!assert_same(&home, &["init"], false, "init"));
}

#[test]
fn init_refuses_when_redaction_inputs_exist() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "init-terms",
        cli: &[],
        files: &[("sensitive-terms.txt", "synthetic-codename\n")],
    });
    assert!(!assert_same(&home, &["init"], false, "init"));
    let home = prepare_setup(&Setup {
        id: "init-profile",
        cli: &[],
        files: &[("profile.yaml", "name: Sam Example\n")],
    });
    assert!(!assert_same(&home, &["init"], false, "init"));
}

/// Negative controls: a malformed machine file, where Python prints
/// `error:` and exits 2, refuses natively (and writes nothing); an extra
/// argument is refused by the argument reader; and the comparison sees the
/// difference between a personal and a corporate home.
#[test]
fn init_negative_controls() {
    strict(NOW);
    let bad = prepare_setup(&Setup {
        id: "init-bad-machine",
        cli: &[],
        files: &[("machine.yaml", "role: [unclosed\n")],
    });
    assert!(!assert_same(&bad, &["init"], false, "init"));
    let native = observe(&bad, &["init"], &Side::Native);
    assert_eq!(native.code, Some(64));
    assert_eq!(
        native.files,
        vec![file_line("machine.yaml", b"role: [unclosed\n")]
    );
    let empty = prepare_setup(&Setup {
        id: "init-extra",
        cli: &[],
        files: &[],
    });
    assert_eq!(
        observe(&empty, &["init", "extra"], &Side::Native).code,
        Some(64)
    );
    let corporate = prepare_setup(&Setup {
        id: "init-corporate-control",
        cli: &[&["machine", "set", "corporate"]],
        files: &[],
    });
    let personal = observe(&empty, &["init"], &Side::Native);
    let python = observe(&corporate, &["init"], &Side::Python);
    assert_ne!(personal.stdout, python.stdout);
    assert_ne!(personal.files, python.files);
    assert!(personal.stdout.contains("jobs/criteria.yaml"));
    assert!(!python.stdout.contains("jobs/criteria.yaml"));
}

fn machine_set(home: &Home, args: &[&str], must_serve: bool) -> bool {
    assert_same(home, args, must_serve, "machine.set")
}

#[test]
fn machine_set_personal_matches_python_in_an_empty_home() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "machine-set-empty",
        cli: &[],
        files: &[],
    });
    assert!(machine_set(
        &home,
        &["machine", "set", "personal", "--label", "desk-1"],
        true
    ));
    assert!(machine_set(&home, &["machine", "set", "personal"], true));
    // An empty label reads as no label, as `a.label or ""` does.
    assert!(machine_set(
        &home,
        &["machine", "set", "personal", "--label", ""],
        true
    ));
}

#[test]
fn machine_set_personal_keeps_the_label_and_key_order() {
    strict(NOW);
    // Plaintext, so the file's content digest is compared too.
    let home = prepare_setup(&Setup {
        id: "machine-set-relabel",
        cli: &[],
        files: &[
            (".plaintext", ""),
            ("machine.yaml", "label: lab_2.b/x\nrole: personal\n"),
        ],
    });
    assert!(machine_set(&home, &["machine", "set", "personal"], true));
    assert!(machine_set(
        &home,
        &["machine", "set", "personal", "--label", "Desk-9"],
        true
    ));
    let only_role = prepare_setup(&Setup {
        id: "machine-set-only-role",
        cli: &[],
        files: &[(".plaintext", ""), ("machine.yaml", "role: personal\n")],
    });
    assert!(machine_set(
        &only_role,
        &["machine", "set", "personal"],
        true
    ));
    let blank = prepare_setup(&Setup {
        id: "machine-set-blank",
        cli: &[],
        files: &[(".plaintext", ""), ("machine.yaml", "# synthetic\n")],
    });
    assert!(machine_set(
        &blank,
        &["machine", "set", "personal", "--label", "m1"],
        true
    ));
}

#[test]
fn machine_set_personal_after_corporate_and_sealed() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "machine-set-from-corporate",
        cli: &[&["machine", "set", "corporate"]],
        files: &[],
    });
    assert!(machine_set(&home, &["machine", "set", "personal"], true));
    let sealed = prepare_setup(&Setup {
        id: "machine-set-sealed",
        cli: &[
            &["machine", "set", "personal", "--label", "desk-1"],
            &["data", "encrypt"],
        ],
        files: &[],
    });
    assert!(machine_set(&sealed, &["machine", "set", "personal"], true));
}

/// The corporate role, labels PyYAML would quote, other machine keys,
/// malformed files and redaction inputs all go to Python.
#[test]
fn machine_set_refuses_what_python_must_run() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "machine-set-refusals",
        cli: &[],
        files: &[],
    });
    for args in [
        &["machine", "set", "corporate"][..],
        &["machine", "set", "personal", "--label", "yes"],
        &["machine", "set", "personal", "--label", "two words"],
        &["machine", "set", "personal", "--label", "9lives"],
    ] {
        assert!(!machine_set(&home, args, false), "{args:?}");
    }
    const CASES: &[(&str, &[(&str, &str)])] = &[
        (
            "machine-set-extra-key",
            &[
                (".plaintext", ""),
                ("machine.yaml", "role: personal\nnote: synthetic\n"),
            ],
        ),
        (
            "machine-set-quoted",
            &[(".plaintext", ""), ("machine.yaml", "label: 'on'\n")],
        ),
        (
            "machine-set-bad",
            &[(".plaintext", ""), ("machine.yaml", "role: [unclosed\n")],
        ),
        (
            "machine-set-list",
            &[(".plaintext", ""), ("machine.yaml", "- personal\n")],
        ),
    ];
    for &(id, files) in CASES {
        let home = prepare_setup(&Setup {
            id,
            cli: &[],
            files,
        });
        assert!(
            !machine_set(&home, &["machine", "set", "personal"], false),
            "{id}"
        );
    }
    let terms = prepare_setup(&Setup {
        id: "machine-set-profile",
        cli: &[],
        files: &[("profile.yaml", "name: Sam Example\n")],
    });
    assert!(!machine_set(&terms, &["machine", "set", "personal"], false));
}

/// Negative controls: two labels give different files, an invalid role is
/// refused by the argument reader, and a refusal writes nothing.
#[test]
fn machine_set_negative_controls() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "machine-set-control",
        cli: &[],
        files: &[(".plaintext", "")],
    });
    let a = observe(
        &home,
        &["machine", "set", "personal", "--label", "desk-1"],
        &Side::Native,
    );
    let b = observe(
        &home,
        &["machine", "set", "personal", "--label", "desk-2"],
        &Side::Python,
    );
    assert_eq!(a.stdout, "role set: personal\n");
    assert_ne!(a.files, b.files);
    let wrong = observe(&home, &["machine", "set", "lab"], &Side::Native);
    assert_eq!(wrong.code, Some(64));
    let refused = observe(&home, &["machine", "set", "corporate"], &Side::Native);
    assert_eq!(refused.code, Some(64));
    assert_eq!(refused.files, vec![file_line(".plaintext", b"")]);
    assert!(refused.events.is_empty());
}

// ---------- "plugin.refresh" and "machine.split-plugin-evidence" ----------

const SOURCES: &str =
    "sources:\n- kind: repo\n  path: /srv/synthetic\n- kind: gh\n  repo: example/synthetic\n";
const CONNECTORS: &str = "connectors:\n- kind: tracker\n  host: tracker.example.com\n";

fn split(home: &Home, dry_run: bool, must_serve: bool) -> bool {
    let mut args = vec!["machine", "split-plugin-evidence"];
    if dry_run {
        args.push("--dry-run");
    }
    assert_same(home, &args, must_serve, "machine.split-plugin-evidence")
}

#[test]
fn plugin_refresh_matches_python_with_no_plugin_installed() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "refresh-none",
        cli: &[],
        files: &[(".plaintext", "")],
    });
    assert!(assert_same(
        &home,
        &["plugin", "refresh"],
        true,
        "plugin.refresh"
    ));
    let corporate = prepare_setup(&Setup {
        id: "refresh-none-corporate",
        cli: &[&["machine", "set", "corporate"]],
        files: &[(".plaintext", "")],
    });
    assert!(assert_same(
        &corporate,
        &["plugin", "refresh"],
        true,
        "plugin.refresh"
    ));
}

#[test]
fn split_plugin_evidence_matches_python_in_an_empty_home() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "split-empty",
        cli: &[],
        files: &[(".plaintext", "")],
    });
    assert!(split(&home, true, true));
    assert!(split(&home, false, true));
}

#[test]
fn split_plugin_evidence_counts_private_sources_and_connectors() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "split-counts",
        cli: &[],
        files: &[
            (".plaintext", ""),
            ("work/sources.yaml", SOURCES),
            ("work/connectors.yaml", CONNECTORS),
        ],
    });
    assert!(split(&home, true, true));
    assert!(split(&home, false, true));
    let native = observe(
        &home,
        &["machine", "split-plugin-evidence", "--dry-run"],
        &Side::Native,
    );
    assert!(native.stdout.contains(
        "work/sources.yaml: 2 -> 2 source(s); work/connectors.yaml: 1 -> 1 connector(s)"
    ));
    let empty = prepare_setup(&Setup {
        id: "split-empty-lists",
        cli: &[],
        files: &[
            (".plaintext", ""),
            ("work/sources.yaml", "sources: []\nnote: kept\n"),
            ("work/connectors.yaml", ""),
        ],
    });
    assert!(split(&empty, false, true));
}

/// Every case where Python could install, deploy, fail or count otherwise
/// refuses natively, and the console script prints Python's result.
#[test]
fn plugin_refresh_and_split_refuse_what_python_must_run() {
    strict(NOW);
    // A lock (a redaction input) or a command fragment: Python reads the
    // plugins, or runs the deployer.
    let lock = prepare_setup(&Setup {
        id: "refresh-lock",
        cli: &[],
        files: &[
            (".plaintext", ""),
            ("plugins.lock.json", "{\"plugins\": {}}"),
        ],
    });
    assert!(!assert_same(
        &lock,
        &["plugin", "refresh"],
        false,
        "plugin.refresh"
    ));
    assert!(!split(&lock, true, false));
    let fragment = prepare_setup(&Setup {
        id: "refresh-fragment",
        cli: &[],
        files: &[
            (".plaintext", ""),
            ("agentbrew/Agentfile.yaml", "commands: []\n"),
        ],
    });
    assert!(!assert_same(
        &fragment,
        &["plugin", "refresh"],
        false,
        "plugin.refresh"
    ));
    // Entry shapes Python fails on: a null list, a list of words, a list
    // at the top of the file.
    const NULL: &[(&str, &str)] = &[(".plaintext", ""), ("work/sources.yaml", "sources:\n")];
    const WORDS: &[(&str, &str)] = &[
        (".plaintext", ""),
        ("work/sources.yaml", "sources:\n- repo\n"),
    ];
    const TOP: &[(&str, &str)] = &[(".plaintext", ""), ("work/sources.yaml", "- kind: repo\n")];
    for (id, files) in [
        ("split-null", NULL),
        ("split-words", WORDS),
        ("split-top-list", TOP),
    ] {
        let home = prepare_setup(&Setup {
            id,
            cli: &[],
            files,
        });
        assert!(!split(&home, false, false), "{id}");
    }
    // Redaction inputs present.
    let terms = prepare_setup(&Setup {
        id: "split-terms",
        cli: &[],
        files: &[
            (".plaintext", ""),
            ("sensitive-terms.txt", "synthetic-codename\n"),
        ],
    });
    assert!(!split(&terms, true, false));
    assert!(!assert_same(
        &terms,
        &["plugin", "refresh"],
        false,
        "plugin.refresh"
    ));
}

/// Negative controls: two homes with different counts print different
/// lines, a dry run and a real run differ, and an unknown flag or an extra
/// argument is refused by the argument reader.
#[test]
fn split_plugin_evidence_negative_controls() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "split-control-empty",
        cli: &[],
        files: &[(".plaintext", "")],
    });
    let full = prepare_setup(&Setup {
        id: "split-control-full",
        cli: &[],
        files: &[(".plaintext", ""), ("work/sources.yaml", SOURCES)],
    });
    let a = observe(&empty, &["machine", "split-plugin-evidence"], &Side::Native);
    let b = observe(&full, &["machine", "split-plugin-evidence"], &Side::Python);
    assert_eq!(a.code, Some(0));
    assert_ne!(a.stdout, b.stdout);
    let dry = observe(
        &empty,
        &["machine", "split-plugin-evidence", "--dry-run"],
        &Side::Native,
    );
    assert_ne!(dry, a);
    let unknown = observe(
        &empty,
        &["machine", "split-plugin-evidence", "--force"],
        &Side::Native,
    );
    assert_eq!(unknown.code, Some(64));
    assert!(unknown.events.is_empty());
    let extra = observe(&empty, &["plugin", "refresh", "extra"], &Side::Native);
    assert_eq!(extra.code, Some(64));
    assert!(extra.events.is_empty());
}
