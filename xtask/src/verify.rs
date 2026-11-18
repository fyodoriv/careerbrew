//! `migration verify --step <id>`: run a registered step's Python oracle and
//! its Rust candidate on the same synthetic cases, each side in its own fresh
//! synthetic home, and compare three contracts:
//! - CLI: stdout and the exit code, byte for byte;
//! - side effects: the entries each side leaves in its synthetic home
//!   (path, mode and content hash), as extra, missing or changed entries;
//! - HTTP (steps with `http_contract`): the `capture-http` fixture cases sent
//!   over a real socket, compared on status, normalized headers and body.
//!   The live oracle must still match the frozen `capture-http` reference.
//!
//! It writes a content-free JSON record: step, per-case pass or fail with
//! hashes and counts, the sources hash, the source revision, whether the
//! candidate came from the test-only flag, and the candidate binary's sha256
//! (checked before and after the runs). The record is also `decide`'s verify
//! input: a `checks` list of `{id, passed}`, a `violations` map of counts,
//! and the `unobserved_violations` this verify cannot see yet. A candidate
//! that cannot run, times out or prints output that is not UTF-8 or too
//! large fails its case. A step with no Rust candidate is refused, never
//! passed.
use crate::{baseline, capture_actions, capture_http, decide};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

pub const SCHEMA: &str = "careerbrew-migration-verify-v1";
/// The oracle arguments before a case's CLI arguments.
pub const ORACLE_PREFIX: &[&str] = &["-B", "-m", "careerbrew"];
/// Violation kinds this verify has no observation for yet. `decide` keeps
/// Python while any is listed: an unobserved count is never a zero.
pub const UNOBSERVED: &[&str] = &["duplicate_writes", "role"];
/// HTTP statuses that refuse access; a candidate that answers another status
/// on such a case is an authorization violation.
const DENIED: &[u64] = &[401, 403];

type Writes = BTreeMap<String, String>;

/// Runs one side in a fresh synthetic home; returns its exit code, stdout
/// and the entries it leaves in that home.
fn side(
    root: &Path,
    program: &Path,
    args: &[&str],
) -> Result<(Option<i32>, String, Writes), String> {
    let scratch = tempfile::tempdir().map_err(|_| "cannot create verify home")?;
    let owned = scratch.path().join("owned");
    let environment = capture_actions::owned_environment(root, &owned)?;
    if environment
        .get("CAREERBREW_HOME")
        .is_none_or(|home| home.is_empty())
    {
        return Err("synthetic CAREERBREW_HOME is empty".into());
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(root)
        .env_clear()
        .envs(&environment);
    let (_, exit, stdout, _) = capture_actions::execute(command)?;
    let writes = capture_actions::files(&owned, &owned, None)?;
    Ok((exit, stdout, writes))
}

/// The counts of entries only the candidate left (extra), only the oracle
/// left (missing), and both left with another mode or content (changed).
fn diff(oracle: &Writes, candidate: &Writes) -> (u64, u64, u64) {
    let extra = candidate
        .keys()
        .filter(|k| !oracle.contains_key(*k))
        .count();
    let missing = oracle
        .keys()
        .filter(|k| !candidate.contains_key(*k))
        .count();
    let changed = oracle
        .iter()
        .filter(|(k, v)| candidate.get(*k).is_some_and(|other| other != *v))
        .count();
    (extra as u64, missing as u64, changed as u64)
}

fn writes_sha256(writes: &Writes) -> String {
    capture_actions::digest(json!(writes).to_string().as_bytes())
}

/// The capture sources plus the baseline, verify and HTTP sources, as one hash.
fn sources_sha256(root: &Path) -> Result<String, String> {
    let mut sources = capture_actions::sources(root)?;
    let map = sources
        .as_object_mut()
        .ok_or("capture sources are not a map")?;
    for name in [
        "xtask/src/baseline.rs",
        "xtask/src/verify.rs",
        "xtask/src/capture_http.rs",
        "xtask/tests/fixtures/capture/http-v1.json",
        "xtask/tests/fixtures/capture/http_driver.py",
        "xtask/tests/fixtures/capture/http-reference-telemetry-v4.json",
    ] {
        let bytes = std::fs::read(root.join(name)).map_err(|_| "verify source missing")?;
        map.insert(name.to_owned(), json!(capture_actions::digest(&bytes)));
    }
    Ok(capture_actions::digest(sources.to_string().as_bytes()))
}

/// The sha256 of the candidate binary's bytes.
fn binary_sha256(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|_| "cannot read the candidate binary")?;
    Ok(capture_actions::digest(&bytes))
}

/// Running violation counts, in `decide::VIOLATIONS` order of names.
#[derive(Default)]
struct Violations {
    contract: u64,
    privacy: u64,
    authorization: u64,
    side_effect: u64,
    missing_writes: u64,
}

