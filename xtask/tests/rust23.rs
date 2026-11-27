//! Registration, evidence identity, exact results and write-failure controls for rust-23.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

#[test]
fn rust_23_registers_the_release_candidate_and_effective_inputs() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let baseline = std::fs::read_to_string(manifest.join("src/baseline.rs")).unwrap();
    assert!(baseline.contains("id: \"rust-23\""));
    assert!(baseline.contains("target/release/voice-component"));
    for source in [
        "tests/oracles/rust23.py",
        "src/careerbrew/models.py",
        "src/careerbrew/judge.py",
        "src/careerbrew/tutor/voice.py",
        "crates/voice/src/models.rs",
        "crates/voice/src/judge.rs",
        "crates/voice/src/voice.rs",
        "crates/voice/src/http.rs",
        "crates/voice/src/synthetic.rs",
        "crates/voice/src/bin/voice-component.rs",
        "tests/fixtures/voice/fake-stt.sh",
        "config/models.yaml",
        "xtask/src/rust23.rs",
    ] {
        assert!(baseline.contains(source), "missing source binding {source}");
    }
    let measure = std::fs::read_to_string(manifest.join("src/measure.rs")).unwrap();
    assert!(measure.contains("step: \"rust-23\""));
    assert!(measure.contains("target/release/voice-component"));
    assert!(measure.contains("rust23::validate_measure"));
    let floors = std::fs::read_to_string(manifest.join("src/noise_floor.rs")).unwrap();
    assert_eq!(floors.matches("step: \"rust-23\"").count(), 2);
    let compare = std::fs::read_to_string(manifest.join("src/rust23.rs")).unwrap();
    assert!(compare.contains("const FLOAT_LEAVES: usize = 12;"));
}

#[test]
fn rust_23_verify_and_write_controls_share_identity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let candidate = root.join("target/release/voice-component");
    let build = Command::new("cargo")
        .args([
            "build",
            "--locked",
            "--offline",
            "--release",
            "-p",
            "careerbrew-voice",
            "--bin",
            "voice-component",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    assert!(candidate.is_file());
    let python = root.join(".venv/bin/python");
    let scratch = tempfile::tempdir().unwrap();
    let verified = scratch.path().join("verify.json");
    let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(["migration", "verify", "--step", "rust-23", "--python"])
        .arg(&python)
        .arg("--out")
        .arg(&verified)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&verified).unwrap()).unwrap();
    assert_eq!(record["case_count"], 10);
    assert_eq!(record["passed_count"], 10);
    assert_eq!(record["float_leaves"], 12);
    assert_eq!(record["violations"]["contract"], 0);
    assert_eq!(record["violations"]["privacy"], 0);
    assert!(record["cases"].as_array().unwrap().iter().all(|case| {
        case["comparison"]
            .as_str()
            .unwrap()
            .contains("typed-results-exact")
    }));

    for (plant, field, failed_case) in [
        ("extra", "side_effect", "component:routes-config"),
        ("missing", "missing_writes", "component:labels"),
        ("duplicate", "duplicate_writes", "component:allowed-routes"),
    ] {
        let planted = scratch.path().join(format!("verify-{plant}.json"));
        let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
            .args(["migration", "verify", "--step", "rust-23", "--python"])
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
        assert!(
            failed.contains(&serde_json::json!(failed_case)),
            "{plant}: {failed:?}"
        );
    }
}

#[test]
fn rust_23_refuses_an_unknown_write_control() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(["migration", "verify", "--step", "rust-23", "--python"])
        .arg(root.join(".venv/bin/python"))
        .arg("--out")
        .arg(scratch.path().join("verify.json"))
        .args(["--test-plant-home-write", "event-duplicate"])
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert!(!run.status.success(), "{run:?}");
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("rust-23 write control"), "{stderr}");
}

#[test]
fn rust_23_measure_refuses_an_invalid_all_report() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let candidate = scratch.path().join("empty-candidate.sh");
    std::fs::write(&candidate, "#!/bin/sh\nprintf '{\"cases\":[]}\\n'\n").unwrap();
    let mut permissions = std::fs::metadata(&candidate).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&candidate, permissions).unwrap();
    let output = scratch.path().join("measure.json");
    let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
        .args(["migration", "measure", "--step", "rust-23", "--python"])
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
    assert!(stderr.contains("rust-23 --all report"), "{stderr}");
}
