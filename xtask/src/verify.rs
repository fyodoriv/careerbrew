//! `migration verify --step <id>`: run a registered step's Python oracle and
//! its Rust candidate on the same synthetic cases, each side in its own fresh
//! synthetic home, and compare three contracts:
//! - CLI: stdout and the exit code, byte for byte, after the case's
//!   registered normalization rules (`normalize.rs`) apply to both stdouts;
//!   the stdout hashes are of the normalized text. A local path no rule
//!   covers refuses verify;
//! - side effects: the entries each side adds, changes or removes in its
//!   synthetic home against the setup snapshot taken before it runs (path,
//!   mode and content hash), as extra, missing or changed entries. This is a
//!   final-state comparison, not a write trace: a rewrite with the same
//!   bytes, or a write that is later removed, is not seen (`duplicate_writes`
//!   stays unobserved). Entries are keyed by their path inside each side's
//!   own synthetic home, so the two homes need no path normalization;
//! - HTTP (steps with `http_contract`): the `capture-http` fixture cases sent
//!   over a real socket, compared on status, normalized headers and body.
//!   The live oracle must still match the frozen `capture-http` reference.
//!   The HTTP side effects are one more case, `http:side-effects`;
//! - registry projection (steps with `registry_projection`): the candidate's
//!   `actions --json` stdout, wrapped as `{"actions": ...}`, must equal the
//!   body of the frozen capture-http `/api/actions` cases (0 and 4). It is
//!   the offline stand-in for a step that serves no HTTP: no server starts.
//!   The live Python oracle must match the same bodies, or verify refuses;
//! - DATA (steps with `data_contract`): the `capture-data` corpus (10 cases, 19 phases) run in order in
//!   one fresh synthetic home per side: the oracle is the Python data driver
//!   (run twice, in two homes, and it must agree with itself), the candidate
//!   gets `--careerbrew-data-contract <trial> <case> <role>`. Each phase's
//!   API calls (function, purpose, target, argument hash, returned value)
//!   must match. A write trace snapshots the home after each phase (mode,
//!   inode and mtime): a path the candidate writes in more phases than the
//!   oracle counts as `duplicate_writes`. The trace has phase granularity: a
//!   path written twice inside one phase is one write. File contents are
//!   left out of the DATA side effects (rule `data-ciphertext`: each home has
//!   its own key); the reader phases prove the content;
//! - role (steps with `role_contract`): the Python oracle makes a fresh home a corporate machine, then
//!   each side runs a job-hunt command (`jobs list`); the oracle refuses it,
//!   and a candidate with another exit code, stdout or writes is a `role`
//!   violation.
//!
//! A contract a step scopes out is listed in `missing_contracts`; `decide`
//! requires that list to equal the step's declared scope. The CLI and
//! registry projection runs get the step's extra environment (`Step::env`)
//! on both sides; the HTTP, DATA and role runs (only under a step that keeps
//! them, or `--test-scope full`) do not. A step with
//! `no_python` runs each candidate side with Python denied (`NO_PYTHON_HOME`).
//!
//! It writes a content-free JSON record: step, per-case pass or fail with
//! hashes and counts, the sources hash, the source revision, whether the
//! candidate or the oracle came from a test-only flag, and the candidate
//! binary's sha256 (checked before and after the runs). The record is also
//! `decide`'s verify input: a `checks` list of `{id, passed}`, a
//! `violations` map of counts, and the `unobserved_violations` this verify
//! cannot see yet. A candidate that cannot run, times out or prints output
//! that is not UTF-8 or too large fails its case. A step with no Rust
//! candidate is refused, never passed.
use crate::{baseline, capture_actions, capture_http, decide, depvec, normalize, vecdriver};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

pub const SCHEMA: &str = "careerbrew-migration-verify-v1";
/// The oracle arguments before a case's CLI arguments.
pub const ORACLE_PREFIX: &[&str] = &["-B", "-m", "careerbrew"];
/// Violation kinds this verify has no observation for yet. `decide` keeps
/// Python while any is listed: an unobserved count is never a zero.
pub const UNOBSERVED: &[&str] = &[];
/// Contracts a step needs that this verify does not check yet.
pub const MISSING_CONTRACTS: &[&str] = &[];
/// Python denial for a candidate run of a `no_python` step. The synthetic
/// environment has no `PATH`, so a bare `python3` is not found; on top of
/// that `PYTHONHOME` points at this missing folder inside the side's scratch
/// folder, so a CPython started by absolute path (without `-E` or `-I`)
/// cannot load its standard library and exits nonzero. A candidate that
/// clears its environment or passes `-E`/`-I` is not caught: the record says
/// `python_denied: "env"`, not a process-level proof.
pub const NO_PYTHON_HOME: &str = "no-python";
/// The test-only scopes `--test-scope` accepts.
pub const TEST_SCOPES: &[&str] = &["full", "allow-python"];
/// The marker an entry the run removed gets in its side-effect map.
const REMOVED: &str = "removed";
/// HTTP statuses that refuse access; a candidate that answers another status
/// or another body on such a case, or no answer, is an authorization
/// violation.
const DENIED: &[u64] = &[401, 403];

