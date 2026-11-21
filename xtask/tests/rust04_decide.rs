//! rust-04g: `migration decide --step rust-04` reads the vector-driver verify
//! record. A record with checks and no missing contracts gets a verdict
//! (exit 0 or 1), never the refusal "the verify record has no checks"; a
//! record with no checks is still refused (exit 2). Synthetic records only.
use serde_json::{json, Value};
use std::path::Path;
use std::process::Output;

const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");

fn baseline() -> Value {
    let samples: Vec<Value> = (1..=30)
        .map(|run| json!({"run": run, "tree_cpu_us": 1000, "tree_max_rss_bytes": 50_000}))
        .collect();
    json!({
        "schema": "careerbrew-migration-baseline-v1",
        "step": "rust-04",
        "os": "linux",
        "runs": 30,
        "meets_protocol_minimum": true,
        "source_revision": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "source_tree_dirty": false,
        "evidence_sources_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "fixtures_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "cache_state": "warm-imports-fresh-synthetic-home",
        "samples": samples,
        "summary": {"cpu_us_p95": 1000, "tree_max_rss_bytes_p95": 50_000},
    })
}

/// The contract fields a vector-driver record carries since rust-04g.
fn verify() -> Value {
    json!({
        "schema": "careerbrew-migration-verify-v1",
        "step": "rust-04",
        "source_revision": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "evidence_sources_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "fixtures_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "candidate_sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        "kind": "vector-driver",
        "contracts": ["vector"],
        "checks": [
            {"id": "approvals:request-simple", "passed": true},
            {"id": "access:route-tie", "passed": true},
        ],
        "violations": {
            "contract": 0, "privacy": 0, "authorization": 0, "role": 0,
            "side_effect": 0, "missing_writes": 0, "duplicate_writes": 0,
        },
        "unobserved_violations": [],
        "missing_contracts": [],
    })
}

fn measure() -> Value {
    let regime = |state: &str| {
        json!({
            "cache_state": state,
            "pair_ids": (1..=30).collect::<Vec<u64>>(),
            "cached": 0,
            "oracle_cpu_us": vec![1000u64; 30],
            "oracle_tree_max_rss_bytes": vec![50_000u64; 30],
            "candidate_cpu_us": vec![900u64; 30],
            "candidate_tree_max_rss_bytes": vec![45_000u64; 30],
        })
    };
    json!({
        "schema": "careerbrew-migration-measure-v1",
        "step": "rust-04",
        "os": "linux",
        "source_revision": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "evidence_sources_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "fixtures_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "candidate_sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        "regimes": [regime("cold"), regime("warm")],
    })
}

fn decide(dir: &Path, verify: &Value) -> (Output, Option<Value>) {
    let write = |name: &str, value: &Value| {
        let path = dir.join(name);
        std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    };
    let out = dir.join("decide.json");
    let output = std::process::Command::new(XTASK)
        .args(["migration", "decide", "--step", "rust-04", "--baseline"])
        .arg(write("baseline.json", &baseline()))
        .arg("--verify")
        .arg(write("verify.json", verify))
        .arg("--measure")
        .arg(write("measure.json", &measure()))
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    let record = std::fs::read(&out)
        .ok()
        .map(|bytes| serde_json::from_slice(&bytes).unwrap());
    (output, record)
}

#[test]
fn a_vector_driver_record_gets_a_verdict_with_no_scope_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let (output, record) = decide(dir.path(), &verify());
    assert_ne!(output.status.code(), Some(2), "{output:?}");
    let record = record.expect("a verdict record");
    let reasons = record["reasons"].as_array().unwrap();
    for reason in [
        "verify-contract-missing",
        "verify-contract-scope-mismatch",
        "verify-check-failed",
    ] {
        assert!(!reasons.iter().any(|r| r == reason), "{reasons:?}");
    }
}

#[test]
fn a_vector_driver_record_with_no_checks_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut verify = verify();
    verify.as_object_mut().unwrap().remove("checks");
    let (output, record) = decide(dir.path(), &verify);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(record.is_none());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no checks"));
}
