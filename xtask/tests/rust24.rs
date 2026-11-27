//! Registration, evidence identity, write and allowlist controls for rust-24.

use std::path::Path;
use std::process::Command;

#[test]
fn rust_24_registers_the_release_candidate_and_effective_inputs() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = std::fs::read_to_string(manifest.parent().unwrap().join("Cargo.toml")).unwrap();
    assert!(workspace.contains("crates/reports"));
    let baseline = std::fs::read_to_string(manifest.join("src/baseline.rs")).unwrap();
    assert!(baseline.contains("id: \"rust-24\""));
    assert!(baseline.contains("target/release/reports-component"));
    for source in [
        "tests/oracles/rust24.py",
        "src/careerbrew/repo_health.py",
        "src/careerbrew/report.py",
        "src/careerbrew/telemetry.py",
        "src/careerbrew/events.py",
        "src/careerbrew/time_saved.py",
        "src/careerbrew/field.py",
        "src/careerbrew/charts.py",
        "src/careerbrew/checklist.py",
        "crates/cbcli/registry/registry.json",
        "crates/reports/src/repo_health.rs",
        "crates/reports/src/telemetry.rs",
        "crates/reports/src/allowlists.rs",
        "crates/reports/src/time_saved.rs",
        "crates/reports/src/field.rs",
        "crates/reports/src/charts.rs",
        "crates/reports/src/checklist.rs",
        "crates/reports/src/synthetic.rs",
        "crates/reports/src/bin/reports-component.rs",
        "crates/policy/src/lib.rs",
        "crates/vault/src/events.rs",
        "xtask/src/rust24.rs",
    ] {
        assert!(baseline.contains(source), "missing source binding {source}");
    }
    let measure = std::fs::read_to_string(manifest.join("src/measure.rs")).unwrap();
    assert!(measure.contains("step: \"rust-24\""));
    assert!(measure.contains("target/release/reports-component"));
    let floors = std::fs::read_to_string(manifest.join("src/noise_floor.rs")).unwrap();
    assert_eq!(floors.matches("step: \"rust-24\"").count(), 2);
}

#[test]
fn rust_24_baseline_verify_measure_and_decide_share_identity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let candidate = root.join("target/release/reports-component");
    let build = Command::new("cargo")
        .args([
            "build",
            "--locked",
            "--offline",
            "--release",
            "-p",
            "careerbrew-reports",
            "--bin",
            "reports-component",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    assert!(candidate.is_file());
    let python = root.join(".venv/bin/python");
    let scratch = tempfile::tempdir().unwrap();
    let base = scratch.path().join("baseline.json");
    let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(["migration", "baseline", "--step", "rust-24", "--python"])
        .arg(&python)
        .arg("--out")
        .arg(&base)
        .args(["--runs", "1", "--test-allow-few-runs"])
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    let verified = scratch.path().join("verify.json");
    let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(["migration", "verify", "--step", "rust-24", "--python"])
        .arg(&python)
        .arg("--out")
        .arg(&verified)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    let measured = scratch.path().join("measure.json");
    let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(["migration", "measure", "--step", "rust-24", "--python"])
        .arg(&python)
        .arg("--out")
        .arg(&measured)
        .args(["--pairs", "1", "--test-allow-few-pairs"])
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    let out = scratch.path().join("decide.json");
    let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(["migration", "decide", "--step", "rust-24", "--baseline"])
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
    assert_eq!(verdict["step"], "rust-24");
    assert_eq!(verdict["decision"], "keep-python");
    let reasons = verdict["reasons"].as_array().unwrap();
    assert!(reasons.contains(&serde_json::json!("baseline-below-protocol-minimum")));
    assert!(reasons.contains(&serde_json::json!("too-few-unique-pairs")));

    for (plant, field, failed_case) in [
        ("extra", "side_effect", "component:repo-health"),
        ("missing", "missing_writes", "component:repo-health"),
        ("allowlist", "contract", "component:telemetry-allowlist"),
    ] {
        let planted = scratch.path().join(format!("verify-{plant}.json"));
        let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
            .args(["migration", "verify", "--step", "rust-24", "--python"])
            .arg(&python)
            .arg("--out")
            .arg(&planted)
            .args(["--test-plant-home-write", plant])
            .env_remove("CAREERBREW_HOME")
            .output()
            .unwrap();
        assert_eq!(run.status.code(), Some(1), "{plant}: {run:?}");
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&planted).unwrap()).unwrap();
        assert!(!record["passed"].as_bool().unwrap());
        assert!(
            record["violations"][field].as_u64().unwrap() > 0,
            "{plant}: {record}"
        );
        let failed = record["failed_cases"].as_array().unwrap();
        assert!(failed.contains(&serde_json::json!(failed_case)));
        let verdict_path = scratch.path().join(format!("decide-{plant}.json"));
        let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
            .args(["migration", "decide", "--step", "rust-24", "--baseline"])
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
