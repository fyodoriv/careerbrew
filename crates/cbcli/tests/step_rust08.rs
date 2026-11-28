//! Parity cases for the rust-08 native handlers: `"pair.init"` and
//! `"pair.join"`.
//!
//! Each case builds a synthetic home with the Python CLI, then compares the
//! native binary and the console script with Python: stdout, exit code,
//! data-root files and event rows. The pairing key lies outside the data
//! root (under the scenario's `HOME`), so more tests read it after a native
//! run and check its bytes and mode. The tests run with no terminal, so
//! only the non-terminal branch is served.
mod common;

use common::*;
use std::os::unix::fs::PermissionsExt;

const NOW: &str = "2026-11-20T09:00:00+00:00";

/// A synthetic Fernet key (bytes 0 to 31), never a real secret.
const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

fn pair_key(home: &Home) -> std::path::PathBuf {
    home.home.join(".config/careerbrew/pair.key")
}

#[test]
fn pair_init_matches_python_in_a_personal_home() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "pair-init-personal",
        cli: &[],
        files: &[],
    });
    assert!(assert_same(&home, &["pair", "init"], true, "pair.init"));
}

#[test]
fn pair_init_prints_the_corporate_line_first() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "pair-init-corporate",
        cli: &[&["machine", "set", "corporate"]],
        files: &[],
    });
    assert!(assert_same(&home, &["pair", "init"], true, "pair.init"));
}

#[test]
fn pair_init_overwrites_an_existing_key() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "pair-init-again",
        cli: &[&["pair", "join", KEY]],
        files: &[],
    });
    assert!(assert_same(&home, &["pair", "init"], true, "pair.init"));
    let native = observe(&home, &["pair", "init"], &Side::Native);
    assert_eq!(native.code, Some(0));
    let written = std::fs::read_to_string(pair_key(&home)).unwrap();
    assert_eq!(written.len(), 44);
    assert!(written.ends_with('='));
    assert_ne!(written, KEY);
    let mode = std::fs::metadata(pair_key(&home)).unwrap().permissions();
    assert_eq!(mode.mode() & 0o777, 0o600);
}

#[test]
fn pair_join_matches_python_with_a_key_argument() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "pair-join-new",
        cli: &[],
        files: &[],
    });
    assert!(assert_same(
        &home,
        &["pair", "join", KEY],
        true,
        "pair.join"
    ));
    let native = observe(&home, &["pair", "join", KEY], &Side::Native);
    assert_eq!(native.code, Some(0));
    assert_eq!(std::fs::read_to_string(pair_key(&home)).unwrap(), KEY);
    let mode = std::fs::metadata(pair_key(&home)).unwrap().permissions();
    assert_eq!(mode.mode() & 0o777, 0o600);
}

#[test]
fn pair_join_replaces_an_existing_key() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "pair-join-again",
        cli: &[&["pair", "init"], &["machine", "set", "corporate"]],
        files: &[],
    });
    assert!(assert_same(
        &home,
        &["pair", "join", KEY],
        true,
        "pair.join"
    ));
}

/// Every case where Python prints `error:` or reads stdin refuses natively,
/// and the console script then prints Python's result.
#[test]
fn pair_commands_refuse_what_python_must_run() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "pair-refusals",
        cli: &[],
        files: &[],
    });
    // Not a key: Python prints its `error:` line.
    assert!(!assert_same(
        &home,
        &["pair", "join", "not-a-pairing-key"],
        false,
        "pair.join"
    ));
    // No key: Python reads stdin.
    assert!(!assert_same(&home, &["pair", "join"], false, "pair.join"));
    // Redaction inputs present: the output goes to Python.
    let terms = prepare_setup(&Setup {
        id: "pair-terms",
        cli: &[],
        files: &[("sensitive-terms.txt", "synthetic-codename\n")],
    });
    assert!(!assert_same(&terms, &["pair", "init"], false, "pair.init"));
    assert!(!assert_same(
        &terms,
        &["pair", "join", KEY],
        false,
        "pair.join"
    ));
    // A machine file Python cannot read.
    let bad = prepare_setup(&Setup {
        id: "pair-bad-machine",
        cli: &[],
        files: &[("machine.yaml", "role: [unclosed\n")],
    });
    assert!(!assert_same(&bad, &["pair", "init"], false, "pair.init"));
    let native = observe(&bad, &["pair", "init"], &Side::Native);
    assert_eq!(native.code, Some(64));
    assert!(!pair_key(&bad).exists());
}

/// Negative controls: the comparison sees a planted difference (a personal
/// and a corporate home print different lines), and an extra argument is
/// refused by the argument reader.
#[test]
fn pair_negative_controls() {
    strict(NOW);
    let personal = prepare_setup(&Setup {
        id: "pair-control-personal",
        cli: &[],
        files: &[],
    });
    let corporate = prepare_setup(&Setup {
        id: "pair-control-corporate",
        cli: &[&["machine", "set", "corporate"]],
        files: &[],
    });
    let a = observe(&personal, &["pair", "init"], &Side::Native);
    let b = observe(&corporate, &["pair", "init"], &Side::Python);
    assert!(a.stdout.starts_with("pairing key written to "), "{a:?}");
    assert!(b.stdout.starts_with("export level: none."), "{b:?}");
    assert_ne!(a, b);
    let extra = observe(&personal, &["pair", "join", KEY, "extra"], &Side::Native);
    assert_eq!(extra.code, Some(64));
    assert!(extra.events.is_empty());
    assert!(!pair_key(&personal).exists());
}
