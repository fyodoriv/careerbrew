//! `migration verify --step <id>`: run a registered step's Python oracle and
//! its Rust candidate on the same synthetic cases, each side in its own fresh
//! synthetic home, and compare stdout and the exit code byte for byte. It
//! writes a content-free JSON record: step, case count, pass or fail per case
//! id, the sources hash and the source revision. A step with no Rust
//! candidate is refused, never passed.
use crate::{baseline, capture_actions};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

pub const SCHEMA: &str = "careerbrew-migration-verify-v1";
/// The oracle arguments before a case's CLI arguments.
pub const ORACLE_PREFIX: &[&str] = &["-B", "-m", "careerbrew"];

/// Runs one side in a fresh synthetic home; returns its exit code and stdout.
fn side(root: &Path, program: &Path, args: &[&str]) -> Result<(Option<i32>, String), String> {
    let scratch = tempfile::tempdir().map_err(|_| "cannot create verify home")?;
    let environment = capture_actions::owned_environment(root, &scratch.path().join("owned"))?;
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
    Ok((exit, stdout))
}

/// The capture sources plus the baseline and verify sources, as one hash.
fn sources_sha256(root: &Path) -> Result<String, String> {
    let mut sources = capture_actions::sources(root)?;
    let map = sources
        .as_object_mut()
        .ok_or("capture sources are not a map")?;
    for name in ["xtask/src/baseline.rs", "xtask/src/verify.rs"] {
        let bytes = std::fs::read(root.join(name)).map_err(|_| "verify source missing")?;
        map.insert(name.to_owned(), json!(capture_actions::digest(&bytes)));
    }
    Ok(capture_actions::digest(sources.to_string().as_bytes()))
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
    let mut cases = Vec::with_capacity(step.cases.len());
    let mut failed = Vec::new();
    for (case, args) in step.cases {
        let mut oracle_args = ORACLE_PREFIX.to_vec();
        oracle_args.extend_from_slice(args);
        let (oracle_exit, oracle_stdout) = side(root, python, &oracle_args)?;
        if oracle_exit != Some(0) {
            return Err(format!("oracle case {case} did not exit 0; verify refused"));
        }
        let (candidate_exit, candidate_stdout) = side(root, &candidate, args)?;
        let exit_matches = candidate_exit == oracle_exit;
        let stdout_matches = candidate_stdout.as_bytes() == oracle_stdout.as_bytes();
        let passed = exit_matches && stdout_matches;
        if !passed {
            failed.push((*case).to_owned());
        }
        cases.push(json!({
            "case": case,
            "passed": passed,
            "exit_matches": exit_matches,
            "stdout_matches": stdout_matches,
            "oracle_stdout_sha256": capture_actions::digest(oracle_stdout.as_bytes()),
            "candidate_stdout_sha256": capture_actions::digest(candidate_stdout.as_bytes()),
        }));
    }
    if sources_sha256(root)? != hash {
        return Err("a relevant source changed during verify".into());
    }
    Ok(json!({
        "schema": SCHEMA,
        "step": step.id,
        "source_revision": baseline::revision(root)?,
        "source_tree_dirty": baseline::dirty(root)?,
        "sources_sha256": hash,
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
