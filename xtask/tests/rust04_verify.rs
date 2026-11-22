//! Controls for `migration verify --step rust-04` (rust-04d): the
//! vector-driver kind runs the `policy-vectors` candidate over the four core
//! sets against `tests/oracles/rust04.py`. A clean run passes; a planted
//! wrong vector fails and names its case; a set the step does not cover is
//! refused. Synthetic canaries only. Each test owns its temp folder.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// The candidate cargo built next to this test's xtask binary (the
/// `careerbrew-policy` package has an integration test, so `cargo test
/// --all` builds its binary).
fn policy_vectors() -> PathBuf {
    let path = Path::new(XTASK).with_file_name("policy-vectors");
    assert!(
        path.is_file(),
        "build it first: cargo build -p careerbrew-policy"
    );
    path
}

fn verify(out: &Path, extra: &[&str]) -> Output {
    Command::new(XTASK)
        .args(["migration", "verify", "--step", "rust-04", "--python"])
        .arg(root().join(".venv/bin/python"))
        .arg("--out")
        .arg(out)
        .arg("--test-candidate")
        .arg(policy_vectors())
        .args(extra)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap()
}

fn read(out: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(out).unwrap()).unwrap()
}

#[test]
fn verify_on_rust_04_passes_over_every_oracle_set() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify(&out, &[]);
    assert!(run.status.success(), "{run:?}");
    let record = read(&out);
    assert_eq!(record["step"], "rust-04");
    assert_eq!(record["kind"], "vector-driver");
    assert_eq!(record["candidate"]["kind"], "rust-binary");
    assert_eq!(record["candidate"]["plant"], "none");
    assert_eq!(record["source_revision"].as_str().unwrap().len(), 40);
    assert_eq!(
        record["evidence_sources_sha256"].as_str().unwrap().len(),
        64
    );
    assert_eq!(record["fixtures_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(record["candidate_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(
        record["sets"],
        serde_json::json!([
            "access",
            "allowlists",
            "approvals",
            "assert-no-echo",
            "assert-redact",
            "canary-core",
            "canary-match",
            "leak-spans",
            "private-path",
            "public-scrub",
            "receipt-validate"
        ])
    );
    assert_eq!(record["unicode"]["python"], "15.0.0");
    assert_eq!(record["unicode"]["rust"], "15.0.0");
    assert_eq!(record["passed"], true);
    assert_eq!(record["failed_cases"], serde_json::json!([]));
    assert!(record["case_count"].as_u64().unwrap() > 11);
    // 2 Unicode probes plus 11 sets, each run on both sides.
    assert_eq!(record["vector_file_hash_checks"], 24);
    // The contract fields `decide` reads (rust-04g): one check per case and
    // no missing contracts, as a `depvec` step writes them.
    let checks = record["checks"].as_array().unwrap();
    assert_eq!(checks.len() as u64, record["case_count"].as_u64().unwrap());
    assert!(checks.iter().all(|check| check["passed"] == true));
    assert_eq!(record["missing_contracts"], serde_json::json!([]));
    assert_eq!(record["violations"]["contract"], 0);
}

#[test]
fn a_planted_wrong_vector_fails_naming_its_case() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify(&out, &["--test-wrong-vector", "canary-core"]);
    assert_eq!(run.status.code(), Some(1));
    let record = read(&out);
    assert_eq!(record["passed"], false);
    assert_eq!(record["planted_wrong_vector"], "canary-core");
    assert_eq!(
        record["failed_cases"],
        serde_json::json!(["canary-core:sensitivities"])
    );
    assert!(String::from_utf8_lossy(&run.stderr).contains("canary-core:sensitivities"));
}

#[test]
fn a_wrong_vector_in_a_set_the_step_does_not_cover_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify(&out, &["--test-wrong-vector", "no-such-set"]);
    assert_eq!(run.status.code(), Some(1));
    assert!(!out.exists());
    assert!(String::from_utf8_lossy(&run.stderr).contains("names no vector set"));
}
