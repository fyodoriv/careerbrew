//! `migration verify --step <id>`: run a registered step's Python oracle and
//! its Rust candidate on the same synthetic cases, each side in its own fresh
//! synthetic home, and compare stdout and the exit code byte for byte. It
//! writes a content-free JSON record: step, case count, pass or fail per case
//! id, the sources hash, the source revision, whether the candidate came
//! from the test-only flag, and the candidate binary's sha256 (checked before
//! and after the runs). A candidate that cannot run, times out or prints
//! output that is not UTF-8 or too large fails its case. A step with no Rust
//! candidate is refused, never passed. Each case's registered normalization
//! rules (`normalize.rs`) apply to both stdouts first; the stdout hashes are
//! of the normalized text. A local path no rule covers refuses verify.
use crate::{baseline, capture_actions, normalize};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

pub const SCHEMA: &str = "careerbrew-migration-verify-v1";
/// The oracle arguments before a case's CLI arguments.
pub const ORACLE_PREFIX: &[&str] = &["-B", "-m", "careerbrew"];

/// A side's stdout held a local path that no registered rule covers.
struct UnregisteredPath;

/// Runs one side in a fresh synthetic home; returns its exit code and its
/// stdout after the case's normalization rules.
#[allow(clippy::type_complexity)]
fn side(
    root: &Path,
    program: &Path,
    args: &[&str],
    rules: &[&str],
) -> Result<Result<(Option<i32>, String), UnregisteredPath>, String> {
    let scratch = tempfile::tempdir().map_err(|_| "cannot create verify home")?;
    let environment = capture_actions::owned_environment(root, &scratch.path().join("owned"))?;
    let home = environment
        .get("CAREERBREW_HOME")
        .filter(|home| !home.is_empty())
        .ok_or("synthetic CAREERBREW_HOME is empty")?
        .clone();
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(root)
        .env_clear()
        .envs(&environment);
    let (_, exit, stdout, _) = capture_actions::execute(command)?;
    let side = normalize::Side {
        home: Path::new(&home),
        scratch: scratch.path(),
    };
    Ok(normalize::apply(rules, &side, root, &stdout)
        .map(|stdout| (exit, stdout))
        .map_err(|()| UnregisteredPath))
}

fn unregistered(case: &str) -> String {
    format!("case {case} prints a local path with no registered normalization rule; verify refused")
}

/// The capture sources plus the baseline and verify sources, as one hash.
fn sources_sha256(root: &Path) -> Result<String, String> {
    let mut sources = capture_actions::sources(root)?;
    let map = sources
        .as_object_mut()
        .ok_or("capture sources are not a map")?;
    for name in [
        "xtask/src/baseline.rs",
        "xtask/src/verify.rs",
        "xtask/src/normalize.rs",
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

/// Builds the record. `candidate` is the test-only override; without it the
/// step's registered candidate is used. `test_oracle` is the test-only
/// script run in place of `python` (it gets the same arguments).
pub fn record(
    root: &Path,
    id: &str,
    python: &Path,
    candidate: Option<&Path>,
    test_oracle: Option<&Path>,
) -> Result<Value, String> {
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
    let mut cases = Vec::with_capacity(step.cases.len());
    let mut failed = Vec::new();
    for (case, args) in step.cases {
        let rules = normalize::rules(step.id, case);
        let mut oracle_args = ORACLE_PREFIX.to_vec();
        oracle_args.extend_from_slice(args);
        let (oracle_exit, oracle_stdout) = side(root, oracle, &oracle_args, rules)?
            .map_err(|UnregisteredPath| unregistered(case))?;
        if oracle_exit != Some(0) {
            return Err(format!("oracle case {case} did not exit 0; verify refused"));
        }
        let oracle_stdout_sha256 = capture_actions::digest(oracle_stdout.as_bytes());
        // A spawn failure, timeout, non-UTF-8 or oversize output fails the
        // case; the reason text stays out of the record (it may hold paths).
        let entry = match side(root, &candidate, args, rules) {
            Ok(Err(UnregisteredPath)) => return Err(unregistered(case)),
            Ok(Ok((candidate_exit, candidate_stdout))) => {
                let exit_matches = candidate_exit == oracle_exit;
                let stdout_matches = candidate_stdout.as_bytes() == oracle_stdout.as_bytes();
                json!({
                    "case": case,
                    "passed": exit_matches && stdout_matches,
                    "candidate_ran": true,
                    "exit_matches": exit_matches,
                    "stdout_matches": stdout_matches,
                    "normalization": rules,
                    "oracle_stdout_sha256": oracle_stdout_sha256,
                    "candidate_stdout_sha256": capture_actions::digest(candidate_stdout.as_bytes()),
                })
            }
            Err(_) => json!({
                "case": case,
                "passed": false,
                "candidate_ran": false,
                "exit_matches": false,
                "stdout_matches": false,
                "normalization": rules,
                "oracle_stdout_sha256": oracle_stdout_sha256,
                "candidate_stdout_sha256": Value::Null,
            }),
        };
        if entry["passed"] != true {
            failed.push((*case).to_owned());
        }
        cases.push(entry);
    }
    if sources_sha256(root)? != hash {
        return Err("a relevant source changed during verify".into());
    }
    if binary_sha256(&candidate)? != candidate_sha256 {
        return Err("the candidate binary changed during verify".into());
    }
    Ok(json!({
        "schema": SCHEMA,
        "step": step.id,
        "source_revision": baseline::revision(root)?,
        "source_tree_dirty": baseline::dirty(root)?,
        "sources_sha256": hash,
        "test_candidate": test_candidate,
        "test_oracle": test_oracle.is_some(),
        "candidate_sha256": candidate_sha256,
        "case_count": cases.len(),
        "passed_count": cases.len() - failed.len(),
        "failed_cases": failed,
        "passed": failed.is_empty(),
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
    test_oracle: Option<&Path>,
    output: &Path,
) -> Result<Value, String> {
    if !output.is_absolute() {
        return Err("--out must be an absolute path".into());
    }
    if output.exists() {
        return Err("verify output already exists".into());
    }
    let record = record(root, id, python, candidate, test_oracle)?;
    capture_actions::write_receipt(&record, output)?;
    Ok(record)
}
