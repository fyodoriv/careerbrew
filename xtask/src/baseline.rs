//! `migration baseline --step <id>`: run a registered step's Python oracle
//! N times through the process-tree collector and write a content-free JSON
//! record (step, command, machine, toolchains, source revision and hash, raw
//! samples, p50 and p95). Each run gets a fresh synthetic home that lives for
//! that run only; nothing is kept between calls.
use crate::{baseline_verdict, capture_actions, resources};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

pub const SCHEMA: &str = "careerbrew-migration-baseline-v1";
pub const DEFAULT_RUNS: usize = 30;
/// The protocol's minimum sample count; fewer runs need the test-only flag.
pub const MIN_RUNS: usize = 30;
/// The oracle imports from the checkout with warm OS file caches; only the
/// careerbrew data folder is fresh for each run.
pub const CACHE_STATE: &str = "warm-imports-fresh-synthetic-home";

/// A registered step: its id and the Python oracle arguments after the
/// interpreter. The oracle must be read-only and must exit 0.
pub struct Step {
    pub id: &'static str,
    pub oracle: &'static [&'static str],
    /// `verify` cases: an id and the CLI arguments after `careerbrew`.
    pub cases: &'static [(&'static str, &'static [&'static str])],
    /// The step's built Rust candidate, relative to the repo root. None until
    /// the port lands; `verify` refuses the step until then.
    pub candidate: Option<&'static str>,
    /// `verify` also compares the HTTP handler contract (the capture-http
    /// fixture cases) when true.
    pub http_contract: bool,
    /// Extra files, relative to the repo root, hashed into the baseline and
    /// measure source hash (a vector step's driver and the Python it reads).
    pub sources: &'static [&'static str],
}

pub const STEPS: &[Step] = &[
    Step {
        // rust-02 is a vector step (`depvec::RUST_02`): verify compares pure
        // value vectors, so it has no CLI cases. Its oracle and candidate run
        // every vector set in one process with `--all`.
        id: "rust-02",
        oracle: &["-B", "xtask/tests/fixtures/purevec/driver.py", "--all"],
        cases: &[],
        candidate: Some("target/debug/careerbrew-purevec"),
        http_contract: false,
        sources: &[
            "xtask/tests/fixtures/purevec/driver.py",
            "src/careerbrew/__init__.py",
            "src/careerbrew/config.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/changes.py",
            "src/careerbrew/calendar.py",
        ],
    },
    Step {
        // rust-03 ports the native CLI with one read-only pilot: the action catalog.
        id: "rust-03",
        oracle: &["-B", "-m", "careerbrew", "actions", "--json"],
        cases: &[("actions-json", &["actions", "--json"])],
        candidate: None,
        // The action catalog is also served at /api/actions.
        http_contract: true,
        sources: &[],
    },
];

pub fn step(id: &str) -> Result<&'static Step, String> {
    STEPS
        .iter()
        .find(|step| step.id == id)
        .ok_or_else(|| format!("unknown step id: {id}"))
}

/// Nearest-rank median.
pub(crate) fn p50(samples: &[u64]) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len().div_ceil(2) - 1]
}

pub(crate) fn toolchain(root: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(root.join("rust-toolchain.toml"))
        .map_err(|_| "cannot read rust-toolchain.toml")?;
    text.lines()
        .find_map(|line| line.trim().strip_prefix("channel = "))
        .map(|value| value.trim_matches('"').to_owned())
        .ok_or_else(|| "rust-toolchain.toml has no channel".to_owned())
}

pub(crate) fn revision(root: &Path) -> Result<String, String> {
    let mut command = Command::new("/usr/bin/git");
    command
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .env_clear();
    let (_, exit, stdout, stderr) = capture_actions::execute(command)?;
    if exit != Some(0) || !stderr.is_empty() {
        return Err("cannot observe source revision".into());
    }
    Ok(stdout.trim().to_owned())
}

/// True when `git status --porcelain` lists any change.
pub(crate) fn dirty(root: &Path) -> Result<bool, String> {
    let mut command = Command::new("/usr/bin/git");
    command
        .args(["status", "--porcelain"])
        .current_dir(root)
        .env_clear();
    let (_, exit, stdout, stderr) = capture_actions::execute(command)?;
    if exit != Some(0) || !stderr.is_empty() {
        return Err("cannot observe tree state".into());
    }
    Ok(!stdout.trim().is_empty())
}

/// The capture sources, the baseline and collector sources, and the step's
/// own sources (none for a CLI step, so its hash is unchanged).
fn hashed_sources(root: &Path, step: &Step) -> Result<Value, String> {
    hashed_sources_with(root, step.sources)
}

/// The capture sources, the baseline and collector sources, and `extra`.
pub(crate) fn hashed_sources_with(root: &Path, extra: &[&str]) -> Result<Value, String> {
    let mut sources = capture_actions::sources(root)?;
    let map = sources
        .as_object_mut()
        .ok_or("capture sources are not a map")?;
    let base = ["xtask/src/baseline.rs", "xtask/src/resources.rs"];
    for name in base.iter().chain(extra) {
        let bytes = std::fs::read(root.join(name)).map_err(|_| "baseline source missing")?;
        map.insert((*name).to_owned(), json!(capture_actions::digest(&bytes)));
    }
    Ok(sources)
}