impl Violations {
    fn total(&self) -> u64 {
        self.contract + self.privacy + self.authorization + self.side_effect + self.missing_writes
    }

    fn to_json(&self) -> Value {
        let mut map = serde_json::Map::new();
        for name in decide::VIOLATIONS {
            let count = match *name {
                "contract" => self.contract,
                "privacy" => self.privacy,
                "authorization" => self.authorization,
                "side_effect" => self.side_effect,
                "missing_writes" => self.missing_writes,
                _ => 0,
            };
            map.insert((*name).to_owned(), json!(count));
        }
        Value::Object(map)
    }
}

/// Runs the CLI cases on both sides; pushes one entry per case.
fn cli_cases(
    root: &Path,
    python: &Path,
    candidate: &Path,
    step: &baseline::Step,
    cases: &mut Vec<Value>,
    violations: &mut Violations,
) -> Result<(), String> {
    for (case, args) in step.cases {
        let mut oracle_args = ORACLE_PREFIX.to_vec();
        oracle_args.extend_from_slice(args);
        let (oracle_exit, oracle_stdout, oracle_writes) = side(root, python, &oracle_args)?;
        if oracle_exit != Some(0) {
            return Err(format!("oracle case {case} did not exit 0; verify refused"));
        }
        let oracle_stdout_sha256 = capture_actions::digest(oracle_stdout.as_bytes());
        // A spawn failure, timeout, non-UTF-8 or oversize output fails the
        // case; the reason text stays out of the record (it may hold paths).
        let entry = match side(root, candidate, args) {
            Ok((candidate_exit, candidate_stdout, candidate_writes)) => {
                let exit_matches = candidate_exit == oracle_exit;
                let stdout_matches = candidate_stdout.as_bytes() == oracle_stdout.as_bytes();
                let (extra, missing, changed) = diff(&oracle_writes, &candidate_writes);
                let writes_match = extra + missing + changed == 0;
                violations.contract += u64::from(!(exit_matches && stdout_matches));
                violations.side_effect += extra + changed;
                violations.missing_writes += missing;
                json!({
                    "case": case,
                    "kind": "cli",
                    "passed": exit_matches && stdout_matches && writes_match,
                    "candidate_ran": true,
                    "exit_matches": exit_matches,
                    "stdout_matches": stdout_matches,
                    "oracle_stdout_sha256": oracle_stdout_sha256,
                    "candidate_stdout_sha256": capture_actions::digest(candidate_stdout.as_bytes()),
                    "writes_match": writes_match,
                    "oracle_writes": oracle_writes.len(),
                    "candidate_writes": candidate_writes.len(),
                    "extra_writes": extra,
                    "missing_writes": missing,
                    "changed_writes": changed,
                    "oracle_writes_sha256": writes_sha256(&oracle_writes),
                    "candidate_writes_sha256": writes_sha256(&candidate_writes),
                })
            }
            Err(_) => {
                violations.contract += 1;
                json!({
                    "case": case,
                    "kind": "cli",
                    "passed": false,
                    "candidate_ran": false,
                    "exit_matches": false,
                    "stdout_matches": false,
                    "oracle_stdout_sha256": oracle_stdout_sha256,
                    "candidate_stdout_sha256": Value::Null,
                    "writes_match": false,
                    "oracle_writes": oracle_writes.len(),
                    "oracle_writes_sha256": writes_sha256(&oracle_writes),
                })
            }
        };
        cases.push(entry);
    }
    Ok(())
}

