//! Registration and evidence-identity controls for rust-06.

use std::path::Path;
use std::process::Command;

#[test]
fn rust_06_registers_the_release_candidate_and_effective_inputs() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let baseline = std::fs::read_to_string(manifest.join("src/baseline.rs")).unwrap();
    assert!(baseline.contains("id: \"rust-06\""));
    assert!(baseline.contains("target/release/vault-sync"));
    for source in [
        "tests/oracles/rust06.py",
        "src/careerbrew/backup.py",
        "src/careerbrew/sync.py",
        "src/careerbrew/cli.py",
        "crates/vault/src/backup.rs",
        "crates/vault/src/sync.rs",
        "crates/vault/src/transfer.rs",
        "crates/vault/src/synthetic_transfer.rs",
        "crates/vault/src/bin/vault-sync.rs",
        "xtask/src/rust06.rs",
        "knowledge/ladders/generic-swe.yaml",
    ] {
        assert!(baseline.contains(source), "missing source binding {source}");
    }
    let measure = std::fs::read_to_string(manifest.join("src/measure.rs")).unwrap();
    assert!(measure.contains("step: \"rust-06\""));
    assert!(measure.contains("target/release/vault-sync"));
    let floors = std::fs::read_to_string(manifest.join("src/noise_floor.rs")).unwrap();
    assert_eq!(floors.matches("step: \"rust-06\"").count(), 2);
}

#[test]
fn rust_06_baseline_verify_measure_and_decide_share_identity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let candidate = root.join("target/release/vault-sync");
    let build = Command::new("cargo")
        .args([
            "build",
            "--locked",
            "--offline",
            "--release",
            "-p",
            "careerbrew-vault",
            "--bin",
            "vault-sync",
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
        .args(["migration", "baseline", "--step", "rust-06", "--python"])
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
        .args(["migration", "verify", "--step", "rust-06", "--python"])
        .arg(&python)
        .arg("--out")
        .arg(&verified)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    let measured = scratch.path().join("measure.json");
    let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(["migration", "measure", "--step", "rust-06", "--python"])
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
        .args(["migration", "decide", "--step", "rust-06", "--baseline"])
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
    assert_eq!(verdict["step"], "rust-06");
    assert_eq!(verdict["decision"], "keep-python");
    let reasons = verdict["reasons"].as_array().unwrap();
    assert!(reasons.contains(&serde_json::json!("baseline-below-protocol-minimum")));
    assert!(reasons.contains(&serde_json::json!("too-few-unique-pairs")));

    for (plant, field, failed_case) in [
        ("extra", "side_effect", "component:archive-roundtrip"),
        ("missing", "missing_writes", "component:archive-roundtrip"),
        (
            "duplicate",
            "duplicate_writes",
            "component:archive-rejections",
        ),
    ] {
        let planted = scratch.path().join(format!("verify-{plant}.json"));
        let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
            .args(["migration", "verify", "--step", "rust-06", "--python"])
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
            .args(["migration", "decide", "--step", "rust-06", "--baseline"])
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
