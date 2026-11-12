pub(crate) mod capture_actions {
    include!("../../src/capture_actions.rs");
}
mod capture_http {
    include!("../../src/capture_http.rs");
    pub fn fault(
        root: &std::path::Path,
        python: &std::path::Path,
        output: &std::path::Path,
        fault: &str,
    ) -> Result<(), String> {
        run_mode(root, python, output, fault)
    }
    pub fn publication(
        root: &std::path::Path,
        python: &std::path::Path,
        pid_path: &std::path::Path,
        fault: &str,
    ) -> Result<serde_json::Value, String> {
        let root = root.canonicalize().unwrap();
        if std::fs::read(root.join("xtask/tests/fixtures/capture/http-v1.json")).unwrap()
            != CASE.as_bytes()
            || std::fs::read(root.join("xtask/tests/fixtures/capture/http-reference-v1.json"))
                .unwrap()
                != HISTORICAL_REFERENCE.as_bytes()
            || shared::digest(HISTORICAL_REFERENCE.as_bytes()) != HISTORICAL_REFERENCE_SHA256
            || std::fs::read(
                root.join("xtask/tests/fixtures/capture/http-reference-rules-promise-v3.json"),
            )
            .unwrap()
                != REFERENCE.as_bytes()
            || shared::digest(REFERENCE.as_bytes()) != REFERENCE_SHA256
        {
            return Err("HTTP control input identity differs".into());
        }
        let fixture: serde_json::Value = serde_json::from_str(CASE).unwrap();
        let sources = shared::sources(&root)?;
        let result = observe(&root, python, &fixture, &sources, fault, pid_path);
        if sources != shared::sources(&root)? {
            return Err("HTTP control sources changed".into());
        }
        if let Ok(execution) = &result {
            let reference: serde_json::Value = serde_json::from_str(REFERENCE).unwrap();
            if projection(execution) != reference || execution["catalog_reference_matches"] != true
            {
                return Err("HTTP control frozen reference differs".into());
            }
        }
        result
    }
}

use std::{path::Path, process::Command};

#[allow(dead_code)] // This HTTP-only control is not part of the DATA control group.
pub fn rejects_changed_reference_inputs_before_python() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = tempfile::tempdir().unwrap();
    let shadow = scratch.path().join("source");
    std::fs::create_dir(&shadow).unwrap();
    let inputs = capture_actions::sources(root()).unwrap();
    for name in inputs.as_object().unwrap().keys() {
        let destination = shadow.join(name);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::copy(root().join(name), destination).unwrap();
    }
    let python = scratch.path().join("python");
    let marker = scratch.path().join("python-ran");
    std::fs::write(
        &python,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(capture_actions::capture(&shadow, &python).is_err());
    assert!(
        marker.exists(),
        "pristine action capture did not launch Python"
    );
    std::fs::remove_file(&marker).unwrap();
    assert!(capture_http::run(&shadow, &python, &scratch.path().join("http.json")).is_err());
    assert!(
        marker.exists(),
        "pristine HTTP capture did not launch Python"
    );
    std::fs::remove_file(&marker).unwrap();
    for name in [
        "docs/plans/2026-11-08-promise-action-delta.json",
        "docs/plans/2026-11-08-rules-action-delta.json",
        "xtask/tests/fixtures/capture/actions-reference-rules-v2.json",
        "xtask/tests/fixtures/capture/http-reference-rules-v2.json",
        "xtask/tests/fixtures/capture/actions-reference-rules-promise-v3.json",
        "xtask/tests/fixtures/capture/http-reference-rules-promise-v3.json",
        "xtask/tests/fixtures/capture/actions-reference-v1.json",
        "xtask/tests/fixtures/capture/actions-reference-promise-v2.json",
        "xtask/tests/fixtures/capture/http-reference-v1.json",
        "xtask/tests/fixtures/capture/http-reference-promise-v2.json",
    ] {
        let path = shadow.join(name);
        let original = std::fs::read(&path).unwrap();
        std::fs::write(&path, b"changed synthetic reference").unwrap();
        assert_eq!(
            capture_actions::capture(&shadow, &python).unwrap_err(),
            "reference input differs",
            "{name}"
        );
        assert_eq!(
            capture_http::run(&shadow, &python, &scratch.path().join("http.json")).unwrap_err(),
            "reference input differs",
            "{name}"
        );
        assert!(!marker.exists(), "Python launched for {name}");
        std::fs::write(&path, original).unwrap();
    }
}