/// Runs the HTTP fixture cases on both servers; pushes one entry per case.
fn http_cases(
    root: &Path,
    python: &Path,
    candidate: &Path,
    cases: &mut Vec<Value>,
    violations: &mut Violations,
) -> Result<Value, String> {
    let driver = root.join("xtask/tests/fixtures/capture/http_driver.py");
    let oracle = capture_http::serve(
        root,
        python,
        &["-B".to_owned(), driver.to_string_lossy().into_owned()],
    )?;
    let projected: Option<Vec<Value>> = oracle
        .responses
        .iter()
        .map(|response| response.as_ref().map(capture_http::project))
        .collect();
    if projected.map(Value::from) != Some(capture_http::reference()?) || oracle.canary_leaks > 0 {
        return Err(
            "the HTTP oracle differs from the capture-http reference; verify refused".into(),
        );
    }
    let served = capture_http::serve(root, candidate, &[capture_http::CONTRACT_FLAG.to_owned()])?;
    violations.privacy += served.canary_leaks;
    for ((name, expected), answer) in oracle
        .names
        .iter()
        .zip(&oracle.responses)
        .zip(&served.responses)
    {
        let expected = expected.as_ref().ok_or("HTTP oracle response missing")?;
        let body_sha256 = |r: &Value| capture_actions::digest(r["body"].to_string().as_bytes());
        let entry = match answer {
            Some(answer) => {
                let status_matches = answer["status"] == expected["status"];
                let headers_match = answer["headers"] == expected["headers"]
                    && answer["status_line"] == expected["status_line"];
                let body_matches = answer["body"] == expected["body"];
                let passed = status_matches && headers_match && body_matches;
                violations.contract += u64::from(!passed);
                let denied = expected["status"]
                    .as_u64()
                    .is_some_and(|status| DENIED.contains(&status));
                violations.authorization += u64::from(denied && !status_matches);
                json!({
                    "case": format!("http:{name}"),
                    "kind": "http",
                    "passed": passed,
                    "candidate_answered": true,
                    "status_matches": status_matches,
                    "headers_match": headers_match,
                    "body_matches": body_matches,
                    "oracle_body_sha256": body_sha256(expected),
                    "candidate_body_sha256": body_sha256(answer),
                })
            }
            None => {
                violations.contract += 1;
                json!({
                    "case": format!("http:{name}"),
                    "kind": "http",
                    "passed": false,
                    "candidate_answered": false,
                    "status_matches": false,
                    "headers_match": false,
                    "body_matches": false,
                    "oracle_body_sha256": body_sha256(expected),
                    "candidate_body_sha256": Value::Null,
                })
            }
        };
        cases.push(entry);
    }
    let (extra, missing, changed) = diff(&oracle.after, &served.after);
    violations.side_effect += extra + changed;
    violations.missing_writes += missing;
    Ok(json!({
        "writes_match": extra + missing + changed == 0,
        "extra_writes": extra,
        "missing_writes": missing,
        "changed_writes": changed,
        "canary_leaks": served.canary_leaks,
    }))
}

/// Builds the record. `candidate` is the test-only override; without it the
/// step's registered candidate is used.
pub fn record(
    root: &Path,
    id: &str,
    python: &Path,
    candidate: Option<&Path>,
) -> Result<Value, String> {
    let step = baseline::step(id)?;
    if !python.is_absolute() {
        return Err("--python must be an absolute path".into());
    }
    if std::env::var_os("CAREERBREW_HOME").is_some_and(|home| home.is_empty()) {
        return Err("CAREERBREW_HOME is set but empty".into());
    }
    let registered = step.candidate.map(|path| root.join(path));
    let test_candidate = candidate.is_some();
    let Some(candidate) = candidate.map(Path::to_path_buf).or(registered) else {
        return Err(format!(
            "step {} has no Rust candidate yet; verify refused",
            step.id
        ));
    };
    if !candidate.is_absolute() {
        return Err("the candidate must be an absolute path".into());
    }
    if !candidate.is_file() {
        return Err("the candidate binary does not exist".into());
    }
    let hash = sources_sha256(root)?;
    let candidate_sha256 = binary_sha256(&candidate)?;
    let mut cases = Vec::new();
    let mut violations = Violations::default();
    cli_cases(root, python, &candidate, step, &mut cases, &mut violations)?;
    let http = if step.http_contract {
        http_cases(root, python, &candidate, &mut cases, &mut violations)?
    } else {
        Value::Null
    };
    if sources_sha256(root)? != hash {
        return Err("a relevant source changed during verify".into());
    }
    if binary_sha256(&candidate)? != candidate_sha256 {
        return Err("the candidate binary changed during verify".into());
    }
    let failed: Vec<String> = cases
        .iter()
        .filter(|case| case["passed"] != true)
        .filter_map(|case| case["case"].as_str().map(str::to_owned))
        .collect();
    let checks: Vec<Value> = cases
        .iter()
        .map(|case| json!({"id": case["case"], "passed": case["passed"]}))
        .collect();
    Ok(json!({
        "schema": SCHEMA,
        "step": step.id,
        "source_revision": baseline::revision(root)?,
        "source_tree_dirty": baseline::dirty(root)?,
        "sources_sha256": hash,
        "test_candidate": test_candidate,
        "candidate_sha256": candidate_sha256,
        "contracts": if step.http_contract { json!(["cli", "side-effect", "http"]) } else { json!(["cli", "side-effect"]) },
        "case_count": cases.len(),
        "passed_count": cases.len() - failed.len(),
        "failed_cases": failed,
        "passed": failed.is_empty() && violations.total() == 0,
        "checks": checks,
        "violations": violations.to_json(),
        "unobserved_violations": UNOBSERVED,
        "http": http,
        "cases": cases,
    }))
}

/// Writes the record to a new absolute file. The record is written even when
/// a case fails, so the caller can read which one.
pub fn run(
    root: &Path,
    id: &str,
    python: &Path,
    candidate: Option<&Path>,
    output: &Path,
) -> Result<Value, String> {
    if !output.is_absolute() {
        return Err("--out must be an absolute path".into());
    }
    if output.exists() {
        return Err("verify output already exists".into());
    }
    let record = record(root, id, python, candidate)?;
    capture_actions::write_receipt(&record, output)?;
    Ok(record)
}
