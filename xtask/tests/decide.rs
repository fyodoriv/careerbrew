//! Controls for `migration decide` over synthetic JSON records: a passing
//! set activates; each keep-python reason id has its own control; each
//! listed refusal (exit 2, no verdict) has its own case.
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
        "source_tree_dirty": false,
        "cache_state": "warm-imports-fresh-synthetic-home",
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
        "unobserved_violations": [],
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
    // Exactly 110% CPU passes (activates), per the documented limit.
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

fn keeps_with(base: Value, verify: Value, measure: Value, reason: &str) {
    let dir = tempfile::tempdir().unwrap();
    let (output, record) = decide(dir.path(), &base, &verify, &measure);
    assert_eq!(output.status.code(), Some(1), "{reason}: {output:?}");
    let record = record.unwrap();
    assert_eq!(record["decision"], "keep-python", "{reason}");
    assert_eq!(record["reasons"], json!([reason]), "{reason}");
}

#[test]
fn each_keep_reason_has_a_control() {
    let mut few = measure(1000);
    for regime in few["regimes"].as_array_mut().unwrap() {
        for key in [
            "pair_ids",
            "candidate_cpu_us",
            "candidate_tree_max_rss_bytes",
        ] {
            regime[key].as_array_mut().unwrap().truncate(29);
        }
    }
    keeps_with(baseline(), verify(), few, "too-few-unique-pairs");

    let mut repeated = measure(1000);
    repeated["regimes"][0]["pair_ids"][1] = json!(1);
    keeps_with(baseline(), verify(), repeated, "too-few-unique-pairs");

    let mut cached = measure(1000);
    cached["regimes"][1]["cached"] = json!(1);
    keeps_with(baseline(), verify(), cached, "cached-sample");

    let mut rss = measure(1000);
    rss["regimes"][0]["candidate_tree_max_rss_bytes"][0] = json!(55_001);
    keeps_with(baseline(), verify(), rss, "rss-regressed");

    let mut below = baseline();
    below["meets_protocol_minimum"] = json!(false);
    keeps_with(
        below,
        verify(),
        measure(1000),
        "baseline-below-protocol-minimum",
    );

    let mut dirty = baseline();
    dirty["source_tree_dirty"] = json!(true);
    keeps_with(dirty, verify(), measure(1000), "baseline-dirty-tree");

    // A violation kind verify could not observe never counts as zero.
    let mut unobserved = verify();
    unobserved["unobserved_violations"] = json!(["duplicate_writes"]);
    keeps_with(
        baseline(),
        unobserved,
        measure(1000),
        "verify-violation-unobserved",
    );

    for state in ["cold", "warm"] {
        let mut one = measure(1000);
        one["regimes"]
            .as_array_mut()
            .unwrap()
            .retain(|regime| regime["cache_state"] != state);
        keeps_with(baseline(), verify(), one, "regime-missing");
    }
}

#[test]
fn bad_paths_and_an_existing_out_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, value: &Value| {
        let path = dir.path().join(name);
        std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    };
    let base = write("baseline.json", &baseline());
    let verify = write("verify.json", &verify());
    let measure = write("measure.json", &measure(1000));
    let existing = write("existing.json", &json!({}));
    let fresh = dir.path().join("fresh.json");
    let run = |base: &Path, out: &Path| {
        std::process::Command::new(XTASK)
            .args(["migration", "decide", "--step", "rust-03", "--baseline"])
            .arg(base)
            .arg("--verify")
            .arg(&verify)
            .arg("--measure")
            .arg(&measure)
            .arg("--out")
            .arg(out)
            .output()
            .unwrap()
    };
    let output = run(Path::new("baseline.json"), &fresh);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(!fresh.exists());
    let output = run(&base, &existing);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(std::fs::read(&existing).unwrap(), b"{}");
}

#[test]
fn malformed_input_is_refused_without_a_verdict() {
    let cases = [
        {
            let mut wrong = verify();
            wrong["checks"][1]["id"] = json!("cli.actions");
            (baseline(), wrong, measure(1000))
        },
        {
            let mut wrong = measure(1000);
            wrong["regimes"][1]["cache_state"] = json!("cold");
            (baseline(), verify(), wrong)
        },
        {
            let mut wrong = measure(1000);
            wrong["regimes"][0]["candidate_cpu_us"]
                .as_array_mut()
                .unwrap()
                .pop();
            (baseline(), verify(), wrong)
        },
        {
            let mut wrong = measure(1000);
            wrong["regimes"][0]["cache_state"] = json!("hot");
            (baseline(), verify(), wrong)
        },
        {
            let mut wrong = verify();
            wrong["violations"].as_object_mut().unwrap().remove("role");
            (baseline(), wrong, measure(1000))
        },
        {
            let mut wrong = baseline();
            wrong["samples"].as_array_mut().unwrap().truncate(3);
            (wrong, verify(), measure(1000))
        },
        {
            let mut wrong = baseline();
            wrong["cache_state"] = json!("cold");
            (wrong, verify(), measure(1000))
        },
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
        {
            let mut wrong = verify();
            wrong.as_object_mut().unwrap().remove("unobserved_violations");
            (baseline(), wrong, measure(1000))
        },
        {
            let mut wrong = verify();
            wrong["unobserved_violations"] = json!(["not-a-kind"]);
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

#[test]
fn an_unknown_step_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, value: &Value| {
        let path = dir.path().join(name);
        std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    };
    let out = dir.path().join("decide.json");
    let output = std::process::Command::new(XTASK)
        .args(["migration", "decide", "--step", "rust-99", "--baseline"])
        .arg(write("baseline.json", &baseline()))
        .arg("--verify")
        .arg(write("verify.json", &verify()))
        .arg("--measure")
        .arg(write("measure.json", &measure(1000)))
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(!out.exists());
}
