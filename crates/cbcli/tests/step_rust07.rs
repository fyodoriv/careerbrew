//! Parity cases for the rust-07 native handlers: `"init"`.
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
