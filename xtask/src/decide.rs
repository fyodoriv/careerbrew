//! `migration decide --step <id> --baseline <file> --verify <file> --measure <file>`:
//! read the three content-free records of one step, apply the decide limits
//! of docs/rust-migration.md, and return a content-free verdict record
//! (`activate` or `keep-python` with fixed reason ids). A malformed record,
//! a wrong schema or a step mismatch is refused: no verdict is written.
//!
//! Input shapes (minimal, until `verify` and `measure` land):
//! - baseline: `careerbrew-migration-baseline-v1` from `baseline.rs`; reads
//!   `step`, `os` (picks the noise floor entry), `meets_protocol_minimum` (checked against the sample count),
//!   `source_tree_dirty`, `cache_state` (must be the baseline regime),
//!   `summary.cpu_us_p95` and the `tree_max_rss_bytes` of each sample. These
//!   feed the protocol checks and the record; the limits use the measure
//!   record's paired oracle samples.
//!
//! All three records must bind the same full source revision, common evidence
//! source hash and fixture hash. Verify and measure must bind the same
//! candidate hash. Missing, malformed or unequal identity is refused.
//! - verify: `careerbrew-migration-verify-v1` with `step`, a nonempty
//!   `checks` list of `{id, passed}`, a `violations` map of counts and an
//!   `unobserved_violations` list of violation names verify cannot see yet
//!   (each one keeps Python: an unobserved count is never a zero), and a
//!   `missing_contracts` list of contracts verify did not check. That list
//!   must equal the step's declared scope (`baseline::Step::scoped_out`): a
//!   missing contract the step does not scope out keeps Python
//!   (`verify-contract-missing`), and a scoped-out contract the record does
//!   not list keeps Python too (`verify-contract-scope-mismatch`: the record
//!   was made under another scope).
//! - measure: `careerbrew-migration-measure-v1` with `step` and a nonempty
//!   `regimes` list of `{cache_state, pair_ids, cached, oracle_cpu_us,
//!   oracle_tree_max_rss_bytes, candidate_cpu_us,
//!   candidate_tree_max_rss_bytes}`; one sample per pair. Each regime's
//!   candidate p95 CPU and max RSS are judged against that regime's own
//!   oracle p95 CPU and max RSS. The noise floor limits apply to the warm
//!   regime only (the floor is measured on the warm baseline); cold uses
//!   the 1.10 ratio. Both `cold` and
//!   `warm` must be present, each with at least 30 unique pairs. Its `os`
//!   must equal the baseline's.
use crate::{baseline, baseline_verdict, noise_floor};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

pub const SCHEMA: &str = "careerbrew-migration-decide-v1";
pub const VERIFY_SCHEMA: &str = "careerbrew-migration-verify-v1";
pub const MEASURE_SCHEMA: &str = "careerbrew-migration-measure-v1";
/// Every violation count the verify record must carry; each must be 0.
pub const VIOLATIONS: &[&str] = &[
    "contract",
    "privacy",
    "authorization",
    "role",
    "side_effect",
    "missing_writes",
    "duplicate_writes",
];

/// The decide, noise floor, limit and step registry sources built into this binary, as
/// one hash, so a record shows which rules produced its verdict.
fn sources_sha256() -> String {
    let sources = json!({
        "xtask/src/decide.rs": crate::capture_actions::digest(include_bytes!("decide.rs")),
        "xtask/src/noise_floor.rs": crate::capture_actions::digest(include_bytes!("noise_floor.rs")),
        "xtask/src/baseline_verdict.rs": crate::capture_actions::digest(include_bytes!("baseline_verdict.rs")),
        // The step registry: `decide` reads each step's declared contract scope.
        "xtask/src/baseline.rs": crate::capture_actions::digest(include_bytes!("baseline.rs")),
    });
    crate::capture_actions::digest(sources.to_string().as_bytes())
}

fn read(path: &Path, name: &str) -> Result<Value, String> {
    if !path.is_absolute() {
        return Err(format!("--{name} must be an absolute path"));
    }
    let bytes = std::fs::read(path).map_err(|_| format!("cannot read the {name} record"))?;
    serde_json::from_slice(&bytes).map_err(|_| format!("the {name} record is not JSON"))
}

fn header(record: &Value, name: &str, schema: &str, step: &str) -> Result<(), String> {
    if record["schema"].as_str() != Some(schema) {
        return Err(format!("the {name} record has the wrong schema"));
    }
    if record["step"].as_str() != Some(step) {
        return Err(format!("the {name} record is for another step"));
    }
    Ok(())
}

