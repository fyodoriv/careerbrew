//! Controls for the rust-02 vector step (pure value vectors): all four
//! migration commands (baseline, verify, measure, decide) accept
//! `--step rust-02`; verify with the registered vector sets passes with
//! zero real-home writes; a planted wrong vector in each set fails its case;
//! a planted write into
//! the caller's `CAREERBREW_HOME` is counted and fails the record; and the
//! refusals (a wrong vector naming no set, a planted write without an
//! absolute home or on a step that does not count writes, an empty
//! `CAREERBREW_HOME`). Each test owns its temp folder and synthetic home.
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn python() -> PathBuf {
    root().join(".venv/bin/python")
}

/// The candidate cargo built next to this test's xtask binary (the
/// `careerbrew-purevec` package has an integration test, so `cargo test
/// --all` builds its binary).
fn purevec() -> PathBuf {
    let path = Path::new(XTASK).with_file_name("careerbrew-purevec");
    assert!(
        path.is_file(),
        "build it first: cargo build -p careerbrew-purevec"
    );
    path
}

/// A verify command for `step` with a synthetic caller `CAREERBREW_HOME`.
fn verify(step: &str, home: &Path, out: &Path, extra: &[&str]) -> std::process::Output {
    Command::new(XTASK)
        .args(["migration", "verify", "--step", step, "--python"])
        .arg(python())
        .arg("--out")
        .arg(out)
        .arg("--test-candidate")
        .arg(purevec())
        .args(extra)
        .env("CAREERBREW_HOME", home)
        .output()
        .unwrap()
}

fn read(out: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(out).unwrap()).unwrap()
}

fn refused(result: &std::process::Output, text: &str, out: &Path) {
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains(text), "{stderr}");
    assert!(!out.exists());
}

#[test]
fn verify_with_registered_sets_passes_with_zero_home_writes() {
    let scratch = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("kept.txt"), b"synthetic").unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify("rust-02", home.path(), &out, &[]);
    assert_eq!(run.status.code(), Some(0), "{run:?}");
    let record = read(&out);
    assert_eq!(record["step"], "rust-02");
    assert_eq!(record["contracts"], serde_json::json!(["pure-vectors"]));
    assert_eq!(
        record["vector_sets"],
        serde_json::json!(["clock", "paths", "settings"])
    );
    assert_eq!(record["failed_cases"], serde_json::json!([]));
    assert_eq!(record["passed"], true);
    assert_eq!(record["real_home_writes"], 0);
    assert_eq!(record["planted_home_write"], false);
    assert!(record["real_homes_checked"].as_u64().unwrap() >= 1);
}

#[test]
fn a_planted_home_write_is_counted_and_fails_the_record() {
    let scratch = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify(
        "rust-02",
        home.path(),
        &out,
        &["--test-plant-home-write", "planted.txt"],
    );
    assert_eq!(run.status.code(), Some(1), "{run:?}");
    let record = read(&out);
    assert_eq!(record["real_home_writes"], 1);
    assert_eq!(record["planted_home_write"], true);
    assert_eq!(
        record["failed_cases"],
        serde_json::json!(["real-home-writes"])
    );
    assert!(
        !home.path().join("planted.txt").exists(),
        "verify must remove its planted file"
    );
}

#[test]
fn a_planted_wrong_clock_vector_fails_its_case() {
    let scratch = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify(
        "rust-02",
        home.path(),
        &out,
        &["--test-wrong-vector", "clock"],
    );
    assert_eq!(run.status.code(), Some(1), "{run:?}");
    let record = read(&out);
    assert_eq!(record["passed"], false);
    assert_eq!(record["planted_wrong_vector"], "clock");
    assert_eq!(
        record["failed_cases"],
        serde_json::json!(["clock:day-bounds-ny-spring"])
    );
}

#[test]
fn a_planted_write_needs_an_absolute_home() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = Command::new(XTASK)
        .args(["migration", "verify", "--step", "rust-02", "--python"])
        .arg(python())
        .arg("--out")
        .arg(&out)
        .arg("--test-candidate")
        .arg(purevec())
        .args(["--test-plant-home-write", "planted.txt"])
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    refused(&run, "needs an absolute CAREERBREW_HOME", &out);
}

