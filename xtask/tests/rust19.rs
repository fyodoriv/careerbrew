//! Registration, evidence identity and write-failure controls for rust-19.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

#[test]
fn rust_19_registers_the_release_candidate_and_effective_inputs() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let baseline = std::fs::read_to_string(manifest.join("src/baseline.rs")).unwrap();
    assert!(baseline.contains("id: \"rust-19\""));
    assert!(baseline.contains("target/release/jobs-component"));
    for source in [
        "tests/oracles/rust19.py",
        "src/careerbrew/jobs/sources.py",
        "src/careerbrew/jobs/criteria.py",
        "src/careerbrew/jobs/want.py",
        "src/careerbrew/jobs/linkedin.py",
        "src/careerbrew/machine.py",
        "src/careerbrew/prep.py",
        "crates/jobs/src/sources.rs",
        "crates/jobs/src/criteria.rs",
        "crates/jobs/src/want.rs",
        "crates/jobs/src/linkedin.rs",
        "crates/jobs/src/prep.rs",
        "crates/jobs/src/synthetic.rs",
        "crates/jobs/src/bin/jobs-component.rs",
        "crates/tutor/src/lib.rs",
        "crates/setup/src/machine.rs",
        "crates/vault/src/store.rs",
        "tests/fixtures/ats/greenhouse-northwind.json",
        "knowledge/skills.yaml",
        "xtask/src/rust19.rs",
    ] {
        assert!(baseline.contains(source), "missing source binding {source}");
    }
    let measure = std::fs::read_to_string(manifest.join("src/measure.rs")).unwrap();
    assert!(measure.contains("step: \"rust-19\""));
    assert!(measure.contains("target/release/jobs-component"));
    let floors = std::fs::read_to_string(manifest.join("src/noise_floor.rs")).unwrap();
    assert_eq!(floors.matches("step: \"rust-19\"").count(), 2);
    let compare = std::fs::read_to_string(manifest.join("src/rust19.rs")).unwrap();
    assert!(compare.contains("0.00001"));
    assert!(compare.contains("0.000001"));
    assert!(compare.contains("FLOAT_LEAVES: usize = 140"));
    assert!(compare.contains("total_floats != FLOAT_LEAVES"));
}

#[test]
fn rust_19_baseline_verify_measure_decide_and_write_controls_share_identity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let candidate = root.join("target/release/jobs-component");
    let build = Command::new("cargo")
        .args([
            "build",
            "--locked",
            "--offline",
            "--release",
            "-p",
            "careerbrew-jobs",
            "--bin",
            "jobs-component",
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
        .args(["migration", "baseline", "--step", "rust-19", "--python"])
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
        .args(["migration", "verify", "--step", "rust-19", "--python"])
        .arg(&python)
        .arg("--out")
        .arg(&verified)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&verified).unwrap()).unwrap();
    assert_eq!(record["case_count"], 6);
    assert_eq!(record["passed_count"], 6);
    assert_eq!(record["float_leaves"], 140);
    assert_eq!(record["violations"]["contract"], 0);
    assert_eq!(record["violations"]["privacy"], 0);
    assert!(record["cases"].as_array().unwrap().iter().all(|case| {
        case["comparison"]
            .as_str()
            .unwrap()
            .contains("typed-float-tolerance")
    }));
    let measured = scratch.path().join("measure.json");
    let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(["migration", "measure", "--step", "rust-19", "--python"])
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
        .args(["migration", "decide", "--step", "rust-19", "--baseline"])
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
    assert_eq!(verdict["step"], "rust-19");
    assert_eq!(verdict["decision"], "keep-python");
    let reasons = verdict["reasons"].as_array().unwrap();
    assert!(reasons.contains(&serde_json::json!("baseline-below-protocol-minimum")));
    assert!(reasons.contains(&serde_json::json!("too-few-unique-pairs")));

    for (plant, field, failed_case) in [
        ("extra", "side_effect", "component:helpers"),
        ("missing", "missing_writes", "component:ats-scan"),
        (
            "duplicate",
            "duplicate_writes",
            "component:duplicate-write-control",
        ),
        (
            "event-duplicate",
            "duplicate_writes",
            "component:want-learn",
        ),
    ] {
        let planted = scratch.path().join(format!("verify-{plant}.json"));
        let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
            .args(["migration", "verify", "--step", "rust-19", "--python"])
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
        if plant == "duplicate" {
            assert_eq!(record["violations"]["side_effect"], 0);
            assert_eq!(record["violations"]["missing_writes"], 0);
            assert_eq!(record["violations"]["contract"], 0);
        }
        let failed = record["failed_cases"].as_array().unwrap();
        assert!(failed.contains(&serde_json::json!(failed_case)));
        let verdict_path = scratch.path().join(format!("decide-{plant}.json"));
        let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
            .args(["migration", "decide", "--step", "rust-19", "--baseline"])
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

#[test]
fn rust_19_measure_refuses_an_invalid_all_report() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let candidate = scratch.path().join("empty-candidate.sh");
    std::fs::write(&candidate, "#!/bin/sh\nprintf '{\"cases\":[]}\\n'\n").unwrap();
    let mut permissions = std::fs::metadata(&candidate).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&candidate, permissions).unwrap();
    let output = scratch.path().join("measure.json");
    let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(["migration", "measure", "--step", "rust-19", "--python"])
        .arg(root.join(".venv/bin/python"))
        .arg("--out")
        .arg(&output)
        .args(["--pairs", "1", "--test-allow-few-pairs", "--test-candidate"])
        .arg(&candidate)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert!(!run.status.success(), "{run:?}");
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("rust-19 --all report"), "{stderr}");
}
