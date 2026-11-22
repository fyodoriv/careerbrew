//! Python/Rust differential controls for rust-27b browser-test receipts.
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Output};

const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");

fn report(executed: bool) -> Value {
    let test = if executed {
        json!({
            "projectId": "chromium",
            "expectedStatus": "passed",
            "status": "expected",
            "results": [{"status": "passed", "retry": 0}]
        })
    } else {
        json!({"projectId": "chromium", "expectedStatus": "passed"})
    };
    json!({
        "errors": [],
        "suites": [{"specs": [{"id": "us-72", "tests": [test]}], "suites": []}]
    })
}

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn write_reports(dir: &Path, discovery: &Value, execution: &Value) {
    std::fs::write(
        dir.join("discovery.json"),
        serde_json::to_vec(discovery).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("execution.json"),
        serde_json::to_vec(execution).unwrap(),
    )
    .unwrap();
}

fn python(dir: &Path, receipt: Option<&Path>) -> Output {
    let mut command = Command::new(root().join(".venv/bin/python"));
    command
        .arg(root().join("scripts/browser-test-receipts.py"))
        .arg(dir.join("discovery.json"))
        .arg(dir.join("execution.json"))
        .env_remove("CAREERBREW_BROWSER_RECEIPT");
    if let Some(path) = receipt {
        command.env("CAREERBREW_BROWSER_RECEIPT", path);
    }
    command.output().unwrap()
}

fn native(dir: &Path, receipt: Option<&Path>) -> Output {
    let mut command = Command::new(XTASK);
    command
        .arg("browser-test-receipts")
        .arg(dir.join("discovery.json"))
        .arg(dir.join("execution.json"))
        .env_remove("CAREERBREW_BROWSER_RECEIPT");
    if let Some(path) = receipt {
        command.env("CAREERBREW_BROWSER_RECEIPT", path);
    }
    command.output().unwrap()
}

fn same(discovery: Value, execution: Value) -> Output {
    let dir = tempfile::tempdir().unwrap();
    write_reports(dir.path(), &discovery, &execution);
    let expected = python(dir.path(), None);
    let actual = native(dir.path(), None);
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
    assert_eq!(actual.status.code(), expected.status.code());
    actual
}

#[test]
fn matching_reports_pass_with_the_exact_success_line() {
    let out = same(report(false), report(true));
    assert!(out.status.success());
    assert_eq!(
        out.stdout,
        b"browser test receipts: all discovered tests passed once\n"
    );
    assert!(out.stderr.is_empty());
}

#[test]
fn wrong_argument_count_matches_the_usage_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    write_reports(dir.path(), &report(false), &report(true));
    let mut expected = Command::new(root().join(".venv/bin/python"));
    let expected = expected
        .arg(root().join("scripts/browser-test-receipts.py"))
        .output()
        .unwrap();
    let actual = Command::new(XTASK)
        .arg("browser-test-receipts")
        .output()
        .unwrap();
    assert_eq!(actual.status.code(), Some(1));
    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
    assert_eq!(
        actual.stderr,
        b"browser test receipts: need discovery and execution reports\n"
    );
}

#[test]
fn duplicate_and_empty_identities_fail_on_both() {
    let mut discovery = report(false);
    let suite = discovery["suites"][0].clone();
    discovery["suites"] = json!([suite.clone(), suite]);
    assert_eq!(same(discovery, report(true)).status.code(), Some(1));

    let mut discovery = report(false);
    discovery["suites"] = json!([]);
    let mut execution = report(true);
    execution["suites"] = json!([]);
    assert_eq!(same(discovery, execution).status.code(), Some(1));
}

#[test]
fn missing_duplicate_failed_and_retried_execution_results_fail_on_both() {
    for mutate in [
        (None, None),
        (Some("failed"), Some(0)),
        (Some("passed"), Some(1)),
    ] {
        let mut execution = report(true);
        if let Some(status) = mutate.0 {
            execution["suites"][0]["specs"][0]["tests"][0]["results"][0]["status"] = json!(status);
        }
        if let Some(retry) = mutate.1 {
            execution["suites"][0]["specs"][0]["tests"][0]["results"][0]["retry"] = json!(retry);
        } else {
            execution["suites"][0]["specs"][0]["tests"][0]["results"] = json!([]);
        }
        assert_eq!(same(report(false), execution).status.code(), Some(1));
    }
}

