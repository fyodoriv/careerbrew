//! Controls for `migration measure`: one positive run through the test-only
//! candidate override, and refusals (a step with no Rust candidate, an
//! unknown step, a candidate that exits nonzero, a relative override, too
//! few runs). Each test owns its temp folder.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");

fn script(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("candidate");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn measure(step: &str, out: &Path, runs: &str) -> Command {
    let mut command = Command::new(XTASK);
    command
        .args(["migration", "measure", "--step", step, "--out"])
        .arg(out)
        .args(["--runs", runs, "--test-allow-few-runs"])
        .env_remove("CAREERBREW_HOME");
    command
}

fn refused(result: std::process::Output, text: &str, out: &Path) {
    assert_eq!(result.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains(text), "{stderr}");
    assert!(!out.exists());
}

#[test]
fn test_candidate_writes_a_valid_record() {
    let scratch = tempfile::tempdir().unwrap();
    let candidate = script(scratch.path(), "exit 0");
    let out = scratch.path().join("measure.json");
    let result = measure("rust-03", &out, "3")
        .arg("--test-candidate")
        .arg(&candidate)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).starts_with("migration measure: step=rust-03"));
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(value["schema"], "careerbrew-migration-measure-v1");
    assert_eq!(value["step"], "rust-03");
    assert_eq!(value["side"], "rust-candidate");
    assert_eq!(value["test_override"], true);
    assert_eq!(value["runs"], 3);
    assert_eq!(value["command"][0], "<candidate>");
    assert_eq!(value["cache_state"], "warm-imports-fresh-synthetic-home");
    assert_eq!(value["meets_protocol_minimum"], false);
    assert_eq!(value["candidate_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(value["source_revision"].as_str().unwrap().len(), 40);
    let samples = value["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 3);
    for sample in samples {
        assert!(sample["tree_max_rss_bytes"].as_u64().unwrap() > 0);
        assert!(sample["tree_cpu_us"].as_u64().is_some());
        assert!(sample["rss_may_include_parent"].is_boolean());
    }
    let summary = &value["summary"];
    assert!(summary["cpu_us_p50"].as_u64().unwrap() <= summary["cpu_us_p95"].as_u64().unwrap());
    assert!(summary["wall_us_p95"].as_u64().is_some());
    // Content-free: no local path of this machine.
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains(scratch.path().to_str().unwrap()));
    // A second run never overwrites a record.
    let before = std::fs::read(&out).unwrap();
    let again = measure("rust-03", &out, "1")
        .arg("--test-candidate")
        .arg(&candidate)
        .output()
        .unwrap();
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stderr).contains("measure output already exists"));
    assert_eq!(std::fs::read(&out).unwrap(), before);
}

#[test]
fn step_without_a_rust_candidate_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("measure.json");
    let result = measure("rust-03", &out, "1").output().unwrap();
    refused(result, "step rust-03 has no Rust candidate yet", &out);
}

#[test]
fn unknown_step_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let candidate = script(scratch.path(), "exit 0");
    let out = scratch.path().join("measure.json");
    let result = measure("rust-99", &out, "1")
        .arg("--test-candidate")
        .arg(&candidate)
        .output()
        .unwrap();
    refused(result, "unknown step id", &out);
}

#[test]
fn nonzero_candidate_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let candidate = script(scratch.path(), "exit 7");
    let out = scratch.path().join("measure.json");
    let result = measure("rust-03", &out, "2")
        .arg("--test-candidate")
        .arg(&candidate)
        .output()
        .unwrap();
    refused(result, "candidate run 1 did not exit 0", &out);
}

#[test]
fn relative_candidate_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("measure.json");
    let result = measure("rust-03", &out, "1")
        .args(["--test-candidate", "candidate"])
        .output()
        .unwrap();
    refused(result, "--test-candidate must be an absolute path", &out);
}

#[test]
fn fewer_than_thirty_runs_are_refused_without_the_test_flag() {
    let scratch = tempfile::tempdir().unwrap();
    let candidate = script(scratch.path(), "exit 0");
    let out = scratch.path().join("measure.json");
    let result = Command::new(XTASK)
        .args(["migration", "measure", "--step", "rust-03", "--out"])
        .arg(&out)
        .args(["--runs", "29", "--test-candidate"])
        .arg(&candidate)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    refused(result, "below the protocol minimum of 30", &out);
}
