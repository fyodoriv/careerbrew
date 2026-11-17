//! Controls for `migration decide` over synthetic JSON records: a passing
//! set activates; a failing verify check and a regressed CPU p95 keep
//! Python with named reasons; a malformed record is refused with no verdict.
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
        "step": "rust-03",
        "meets_protocol_minimum": true,
        "samples": samples,
        "summary": {"cpu_us_p95": 1000, "tree_max_rss_bytes_p95": 50_000},
    })
}

fn verify() -> Value {
    json!({
        "schema": "careerbrew-migration-verify-v1",
        "step": "rust-03",
        "checks": [{"id": "cli.actions", "passed": true}, {"id": "story.us-53", "passed": true}],
        "violations": {
            "contract": 0, "privacy": 0, "authorization": 0, "role": 0,
            "side_effect": 0, "missing_writes": 0, "duplicate_writes": 0,
        },
    })
}

fn measure(cpu: u64) -> Value {
    let regime = |state: &str| {
        json!({
            "cache_state": state,
            "pair_ids": (1..=30).collect::<Vec<u64>>(),
            "cached": 0,
            "candidate_cpu_us": vec![cpu; 30],
            "candidate_tree_max_rss_bytes": vec![55_000u64; 30],
        })
    };
    json!({
        "schema": "careerbrew-migration-measure-v1",
        "step": "rust-03",
        "regimes": [regime("cold"), regime("warm")],
    })
}

fn decide(dir: &Path, base: &Value, verify: &Value, measure: &Value) -> (Output, Option<Value>) {
    let write = |name: &str, value: &Value| {
        let path = dir.join(name);
        std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    };
    let out = dir.join("decide.json");
    let output = std::process::Command::new(XTASK)
        .args(["migration", "decide", "--step", "rust-03", "--baseline"])
        .arg(write("baseline.json", base))
        .arg("--verify")
        .arg(write("verify.json", verify))
        .arg("--measure")
        .arg(write("measure.json", measure))
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
fn passing_records_activate() {
    let dir = tempfile::tempdir().unwrap();
    // Exactly 110% CPU keeps, per the documented limit.
    let (output, record) = decide(dir.path(), &baseline(), &verify(), &measure(1100));
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let record = record.unwrap();
    assert_eq!(record["schema"], "careerbrew-migration-decide-v1");
    assert_eq!(record["decision"], "activate");
    assert_eq!(record["reasons"], json!([]));
}

#[test]
fn failing_verify_keeps_python() {
    let dir = tempfile::tempdir().unwrap();
    let mut failing = verify();
    failing["checks"][1]["passed"] = json!(false);
    failing["violations"]["missing_writes"] = json!(1);
    let (output, record) = decide(dir.path(), &baseline(), &failing, &measure(1000));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let record = record.unwrap();
    assert_eq!(record["decision"], "keep-python");
    assert_eq!(
        record["reasons"],
        json!(["verify-check-failed", "verify-violation"])
    );
}

#[test]
fn regressed_cpu_keeps_python() {
    let dir = tempfile::tempdir().unwrap();
    let (output, record) = decide(dir.path(), &baseline(), &verify(), &measure(1101));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let record = record.unwrap();
    assert_eq!(record["decision"], "keep-python");
    assert_eq!(record["reasons"], json!(["cpu-regressed"]));
}

#[test]
fn malformed_input_is_refused_without_a_verdict() {
    let cases = [
        {
            let mut wrong = verify();
            wrong["schema"] = json!("other");
            (baseline(), wrong, measure(1000))
        },
        {
            let mut wrong = measure(1000);
            wrong["step"] = json!("rust-04");
            (baseline(), verify(), wrong)
        },
        {
            let mut wrong = baseline();
            wrong["summary"]["cpu_us_p95"] = json!("fast");
            (wrong, verify(), measure(1000))
        },
        {
            let mut wrong = verify();
            wrong["checks"] = json!([]);
            (baseline(), wrong, measure(1000))
        },
    ];
    for (base, verify, measure) in cases {
        let dir = tempfile::tempdir().unwrap();
        let (output, record) = decide(dir.path(), &base, &verify, &measure);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(record.is_none());
    }
}
