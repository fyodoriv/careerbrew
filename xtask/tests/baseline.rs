//! Controls for `migration baseline`: one positive run on the registered
//! step, and three refusals (unknown step, a planted oracle that exits
//! nonzero, an empty CAREERBREW_HOME). Each test owns its temp folder.
use std::path::{Path, PathBuf};
use std::process::Command;

const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn baseline(python: &Path, step: &str, out: &Path, runs: &str) -> Command {
    let mut command = Command::new(XTASK);
    command
        .args(["migration", "baseline", "--step", step, "--python"])
        .arg(python)
        .arg("--out")
        .arg(out)
        .args(["--runs", runs, "--test-allow-few-runs"])
        .env_remove("CAREERBREW_HOME");
    command
}

/// Appends a line to the file `CAREERBREW_RESOURCES_RECORD` names; the hosted
/// gate prints that file, since a passing `cargo test` hides stderr.
fn record(line: &str) {
    eprintln!("{line}");
    if let Some(path) = std::env::var_os("CAREERBREW_RESOURCES_RECORD") {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open resources record");
        writeln!(file, "{line}").expect("write resources record");
    }
}

#[test]
fn registered_step_writes_a_valid_record() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("baseline.json");
    let python = root().join(".venv/bin/python");
    let result = baseline(&python, "rust-03", &out, "3").output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8(result.stdout).unwrap();
    record(&format!("baseline {}", stdout.trim()));
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(value["schema"], "careerbrew-migration-baseline-v1");
    assert_eq!(value["step"], "rust-03");
    assert_eq!(value["runs"], 3);
    assert_eq!(value["command"][0], "<python>");
    assert_eq!(value["cache_state"], "warm-imports-fresh-synthetic-home");
    assert_eq!(value["meets_protocol_minimum"], false);
    assert!(value["source_tree_dirty"].is_boolean());
    assert_eq!(value["fixtures_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(value["source_revision"].as_str().unwrap().len(), 40);
    assert_eq!(value["sources_sha256"].as_str().unwrap().len(), 64);
    assert!(value["python_version"]
        .as_str()
        .unwrap()
        .starts_with("Python 3"));
    let samples = value["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 3);
    for sample in samples {
        assert!(sample["tree_cpu_us"].as_u64().unwrap() > 0);
        assert!(sample["tree_max_rss_bytes"].as_u64().unwrap() > 1024 * 1024);
        assert!(sample["rss_may_include_parent"].is_boolean());
        assert!(sample["parent_max_rss_bytes"].as_u64().is_some());
    }
    let summary = &value["summary"];
    assert!(summary["cpu_us_p50"].as_u64().unwrap() <= summary["cpu_us_p95"].as_u64().unwrap());
    // The record is content-free: no local path of this machine.
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains(scratch.path().to_str().unwrap()));
    assert!(!text.contains(root().to_str().unwrap()));
    // A second run never overwrites a record.
    let before = std::fs::read(&out).unwrap();
    let again = baseline(&python, "rust-03", &out, "1").output().unwrap();
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stderr).contains("baseline output already exists"));
    assert_eq!(std::fs::read(&out).unwrap(), before);
}

#[test]
fn fewer_than_thirty_runs_are_refused_without_the_test_flag() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("baseline.json");
    let result = Command::new(XTASK)
        .args(["migration", "baseline", "--step", "rust-03", "--python"])
        .arg(root().join(".venv/bin/python"))
        .arg("--out")
        .arg(&out)
        .args(["--runs", "29"])
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("below the protocol minimum of 30"));
    assert!(!out.exists());
}

#[test]
fn relative_out_is_refused() {
    let result = Command::new(XTASK)
        .args(["migration", "baseline", "--step", "rust-03", "--python"])
        .arg(root().join(".venv/bin/python"))
        .args([
            "--out",
            "baseline.json",
            "--runs",
            "1",
            "--test-allow-few-runs",
        ])
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("--out must be an absolute path"));
}

#[test]
fn unknown_step_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("baseline.json");
    let result = baseline(&root().join(".venv/bin/python"), "rust-99", &out, "1")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("unknown step id"));
    assert!(!out.exists());
}

#[test]
fn planted_nonzero_oracle_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = tempfile::tempdir().unwrap();
    let python = scratch.path().join("python");
    std::fs::write(
        &python,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'Python 3.0.0'; exit 0; fi\nexit 7\n",
    )
    .unwrap();
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o700)).unwrap();
    let out = scratch.path().join("baseline.json");
    let result = baseline(&python, "rust-03", &out, "2").output().unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("did not exit 0"));
    assert!(!out.exists());
}

#[test]
fn empty_careerbrew_home_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("baseline.json");
    let result = baseline(&root().join(".venv/bin/python"), "rust-03", &out, "1")
        .env("CAREERBREW_HOME", "")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("CAREERBREW_HOME is set but empty"));
    assert!(!out.exists());
}
