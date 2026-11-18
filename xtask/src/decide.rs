//! `migration decide --step <id> --baseline <file> --verify <file> --measure <file>`:
//! read the three content-free records of one step, apply the decide limits
//! of docs/rust-migration.md, and return a content-free verdict record
//! (`activate` or `keep-python` with fixed reason ids). A malformed record,
//! a wrong schema or a step mismatch is refused: no verdict is written.
//!
//! Input shapes (minimal, until `verify` and `measure` land):
//! - baseline: `careerbrew-migration-baseline-v1` from `baseline.rs`; reads
//!   `step`, `meets_protocol_minimum` (checked against the sample count),
//!   `source_tree_dirty`, `cache_state` (must be the baseline regime),
//!   `summary.cpu_us_p95` and the `tree_max_rss_bytes` of each sample.
//! - verify: `careerbrew-migration-verify-v1` with `step`, a nonempty
//!   `checks` list of `{id, passed}`, a `violations` map of counts and an
//!   `unobserved_violations` list of violation names verify cannot see yet
//!   (each one keeps Python: an unobserved count is never a zero), and a
//!   `missing_contracts` list of contracts verify did not check (each one
//!   keeps Python too).
//! - measure: `careerbrew-migration-measure-v1` with `step` and a nonempty
//!   `regimes` list of `{cache_state, pair_ids, cached, candidate_cpu_us,
//!   candidate_tree_max_rss_bytes}`; one sample per pair. Both `cold` and
//!   `warm` must be present, each with at least 30 unique pairs.
use crate::{baseline, baseline_verdict};
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
/// No noise floor is registered yet, so every case uses the 110% ratio.
const LIMIT: baseline_verdict::ResourceLimit = baseline_verdict::ResourceLimit {
    floor: 0,
    absolute: 0,
};

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
pub fn verdict(step: &str, base: &Value, verify: &Value, measure: &Value) -> Result<Value, String> {
    baseline::step(step)?;
    header(base, "baseline", baseline::SCHEMA, step)?;
    header(verify, "verify", VERIFY_SCHEMA, step)?;
    header(measure, "measure", MEASURE_SCHEMA, step)?;

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
    if !missing_contracts.is_empty() {
        reasons.insert("verify-contract-missing");
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
        let cpu = numbers(&regime["candidate_cpu_us"], "measure candidate_cpu_us")?;
        let rss = numbers(
            &regime["candidate_tree_max_rss_bytes"],
            "measure candidate_tree_max_rss_bytes",
        )?;
        if pairs.is_empty() || cpu.len() != pairs.len() || rss.len() != pairs.len() {
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
        if !baseline_verdict::within_limit(base_cpu, cpu_p95, LIMIT) {
            reasons.insert("cpu-regressed");
        }
        if !baseline_verdict::within_limit(base_rss, rss_max, LIMIT) {
            reasons.insert("rss-regressed");
        }
        summary.push(json!({
            "cache_state": state,
            "pairs": pairs.len(),
            "candidate_cpu_us_p95": cpu_p95,
            "candidate_tree_max_rss_bytes": rss_max,
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
        "noise_floor": "unregistered",
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
    )?;
    crate::capture_actions::write_receipt(&record, output)?;
    Ok(record)
}
