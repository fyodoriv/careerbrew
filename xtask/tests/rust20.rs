//! Registration, evidence identity, exact FSRS and write-failure controls for rust-20.

use std::path::Path;
use std::process::Command;

#[test]
fn rust_20_registers_the_release_candidate_and_effective_inputs() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let baseline = std::fs::read_to_string(manifest.join("src/baseline.rs")).unwrap();
    assert!(baseline.contains("id: \"rust-20\""));
    assert!(baseline.contains("target/release/tutor-component"));
    for source in [
        "tests/oracles/rust20.py",
        "src/careerbrew/notion_import.py",
        "src/careerbrew/tutor/cards.py",
        "src/careerbrew/tutor/public.py",
        "src/careerbrew/tutor/engine.py",
        "src/careerbrew/tutor/mistakes.py",
        "src/careerbrew/plugins.py",
        "crates/tutor/src/fsrs.rs",
        "crates/tutor/src/cards.rs",
        "crates/tutor/src/public.rs",
        "crates/tutor/src/notion.rs",
        "crates/tutor/src/engine.rs",
        "crates/tutor/src/mistakes.rs",
        "crates/tutor/src/synthetic.rs",
        "crates/tutor/src/bin/tutor-component.rs",
        "crates/setup/src/plugins.rs",
        "crates/learn/src/cards.rs",
        "crates/policy/src/canary.rs",
        "crates/vault/src/store.rs",
        "xtask/src/rust20.rs",
    ] {
        assert!(baseline.contains(source), "missing source binding {source}");
    }
    let measure = std::fs::read_to_string(manifest.join("src/measure.rs")).unwrap();
    assert!(measure.contains("step: \"rust-20\""));
    assert!(measure.contains("target/release/tutor-component"));
    let floors = std::fs::read_to_string(manifest.join("src/noise_floor.rs")).unwrap();
    assert_eq!(floors.matches("step: \"rust-20\"").count(), 2);
    let compare = std::fs::read_to_string(manifest.join("src/rust20.rs")).unwrap();
    assert!(compare.contains("0.00001"));
    assert!(compare.contains("0.000001"));
    assert!(compare.contains("2026-10-21T09:10:00.123456+00:00"));
}

#[test]
fn rust_20_baseline_verify_measure_decide_and_write_controls_share_identity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let candidate = root.join("target/release/tutor-component");
    let build = Command::new("cargo")
        .args([
            "build",
            "--locked",
            "--offline",
            "--release",
            "-p",
            "careerbrew-tutor",
            "--bin",
            "tutor-component",
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
        .args(["migration", "baseline", "--step", "rust-20", "--python"])
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
        .args(["migration", "verify", "--step", "rust-20", "--python"])
        .arg(&python)
        .arg("--out")
        .arg(&verified)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&verified).unwrap()).unwrap();
    assert_eq!(record["case_count"], 8);
    assert_eq!(record["passed_count"], 8);
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
        .args(["migration", "measure", "--step", "rust-20", "--python"])
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
        .args(["migration", "decide", "--step", "rust-20", "--baseline"])
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
    assert_eq!(verdict["step"], "rust-20");
    assert_eq!(verdict["decision"], "keep-python");
    let reasons = verdict["reasons"].as_array().unwrap();
    assert!(reasons.contains(&serde_json::json!("baseline-below-protocol-minimum")));
    assert!(reasons.contains(&serde_json::json!("too-few-unique-pairs")));

    for (plant, field, failed_case) in [
        ("extra", "side_effect", "component:fsrs-old-state"),
        ("missing", "missing_writes", "component:fsrs-old-state"),
        (
            "duplicate",
            "duplicate_writes",
            "component:duplicate-write-control",
        ),
        (
            "event-duplicate",
            "duplicate_writes",
            "component:providers-and-mistakes",
        ),
    ] {
        let planted = scratch.path().join(format!("verify-{plant}.json"));
        let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
            .args(["migration", "verify", "--step", "rust-20", "--python"])
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
            .args(["migration", "decide", "--step", "rust-20", "--baseline"])
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