type Writes = BTreeMap<String, String>;

/// The entries a run added or changed against the setup snapshot, plus the
/// setup entries it removed (marked `removed`).
fn effects(before: &Writes, after: &Writes) -> Writes {
    let mut effects: Writes = after
        .iter()
        .filter(|(k, v)| before.get(*k) != Some(*v))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for key in before.keys().filter(|k| !after.contains_key(*k)) {
        effects.insert(key.clone(), REMOVED.to_owned());
    }
    effects
}

/// A side's stdout held a local path that no registered rule covers.
struct UnregisteredPath;

/// How one side runs: the step's extra environment, and whether Python is
/// denied to it.
struct Run {
    env: &'static [(&'static str, &'static str)],
    deny_python: bool,
}

impl Run {
    fn apply(&self, environment: &mut BTreeMap<String, String>, scratch: &Path) {
        for (key, value) in self.env {
            environment.insert((*key).to_owned(), (*value).to_owned());
        }
        if self.deny_python {
            environment.remove("PYTHONPATH");
            environment.insert(
                "PYTHONHOME".to_owned(),
                scratch.join(NO_PYTHON_HOME).to_string_lossy().into_owned(),
            );
        }
    }
}

/// Runs one side in a fresh synthetic home; returns its exit code, its
/// stdout after the case's normalization rules, and its side effects in
/// that home.
#[allow(clippy::type_complexity)]
fn side(
    root: &Path,
    program: &Path,
    args: &[&str],
    rules: &[&str],
    run: &Run,
) -> Result<Result<(Option<i32>, String, Writes), UnregisteredPath>, String> {
    let scratch = tempfile::tempdir().map_err(|_| "cannot create verify home")?;
    let owned = scratch.path().join("owned");
    let mut environment = capture_actions::owned_environment(root, &owned)?;
    run.apply(&mut environment, scratch.path());
    let home = environment
        .get("CAREERBREW_HOME")
        .filter(|home| !home.is_empty())
        .ok_or("synthetic CAREERBREW_HOME is empty")?
        .clone();
    let before = capture_actions::files(&owned, &owned, None)?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(root)
        .env_clear()
        .envs(&environment);
    let (_, exit, stdout, _) = capture_actions::execute(command)?;
    let writes = effects(&before, &capture_actions::files(&owned, &owned, None)?);
    let side = normalize::Side {
        home: Path::new(&home),
        scratch: scratch.path(),
    };
    Ok(normalize::apply(rules, &side, root, &stdout)
        .map(|stdout| (exit, stdout, writes))
        .map_err(|()| UnregisteredPath))
}

fn unregistered(case: &str) -> String {
    format!("case {case} prints a local path with no registered normalization rule; verify refused")
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
        "xtask/src/normalize.rs",
        "xtask/src/capture_http.rs",
        "xtask/tests/fixtures/capture/http-v1.json",
        "xtask/tests/fixtures/capture/http_driver.py",
        "xtask/tests/fixtures/capture/http-reference-indie-v5.json",
        DATA_DRIVER,
        DATA_FIXTURE,
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
    role: u64,
    side_effect: u64,
    missing_writes: u64,
    duplicate_writes: u64,
}

impl Violations {
    fn total(&self) -> u64 {
        self.contract
            + self.privacy
            + self.authorization
            + self.role
            + self.side_effect
            + self.missing_writes
            + self.duplicate_writes
    }