pub fn captures_actual_twenty_one_case_handler_corpus() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let output = scratch.path().join("http.json");
    let result = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(["migration", "capture-http", "--python"])
        .arg(root.join(".venv/bin/python"))
        .arg("--out")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual HTTP capture must succeed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert!(
        capture_http::run(root, &root.join(".venv/bin/python"), &output)
            .unwrap_err()
            .contains("already exists")
    );
    assert_eq!(receipt["qualification"], "bounded-handler-contract");
    assert_eq!(receipt["executions"].as_array().unwrap().len(), 2);
    for execution in receipt["executions"].as_array().unwrap() {
        assert_eq!(execution["responses"].as_array().unwrap().len(), 21);
        assert_eq!(execution["exit"], 0);
        assert!(execution["pid"].as_u64().unwrap() > 0);
    }
}

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}
fn fault(name: &str) -> String {
    let scratch = tempfile::tempdir().unwrap();
    let output = scratch.path().join("http.json");
    let error =
        capture_http::fault(root(), &root().join(".venv/bin/python"), &output, name).unwrap_err();
    assert!(
        !output.exists(),
        "rejected execution must not publish a qualified receipt"
    );
    error
}
pub fn rejects_real_catalog_removal_in_both_servers() {
    let error = fault("remove-action");
    eprintln!("{error}");
    assert!(error.contains("action reference differs"));
    let evidence = error
        .split_once("; completed HTTP executions: ")
        .expect("catalog mismatch must follow two actual completed executions")
        .1;
    let evidence: serde_json::Value = serde_json::from_str(evidence).unwrap();
    let executions = evidence.as_array().unwrap();
    assert_eq!(executions.len(), 2);
    assert!(executions[0]["pid"].as_u64().unwrap() > 0);
    assert!(executions[1]["pid"].as_u64().unwrap() > 0);
    assert_ne!(executions[0]["pid"], executions[1]["pid"]);
    for execution in executions {
        assert_eq!(execution["exit"], 0);
        assert_eq!(execution["dispatches"], 21);
        assert_eq!(execution["responses"], 21);
    }
}
pub fn rejects_real_weakened_header() {
    assert!(fault("weak-header").contains("security header differs"));
}
pub fn rejects_real_authentication_bypass() {
    assert!(fault("auth-bypass").contains("status differs"));
}
pub fn rejects_real_wrong_loader_root() {
    assert!(fault("wrong-root").contains("outside checkout"));
}
pub fn rejects_real_served_source_change() {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for item in std::fs::read_dir(from).unwrap() {
            let item = item.unwrap();
            let source = item.path();
            let dest = to.join(item.file_name());
            if source.is_dir() {
                copy(&source, &dest);
            } else {
                std::fs::copy(source, dest).unwrap();
            }
        }
    }
    let scratch = tempfile::tempdir().unwrap();
    let shadow = scratch.path().join("source");
    std::fs::create_dir(&shadow).unwrap();
    for folder in ["src", "xtask"] {
        copy(&root().join(folder), &shadow.join(folder));
    }
    for name in ["pyproject.toml", "uv.lock", "Cargo.lock"] {
        std::fs::copy(root().join(name), shadow.join(name)).unwrap();
    }
    std::fs::create_dir_all(shadow.join("docs/plans")).unwrap();
    std::fs::copy(
        root().join("docs/plans/2026-11-08-promise-action-delta.json"),
        shadow.join("docs/plans/2026-11-08-promise-action-delta.json"),
    )
    .unwrap();
    std::fs::copy(
        root().join("docs/plans/2026-11-08-rules-action-delta.json"),
        shadow.join("docs/plans/2026-11-08-rules-action-delta.json"),
    )
    .unwrap();
    std::fs::write(
        shadow.join(".synthetic-http-source-copy"),
        b"owned synthetic source",
    )
    .unwrap();
    let output = scratch.path().join("http.json");
    let error = capture_http::fault(
        &shadow,
        &root().join(".venv/bin/python"),
        &output,
        "served-source",
    )
    .unwrap_err();
    assert!(error.contains("module hash differs"), "{error}");
    assert!(!output.exists());
}
pub fn rejects_real_stdout_canary() {
    assert!(fault("stdout-canary").contains("canary leaked"));
}
pub fn rejects_real_stderr_canary() {
    assert!(fault("stderr-canary").contains("canary leaked"));
}
pub fn rejects_real_retained_canary() {
    assert!(fault("retained-canary").contains("retained content"));
}
pub fn rejects_real_extra_write() {
    assert!(fault("extra-write").contains("retained owned writes differ"));
}
pub fn retains_real_skipped_and_nonzero_exit_results() {
    assert!(fault("skip").contains("observed child exit Some(0)"));
    assert!(fault("nonzero").contains("observed child exit Some(17)"));
}
pub fn rejects_real_wire_defects_and_reaps_hanging_handler() {
    for (mode, expected) in [
        ("bad-status", "status invalid"),
        ("invalid-date", "Date invalid"),
        ("invalid-weekday", "Date invalid"),
        ("malformed-header", "header malformed"),
        ("duplicate-length", "duplicate HTTP header"),
        ("transfer-encoding", "transfer encoding unsupported"),
        ("truncated-body", "body length differs"),
        ("oversized-response", "response exceeds limit"),
    ] {
        let error = fault(mode);
        assert!(error.contains(expected), "{mode}: {error}");
    }
    let scratch = tempfile::tempdir().unwrap();
    let output = scratch.path().join("http.json");
    let error =
        capture_http::fault(root(), &root().join(".venv/bin/python"), &output, "hang").unwrap_err();
    assert!(error.contains("exceeded timeout"), "{error}");
    assert!(error.contains("observed child exit None"), "{error}");
    let pid: i32 = std::fs::read_to_string(output.with_extension("pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "owned direct child must be reaped"
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
    assert!(!output.exists());
}
pub fn preserves_shared_cli_capture_contract() {
    let scratch = tempfile::tempdir().unwrap();
    capture_actions::run(
        root(),
        &root().join(".venv/bin/python"),
        &scratch.path().join("actions.json"),
    )
    .unwrap();
}

fn assert_publication_child_reaped(evidence: &serde_json::Value) -> u32 {
    let actual = evidence["actual_parent_observed_pid"].as_u64().unwrap();
    let pid = i32::try_from(actual).unwrap();
    assert!(pid > 0);
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let waited = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    let wait_error = std::io::Error::last_os_error().raw_os_error();
    let exists = unsafe { libc::kill(pid, 0) };
    let existence_error = std::io::Error::last_os_error().raw_os_error();
    eprintln!("publication-child-postconditions: pid={pid} waitid_wnowait={waited} wait_errno={wait_error:?} existence={exists} existence_errno={existence_error:?}");
    assert_eq!(waited, -1);
    assert_eq!(wait_error, Some(libc::ECHILD));
    assert_eq!(exists, -1);
    assert_eq!(existence_error, Some(libc::ESRCH));
    u32::try_from(actual).unwrap()
}

pub fn publishes_complete_readiness_after_paused_real_write() {
    let scratch = tempfile::tempdir().unwrap();
    let pid_path = scratch.path().join("child.pid");
    let result = capture_http::publication(
        root(),
        &root().join(".venv/bin/python"),
        &pid_path,
        "paused-readiness",
    );
    let evidence: serde_json::Value = serde_json::from_slice(
        &std::fs::read(pid_path.with_extension("publication.json")).unwrap(),
    )
    .unwrap();
    eprintln!("actual-publication-observation: {evidence}");
    if let Err(error) = &result {
        eprintln!("actual-publication-refusal: {error}");
    }
    let pid = assert_publication_child_reaped(&evidence);
    let execution = result.expect("complete readiness must follow the actual paused write");
    eprintln!(
        "actual-publication-completed: {}",
        serde_json::json!({"pid":execution["pid"],"actual_exit":execution["exit"],"server":execution["server"],"dispatches":execution["requests"],"response_count":execution["responses"].as_array().unwrap().len(),"modules":execution["observer"]["modules"],"retained_preserved":execution["retained_before"] == execution["retained_after"]})
    );
    assert_eq!(evidence["final_present_while_paused"], false);
    assert_eq!(evidence["acknowledged"], true);
    assert_eq!(execution["pid"], pid);
    assert_eq!(execution["exit"], 0);
    assert_eq!(execution["responses"].as_array().unwrap().len(), 21);
    assert_eq!(execution["requests"].as_array().unwrap().len(), 21);
}

pub fn rejects_fully_published_malformed_readiness() {
    let scratch = tempfile::tempdir().unwrap();
    let pid_path = scratch.path().join("child.pid");
    let result = capture_http::publication(
        root(),
        &root().join(".venv/bin/python"),
        &pid_path,
        "malformed-readiness",
    );
    let evidence: serde_json::Value = serde_json::from_slice(
        &std::fs::read(pid_path.with_extension("publication.json")).unwrap(),
    )
    .unwrap();
    eprintln!("actual-publication-observation: {evidence}");
    assert_publication_child_reaped(&evidence);
    assert_eq!(evidence["closed_publication"], true);
    assert_eq!(
        evidence["actual_final_bytes"],
        serde_json::json!(b"{\"pid\":")
    );
    let error = result.unwrap_err();
    eprintln!("actual-publication-refusal: {error}");
    assert!(error.starts_with("HTTP readiness malformed; "), "{error}");
    assert!(!error.contains("cleanup failed"), "{error}");
}
