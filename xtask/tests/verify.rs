//! Controls for `migration verify`: a registered step with no Rust candidate
//! is refused; with the test-only candidate flag, a candidate that matches the
//! oracle passes, one with a different stdout fails naming the case, and one
//! with the same stdout but a nonzero exit fails, and one that prints
//! invalid UTF-8 fails its case with the record written. Refusals: an unknown
//! step, a relative `--out`, an oracle that exits nonzero and an empty
//! CAREERBREW_HOME. The passing test also checks that a second run refuses
//! its existing output. Each test owns its temp folder.
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

fn verify(step: &str, out: &Path) -> Command {
    let mut command = Command::new(XTASK);
    command
        .args(["migration", "verify", "--step", step, "--python"])
        .arg(python())
        .arg("--out")
        .arg(out)
        .env_remove("CAREERBREW_HOME");
    command
}

/// Writes an executable shell candidate with the given body.
fn candidate(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("candidate");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn read(out: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap()
}

#[test]
fn a_step_with_no_candidate_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let result = verify("rust-03", &out).output().unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr)
        .contains("step rust-03 has no Rust candidate yet; verify refused"));
    assert!(!out.exists());
}

#[test]
fn a_candidate_identical_to_the_oracle_passes() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let same = candidate(
        scratch.path(),
        &format!("exec '{}' -B -m careerbrew \"$@\"", python().display()),
    );
    let result = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&same)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout)
        .contains("step=rust-03 cases=1 passed=1 failed=none"));
    let value = read(&out);
    assert_eq!(value["schema"], "careerbrew-migration-verify-v1");
    assert_eq!(value["step"], "rust-03");
    assert_eq!(value["case_count"], 1);
    assert_eq!(value["passed"], true);
    assert_eq!(value["cases"][0]["case"], "actions-json");
    assert_eq!(value["cases"][0]["passed"], true);
    assert_eq!(value["source_revision"].as_str().unwrap().len(), 40);
    assert_eq!(value["sources_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(value["test_candidate"], true);
    assert_eq!(value["candidate_sha256"].as_str().unwrap().len(), 64);
    // Content-free: no local path of this machine.
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains(scratch.path().to_str().unwrap()));
    assert!(!text.contains(root().to_str().unwrap()));
    // A second run never overwrites a record.
    let before = std::fs::read(&out).unwrap();
    let again = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&same)
        .output()
        .unwrap();
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stderr).contains("verify output already exists"));
    assert_eq!(std::fs::read(&out).unwrap(), before);
}

#[test]
fn a_planted_different_output_fails_naming_the_case() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let wrong = candidate(scratch.path(), "printf '[]'");
    let result = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&wrong)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("actions-json"));
    let value = read(&out);
    assert_eq!(value["passed"], false);
    assert_eq!(value["failed_cases"][0], "actions-json");
    assert_eq!(value["cases"][0]["stdout_matches"], false);
    assert_eq!(value["cases"][0]["exit_matches"], true);
}

#[test]
fn a_planted_nonzero_candidate_fails() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let nonzero = candidate(
        scratch.path(),
        &format!("'{}' -B -m careerbrew \"$@\"\nexit 3", python().display()),
    );
    let result = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&nonzero)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    let value = read(&out);
    assert_eq!(value["passed"], false);
    assert_eq!(value["cases"][0]["exit_matches"], false);
    assert_eq!(value["cases"][0]["stdout_matches"], true);
}

#[test]
fn an_unknown_step_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let result = verify("rust-99", &out).output().unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("unknown step id"));
    assert!(!out.exists());
}

#[test]
fn a_relative_out_is_refused() {
    let result = verify("rust-03", Path::new("verify.json"))
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("--out must be an absolute path"));
}

#[test]
fn a_candidate_with_invalid_utf8_output_fails_its_case() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let bad = candidate(scratch.path(), "printf '\\377\\376'");
    let result = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&bad)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("actions-json"));
    let value = read(&out);
    assert_eq!(value["passed"], false);
    assert_eq!(value["failed_cases"][0], "actions-json");
    assert_eq!(value["cases"][0]["candidate_ran"], false);
    assert_eq!(value["cases"][0]["passed"], false);
}

#[test]
fn a_nonzero_oracle_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let python = scratch.path().join("python");
    std::fs::write(&python, "#!/bin/sh\nexit 7\n").unwrap();
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o700)).unwrap();
    let same = candidate(scratch.path(), "exit 0");
    let out = scratch.path().join("verify.json");
    let result = Command::new(XTASK)
        .args(["migration", "verify", "--step", "rust-03", "--python"])
        .arg(&python)
        .arg("--out")
        .arg(&out)
        .arg("--test-candidate")
        .arg(&same)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("did not exit 0"));
    assert!(!out.exists());
}

#[test]
fn an_empty_careerbrew_home_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let result = verify("rust-03", &out)
        .env("CAREERBREW_HOME", "")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("CAREERBREW_HOME is set but empty"));
    assert!(!out.exists());
}