fn identity_field<'a>(
    record: &'a Value,
    name: &str,
    field: &str,
    length: usize,
) -> Result<&'a str, String> {
    let value = record
        .get(field)
        .ok_or_else(|| format!("the {name} record lacks {field}"))?;
    let text = value
        .as_str()
        .ok_or_else(|| format!("the {name} record has malformed {field}"))?;
    if text.len() != length
        || !text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(format!("the {name} record has malformed {field}"));
    }
    Ok(text)
}

/// All records must describe the same observed source and fixtures. Verify
/// and measure must also describe the same candidate bytes.
fn bind_identity(base: &Value, verify: &Value, measure: &Value) -> Result<(), String> {
    for (field, length) in [
        ("source_revision", 40),
        ("evidence_sources_sha256", 64),
        ("fixtures_sha256", 64),
    ] {
        let expected = identity_field(base, "baseline", field, length)?;
        for (record, name) in [(verify, "verify"), (measure, "measure")] {
            if identity_field(record, name, field, length)? != expected {
                return Err(format!(
                    "the {name} record has another {field} than the baseline"
                ));
            }
        }
    }
    let verified = identity_field(verify, "verify", "candidate_sha256", 64)?;
    let measured = identity_field(measure, "measure", "candidate_sha256", 64)?;
    if verified != measured {
        return Err("the verify and measure records have different candidate_sha256".into());
    }
    Ok(())
}

fn number(value: &Value, what: &str) -> Result<u64, String> {
    value.as_u64().ok_or_else(|| format!("malformed {what}"))
}

fn numbers(value: &Value, what: &str) -> Result<Vec<u64>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("malformed {what}"))?
        .iter()
        .map(|item| number(item, what))
        .collect()
}

