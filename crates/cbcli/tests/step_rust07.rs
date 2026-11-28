//! Parity cases for the rust-07 native handlers: `"init"` and
//! `"machine.set"`.
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
