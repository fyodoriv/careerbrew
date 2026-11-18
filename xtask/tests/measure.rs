//! Controls for `migration measure --pairs`: one paired positive run through
//! the test-only candidate override, one end-to-end run where `decide`
//! consumes the measure record next to a real baseline record, and refusals
//! (a step with no Rust candidate, an unknown step, a candidate that exits
//! nonzero, a relative override or `--python`, too few pairs, zero pairs, a
//! relative `--out`, an existing `--out`, an empty `CAREERBREW_HOME`, the
//! test override without the test flag, the old `--runs` flag, and a
//! candidate change during the runs). A source or fixture change during the
//! runs has an in-crate control (`measure::controls`), because this binary
//! must not edit the shared checkout. Each test owns its temp folder.
use serde_json::{json, Value};
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

fn script(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("candidate");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

/// A test-flag measure command without a candidate.
fn measure(step: &str, out: &Path, pairs: &str) -> Command {
    let mut command = Command::new(XTASK);
    command
        .args(["migration", "measure", "--step", step, "--python"])
        .arg(python())
        .arg("--out")
        .arg(out)
        .args(["--pairs", pairs, "--test-allow-few-pairs"])
        .env_remove("CAREERBREW_HOME");
    command
}

fn with_candidate(step: &str, out: &Path, pairs: &str, candidate: &Path) -> Command {
    let mut command = measure(step, out, pairs);
    command.arg("--test-candidate").arg(candidate);
    command
}

fn refused(result: std::process::Output, text: &str, out: &Path) {
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains(text), "{stderr}");
    assert!(!out.exists());
}

fn passed(result: &std::process::Output) {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn test_candidate_writes_a_paired_cold_and_warm_record() {
    let scratch = tempfile::tempdir().unwrap();
    let candidate = script(scratch.path(), "exit 0");
    let out = scratch.path().join("measure.json");
    let result = with_candidate("rust-03", &out, "3", &candidate)
        .output()
        .unwrap();
    passed(&result);
    assert!(String::from_utf8_lossy(&result.stdout).starts_with("migration measure: step=rust-03"));
    let value: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(value["schema"], "careerbrew-migration-measure-v1");
    assert_eq!(value["step"], "rust-03");
    assert_eq!(value["test_override"], true);
    assert_eq!(value["pairs"], 3);
    assert_eq!(value["meets_protocol_minimum"], false);
    assert_eq!(value["oracle_command"][0], "<python>");
    assert_eq!(value["candidate_command"][0], "<candidate>");
    assert_eq!(value["candidate_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(value["source_revision"].as_str().unwrap().len(), 40);
    let regimes = value["regimes"].as_array().unwrap();
    assert_eq!(regimes.len(), 2);
    assert_eq!(regimes[0]["cache_state"], "cold");
    assert_eq!(regimes[1]["cache_state"], "warm");
    // `orders` logs the side order each pair actually ran in: cold odd and
    // warm even pairs start with the oracle.
    let (oc, co) = (
        json!(["oracle", "candidate"]),
        json!(["candidate", "oracle"]),
    );
    assert_eq!(regimes[0]["orders"], json!([oc, co, oc]));
    assert_eq!(regimes[1]["orders"], json!([co, oc, co]));
    assert_eq!(regimes[0]["first"], oc);
    assert_eq!(regimes[1]["first"], co);
    for regime in regimes {
        assert_eq!(regime["pair_ids"], json!([1, 2, 3]));
        assert_eq!(regime["cached"], 0);
        for side in ["oracle", "candidate"] {
            let cpu = regime[format!("{side}_cpu_us")].as_array().unwrap();
            assert_eq!(cpu.len(), 3);
            assert!(cpu.iter().all(|value| value.as_u64().is_some()));
            let rss = regime[format!("{side}_tree_max_rss_bytes")]
                .as_array()
                .unwrap();
            assert_eq!(rss.len(), 3);
            assert!(rss.iter().all(|value| value.as_u64().unwrap() > 0));
            assert_eq!(
                regime[format!("{side}_wall_us")].as_array().unwrap().len(),
                3
            );
            let summary = &regime["summary"][side];
            assert!(
                summary["cpu_us_p50"].as_u64().unwrap() <= summary["cpu_us_p95"].as_u64().unwrap()
            );
        }
    }
    // Content-free: no local path of this machine (scratch, repo root, home).
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains(scratch.path().to_str().unwrap()));
    assert!(!text.contains(root().to_str().unwrap()));
    if let Some(home) = std::env::var_os("HOME").filter(|home| home.len() > 1) {
        assert!(!text.contains(home.to_str().unwrap()));
    }
    // A second run never overwrites a record.
    let before = std::fs::read(&out).unwrap();
    let again = with_candidate("rust-03", &out, "1", &candidate)
        .output()
        .unwrap();
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stderr).contains("measure output already exists"));
    assert_eq!(std::fs::read(&out).unwrap(), before);
}