#[test]
fn a_planted_write_is_refused_where_writes_are_not_counted() {
    let scratch = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify(
        "rust-01",
        home.path(),
        &out,
        &["--test-plant-home-write", "planted.txt"],
    );
    refused(&run, "only to a step that counts home writes", &out);
    let run = verify(
        "rust-03",
        home.path(),
        &out,
        &["--test-plant-home-write", "planted.txt"],
    );
    refused(&run, "only to a vector step", &out);
}

#[test]
fn a_planted_wrong_paths_vector_fails_its_case() {
    let scratch = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify(
        "rust-02",
        home.path(),
        &out,
        &["--test-wrong-vector", "paths"],
    );
    assert_eq!(run.status.code(), Some(1), "{run:?}");
    let record = read(&out);
    assert_eq!(record["passed"], false);
    assert_eq!(record["planted_wrong_vector"], "paths");
    assert_eq!(
        record["failed_cases"],
        serde_json::json!(["paths:root-unset"])
    );
    assert_eq!(record["violations"]["contract"], 1);
}

#[test]
fn a_planted_wrong_settings_vector_fails_its_case() {
    let scratch = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify(
        "rust-02",
        home.path(),
        &out,
        &["--test-wrong-vector", "settings"],
    );
    assert_eq!(run.status.code(), Some(1), "{run:?}");
    let record = read(&out);
    assert_eq!(record["passed"], false);
    assert_eq!(record["planted_wrong_vector"], "settings");
    assert_eq!(
        record["failed_cases"],
        serde_json::json!(["settings:hhmm-0900"])
    );
    assert_eq!(record["violations"]["contract"], 1);
}

#[test]
fn a_wrong_vector_that_names_no_set_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify(
        "rust-02",
        home.path(),
        &out,
        &["--test-wrong-vector", "no-such-set"],
    );
    refused(&run, "names no vector set", &out);
}

#[test]
fn an_empty_careerbrew_home_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify("rust-02", Path::new(""), &out, &[]);
    refused(&run, "CAREERBREW_HOME is set but empty", &out);
}

#[test]
fn baseline_measure_and_decide_accept_rust_02() {
    let scratch = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let base = scratch.path().join("baseline.json");
    let result = Command::new(XTASK)
        .args(["migration", "baseline", "--step", "rust-02", "--python"])
        .arg(python())
        .arg("--out")
        .arg(&base)
        .args(["--runs", "1", "--test-allow-few-runs"])
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    assert_eq!(read(&base)["step"], "rust-02");
    // The override runs with no arguments, so a wrapper adds the registered
    // candidate's `--all`.
    let wrapper = scratch.path().join("candidate");
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\nexec '{}' --all\n", purevec().display()),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let measured = scratch.path().join("measure.json");
    let result = Command::new(XTASK)
        .args(["migration", "measure", "--step", "rust-02", "--python"])
        .arg(python())
        .arg("--out")
        .arg(&measured)
        .args(["--pairs", "1", "--test-allow-few-pairs"])
        .arg("--test-candidate")
        .arg(&wrapper)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    assert_eq!(read(&measured)["step"], "rust-02");
    let verified = scratch.path().join("verify.json");
    verify("rust-02", home.path(), &verified, &[]);
    assert_eq!(read(&verified)["step"], "rust-02");
    let out = scratch.path().join("decide.json");
    let result = Command::new(XTASK)
        .args(["migration", "decide", "--step", "rust-02", "--baseline"])
        .arg(&base)
        .arg("--verify")
        .arg(&verified)
        .arg("--measure")
        .arg(&measured)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    // Keep-python (exit 1), not a refusal (exit 2): decide read every record.
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    let verdict = read(&out);
    assert_eq!(verdict["step"], "rust-02");
    assert_eq!(verdict["decision"], "keep-python");
}
