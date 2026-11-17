//! `migration measure --step <id>`: run a registered step's Rust candidate
//! N times through the process-tree collector, the same way `baseline` runs
//! the Python oracle, and write a content-free JSON record with the same
//! shape (raw samples, p50 and p95). A step with no Rust candidate fails.
//! Interim: the candidate runs alone in one warm regime. The protocol's
//! paired oracle and candidate runs (`--pairs`) and the cold regime are not
//! built yet.
use crate::{baseline, capture_actions, resources};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "careerbrew-migration-measure-v1";

/// A registered Rust candidate: the step id, the executable path relative to
/// the repo root, and its arguments. The candidate must be read-only and must
/// exit 0.
// Constructed once rust-03 registers its first candidate.
#[allow(dead_code)]
pub struct Candidate {
    pub step: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
}

/// No step has a Rust candidate yet; `rust-03` adds the first one.
pub const CANDIDATES: &[Candidate] = &[];

/// Where the candidate comes from: the registry, or the test-only override.
pub enum Source<'a> {
    Registered,
    TestOverride(&'a Path),
}

fn candidate(root: &Path, id: &str, source: &Source) -> Result<(PathBuf, Vec<String>), String> {
    // The step must be registered even when a test overrides the candidate.
    let step = baseline::step(id)?;
    match source {
        Source::TestOverride(path) => {
            if !path.is_absolute() {
                return Err("--test-candidate must be an absolute path".into());
            }
            Ok((path.to_path_buf(), Vec::new()))
        }
        Source::Registered => {
            let found = CANDIDATES
                .iter()
                .find(|candidate| candidate.step == step.id)
                .ok_or_else(|| format!("step {} has no Rust candidate yet", step.id))?;
            Ok((
                root.join(found.program),
                found.args.iter().map(|arg| (*arg).to_owned()).collect(),
            ))
        }
    }
}

pub fn record(
    root: &Path,
    id: &str,
    source: &Source,
    runs: usize,
    allow_few_runs: bool,
) -> Result<Value, String> {
    if runs == 0 {
        return Err("--runs must be at least 1".into());
    }
    if runs < baseline::MIN_RUNS && !allow_few_runs {
        return Err(format!(
            "--runs {runs} is below the protocol minimum of {}",
            baseline::MIN_RUNS
        ));
    }
    // A test override never makes a protocol record: it needs the test flag.
    let test_override = matches!(source, Source::TestOverride(_));
    if test_override && !allow_few_runs {
        return Err("--test-candidate needs --test-allow-few-runs".into());
    }
    let (program, args) = candidate(root, id, source)?;
    if std::env::var_os("CAREERBREW_HOME").is_some_and(|home| home.is_empty()) {
        return Err("CAREERBREW_HOME is set but empty".into());
    }
    // Binds only the candidate executable file, not files it may load.
    let binary = std::fs::read(&program).map_err(|_| "cannot read the Rust candidate")?;
    let candidate_sha256 = capture_actions::digest(&binary);
    let sources = baseline::hashed_sources_with(root, &["xtask/src/measure.rs"])?;
    let fixtures = baseline::fixtures_sha256(root)?;
    let sources_sha256 = capture_actions::digest(sources.to_string().as_bytes());
    let os_args = args.iter().map(OsString::from).collect::<Vec<_>>();
    let (samples, summary, method) =
        baseline::sample(root, &program, &os_args, runs, "candidate", "measure")?;
    if baseline::hashed_sources_with(root, &["xtask/src/measure.rs"])? != sources
        || baseline::fixtures_sha256(root)? != fixtures
        || std::fs::read(&program)
            .map(|bytes| capture_actions::digest(&bytes))
            .ok()
            != Some(candidate_sha256.clone())
    {
        return Err("a relevant source changed during the measure".into());
    }
    let (os, _) = resources::host_unit()?;
    let mut command = vec!["<candidate>".to_owned()];
    command.extend(args);
    Ok(json!({
        "schema": SCHEMA,
        "step": id,
        "side": "rust-candidate",
        "test_override": test_override,
        "command": command,
        "candidate_sha256": candidate_sha256,
        "os": os,
        "arch": std::env::consts::ARCH,
        "rust_toolchain": baseline::toolchain(root)?,
        "source_revision": baseline::revision(root)?,
        "source_tree_dirty": baseline::dirty(root)?,
        "sources_sha256": sources_sha256,
        "fixtures_sha256": fixtures,
        "cache_state": baseline::CACHE_STATE,
        "meets_protocol_minimum": runs >= baseline::MIN_RUNS && !test_override,
        "tree_method": method,
        "runs": runs,
        "samples": samples,
        "summary": summary,
    }))
}

pub fn run(
    root: &Path,
    id: &str,
    source: &Source,
    runs: usize,
    allow_few_runs: bool,
    output: &Path,
) -> Result<Value, String> {
    if !output.is_absolute() {
        return Err("--out must be an absolute path".into());
    }
    if output.exists() {
        return Err("measure output already exists".into());
    }
    let record = record(root, id, source, runs, allow_few_runs)?;
    capture_actions::write_receipt(&record, output)?;
    Ok(record)
}