    fn to_json(&self) -> Value {
        let mut map = serde_json::Map::new();
        for name in decide::VIOLATIONS {
            let count = match *name {
                "contract" => self.contract,
                "privacy" => self.privacy,
                "authorization" => self.authorization,
                "role" => self.role,
                "side_effect" => self.side_effect,
                "missing_writes" => self.missing_writes,
                "duplicate_writes" => self.duplicate_writes,
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
    deny_python: bool,
    cases: &mut Vec<Value>,
    violations: &mut Violations,
) -> Result<(), String> {
    let oracle_run = Run {
        env: step.env,
        deny_python: false,
    };
    let candidate_run = Run {
        env: step.env,
        deny_python,
    };
    for (case, args) in step.cases {
        let rules = normalize::rules(step.id, case);
        let mut oracle_args = ORACLE_PREFIX.to_vec();
        oracle_args.extend_from_slice(args);
        let (oracle_exit, oracle_stdout, oracle_writes) =
            side(root, python, &oracle_args, rules, &oracle_run)?
                .map_err(|UnregisteredPath| unregistered(case))?;
        // 0, or 2 for an argparse usage error (`actions --bogus`): the
        // candidate must then match that exit and its empty stdout.
        if !matches!(oracle_exit, Some(0 | 2)) {
            return Err(format!(
                "oracle case {case} did not exit 0 (or 2 for a usage error); verify refused"
            ));
        }
        let oracle_stdout_sha256 = capture_actions::digest(oracle_stdout.as_bytes());
        // A spawn failure, timeout, non-UTF-8 or oversize output fails the
        // case; the reason text stays out of the record (it may hold paths).
        let entry = match side(root, candidate, args, rules, &candidate_run) {
            Ok(Err(UnregisteredPath)) => return Err(unregistered(case)),
            Ok(Ok((candidate_exit, candidate_stdout, candidate_writes))) => {
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
                    "normalization": rules,
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
                    "normalization": rules,
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
                violations.authorization += u64::from(denied && !(status_matches && body_matches));
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
                violations.authorization += u64::from(
                    expected["status"]
                        .as_u64()
                        .is_some_and(|status| DENIED.contains(&status)),
                );
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
    let oracle_effects = effects(&oracle.before, &oracle.after);
    let served_effects = effects(&served.before, &served.after);
    let (extra, missing, changed) = diff(&oracle_effects, &served_effects);
    violations.side_effect += extra + changed;
    violations.missing_writes += missing;
    cases.push(json!({
        "case": "http:side-effects",
        "kind": "http-side-effect",
        "passed": extra + missing + changed == 0,
        "writes_match": extra + missing + changed == 0,
        "oracle_writes": oracle_effects.len(),
        "candidate_writes": served_effects.len(),
        "extra_writes": extra,
        "missing_writes": missing,
        "changed_writes": changed,
        "oracle_writes_sha256": writes_sha256(&oracle_effects),
        "candidate_writes_sha256": writes_sha256(&served_effects),
    }));
    Ok(json!({
        "writes_match": extra + missing + changed == 0,
        "extra_writes": extra,
        "missing_writes": missing,
        "changed_writes": changed,
        "canary_leaks": served.canary_leaks,
    }))
}

/// The CLI arguments the registry projection runs on both sides.
const PROJECTION_ARGS: &[&str] = &["actions", "--json"];
/// The frozen capture-http cases whose body is the action catalog
/// (`actions` and `localhost`, both `GET /api/actions`).
const PROJECTION_HTTP_CASES: &[(usize, &str)] = &[(0, "actions"), (4, "localhost")];
const PROJECTION_CASE: &str = "registry-projection";

/// `{"actions": <stdout as JSON>}`, or None when stdout is not JSON.
fn projection_body(stdout: &str) -> Option<Value> {
    serde_json::from_str::<Value>(stdout)
        .ok()
        .map(|rows| json!({"actions": rows}))
}

/// The registry projection: the candidate's `actions --json` stdout, wrapped
/// as `{"actions": ...}`, must equal the body of the frozen capture-http
/// `/api/actions` cases. The live Python oracle must match the same bodies,
/// or verify refuses. Pushes one entry.
fn registry_projection(
    root: &Path,
    python: &Path,
    candidate: &Path,
    step: &baseline::Step,
    deny_python: bool,
    cases: &mut Vec<Value>,
    violations: &mut Violations,
) -> Result<(), String> {
    let reference = capture_http::reference()?;
    let mut bodies = Vec::new();
    for (index, name) in PROJECTION_HTTP_CASES {
        let case = reference
            .get(*index)
            .filter(|case| case["name"] == *name && case["path"] == "/api/actions")
            .ok_or("the capture-http reference has no action catalog case")?;
        bodies.push(&case["body"]);
    }
    let matches = |body: &Option<Value>| {
        body.as_ref()
            .is_some_and(|body| bodies.iter().all(|expected| *expected == body))
    };
    let oracle_run = Run {
        env: step.env,
        deny_python: false,
    };
    let mut oracle_args = ORACLE_PREFIX.to_vec();
    oracle_args.extend_from_slice(PROJECTION_ARGS);
    let (oracle_exit, oracle_stdout, _) = side(root, python, &oracle_args, &[], &oracle_run)?
        .map_err(|UnregisteredPath| unregistered(PROJECTION_CASE))?;
    let oracle_body = projection_body(&oracle_stdout);
    if oracle_exit != Some(0) || !matches(&oracle_body) {
        return Err(
            "the registry projection oracle differs from the capture-http reference; verify refused"
                .into(),
        );
    }
    let candidate_run = Run {
        env: step.env,
        deny_python,
    };
    // A stdout with a local path cannot equal the path-free reference: it
    // fails the case. A spawn failure, timeout, non-UTF-8 or oversize output
    // fails it too; no reason text reaches the record.
    let answer = side(root, candidate, PROJECTION_ARGS, &[], &candidate_run);
    let (ran, exit_zero, body) = match answer {
        Ok(Ok((exit, stdout, _))) => (true, exit == Some(0), projection_body(&stdout)),
        Ok(Err(UnregisteredPath)) => (true, false, None),
        Err(_) => (false, false, None),
    };
    let body_matches = matches(&body);
    let passed = exit_zero && body_matches;
    violations.contract += u64::from(!passed);
    let digest = |body: &Option<Value>| {
        body.as_ref()
            .map(|body| capture_actions::digest(body.to_string().as_bytes()))
    };
    cases.push(json!({
        "case": PROJECTION_CASE,
        "kind": "registry-projection",
        "passed": passed,
        "candidate_ran": ran,
        "exit_zero": exit_zero,
        "body_matches": body_matches,
        "http_cases": PROJECTION_HTTP_CASES.iter().map(|(_, name)| format!("http:{name}")).collect::<Vec<_>>(),
        "oracle_body_sha256": digest(&oracle_body),
        "candidate_body_sha256": digest(&body),
    }));
    Ok(())
}

/// The flag verify passes when it runs one DATA contract phase on the
/// candidate; the arguments after it are the trial, the case id and the role.
pub const DATA_FLAG: &str = "--careerbrew-data-contract";
/// The Python data driver: the DATA oracle, one phase per process.
const DATA_DRIVER: &str = "xtask/tests/fixtures/capture/data_driver.py";
/// The bounded data corpus (`capture-data`), with its own sha256 check.
const DATA_FIXTURE: &str = "docs/plans/2026-11-05-rust-data-fixture-v1.json";
/// The trial name the driver records; verify runs one trial per side.
const DATA_TRIAL: &str = "first";
/// The setup that makes a synthetic home a corporate machine (run with the
/// Python oracle on both sides), and the job-hunt case it must refuse.
const ROLE_SETUP: &[&str] = &["machine", "set", "corporate"];
const ROLE_CASE: (&str, &[&str]) = ("role:jobs-list", &["jobs", "list"]);

/// One write-trace entry: the final-state entry plus inode and mtime, so a
/// rewrite with the same bytes still counts as a write.
type Trace = BTreeMap<String, (String, u64, i64, i64)>;

/// Walks the owned root for the DATA contract. `protocol/` is the report
/// channel and is left out. A file's entry is its mode only: data records
/// are encrypted with a fresh key in each home, so the ciphertext differs on
/// every run (rule `data-ciphertext`); the reader phases prove the content.
/// Adds each file that holds the corpus canary to `leaks`.
fn trace(
    owned: &Path,
    folder: &Path,
    canary: &str,
    out: &mut Trace,
    leaks: &mut BTreeSet<String>,
) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    for entry in std::fs::read_dir(folder).map_err(|_| "cannot list data home")? {
        let path = entry.map_err(|_| "cannot list data entry")?.path();
        let relative = path
            .strip_prefix(owned)
            .map_err(|_| "data entry escaped its home")?
            .to_string_lossy()
            .into_owned();
        if relative == "protocol" {
            continue;
        }
        let meta = std::fs::symlink_metadata(&path).map_err(|_| "cannot inspect data entry")?;
        let mode = meta.permissions().mode() & 0o777;
        let stamp = (meta.ino(), meta.mtime(), meta.mtime_nsec());
        if meta.is_dir() {
            out.insert(
                relative,
                (format!("directory:{mode:o}"), stamp.0, stamp.1, stamp.2),
            );
            trace(owned, &path, canary, out, leaks)?;
        } else if meta.is_file() {
            let bytes = std::fs::read(&path).map_err(|_| "cannot read data entry")?;
            if bytes
                .windows(canary.len())
                .any(|window| window == canary.as_bytes())
            {
                leaks.insert(relative.clone());
            }
            out.insert(
                relative,
                (format!("file:{mode:o}"), stamp.0, stamp.1, stamp.2),
            );
        } else {
            out.insert(relative, ("other".to_owned(), stamp.0, stamp.1, stamp.2));
        }
    }
    Ok(())
}

fn entries(trace: &Trace) -> Writes {
    trace
        .iter()
        .map(|(k, v)| (k.clone(), v.0.clone()))
        .collect()
}

/// Makes a fresh synthetic home with the corpus baseline folders, as
/// `capture-data` does, and returns its environment.
fn data_home(
    root: &Path,
    owned: &Path,
    fixture: &Value,
) -> Result<BTreeMap<String, String>, String> {
    use std::os::unix::fs::PermissionsExt;
    let environment = capture_actions::owned_environment(root, owned)?;
    for entry in fixture["baseline"]
        .as_array()
        .ok_or("data baseline missing")?
    {
        let path = owned.join(entry["path"].as_str().ok_or("data baseline path missing")?);
        let mode = u32::from_str_radix(entry["mode"].as_str().ok_or("data mode missing")?, 8)
            .map_err(|_| "data mode invalid")?;
        std::fs::create_dir_all(&path).map_err(|_| "cannot prepare data baseline")?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|_| "cannot set data baseline mode")?;
    }
    Ok(environment)
}

/// One side's result for one corpus case.
#[derive(PartialEq)]
struct DataCase {
    /// Per phase, the projected API calls, or None when the phase failed.
    calls: Vec<Option<Value>>,
    /// The case's side effects against the state before it.
    writes: Writes,
    /// How many phases wrote each path.
    counts: BTreeMap<String, u64>,
    leaked: bool,
}

/// The fields of a phase report that both sides must match.
fn project(report: &Value) -> Option<Value> {
    let calls = report["api_calls"].as_array()?;
    Some(json!(calls
        .iter()
        .map(|call| json!({
            "function": call["function"],
            "purpose": call["purpose"],
            "target": call["target"],
            "arguments_sha256": call["arguments_sha256"],
            "returned": call["returned"],
        }))
        .collect::<Vec<_>>()))
}

/// Runs the whole corpus on one side, in one fresh synthetic home (the cases
/// build on each other, as in `capture-data`). `prefix` comes before the
/// trial, case and role arguments.
fn data_side(
    root: &Path,
    program: &Path,
    prefix: &[&str],
    fixture: &Value,
) -> Result<Vec<DataCase>, String> {
    let canary = fixture["canary"].as_str().ok_or("data canary missing")?;
    let scratch = tempfile::tempdir().map_err(|_| "cannot create data home")?;
    let owned = scratch
        .path()
        .canonicalize()
        .map_err(|_| "cannot canonicalize data home")?
        .join("owned");
    let environment = data_home(root, &owned, fixture)?;
    let mut results = Vec::new();
    let mut state = Trace::new();
    let mut leaks = BTreeSet::new();
    trace(&owned, &owned, canary, &mut state, &mut leaks)?;
    for case in fixture["cases"].as_array().ok_or("data cases missing")? {
        let id = case["case"].as_str().ok_or("data case id missing")?;
        let start = state.clone();
        let start_leaks = leaks.clone();
        let mut calls = Vec::new();
        let mut counts = BTreeMap::new();
        for spec in case["phases"].as_array().ok_or("data phases missing")? {
            let role = spec["role"].as_str().ok_or("data role missing")?;
            let report = owned.join(format!("protocol/{id:0>2}-{role}-api.json"));
            let mut command = Command::new(program);
            command
                .args(prefix)
                .args([DATA_TRIAL, id, role])
                .current_dir(root)
                .env_clear()
                .envs(&environment)
                .env("CAREERBREW_DATA_FIXTURE", root.join(DATA_FIXTURE))
                .env("CAREERBREW_CAPTURE_ROOT", root)
                .env("CAREERBREW_DATA_OWNED", &owned)
                .env("CAREERBREW_DATA_REPORT", &report);
            let ran = matches!(capture_actions::execute(command), Ok((_, Some(0), _, _)));
            let projected = ran
                .then(|| std::fs::read(&report).ok())
                .flatten()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .filter(|value| value["case"] == id && value["role"] == role)
                .and_then(|value| project(&value));
            calls.push(projected);
            let mut after = Trace::new();
            leaks.clear();
            trace(&owned, &owned, canary, &mut after, &mut leaks)?;
            for (path, entry) in &after {
                if state.get(path) != Some(entry) {
                    *counts.entry(path.clone()).or_insert(0) += 1;
                }
            }
            for path in state.keys().filter(|path| !after.contains_key(*path)) {
                *counts.entry(path.clone()).or_insert(0) += 1;
            }
            state = after;
        }
        results.push(DataCase {
            calls,
            writes: effects(&entries(&start), &entries(&state)),
            counts,
            // Only paths that start to hold the canary in this case count, so
            // a leak is charged once, to the case that made it.
            leaked: !leaks.is_subset(&start_leaks),
        });
    }
    Ok(results)
}

/// Runs the DATA corpus on the oracle twice (it must agree with itself
/// across two fresh homes) and once on the candidate; pushes one entry per
/// corpus case.
fn data_cases(
    root: &Path,
    python: &Path,
    candidate: &Path,
    cases: &mut Vec<Value>,
    violations: &mut Violations,
) -> Result<(), String> {
    let bytes = std::fs::read(root.join(DATA_FIXTURE)).map_err(|_| "data fixture missing")?;
    let fixture: Value = serde_json::from_slice(&bytes).map_err(|_| "data fixture invalid")?;
    let driver = root.join(DATA_DRIVER);
    let driver = driver.to_str().ok_or("data driver path is not UTF-8")?;
    let oracle_prefix = ["-B", "-S", driver];
    let oracle = data_side(root, python, &oracle_prefix, &fixture)?;
    if oracle
        .iter()
        .any(|case| case.leaked || case.calls.iter().any(Option::is_none))
    {
        return Err("the DATA oracle failed a phase; verify refused".into());
    }
    if data_side(root, python, &oracle_prefix, &fixture)? != oracle {
        return Err("the DATA oracle differs across two fresh homes; verify refused".into());
    }
    let served = data_side(root, candidate, &[DATA_FLAG], &fixture)?;
    let ids = fixture["cases"].as_array().ok_or("data cases missing")?;
    for ((spec, expected), answer) in ids.iter().zip(&oracle).zip(&served) {
        let calls_match = answer.calls == expected.calls;
        let (extra, missing, changed) = diff(&expected.writes, &answer.writes);
        let duplicates: u64 = answer
            .counts
            .iter()
            .map(|(path, count)| {
                count.saturating_sub(expected.counts.get(path).copied().unwrap_or(0))
            })
            .sum();
        let passed = calls_match && extra + missing + changed + duplicates == 0 && !answer.leaked;
        violations.contract += u64::from(!calls_match);
        violations.side_effect += extra + changed;
        violations.missing_writes += missing;
        violations.duplicate_writes += duplicates;
        violations.privacy += u64::from(answer.leaked);
        let digest =
            |calls: &[Option<Value>]| capture_actions::digest(json!(calls).to_string().as_bytes());
        cases.push(json!({
            "case": format!("data:{}", spec["case"].as_str().unwrap_or("?")),
            "kind": "data",
            "passed": passed,
            "phases": expected.calls.len(),
            "candidate_phases_ran": answer.calls.iter().filter(|c| c.is_some()).count(),
            "calls_match": calls_match,
            "normalization": ["data-ciphertext"],
            "oracle_calls_sha256": digest(&expected.calls),
            "candidate_calls_sha256": digest(&answer.calls),
            "writes_match": extra + missing + changed == 0,
            "extra_writes": extra,
            "missing_writes": missing,
            "changed_writes": changed,
            "duplicate_writes": duplicates,
            "canary_leaked": answer.leaked,
            "oracle_writes_sha256": writes_sha256(&expected.writes),
            "candidate_writes_sha256": writes_sha256(&answer.writes),
        }));
    }
    Ok(())
}

/// Runs the role case on one side: the Python oracle makes a fresh synthetic
/// home a corporate machine, then `program` runs the job-hunt command.
#[allow(clippy::type_complexity)]
fn role_side(
    root: &Path,
    python: &Path,
    program: &Path,
    prefix: &[&str],
) -> Result<Result<(Option<i32>, String, Writes), ()>, String> {
    let scratch = tempfile::tempdir().map_err(|_| "cannot create role home")?;
    let owned = scratch.path().join("owned");
    let environment = capture_actions::owned_environment(root, &owned)?;
    let mut setup = Command::new(python);
    setup
        .args(ORACLE_PREFIX)
        .args(ROLE_SETUP)
        .current_dir(root)
        .env_clear()
        .envs(&environment);
    if capture_actions::execute(setup)?.1 != Some(0) {
        return Err("the corporate role setup failed; verify refused".into());
    }
    let before = capture_actions::files(&owned, &owned, None)?;
    let mut command = Command::new(program);
    command
        .args(prefix)
        .args(ROLE_CASE.1)
        .current_dir(root)
        .env_clear()
        .envs(&environment);
    let Ok((_, exit, stdout, _)) = capture_actions::execute(command) else {
        return Ok(Err(()));
    };
    let writes = effects(&before, &capture_actions::files(&owned, &owned, None)?);
    let home = environment
        .get("CAREERBREW_HOME")
        .ok_or("synthetic CAREERBREW_HOME is empty")?;
    let side = normalize::Side {
        home: Path::new(home),
        scratch: scratch.path(),
    };
    Ok(normalize::apply(&[], &side, root, &stdout).map(|stdout| (exit, stdout, writes)))
}

/// The corporate-role case: the oracle refuses a job-hunt command on a
/// corporate machine; a candidate that answers another way, prints another
/// text or writes another entry is a role violation.
fn role_case(
    root: &Path,
    python: &Path,
    candidate: &Path,
    cases: &mut Vec<Value>,
    violations: &mut Violations,
) -> Result<(), String> {
    let (case, _) = ROLE_CASE;
    let (oracle_exit, oracle_stdout, oracle_writes) =
        role_side(root, python, python, ORACLE_PREFIX)?.map_err(|()| unregistered(case))?;
    if oracle_exit.is_none_or(|code| code == 0) {
        return Err(
            "the oracle did not refuse the job-hunt command on a corporate machine; verify refused"
                .into(),
        );
    }
    let answer = role_side(root, python, candidate, &[])?.ok();
    let (exit_matches, stdout_matches, extra, missing, changed) = match &answer {
        Some((exit, stdout, writes)) => {
            let (extra, missing, changed) = diff(&oracle_writes, writes);
            (
                *exit == oracle_exit,
                *stdout == oracle_stdout,
                extra,
                missing,
                changed,
            )
        }
        None => (false, false, 0, 0, 0),
    };
    let passed = exit_matches && stdout_matches && extra + missing + changed == 0;
    violations.role += u64::from(!passed);
    cases.push(json!({
        "case": case,
        "kind": "role",
        "passed": passed,
        "candidate_ran": answer.is_some(),
        "oracle_refused": true,
        "exit_matches": exit_matches,
        "stdout_matches": stdout_matches,
        "writes_match": answer.is_some() && extra + missing + changed == 0,
        "extra_writes": extra,
        "missing_writes": missing,
        "changed_writes": changed,
        "oracle_stdout_sha256": capture_actions::digest(oracle_stdout.as_bytes()),
    }));
    Ok(())
}

/// Builds the record. `candidate` is the test-only override; without it the
/// step's registered candidate is used. `test_oracle` is the test-only
/// script run in place of `python` (it gets the same arguments).
#[allow(clippy::too_many_arguments)]
pub fn record(
    root: &Path,
    id: &str,
    python: &Path,
    candidate: Option<&Path>,
    test_oracle: Option<&Path>,
    wrong: Option<&str>,
    plant_write: Option<&str>,
    test_scope: Option<&str>,
) -> Result<Value, String> {
    if test_scope.is_some_and(|scope| !TEST_SCOPES.contains(&scope)) {
        return Err("--test-scope must be full or allow-python".into());
    }
    // A vector-driver step (rust-04): one generated file feeds both drivers.
    if let Some(step) = vecdriver::step(id) {
        if test_scope.is_some() || test_oracle.is_some() || plant_write.is_some() {
            return Err(
                "--test-scope, --test-oracle and --test-plant-home-write do not apply to a vector-driver step"
                    .into(),
            );
        }
        let binary = candidate.map_or_else(|| root.join(step.candidate), Path::to_path_buf);
        let mut record = vecdriver::record(
            root,
            python,
            Path::new(step.generator),
            Path::new(step.driver),
            vecdriver::Candidate::Binary(&binary),
            if wrong.is_some() {
                vecdriver::Plant::ChangeByte
            } else {
                vecdriver::Plant::None
            },
            vecdriver::Scope {
                only: Some(step.sets),
                plant_set: wrong,
            },
        )?;
        record["schema"] = json!(SCHEMA);
        record["step"] = json!(step.id);
        record["planted_wrong_vector"] = json!(wrong);
        return Ok(record);
    }
    // A vector step (rust-01, rust-02) has no CLI contract: it compares vectors.
    if let Some(step) = depvec::step(id) {
        if test_scope.is_some() {
            return Err("--test-scope applies only to a CLI step".into());
        }
        return depvec::record(
            root,
            step,
            python,
            candidate,
            test_oracle,
            wrong,
            plant_write,
        );
    }
    if wrong.is_some() {
        return Err("--test-wrong-vector applies only to a vector step".into());
    }
    if plant_write.is_some() {
        return Err("--test-plant-home-write applies only to a vector step".into());
    }
    let step = baseline::step(id)?;
    if !python.is_absolute() {
        return Err("--python must be an absolute path".into());
    }
    if test_oracle.is_some_and(|oracle| !oracle.is_absolute()) {
        return Err("--test-oracle must be an absolute path".into());
    }
    let oracle = test_oracle.unwrap_or(python);
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
    // `full` is the surface every CLI step had before contracts were scoped
    // (HTTP, DATA and role, Python allowed, no registry projection: the HTTP
    // contract compares the same /api/actions bodies over a socket).
    let full = test_scope == Some("full");
    let deny_python = step.no_python && test_scope.is_none();
    let (http_on, data_on, role_on) = if full {
        (true, true, true)
    } else {
        (step.http_contract, step.data_contract, step.role_contract)
    };
    let projection_on = step.registry_projection && !full;
    let mut cases = Vec::new();
    let mut violations = Violations::default();
    cli_cases(
        root,
        oracle,
        &candidate,
        step,
        deny_python,
        &mut cases,
        &mut violations,
    )?;
    let http = if http_on {
        http_cases(root, python, &candidate, &mut cases, &mut violations)?
    } else {
        Value::Null
    };
    if projection_on {
        registry_projection(
            root,
            python,
            &candidate,
            step,
            deny_python,
            &mut cases,
            &mut violations,
        )?;
    }
    if data_on {
        data_cases(root, python, &candidate, &mut cases, &mut violations)?;
    }
    if role_on {
        role_case(root, python, &candidate, &mut cases, &mut violations)?;
    }
    let mut contracts = vec!["cli", "side-effect"];
    let mut missing: Vec<&str> = MISSING_CONTRACTS.to_vec();
    for (name, on) in baseline::SCOPED.iter().zip([http_on, data_on, role_on]) {
        if on {
            contracts.push(name);
        } else {
            missing.push(name);
        }
        if *name == "http" && projection_on {
            contracts.push(PROJECTION_CASE);
        }
    }
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
        "test_oracle": test_oracle.is_some(),
        "candidate_sha256": candidate_sha256,
        "test_scope": test_scope,
        "python_denied": if deny_python { json!("env") } else { Value::Null },
        "contracts": contracts,
        "case_count": cases.len(),
        "passed_count": cases.len() - failed.len(),
        "failed_cases": failed,
        "passed": failed.is_empty() && violations.total() == 0,
        "checks": checks,
        "violations": violations.to_json(),
        "unobserved_violations": UNOBSERVED,
        "missing_contracts": missing,
        "side_effects_observed": "final-state",
        "writes_traced": "per-phase",
        "http": http,
        "cases": cases,
    }))
}

/// The test-only plants of a vector step: a wrong vector in one set, and a
/// file written into the caller's `CAREERBREW_HOME` during verify.
pub struct Plants<'a> {
    pub wrong: Option<&'a str>,
    pub home_write: Option<&'a str>,
    /// `--test-scope`: `full` runs HTTP, DATA and role whatever the step
    /// scopes, with Python allowed and no registry projection; `allow-python`
    /// keeps the step's scope but lifts the Python denial.
    pub scope: Option<&'a str>,
}

