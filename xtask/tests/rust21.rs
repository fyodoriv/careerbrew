//! Registration, evidence identity and planted controls for rust-21.

use std::path::Path;
use std::process::Command;

#[test]
fn rust_21_registers_the_release_candidate_and_effective_inputs() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let baseline = std::fs::read_to_string(manifest.join("src/baseline.rs")).unwrap();
    assert!(baseline.contains("id: \"rust-21\""));
    assert!(baseline.contains("target/release/teacher-component"));
    for source in [
        "tests/oracles/rust21.py",
        "src/careerbrew/teacher.py",
        "src/careerbrew/goals.py",
        "src/careerbrew/approvals.py",
        "src/careerbrew/tutor/exercises.py",
        "src/careerbrew/tutor/mistakes.py",
        "src/careerbrew/tutor/engine.py",
        "crates/teacher/src/goals.rs",
        "crates/teacher/src/seams.rs",
        "crates/teacher/src/teacher.rs",
        "crates/teacher/src/synthetic.rs",
        "crates/teacher/src/bin/teacher-component.rs",
        "crates/tutor/src/cards.rs",
        "crates/tutor/src/mistakes.rs",
        "crates/policy/src/approvals.rs",
        "tests/fixtures/rust21/runner.json",
        "tests/fixtures/rust21/loops.json",
        "knowledge/teaching.yaml",
        "xtask/src/rust21.rs",
    ] {
        assert!(baseline.contains(source), "missing source binding {source}");
    }
    let measure = std::fs::read_to_string(manifest.join("src/measure.rs")).unwrap();
    assert!(measure.contains("step: \"rust-21\""));
    assert!(measure.contains("target/release/teacher-component"));
    let floors = std::fs::read_to_string(manifest.join("src/noise_floor.rs")).unwrap();
    assert_eq!(floors.matches("step: \"rust-21\"").count(), 2);
}

fn xtask(args: &[&str], python: &Path, out: &Path, extra: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(args)
        .arg("--python")
        .arg(python)
        .arg("--out")
        .arg(out)
        .args(extra)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap()
}

#[test]
fn rust_21_baseline_verify_measure_and_decide_share_identity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let candidate = root.join("target/release/teacher-component");
    let build = Command::new("cargo")
        .args([
            "build",
            "--locked",
            "--offline",
            "--release",
            "-p",
            "careerbrew-teacher",
            "--bin",
            "teacher-component",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    assert!(candidate.is_file());
    let python = root.join(".venv/bin/python");
    let scratch = tempfile::tempdir().unwrap();
    let base = scratch.path().join("baseline.json");
    let run = xtask(
        &["migration", "baseline", "--step", "rust-21"],
        &python,
        &base,
        &["--runs", "1", "--test-allow-few-runs"],
    );
    assert!(run.status.success(), "{run:?}");
    let verified = scratch.path().join("verify.json");
    let run = xtask(
        &["migration", "verify", "--step", "rust-21"],
        &python,
        &verified,
        &[],
    );
    assert!(run.status.success(), "{run:?}");
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&verified).unwrap()).unwrap();
    assert_eq!(record["case_count"], 7, "{record}");
    assert_eq!(record["passed_count"], 7, "{record}");
    let measured = scratch.path().join("measure.json");
    let run = xtask(
        &["migration", "measure", "--step", "rust-21"],
        &python,
        &measured,
        &["--pairs", "1", "--test-allow-few-pairs"],
    );
    assert!(run.status.success(), "{run:?}");
    let out = scratch.path().join("decide.json");
    let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(["migration", "decide", "--step", "rust-21", "--baseline"])
        .arg(&base)
        .arg("--verify")
        .arg(&verified)
        .arg("--measure")
        .arg(&measured)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(1), "{run:?}");
    let verdict: serde_json::Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(verdict["step"], "rust-21");
    assert_eq!(verdict["decision"], "keep-python");
    let reasons = verdict["reasons"].as_array().unwrap();
    assert!(reasons.contains(&serde_json::json!("baseline-below-protocol-minimum")));
    assert!(reasons.contains(&serde_json::json!("too-few-unique-pairs")));

    // Planted controls: each must fail verify and block activation.
    for (plant, field, failed_case) in [
        ("extra", "side_effect", "component:goals-queue"),
        ("missing", "missing_writes", "component:teacher-drafts"),
        ("utc-day", "contract", "component:teacher-local-day"),
    ] {
        let planted = scratch.path().join(format!("verify-{plant}.json"));
        let run = xtask(
            &["migration", "verify", "--step", "rust-21"],
            &python,
            &planted,
            &["--test-plant-home-write", plant],
        );
        assert_eq!(run.status.code(), Some(1), "{plant}: {run:?}");
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&planted).unwrap()).unwrap();
        assert!(!record["passed"].as_bool().unwrap());
        assert!(
            record["violations"][field].as_u64().unwrap() > 0,
            "{plant}: {record}"
        );
        let failed = record["failed_cases"].as_array().unwrap();
        assert!(
            failed.contains(&serde_json::json!(failed_case)),
            "{plant}: {record}"
        );
        let verdict_path = scratch.path().join(format!("decide-{plant}.json"));
        let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
            .args(["migration", "decide", "--step", "rust-21", "--baseline"])
            .arg(&base)
            .arg("--verify")
            .arg(&planted)
            .arg("--measure")
            .arg(&measured)
            .arg("--out")
            .arg(&verdict_path)
            .output()
            .unwrap();
        assert_eq!(run.status.code(), Some(1), "{plant}: {run:?}");
        let verdict: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&verdict_path).unwrap()).unwrap();
        assert_eq!(verdict["decision"], "keep-python");
        assert!(verdict["reasons"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("verify-violation")));
    }
}
