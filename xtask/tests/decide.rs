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
        "os": "linux",
        "runs": 30,
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
        // rust-03 scopes out HTTP, DATA and role; decide requires this list.
        "missing_contracts": ["http", "data", "role"],
    })
}

fn measure(cpu: u64) -> Value {
    let regime = |state: &str| {
        json!({
            "cache_state": state,
            "pair_ids": (1..=30).collect::<Vec<u64>>(),
            "cached": 0,
            "oracle_cpu_us": vec![1000u64; 30],
            "oracle_tree_max_rss_bytes": vec![50_000u64; 30],
            "candidate_cpu_us": vec![cpu; 30],
            "candidate_tree_max_rss_bytes": vec![55_000u64; 30],
        })
    };
    json!({
        "schema": "careerbrew-migration-measure-v1",
        "step": "rust-03",
        "os": "linux",
        "regimes": [regime("cold"), regime("warm")],
    })
}

fn decide(dir: &Path, base: &Value, verify: &Value, measure: &Value) -> (Output, Option<Value>) {
    decide_with(dir, base, verify, measure, None)
}

/// Runs decide; `floor` is the test-only noise floor override, if any.
fn decide_with(
    dir: &Path,
    base: &Value,
    verify: &Value,
    measure: &Value,
    floor: Option<&Value>,
) -> (Output, Option<Value>) {
    let write = |name: &str, value: &Value| {
        let path = dir.join(name);
        std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    };
    let out = dir.join("decide.json");
    let mut command = std::process::Command::new(XTASK);
    command
        .args(["migration", "decide", "--step", "rust-03", "--baseline"])
        .arg(write("baseline.json", base))
        .arg("--verify")
        .arg(write("verify.json", verify))
        .arg("--measure")
        .arg(write("measure.json", measure))
        .arg("--out")
        .arg(&out);
    if let Some(floor) = floor {
        command
            .arg("--test-noise-floor")
            .arg(write("floor.json", floor));
    }
    let output = command.output().unwrap();
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
            "oracle_cpu_us",
            "oracle_tree_max_rss_bytes",
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

    // A contract verify did not check that the step does not scope out
    // (the CLI contract) keeps Python.
    let mut uncovered = verify();
    uncovered["missing_contracts"] = json!(["http", "data", "role", "cli"]);
    keeps_with(
        baseline(),
        uncovered,
        measure(1000),
        "verify-contract-missing",
    );

    // A record that does not list a scoped-out contract was made under
    // another scope (here: one that claims HTTP): it keeps Python.
    let mut other_scope = verify();
    other_scope["missing_contracts"] = json!(["data", "role"]);
    keeps_with(
        baseline(),
        other_scope,
        measure(1000),
        "verify-contract-scope-mismatch",
    );
    let mut full_scope = verify();
    full_scope["missing_contracts"] = json!([]);
    keeps_with(
        baseline(),
        full_scope,
        measure(1000),
        "verify-contract-scope-mismatch",
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
            wrong
                .as_object_mut()
                .unwrap()
                .remove("unobserved_violations");
            (baseline(), wrong, measure(1000))
        },
        {
            let mut wrong = verify();
            wrong.as_object_mut().unwrap().remove("missing_contracts");
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

/// A test-only floor: CPU floor 2,000 us with a 1,600 us ceiling below it;
/// RSS floor 1,000 bytes (the 50,000 byte baseline is above it).
fn test_floor() -> Value {
    json!({
        "cpu_floor_us": 2000, "cpu_absolute_us": 1600,
        "rss_floor_bytes": 1000, "rss_absolute_bytes": 1100,
    })
}

#[test]
fn an_unmeasured_floor_falls_back_to_the_ratio() {
    // rust-03 has no measured floor on either OS: every case uses 1.10 x B.
    // 1,500 us activates under the test floor below (ceiling 1,600 us), so
    // only the ratio fallback holds it here.
    let dir = tempfile::tempdir().unwrap();
    let (output, record) = decide(dir.path(), &baseline(), &verify(), &measure(1500));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let record = record.unwrap();
    assert_eq!(record["noise_floor"], "unmeasured");
    assert_eq!(record["noise_floor_os"], "linux");
    assert_eq!(record["test_noise_floor"], false);
    assert_eq!(record["reasons"], json!(["cpu-regressed"]));
    // The record names the limits it used and hashes its sources.
    let limits = &record["noise_floor_limits"];
    for name in [
        "cpu_floor_us",
        "cpu_absolute_us",
        "rss_floor_bytes",
        "rss_absolute_bytes",
    ] {
        assert_eq!(limits[name], 0, "{name}");
    }
    assert_eq!(record["noise_floor_method"], Value::Null);
    assert_eq!(record["noise_floor_record"], Value::Null);
    assert_eq!(record["sources_sha256"].as_str().map(str::len), Some(64));
}

#[test]
fn a_test_floor_record_names_its_limits() {
    let dir = tempfile::tempdir().unwrap();
    let floor = test_floor();
    let (_, record) = decide_with(
        dir.path(),
        &baseline(),
        &verify(),
        &measure(1000),
        Some(&floor),
    );
    assert_eq!(record.unwrap()["noise_floor_limits"], floor);
}

#[test]
fn a_measure_record_from_another_os_is_refused() {
    for os in [json!("macos"), Value::Null] {
        let dir = tempfile::tempdir().unwrap();
        let mut measured = measure(1000);
        measured["os"] = os;
        let (output, record) = decide(dir.path(), &baseline(), &verify(), &measured);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(record.is_none());
    }
}

#[test]
fn a_baseline_below_the_floor_within_its_ceiling_activates() {
    // B = 1,000 us is below the 2,000 us floor: 1,500 us is 150% of B but
    // under the 1,600 us ceiling, so it keeps.
    let dir = tempfile::tempdir().unwrap();
    let floor = test_floor();
    // The floor comes from the warm baseline, so it covers only the warm
    // regime: the cold candidate stays within the 1.10 ratio here.
    let mut measured = measure(1500);
    measured["regimes"][0]["candidate_cpu_us"] = json!(vec![1000u64; 30]);
    let (output, record) = decide_with(dir.path(), &baseline(), &verify(), &measured, Some(&floor));
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let record = record.unwrap();
    assert_eq!(record["decision"], "activate");
    assert_eq!(record["noise_floor"], "test-override");
    assert_eq!(record["test_noise_floor"], true);
}

#[test]
fn a_baseline_below_the_floor_beyond_its_ceiling_holds() {
    let dir = tempfile::tempdir().unwrap();
    let floor = test_floor();
    let (output, record) = decide_with(
        dir.path(),
        &baseline(),
        &verify(),
        &measure(1601),
        Some(&floor),
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(record.unwrap()["reasons"], json!(["cpu-regressed"]));
}

#[test]
fn a_baseline_at_or_above_the_floor_uses_the_ratio() {
    let dir = tempfile::tempdir().unwrap();
    let mut floor = test_floor();
    floor["cpu_floor_us"] = json!(1000);
    let (output, record) = decide_with(
        dir.path(),
        &baseline(),
        &verify(),
        &measure(1101),
        Some(&floor),
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(record.unwrap()["reasons"], json!(["cpu-regressed"]));
    let dir = tempfile::tempdir().unwrap();
    let (output, _) = decide_with(
        dir.path(),
        &baseline(),
        &verify(),
        &measure(1100),
        Some(&floor),
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn a_malformed_floor_or_an_unknown_os_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut floor = test_floor();
    floor.as_object_mut().unwrap().remove("rss_absolute_bytes");
    let (output, record) = decide_with(
        dir.path(),
        &baseline(),
        &verify(),
        &measure(1000),
        Some(&floor),
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(record.is_none());
    let dir = tempfile::tempdir().unwrap();
    let mut base = baseline();
    base["os"] = json!("plan9");
    let (output, record) = decide(dir.path(), &base, &verify(), &measure(1000));
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(record.is_none());
}

/// `migration noise-floor`: spread = nearest-rank p95 - p5 of the samples;
/// floor = 10 x spread; ceiling below the floor = floor x 1.10.
#[test]
fn noise_floor_measures_a_baseline_record() {
    let dir = tempfile::tempdir().unwrap();
    let mut base = baseline();
    base["samples"] = json!((0..30u64)
        .map(|i| json!({"run": i + 1, "tree_cpu_us": 1000 + i, "tree_max_rss_bytes": 50_000 + 10 * i}))
        .collect::<Vec<_>>());
    let path = dir.path().join("baseline.json");
    std::fs::write(&path, serde_json::to_vec(&base).unwrap()).unwrap();
    let output = std::process::Command::new(XTASK)
        .args(["migration", "noise-floor", "--baseline"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["step"], "rust-03");
    assert_eq!(value["os"], "linux");
    assert_eq!(value["cpu_floor_us"], 270);
    assert_eq!(value["cpu_absolute_us"], 297);
    assert_eq!(value["rss_floor_bytes"], 2700);
    assert_eq!(value["rss_absolute_bytes"], 2970);
    // A record below the protocol minimum, or a relative path, is refused.
    base["meets_protocol_minimum"] = json!(false);
    std::fs::write(&path, serde_json::to_vec(&base).unwrap()).unwrap();
    let output = std::process::Command::new(XTASK)
        .args(["migration", "noise-floor", "--baseline"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    // A record decide would refuse is refused here too: another cache
    // state, or a run count that differs from its samples.
    for (key, value) in [("cache_state", json!("cold")), ("runs", json!(31))] {
        let mut bad = baseline();
        bad[key] = value;
        std::fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
        let output = std::process::Command::new(XTASK)
            .args(["migration", "noise-floor", "--baseline"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{key}: {output:?}");
    }
    let output = std::process::Command::new(XTASK)
        .args(["migration", "noise-floor", "--baseline", "baseline.json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
}

#[test]
fn self_test_runs_the_decide_slow_path_controls() {
    let output = std::process::Command::new(XTASK)
        .args(["migration", "self-test"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "migration self-test: 24 controls passed\n"
    );
}

/// Sets one regime's paired oracle CPU samples and its candidate CPU.
fn paired(state: usize, oracle: u64, candidate: u64) -> Value {
    let mut record = measure(1000);
    record["regimes"][state]["oracle_cpu_us"] = json!(vec![oracle; 30]);
    record["regimes"][state]["candidate_cpu_us"] = json!(vec![candidate; 30]);
    record
}

#[test]
fn a_cold_candidate_over_its_cold_oracle_holds_under_the_baseline() {
    // Cold candidate 600 us is under the 1,000 us warm baseline, but 1.2x
    // its own paired cold oracle (500 us): the cold regime holds the step.
    keeps_with(baseline(), verify(), paired(0, 500, 600), "cpu-regressed");
}

#[test]
fn a_warm_candidate_over_its_warm_oracle_holds_under_the_baseline() {
    keeps_with(baseline(), verify(), paired(1, 500, 600), "cpu-regressed");
}

#[test]
fn each_regime_reports_its_own_oracle_p95() {
    let dir = tempfile::tempdir().unwrap();
    // A cold oracle at 2,000 us lets a 2,100 us cold candidate pass, though
    // it is over the warm baseline: the regime is judged by its own pairs.
    let (output, record) = decide(dir.path(), &baseline(), &verify(), &paired(0, 2000, 2100));
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let record = record.unwrap();
    assert_eq!(record["decision"], "activate");
    assert_eq!(record["regimes"][0]["oracle_cpu_us_p95"], 2000);
    assert_eq!(record["regimes"][1]["oracle_cpu_us_p95"], 1000);
    assert_eq!(record["regimes"][0]["oracle_tree_max_rss_bytes"], 50_000);
}

#[test]
fn a_regime_without_matching_oracle_samples_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut missing = measure(1000);
    missing["regimes"][0]
        .as_object_mut()
        .unwrap()
        .remove("oracle_cpu_us");
    let mut short = measure(1000);
    short["regimes"][1]["oracle_tree_max_rss_bytes"]
        .as_array_mut()
        .unwrap()
        .pop();
    for wrong in [missing, short] {
        let (output, record) = decide(dir.path(), &baseline(), &verify(), &wrong);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(record.is_none());
    }
}

/// Sets one regime's paired oracle RSS samples and its candidate RSS.
fn paired_rss(state: usize, oracle: u64, candidate: u64) -> Value {
    let mut record = measure(1000);
    record["regimes"][state]["oracle_tree_max_rss_bytes"] = json!(vec![oracle; 30]);
    record["regimes"][state]["candidate_tree_max_rss_bytes"] = json!(vec![candidate; 30]);
    record
}

#[test]
fn a_cold_candidate_over_its_cold_oracle_rss_holds_under_the_baseline() {
    // Cold candidate 45,000 bytes is under the 50,000 byte baseline maximum,
    // but 1.125x its own paired cold oracle (40,000 bytes).
    keeps_with(
        baseline(),
        verify(),
        paired_rss(0, 40_000, 45_000),
        "rss-regressed",
    );
}

#[test]
fn a_warm_candidate_over_its_warm_oracle_rss_holds_under_the_baseline() {
    keeps_with(
        baseline(),
        verify(),
        paired_rss(1, 40_000, 45_000),
        "rss-regressed",
    );
}

#[test]
fn each_regime_reports_its_own_oracle_rss() {
    // A cold oracle at 60,000 bytes lets a 62,000 byte cold candidate pass,
    // though it is over the baseline maximum.
    let dir = tempfile::tempdir().unwrap();
    let (output, record) = decide(
        dir.path(),
        &baseline(),
        &verify(),
        &paired_rss(0, 60_000, 62_000),
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let record = record.unwrap();
    assert_eq!(record["regimes"][0]["oracle_tree_max_rss_bytes"], 60_000);
    assert_eq!(record["regimes"][1]["oracle_tree_max_rss_bytes"], 50_000);
}

#[test]
fn the_warm_noise_floor_does_not_cover_the_cold_regime() {
    // The floor is measured on the warm baseline. Under it a 1,500 us warm
    // candidate passes (ceiling 1,600 us), but the same cold candidate is
    // judged by the 1.10 ratio against its 1,000 us cold oracle and holds.
    let dir = tempfile::tempdir().unwrap();
    let floor = test_floor();
    let (output, record) = decide_with(
        dir.path(),
        &baseline(),
        &verify(),
        &measure(1500),
        Some(&floor),
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let record = record.unwrap();
    assert_eq!(record["reasons"], json!(["cpu-regressed"]));
    assert_eq!(record["regimes"][0]["noise_floor_applied"], false);
    assert_eq!(record["regimes"][1]["noise_floor_applied"], true);
}

#[test]
fn a_registry_projection_check_is_judged_like_any_check() {
    // rust-03's offline contract: a passing projection activates, a failed
    // one keeps Python.
    let dir = tempfile::tempdir().unwrap();
    let mut passing = verify();
    passing["checks"] = json!([
        {"id": "actions-json", "passed": true},
        {"id": "registry-projection", "passed": true},
    ]);
    let (output, record) = decide(dir.path(), &baseline(), &passing, &measure(1000));
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(record.unwrap()["decision"], "activate");
    let mut failing = passing.clone();
    failing["checks"][1]["passed"] = json!(false);
    keeps_with(baseline(), failing, measure(1000), "verify-check-failed");
}