/// Writes the record to a new absolute file. The record is written even when
/// a case fails, so the caller can read which one.
pub fn run(
    root: &Path,
    id: &str,
    python: &Path,
    candidate: Option<&Path>,
    test_oracle: Option<&Path>,
    plants: Plants,
    output: &Path,
) -> Result<Value, String> {
    let Plants {
        wrong,
        home_write,
        scope,
    } = plants;
    if !output.is_absolute() {
        return Err("--out must be an absolute path".into());
    }
    if output.exists() {
        return Err("verify output already exists".into());
    }
    let record = record(
        root,
        id,
        python,
        candidate,
        test_oracle,
        wrong,
        home_write,
        scope,
    )?;
    capture_actions::write_receipt(&record, output)?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_03_uses_its_registered_release_binary_and_refuses_it_unbuilt() {
        let step = baseline::step("rust-03").unwrap();
        assert_eq!(step.candidate, Some("target/release/careerbrew-native"));
        assert_eq!(step.cases[0].0, "actions-json");
        // A root with no build: the registered candidate is refused, never skipped.
        let scratch = tempfile::tempdir().unwrap();
        let error = record(
            scratch.path(),
            "rust-03",
            Path::new("/usr/bin/true"),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap_err();
        assert_eq!(error, "the candidate binary does not exist");
    }

    #[test]
    fn every_rust_03_case_has_a_normalization_entry() {
        let step = baseline::step("rust-03").unwrap();
        for (case, _) in step.cases {
            assert_eq!(
                normalize::rules("rust-03", case),
                [normalize::SYNTHETIC_HOME],
                "{case}"
            );
        }
    }
}