#[test]
fn python_zero_equivalents_remain_accepted() {
    for retry in [json!(0.0), json!(false)] {
        let mut execution = report(true);
        execution["suites"][0]["specs"][0]["tests"][0]["results"][0]["retry"] = retry;
        assert!(same(report(false), execution).status.success());
    }
}

#[test]
fn mismatched_and_malformed_reports_fail_with_one_generic_diagnostic() {
    let mut execution = report(true);
    execution["suites"][0]["specs"][0]["id"] = json!("other");
    let out = same(report(false), execution);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        out.stderr,
        b"browser test receipts: incomplete or mismatched reports\n"
    );

    let dir = tempfile::tempdir().unwrap();
    write_reports(dir.path(), &report(false), &report(true));
    std::fs::write(dir.path().join("execution.json"), b"{").unwrap();
    let expected = python(dir.path(), None);
    let actual = native(dir.path(), None);
    assert_eq!(actual.status.code(), Some(1));
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}

#[test]
fn identity_limits_count_unicode_characters_on_both() {
    let mut discovery = report(false);
    let mut execution = report(true);
    discovery["suites"][0]["specs"][0]["id"] = json!("é".repeat(257));
    execution["suites"][0]["specs"][0]["id"] = json!("é".repeat(257));
    assert_eq!(same(discovery, execution).status.code(), Some(1));

    let mut discovery = report(false);
    let mut execution = report(true);
    let discovery_specs = (0..10_001)
        .map(|id| {
            json!({"id": format!("spec-{id}"), "tests": [{"projectId": "chromium", "expectedStatus": "passed"}]})
        })
        .collect::<Vec<_>>();
    let execution_specs = (0..10_001)
        .map(|id| {
            json!({"id": format!("spec-{id}"), "tests": [{"projectId": "chromium", "expectedStatus": "passed", "status": "expected", "results": [{"status": "passed", "retry": 0}]}]})
        })
        .collect::<Vec<_>>();
    discovery["suites"][0]["specs"] = Value::Array(discovery_specs);
    execution["suites"][0]["specs"] = Value::Array(execution_specs);
    assert_eq!(same(discovery, execution).status.code(), Some(1));
}

#[test]
fn receipts_are_sorted_exact_and_private_on_both() {
    let dir = tempfile::tempdir().unwrap();
    let mut discovery = report(false);
    let mut execution = report(true);
    discovery["suites"][0]["specs"][0]["id"] = json!("é");
    execution["suites"][0]["specs"][0]["id"] = json!("é");
    let mut second_discovery = discovery["suites"][0]["specs"][0].clone();
    let mut second_execution = execution["suites"][0]["specs"][0].clone();
    second_discovery["id"] = json!("alpha");
    second_execution["id"] = json!("alpha");
    discovery["suites"][0]["specs"]
        .as_array_mut()
        .unwrap()
        .push(second_discovery);
    execution["suites"][0]["specs"]
        .as_array_mut()
        .unwrap()
        .push(second_execution);
    write_reports(dir.path(), &discovery, &execution);
    let expected_receipt = dir.path().join("python.json");
    let actual_receipt = dir.path().join("native.json");
    let expected = python(dir.path(), Some(&expected_receipt));
    let actual = native(dir.path(), Some(&actual_receipt));
    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
    assert!(actual.status.success());
    let expected_bytes = std::fs::read(&expected_receipt).unwrap();
    let actual_bytes = std::fs::read(&actual_receipt).unwrap();
    assert_eq!(actual_bytes, expected_bytes);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&actual_receipt)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn existing_files_and_symlinks_are_never_replaced_and_temp_files_are_removed() {
    for symlink in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        write_reports(dir.path(), &report(false), &report(true));
        let destination = dir.path().join("receipt.json");
        if symlink {
            std::fs::write(dir.path().join("target.json"), "keep").unwrap();
            std::os::unix::fs::symlink(dir.path().join("target.json"), &destination).unwrap();
        } else {
            std::fs::write(&destination, "keep").unwrap();
        }
        let out = native(dir.path(), Some(&destination));
        assert_eq!(out.status.code(), Some(1));
        if symlink {
            assert!(destination
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink());
            assert_eq!(
                std::fs::read_to_string(dir.path().join("target.json")).unwrap(),
                "keep"
            );
        } else {
            assert_eq!(std::fs::read_to_string(&destination).unwrap(), "keep");
        }
        assert!(!std::fs::read_dir(dir.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".browser-receipt-")
        }));
    }
}
