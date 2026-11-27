//! Registration, evidence identity and planted controls for rust-16.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn release_candidate() -> PathBuf {
    let root = root();
    let build = Command::new("cargo")
        .args([
            "build",
            "--locked",
            "--offline",
            "--release",
            "-p",
            "careerbrew-workexec",
            "--bin",
            "workexec-component",
        ])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    let candidate = root.join("target/release/workexec-component");
    assert!(candidate.is_file());
    candidate
}

fn verify(python: &Path, out: &Path, plant: Option<&str>) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"));
    command
        .args(["migration", "verify", "--step", "rust-16", "--python"])
        .arg(python)
        .arg("--out")
        .arg(out)
        .env_remove("CAREERBREW_HOME");
    if let Some(plant) = plant {
        command.args(["--test-plant-home-write", plant]);
    }
    command.output().unwrap()
}

#[test]
fn rust_16_registers_the_release_candidate_and_effective_inputs() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = std::fs::read_to_string(manifest.parent().unwrap().join("Cargo.toml")).unwrap();
    assert!(workspace.contains("crates/workexec"));
    let baseline = std::fs::read_to_string(manifest.join("src/baseline.rs")).unwrap();
    assert!(baseline.contains("id: \"rust-16\""));
    assert!(baseline.contains("target/release/workexec-component"));
    for source in [
        "tests/oracles/rust16.py",
        "src/careerbrew/workday/gate.py",
        "src/careerbrew/workday/timing.py",
        "src/careerbrew/workday/watch.py",
        "src/careerbrew/workday/end.py",
        "src/careerbrew/workday/cron_check.py",
        "src/careerbrew/workday/status.py",
        "crates/workexec/src/gate.rs",
        "crates/workexec/src/timing.rs",
        "crates/workexec/src/watch.rs",
        "crates/workexec/src/end.rs",
        "crates/workexec/src/cron_check.rs",
        "crates/workexec/src/synthetic.rs",
        "crates/workexec/src/bin/workexec-component.rs",
        "crates/workday/src/status.rs",
        "crates/sessions/src/sessions.rs",
        "crates/vault/src/store.rs",
        "crates/vault/src/lock.rs",
        "xtask/src/rust16.rs",
    ] {
        assert!(baseline.contains(source), "missing source binding {source}");
    }
    let measure = std::fs::read_to_string(manifest.join("src/measure.rs")).unwrap();
    assert!(measure.contains("step: \"rust-16\""));
    assert!(measure.contains("target/release/workexec-component"));
    let floors = std::fs::read_to_string(manifest.join("src/noise_floor.rs")).unwrap();
    assert_eq!(floors.matches("step: \"rust-16\"").count(), 2);
}

#[test]
fn rust_16_verify_matches_and_every_planted_control_fails() {
    release_candidate();
    let python = root().join(".venv/bin/python");
    let scratch = tempfile::tempdir().unwrap();
    let verified = scratch.path().join("verify.json");
    let run = verify(&python, &verified, None);
    assert!(run.status.success(), "{run:?}");
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&verified).unwrap()).unwrap();
    assert_eq!(record["passed"], true, "{record}");
    assert_eq!(record["case_count"], 6);
    assert_eq!(record["passed_count"], 6);

    for (plant, field, failed_case) in [
        ("extra", "side_effect", "component:loop-records"),
        ("duplicate-dispatch", "contract", "component:loop-pass"),
        ("lost-wrap", "contract", "component:wrap-control"),
        ("false-success", "contract", "component:gate-check"),
    ] {
        let planted = scratch.path().join(format!("verify-{plant}.json"));
        let run = verify(&python, &planted, Some(plant));
        assert_eq!(run.status.code(), Some(1), "{plant}: {run:?}");
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&planted).unwrap()).unwrap();
        assert!(!record["passed"].as_bool().unwrap());
        assert!(
            record["violations"][field].as_u64().unwrap() > 0,
            "{plant}: {record}"
        );
        let failed = record["failed_cases"].as_array().unwrap();
        // The write map is cumulative, so a planted file also shows in later phases.
        assert_eq!(failed[0], serde_json::json!(failed_case), "{plant}");
    }
    let refused = verify(&python, &scratch.path().join("bad.json"), Some("missing"));
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(String::from_utf8_lossy(&refused.stderr).contains("rust-16 control must be"));
}

#[test]
fn rust_16_baseline_verify_measure_and_decide_share_identity() {
    release_candidate();
    let python = root().join(".venv/bin/python");
    let scratch = tempfile::tempdir().unwrap();
    let base = scratch.path().join("baseline.json");
    let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(["migration", "baseline", "--step", "rust-16", "--python"])
        .arg(&python)
        .arg("--out")
        .arg(&base)
        .args(["--runs", "1", "--test-allow-few-runs"])
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    let verified = scratch.path().join("verify.json");
    let run = verify(&python, &verified, None);
    assert!(run.status.success(), "{run:?}");
    let measured = scratch.path().join("measure.json");
    let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(["migration", "measure", "--step", "rust-16", "--python"])
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
        .args(["migration", "decide", "--step", "rust-16", "--baseline"])
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
    assert_eq!(verdict["step"], "rust-16");
    assert_eq!(verdict["decision"], "keep-python");
    let reasons = verdict["reasons"].as_array().unwrap();
    assert!(reasons.contains(&serde_json::json!("baseline-below-protocol-minimum")));
    assert!(reasons.contains(&serde_json::json!("too-few-unique-pairs")));
}
