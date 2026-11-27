//! Registration, evidence identity and planted controls for rust-25 (the native web front).

use std::path::Path;
use std::process::Command;

#[test]
fn rust_25_registers_the_release_candidate_and_effective_inputs() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let baseline = std::fs::read_to_string(manifest.join("src/baseline.rs")).unwrap();
    assert!(baseline.contains("id: \"rust-25\""));
    assert!(baseline.contains("target/release/web-component"));
    for source in [
        "tests/oracles/rust25.py",
        "src/careerbrew/server.py",
        "src/careerbrew/config.py",
        "crates/web/src/server.rs",
        "crates/web/src/token.rs",
        "crates/web/src/version.rs",
        "crates/web/src/mime.rs",
        "crates/web/src/http.rs",
        "crates/web/src/cases.rs",
        "crates/web/src/bin/web-component.rs",
        "xtask/src/rust25.rs",
        "tests/fixtures/web/rust25-cases.json",
    ] {
        assert!(baseline.contains(source), "missing source binding {source}");
    }
    let measure = std::fs::read_to_string(manifest.join("src/measure.rs")).unwrap();
    assert!(measure.contains("step: \"rust-25\""));
    let floors = std::fs::read_to_string(manifest.join("src/noise_floor.rs")).unwrap();
    assert_eq!(floors.matches("step: \"rust-25\"").count(), 2);
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
fn rust_25_verify_passes_and_every_planted_control_fails() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let build = Command::new("cargo")
        .args([
            "build",
            "--locked",
            "--offline",
            "--release",
            "-p",
            "careerbrew-web",
            "--bin",
            "web-component",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    let python = root.join(".venv/bin/python");
    let scratch = tempfile::tempdir().unwrap();
    let base = scratch.path().join("baseline.json");
    let verb = ["migration", "baseline", "--step", "rust-25"];
    let run = xtask(
        &verb,
        &python,
        &base,
        &["--runs", "1", "--test-allow-few-runs"],
    );
    assert!(run.status.success(), "{run:?}");
    let verified = scratch.path().join("verify.json");
    let verb = ["migration", "verify", "--step", "rust-25"];
    let run = xtask(&verb, &python, &verified, &[]);
    assert!(run.status.success(), "{run:?}");
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&verified).unwrap()).unwrap();
    assert_eq!(record["passed"], true, "{record}");
    assert!(record["case_count"].as_u64().unwrap() >= 30);
    assert_eq!(record["case_count"], record["passed_count"]);
    assert_eq!(
        record["missing_contracts"],
        serde_json::json!(["data", "role"])
    );
    let measured = scratch.path().join("measure.json");
    let verb = ["migration", "measure", "--step", "rust-25"];
    let run = xtask(
        &verb,
        &python,
        &measured,
        &["--pairs", "1", "--test-allow-few-pairs"],
    );
    assert!(run.status.success(), "{run:?}");

    for (plant, failed_case) in [
        ("csp", "http:changes-ok"),
        ("cookie", "http:session-set-ok"),
        ("body", "http:changes-bad-host"),
        ("missing", "http:static-traversal"),
    ] {
        let planted = scratch.path().join(format!("verify-{plant}.json"));
        let verb = ["migration", "verify", "--step", "rust-25"];
        let run = xtask(
            &verb,
            &python,
            &planted,
            &["--test-plant-home-write", plant],
        );
        assert_eq!(run.status.code(), Some(1), "{plant}: {run:?}");
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&planted).unwrap()).unwrap();
        assert!(!record["passed"].as_bool().unwrap());
        assert!(
            record["violations"]["contract"].as_u64().unwrap() > 0,
            "{plant}"
        );
        let failed = record["failed_cases"].as_array().unwrap();
        assert_eq!(failed, &vec![serde_json::json!(failed_case)], "{plant}");
        let verdict_path = scratch.path().join(format!("decide-{plant}.json"));
        let run = Command::new(env!("CARGO_BIN_EXE_careerbrew-xtask"))
            .args(["migration", "decide", "--step", "rust-25", "--baseline"])
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