/// Returns the verdict record, or a refusal reason for malformed input.
/// `test_floor` is the test-only noise floor override; without it the
/// registry entry for the step and the baseline's OS is used.
pub fn verdict(
    step: &str,
    base: &Value,
    verify: &Value,
    measure: &Value,
    test_floor: Option<&Value>,
) -> Result<Value, String> {
    let registered = baseline::step(step)?;
    // A vector step's record comes from `depvec` or `vecdriver`, which run
    // no CLI contract and list none as missing, so it declares no scope.
    let declared = if crate::depvec::step(step).is_some() || crate::vecdriver::step(step).is_some()
    {
        Vec::new()
    } else {
        registered.scoped_out()
    };
    header(base, "baseline", baseline::SCHEMA, step)?;
    let os = base["os"]
        .as_str()
        .filter(|os| matches!(*os, "macos" | "linux"))
        .ok_or("the baseline record has an unknown OS")?;
    let chosen = match test_floor {
        Some(value) => {
            let (cpu, rss) = noise_floor::from_test(value)?;
            noise_floor::Chosen {
                cpu,
                rss,
                state: "test-override",
                method: None,
                record: None,
            }
        }
        None => noise_floor::limits(step, os)?,
    };
    let (cpu_limit, rss_limit) = (chosen.cpu, chosen.rss);
    header(verify, "verify", VERIFY_SCHEMA, step)?;
    header(measure, "measure", MEASURE_SCHEMA, step)?;
    bind_identity(base, verify, measure)?;
    if measure["os"].as_str() != Some(os) {
        return Err("the measure record is from another OS than the baseline".into());
    }

    let base_cpu = number(&base["summary"]["cpu_us_p95"], "baseline cpu_us_p95")?;
    let samples = base["samples"]
        .as_array()
        .filter(|samples| !samples.is_empty())
        .ok_or("malformed baseline samples")?;
    let base_rss = samples
        .iter()
        .map(|sample| number(&sample["tree_max_rss_bytes"], "baseline tree_max_rss_bytes"))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    let base_minimum = base["meets_protocol_minimum"]
        .as_bool()
        .ok_or("malformed baseline meets_protocol_minimum")?;
    if base_minimum && samples.len() < baseline::MIN_RUNS {
        return Err("the baseline claims the protocol minimum with too few samples".into());
    }
    let base_dirty = base["source_tree_dirty"]
        .as_bool()
        .ok_or("malformed baseline source_tree_dirty")?;
    if base["cache_state"].as_str() != Some(baseline::CACHE_STATE) {
        return Err("the baseline record has another cache state".into());
    }

    let checks = verify["checks"]
        .as_array()
        .filter(|checks| !checks.is_empty())
        .ok_or("the verify record has no checks")?;
    let mut ids = BTreeSet::new();
    let mut failed_checks = 0u64;
    for check in checks {
        let id = check["id"].as_str().filter(|id| !id.is_empty());
        let passed = check["passed"].as_bool();
        let (Some(id), Some(passed)) = (id, passed) else {
            return Err("malformed verify check".into());
        };
        if !ids.insert(id) {
            return Err("the verify record repeats a check id".into());
        }
        failed_checks += u64::from(!passed);
    }
    let mut violations = 0u64;
    for name in VIOLATIONS {
        violations += number(&verify["violations"][*name], "verify violation count")?;
    }
    let unobserved = verify["unobserved_violations"]
        .as_array()
        .ok_or("malformed verify unobserved_violations")?;
    for name in unobserved {
        if !name.as_str().is_some_and(|name| VIOLATIONS.contains(&name)) {
            return Err("malformed verify unobserved_violations".into());
        }
    }

    let missing_contracts = verify["missing_contracts"]
        .as_array()
        .filter(|names| {
            names
                .iter()
                .all(|name| name.as_str().is_some_and(|n| !n.is_empty()))
        })
        .ok_or("malformed verify missing_contracts")?;

    let regimes = measure["regimes"]
        .as_array()
        .filter(|regimes| !regimes.is_empty())
        .ok_or("the measure record has no regimes")?;
    let mut reasons = BTreeSet::new();
    if !base_minimum {
        reasons.insert("baseline-below-protocol-minimum");
    }
    if base_dirty {
        reasons.insert("baseline-dirty-tree");
    }
    if failed_checks > 0 {
        reasons.insert("verify-check-failed");
    }
    if violations > 0 {
        reasons.insert("verify-violation");
    }
    if !unobserved.is_empty() {
        reasons.insert("verify-violation-unobserved");
    }
    // A test-only scope (`--test-scope`) never activates, and a `no_python`
    // step needs a record that ran its candidate with Python denied.
    if !verify["test_scope"].is_null() {
        reasons.insert("verify-test-scope");
    }
    if registered.no_python && verify["python_denied"].as_str() != Some("env") {
        reasons.insert("verify-python-not-denied");
    }
    let listed: BTreeSet<&str> = missing_contracts.iter().filter_map(Value::as_str).collect();
    if listed.iter().any(|name| !declared.contains(name)) {
        reasons.insert("verify-contract-missing");
    }
    if declared.iter().any(|name| !listed.contains(name)) {
        reasons.insert("verify-contract-scope-mismatch");
    }
    let mut states = BTreeSet::new();
    let mut summary = Vec::new();
    for regime in regimes {
        let state = regime["cache_state"]
            .as_str()
            .filter(|state| matches!(*state, "cold" | "warm"))
            .ok_or("malformed measure cache_state")?;
        if !states.insert(state) {
            return Err("the measure record repeats a cache state".into());
        }
        let pairs = numbers(&regime["pair_ids"], "measure pair_ids")?;
        let cached = number(&regime["cached"], "measure cached count")?;
        let oracle_cpu = numbers(&regime["oracle_cpu_us"], "measure oracle_cpu_us")?;
        let oracle_rss = numbers(
            &regime["oracle_tree_max_rss_bytes"],
            "measure oracle_tree_max_rss_bytes",
        )?;
        let cpu = numbers(&regime["candidate_cpu_us"], "measure candidate_cpu_us")?;
        let rss = numbers(
            &regime["candidate_tree_max_rss_bytes"],
            "measure candidate_tree_max_rss_bytes",
        )?;
        if pairs.is_empty()
            || cpu.len() != pairs.len()
            || rss.len() != pairs.len()
            || oracle_cpu.len() != pairs.len()
            || oracle_rss.len() != pairs.len()
        {
            return Err("measure samples do not match its pairs".into());
        }
        let unique: BTreeSet<_> = pairs.iter().collect();
        if pairs.len() < 30 || unique.len() != pairs.len() {
            reasons.insert("too-few-unique-pairs");
        }
        if cached > 0 {
            reasons.insert("cached-sample");
        }
        let cpu_p95 = baseline_verdict::cpu_p95(cpu);
        let rss_max = rss.into_iter().max().unwrap_or(0);
        let oracle_cpu_p95 = baseline_verdict::cpu_p95(oracle_cpu);
        let oracle_rss_max = oracle_rss.into_iter().max().unwrap_or(0);
        // The floor is measured on the warm baseline only, so it covers only
        // the warm regime; cold uses the 1.10 ratio until a cold floor exists.
        let floor_applied = state == "warm";
        let ratio = baseline_verdict::ResourceLimit {
            floor: 0,
            absolute: 0,
        };
        let (cpu_limit, rss_limit) = if floor_applied {
            (cpu_limit, rss_limit)
        } else {
            (ratio, ratio)
        };
        if !baseline_verdict::within_limit(oracle_cpu_p95, cpu_p95, cpu_limit) {
            reasons.insert("cpu-regressed");
        }
        if !baseline_verdict::within_limit(oracle_rss_max, rss_max, rss_limit) {
            reasons.insert("rss-regressed");
        }
        summary.push(json!({
            "cache_state": state,
            "pairs": pairs.len(),
            "oracle_cpu_us_p95": oracle_cpu_p95,
            "oracle_tree_max_rss_bytes": oracle_rss_max,
            "candidate_cpu_us_p95": cpu_p95,
            "candidate_tree_max_rss_bytes": rss_max,
            "noise_floor_applied": floor_applied,
        }));
    }
    if !(states.contains("cold") && states.contains("warm")) {
        reasons.insert("regime-missing");
    }
    let decision = if reasons.is_empty() {
        "activate"
    } else {
        "keep-python"
    };
    Ok(json!({
        "schema": SCHEMA,
        "step": step,
        "decision": decision,
        "reasons": reasons.into_iter().collect::<Vec<_>>(),
        "limit_ratio": "1.10",
        "noise_floor": chosen.state,
        "noise_floor_os": os,
        "noise_floor_limits": chosen.limits_json(),
        "noise_floor_method": chosen.method,
        "noise_floor_record": chosen.record,
        "sources_sha256": sources_sha256(),
        "test_noise_floor": test_floor.is_some(),
        "baseline_cache_state": baseline::CACHE_STATE,
        "baseline_cpu_us_p95": base_cpu,
        "baseline_tree_max_rss_bytes": base_rss,
        "verify_checks": checks.len(),
        "verify_failed_checks": failed_checks,
        "verify_violations": violations,
        "verify_unobserved_violations": unobserved.len(),
        "verify_missing_contracts": missing_contracts.len(),
        "regimes": summary,
    }))
}