/// One hash over the HTTP fixtures the oracle environment points at.
pub(crate) fn fixtures_sha256(root: &Path) -> Result<String, String> {
    let files = capture_actions::files(root, &root.join("tests/fixtures/http"), None)?;
    Ok(capture_actions::digest(json!(files).to_string().as_bytes()))
}

pub fn record(
    root: &Path,
    id: &str,
    python: &Path,
    runs: usize,
    allow_few_runs: bool,
) -> Result<Value, String> {
    let step = step(id)?;
    if runs == 0 {
        return Err("--runs must be at least 1".into());
    }
    if runs < MIN_RUNS && !allow_few_runs {
        return Err(format!(
            "--runs {runs} is below the protocol minimum of {MIN_RUNS}"
        ));
    }
    if !python.is_absolute() {
        return Err("--python must be an absolute path".into());
    }
    if std::env::var_os("CAREERBREW_HOME").is_some_and(|home| home.is_empty()) {
        return Err("CAREERBREW_HOME is set but empty".into());
    }
    let python_version = capture_actions::version(python)?;
    let sources = hashed_sources(root, step)?;
    let fixtures = fixtures_sha256(root)?;
    let sources_sha256 = capture_actions::digest(sources.to_string().as_bytes());
    let args = step.oracle.iter().map(OsString::from).collect::<Vec<_>>();
    let (samples, summary, method) = sample(root, python, &args, runs, "oracle", "baseline")?;
    if hashed_sources(root, step)? != sources || fixtures_sha256(root)? != fixtures {
        return Err("a relevant source changed during the baseline".into());
    }
    let (os, _) = resources::host_unit()?;
    let mut command = vec!["<python>".to_owned()];
    command.extend(step.oracle.iter().map(|arg| (*arg).to_owned()));
    Ok(json!({
        "schema": SCHEMA,
        "step": step.id,
        "command": command,
        "os": os,
        "arch": std::env::consts::ARCH,
        "rust_toolchain": toolchain(root)?,
        "python_version": python_version.trim(),
        "source_revision": revision(root)?,
        "source_tree_dirty": dirty(root)?,
        "sources_sha256": sources_sha256,
        "fixtures_sha256": fixtures,
        "cache_state": CACHE_STATE,
        "meets_protocol_minimum": runs >= MIN_RUNS,
        "tree_method": method,
        "runs": runs,
        "samples": samples,
        "summary": summary,
    }))
}

/// Runs `program args` `runs` times, each in a fresh synthetic home, through
/// the process-tree collector. Returns the raw samples, the p50 and p95
/// summary and the tree method. A run that does not exit 0 is refused.
pub(crate) fn sample(
    root: &Path,
    program: &Path,
    args: &[OsString],
    runs: usize,
    role: &str,
    kind: &str,
) -> Result<(Vec<Value>, Value, &'static str), String> {
    let mut samples = Vec::with_capacity(runs);
    let (mut cpu, mut rss, mut wall) = (Vec::new(), Vec::new(), Vec::new());
    let mut method = "unknown";
    for run in 1..=runs {
        let scratch = tempfile::tempdir().map_err(|_| format!("cannot create {kind} home"))?;
        let environment = capture_actions::owned_environment(root, &scratch.path().join("owned"))?;
        if environment
            .get("CAREERBREW_HOME")
            .is_none_or(|home| home.is_empty())
        {
            return Err("synthetic CAREERBREW_HOME is empty".into());
        }
        let usage = resources::measure_in(program, args, Some(&environment), Some(root))?;
        if !usage.passed() {
            return Err(format!("{role} run {run} did not exit 0; {kind} refused"));
        }
        method = usage.tree_method;
        let cpu_us = usage
            .tree_user_cpu_us
            .saturating_add(usage.tree_system_cpu_us);
        cpu.push(cpu_us);
        rss.push(usage.tree_max_rss_bytes);
        wall.push(usage.wall_us);
        samples.push(json!({
            "run": run,
            "tree_user_cpu_us": usage.tree_user_cpu_us,
            "tree_system_cpu_us": usage.tree_system_cpu_us,
            "tree_cpu_us": cpu_us,
            "tree_max_rss_bytes": usage.tree_max_rss_bytes,
            "descendants": usage.descendants,
            "wall_us": usage.wall_us,
            "parent_max_rss_bytes": usage.parent_max_rss_bytes,
            "rss_may_include_parent": usage.rss_may_include_parent(),
        }));
    }
    let summary = json!({
        "cpu_us_p50": p50(&cpu),
        "cpu_us_p95": baseline_verdict::cpu_p95(cpu.clone()),
        "tree_max_rss_bytes_p50": p50(&rss),
        "tree_max_rss_bytes_p95": baseline_verdict::cpu_p95(rss.clone()),
        "wall_us_p50": p50(&wall),
        "wall_us_p95": baseline_verdict::cpu_p95(wall.clone()),
    });
    Ok((samples, summary, method))
}

pub fn run(
    root: &Path,
    id: &str,
    python: &Path,
    runs: usize,
    allow_few_runs: bool,
    output: &Path,
) -> Result<Value, String> {
    if !output.is_absolute() {
        return Err("--out must be an absolute path".into());
    }
    if output.exists() {
        return Err("baseline output already exists".into());
    }
    let record = record(root, id, python, runs, allow_few_runs)?;
    capture_actions::write_receipt(&record, output)?;
    Ok(record)
}
