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

/// A registered step: its id and the Python oracle arguments after the
/// interpreter. The oracle must be read-only and must exit 0.
pub struct Step {
    pub id: &'static str,
    pub oracle: &'static [&'static str],
}

pub const STEPS: &[Step] = &[Step {
    // rust-03 ports the native CLI with one read-only pilot: the action catalog.
    id: "rust-03",
    oracle: &["-B", "-m", "careerbrew", "actions", "--json"],
}];

pub fn step(id: &str) -> Result<&'static Step, String> {
    STEPS
        .iter()
        .find(|step| step.id == id)
        .ok_or_else(|| format!("unknown step id: {id}"))
}

/// Nearest-rank median.
fn p50(samples: &[u64]) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len().div_ceil(2) - 1]
}

fn toolchain(root: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(root.join("rust-toolchain.toml"))
        .map_err(|_| "cannot read rust-toolchain.toml")?;
    text.lines()
        .find_map(|line| line.trim().strip_prefix("channel = "))
        .map(|value| value.trim_matches('"').to_owned())
        .ok_or_else(|| "rust-toolchain.toml has no channel".to_owned())
}

fn revision(root: &Path) -> Result<String, String> {
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

pub fn record(root: &Path, id: &str, python: &Path, runs: usize) -> Result<Value, String> {
    let step = step(id)?;
    if runs == 0 {
        return Err("--runs must be at least 1".into());
    }
    if !python.is_absolute() {
        return Err("--python must be an absolute path".into());
    }
    if std::env::var_os("CAREERBREW_HOME").is_some_and(|home| home.is_empty()) {
        return Err("CAREERBREW_HOME is set but empty".into());
    }
    let python_version = capture_actions::version(python)?;
    let sources = capture_actions::sources(root)?;
    let sources_sha256 = capture_actions::digest(sources.to_string().as_bytes());
    let args = step.oracle.iter().map(OsString::from).collect::<Vec<_>>();
    let mut samples = Vec::with_capacity(runs);
    let (mut cpu, mut rss, mut wall) = (Vec::new(), Vec::new(), Vec::new());
    let mut method = "unknown";
    for run in 1..=runs {
        let scratch = tempfile::tempdir().map_err(|_| "cannot create baseline home")?;
        let environment = capture_actions::owned_environment(root, &scratch.path().join("owned"))?;
        if environment
            .get("CAREERBREW_HOME")
            .is_none_or(|home| home.is_empty())
        {
            return Err("synthetic CAREERBREW_HOME is empty".into());
        }
        let usage = resources::measure_in(python, &args, Some(&environment))?;
        if !usage.passed() {
            return Err(format!("oracle run {run} did not exit 0; baseline refused"));
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
        }));
    }
    if capture_actions::sources(root)? != sources {
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
        "sources_sha256": sources_sha256,
        "cache_state": "fresh-synthetic-home",
        "tree_method": method,
        "runs": runs,
        "samples": samples,
        "summary": {
            "cpu_us_p50": p50(&cpu),
            "cpu_us_p95": baseline_verdict::cpu_p95(cpu.clone()),
            "tree_max_rss_bytes_p50": p50(&rss),
            "tree_max_rss_bytes_p95": baseline_verdict::cpu_p95(rss.clone()),
            "wall_us_p50": p50(&wall),
            "wall_us_p95": baseline_verdict::cpu_p95(wall.clone()),
        },
    }))
}

pub fn run(
    root: &Path,
    id: &str,
    python: &Path,
    runs: usize,
    output: &Path,
) -> Result<Value, String> {
    if output.exists() {
        return Err("baseline output already exists".into());
    }
    let record = record(root, id, python, runs)?;
    capture_actions::write_receipt(&record, output)?;
    Ok(record)
}