pub fn run(
    step: &str,
    base: &Path,
    verify: &Path,
    measure: &Path,
    test_floor: Option<&Path>,
    output: &Path,
) -> Result<Value, String> {
    if !output.is_absolute() {
        return Err("--out must be an absolute path".into());
    }
    if output.exists() {
        return Err("decide output already exists".into());
    }
    let record = verdict(
        step,
        &read(base, "baseline")?,
        &read(verify, "verify")?,
        &read(measure, "measure")?,
        test_floor
            .map(|path| read(path, "test-noise-floor"))
            .transpose()?
            .as_ref(),
    )?;
    crate::capture_actions::write_receipt(&record, output)?;
    Ok(record)
}

/// Planted slow-path controls for `migration self-test`, on synthetic
/// records of 30 samples and pairs: a 3x CPU and a 2x RSS candidate must
/// hold, an equal-cost one must activate. Each control takes the harness
/// under test, so a harness that accepts a slow path fails its control.
pub mod controls {
    use super::*;

    pub type Harness = fn(&Value, &Value, &Value) -> Value;

    pub fn run() -> usize {
        let cases: [fn(Harness); 3] = [slow_cpu_holds, big_rss_holds, equal_cost_activates];
        for case in cases {
            case(real);
        }
        cases.len()
    }

    fn real(base: &Value, verify: &Value, measure: &Value) -> Value {
        // A fixed zero floor keeps the 1.10 ratio, so a registered noise
        // floor for rust-03 cannot change what these controls prove.
        let floor = json!({
            "cpu_floor_us": 0, "cpu_absolute_us": 0,
            "rss_floor_bytes": 0, "rss_absolute_bytes": 0,
        });
        verdict("rust-03", base, verify, measure, Some(&floor))
            .expect("synthetic records are valid")
    }