#[test]
fn decide_consumes_the_measure_record_with_a_baseline_record() {
    let scratch = tempfile::tempdir().unwrap();
    let candidate = script(scratch.path(), "exit 0");
    let base = scratch.path().join("baseline.json");
    let result = Command::new(XTASK)
        .args(["migration", "baseline", "--step", "rust-03", "--python"])
        .arg(python())
        .arg("--out")
        .arg(&base)
        .args(["--runs", "1", "--test-allow-few-runs"])
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    passed(&result);
    let measured = scratch.path().join("measure.json");
    passed(
        &with_candidate("rust-03", &measured, "1", &candidate)
            .output()
            .unwrap(),
    );
    let verify = scratch.path().join("verify.json");
    let record = json!({
        "schema": "careerbrew-migration-verify-v1",
        "step": "rust-03",
        "checks": [{"id": "cli.actions", "passed": true}],
        "violations": {
            "contract": 0, "privacy": 0, "authorization": 0, "role": 0,
            "side_effect": 0, "missing_writes": 0, "duplicate_writes": 0,
        },
        "unobserved_violations": [],
        "missing_contracts": [],
    });
    std::fs::write(&verify, serde_json::to_vec(&record).unwrap()).unwrap();
    let out = scratch.path().join("decide.json");
    let result = Command::new(XTASK)
        .args(["migration", "decide", "--step", "rust-03", "--baseline"])
        .arg(&base)
        .arg("--verify")
        .arg(&verify)
        .arg("--measure")
        .arg(&measured)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    // Keep-python (exit 1), not a refusal (exit 2): decide read the record.
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    let verdict: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(verdict["decision"], "keep-python");
    let reasons = verdict["reasons"].as_array().unwrap();
    assert!(reasons.contains(&json!("too-few-unique-pairs")));
    assert!(reasons.contains(&json!("baseline-below-protocol-minimum")));
    assert!(!reasons.contains(&json!("regime-missing")));
    assert!(!reasons.contains(&json!("cached-sample")));
    let summary = verdict["regimes"].as_array().unwrap();
    assert_eq!(summary.len(), 2);
    assert_eq!(summary[0]["cache_state"], "cold");
    assert_eq!(summary[1]["cache_state"], "warm");
    assert_eq!(summary[0]["pairs"], 1);
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
    let result = with_candidate("rust-99", &out, "1", &candidate)
        .output()
        .unwrap();
    refused(result, "unknown step id", &out);
}

#[test]
fn nonzero_candidate_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let candidate = script(scratch.path(), "exit 7");
    let out = scratch.path().join("measure.json");
    let result = with_candidate("rust-03", &out, "2", &candidate)
        .output()
        .unwrap();
    refused(result, "did not exit 0", &out);
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
fn relative_python_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let candidate = script(scratch.path(), "exit 0");
    let out = scratch.path().join("measure.json");
    let result = Command::new(XTASK)
        .args(["migration", "measure", "--step", "rust-03"])
        .args(["--python", "python", "--out"])
        .arg(&out)
        .args(["--pairs", "1", "--test-allow-few-pairs", "--test-candidate"])
        .arg(&candidate)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    refused(result, "--python must be an absolute path", &out);
}

#[test]
fn fewer_than_thirty_pairs_are_refused_without_the_test_flag() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("measure.json");
    let result = Command::new(XTASK)
        .args(["migration", "measure", "--step", "rust-03", "--python"])
        .arg(python())
        .arg("--out")
        .arg(&out)
        .args(["--pairs", "29"])
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    refused(
        result,
        "--pairs 29 is below the protocol minimum of 30",
        &out,
    );
}

#[test]
fn zero_pairs_are_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let candidate = script(scratch.path(), "exit 0");
    let out = scratch.path().join("measure.json");
    let result = with_candidate("rust-03", &out, "0", &candidate)
        .output()
        .unwrap();
    refused(result, "--pairs must be at least 1", &out);
}

#[test]
fn relative_out_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let candidate = script(scratch.path(), "exit 0");
    let out = Path::new("measure.json");
    let result = with_candidate("rust-03", out, "1", &candidate)
        .output()
        .unwrap();
    refused(result, "--out must be an absolute path", out);
}

#[test]
fn empty_careerbrew_home_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let candidate = script(scratch.path(), "exit 0");
    let out = scratch.path().join("measure.json");
    let result = with_candidate("rust-03", &out, "1", &candidate)
        .env("CAREERBREW_HOME", "")
        .output()
        .unwrap();
    refused(result, "CAREERBREW_HOME is set but empty", &out);
}

#[test]
fn test_candidate_without_the_test_flag_is_refused_at_thirty_pairs() {
    let scratch = tempfile::tempdir().unwrap();
    // The candidate would write a marker if it ever ran.
    let marker = scratch.path().join("ran");
    let candidate = script(scratch.path(), &format!("touch '{}'", marker.display()));
    let out = scratch.path().join("measure.json");
    let result = Command::new(XTASK)
        .args(["migration", "measure", "--step", "rust-03", "--python"])
        .arg(python())
        .arg("--out")
        .arg(&out)
        .args(["--pairs", "30", "--test-candidate"])
        .arg(&candidate)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    refused(
        result,
        "--test-candidate needs --test-allow-few-pairs",
        &out,
    );
    assert!(!marker.exists());
}

#[test]
fn old_runs_flag_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("measure.json");
    let result = Command::new(XTASK)
        .args(["migration", "measure", "--step", "rust-03", "--runs", "30"])
        .arg("--out")
        .arg(&out)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("--runs is replaced by --pairs"));
    assert!(!out.exists());
}

#[test]
fn candidate_change_during_the_runs_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    // The candidate appends a comment to itself, so its bytes change.
    let candidate = script(scratch.path(), "echo '# changed' >> \"$0\"");
    let out = scratch.path().join("measure.json");
    let result = with_candidate("rust-03", &out, "1", &candidate)
        .output()
        .unwrap();
    refused(result, "a relevant source changed during the measure", &out);
}

#[test]
fn existing_out_is_refused_before_any_run() {
    let scratch = tempfile::tempdir().unwrap();
    let marker = scratch.path().join("ran");
    let candidate = script(scratch.path(), &format!("touch '{}'", marker.display()));
    let out = scratch.path().join("measure.json");
    std::fs::write(&out, b"keep").unwrap();
    let result = with_candidate("rust-03", &out, "1", &candidate)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(String::from_utf8_lossy(&result.stderr).contains("measure output already exists"));
    assert_eq!(std::fs::read(&out).unwrap(), b"keep");
    assert!(!marker.exists());
}