    fn base() -> Value {
        let samples: Vec<Value> = (1..=30)
            .map(
                |run| json!({"run": run, "tree_cpu_us": 100_000, "tree_max_rss_bytes": 50_000_000}),
            )
            .collect();
        json!({
            "schema": baseline::SCHEMA, "step": "rust-03", "os": "linux",
            "runs": 30, "meets_protocol_minimum": true, "source_tree_dirty": false,
            "source_revision": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "evidence_sources_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "fixtures_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "cache_state": baseline::CACHE_STATE, "samples": samples,
            "summary": {"cpu_us_p95": 100_000},
        })
    }

    fn verify() -> Value {
        let violations: serde_json::Map<String, Value> = VIOLATIONS
            .iter()
            .map(|name| ((*name).to_string(), json!(0)))
            .collect();
        json!({
            "schema": VERIFY_SCHEMA, "step": "rust-03",
            "source_revision": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "evidence_sources_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "fixtures_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "candidate_sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            "checks": [{"id": "actions-json", "passed": true}],
            "violations": violations,
            "unobserved_violations": [],
            "missing_contracts": baseline::step("rust-03").expect("registered").scoped_out(),
            "test_scope": null, "python_denied": "env",
        })
    }

    fn measure(cpu: u64, rss: u64) -> Value {
        let regime = |state: &str| {
            json!({
                "cache_state": state, "pair_ids": (1..=30).collect::<Vec<u64>>(), "cached": 0,
                "oracle_cpu_us": vec![100_000u64; 30],
                "oracle_tree_max_rss_bytes": vec![50_000_000u64; 30],
                "candidate_cpu_us": vec![cpu; 30], "candidate_tree_max_rss_bytes": vec![rss; 30],
            })
        };
        json!({"schema": MEASURE_SCHEMA, "step": "rust-03", "os": "linux",
               "source_revision": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
               "evidence_sources_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
               "fixtures_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
               "candidate_sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
               "regimes": [regime("cold"), regime("warm")]})
    }

    fn holds(record: &Value, reason: &str) {
        assert_eq!(
            record["decision"], "keep-python",
            "planted {reason} accepted"
        );
        assert_eq!(record["reasons"], json!([reason]), "planted {reason}");
    }

    pub fn slow_cpu_holds(harness: Harness) {
        let record = harness(&base(), &verify(), &measure(300_000, 50_000_000));
        holds(&record, "cpu-regressed");
    }

    pub fn big_rss_holds(harness: Harness) {
        let record = harness(&base(), &verify(), &measure(100_000, 100_000_000));
        holds(&record, "rss-regressed");
    }

    pub fn equal_cost_activates(harness: Harness) {
        let record = harness(&base(), &verify(), &measure(100_000, 50_000_000));
        assert_eq!(record["decision"], "activate", "equal cost held: {record}");
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn always_activate(_: &Value, _: &Value, _: &Value) -> Value {
            json!({"decision": "activate", "reasons": []})
        }

        #[test]
        fn controls_pass_on_the_real_harness() {
            assert_eq!(run(), 3);
        }

        #[test]
        #[should_panic(expected = "planted cpu-regressed accepted")]
        fn slow_cpu_control_fails_an_accepting_harness() {
            slow_cpu_holds(always_activate);
        }

        fn always_hold(_: &Value, _: &Value, _: &Value) -> Value {
            json!({"decision": "keep-python", "reasons": ["cpu-regressed"]})
        }

        #[test]
        #[should_panic(expected = "equal cost held")]
        fn equal_cost_control_fails_a_holding_harness() {
            equal_cost_activates(always_hold);
        }

        #[test]
        #[should_panic(expected = "planted rss-regressed accepted")]
        fn big_rss_control_fails_an_accepting_harness() {
            big_rss_holds(always_activate);
        }

        /// A vector step's verify record lists no missing contracts and
        /// carries no `test_scope` or `python_denied`; it must still activate.
        #[test]
        fn a_vector_step_record_activates() {
            let mut base = base();
            base["step"] = json!("rust-02");
            let mut verify = verify();
            verify["step"] = json!("rust-02");
            verify["missing_contracts"] = json!([]);
            let record = verify.as_object_mut().unwrap();
            record.remove("test_scope");
            record.remove("python_denied");
            let mut measure = measure(100_000, 50_000_000);
            measure["step"] = json!("rust-02");
            let floor = json!({
                "cpu_floor_us": 0, "cpu_absolute_us": 0,
                "rss_floor_bytes": 0, "rss_absolute_bytes": 0,
            });
            let record = verdict("rust-02", &base, &verify, &measure, Some(&floor)).unwrap();
            assert_eq!(record["decision"], "activate", "{record}");
        }
    }
}
