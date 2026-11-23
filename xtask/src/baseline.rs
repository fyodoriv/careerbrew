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
    /// `verify` also runs the DATA contract (the capture-data corpus) when
    /// true.
    pub data_contract: bool,
    /// `verify` also runs the corporate-role case (`jobs list`) when true.
    pub role_contract: bool,
    /// `verify` also compares the candidate's `actions --json` stdout,
    /// wrapped as `{"actions": ...}`, with the body of the frozen
    /// capture-http `/api/actions` cases (offline: no server starts).
    pub registry_projection: bool,
    /// `verify` runs every candidate CLI case with Python denied (see
    /// `verify::NO_PYTHON_HOME`). A step whose candidate may still start
    /// Python leaves it false.
    pub no_python: bool,
    /// Extra environment for both sides of every baseline, measure and
    /// verify run (after the synthetic environment).
    pub env: &'static [(&'static str, &'static str)],
    /// Extra files, relative to the repo root, hashed into the baseline and
    /// measure source hash (a vector step's driver and the Python it reads).
    pub sources: &'static [&'static str],
    /// Exact fixture files, relative to the repo root, whose bytes join the
    /// shared evidence identity. Directories and unregistered files are not read.
    pub fixtures: &'static [&'static str],
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
        // A vector step never reaches the CLI contracts (`verify` hands it to
        // `depvec`), and `decide` gives a vector step no declared scope, so
        // these flags have no effect on its verdict.
        http_contract: false,
        data_contract: true,
        role_contract: true,
        registry_projection: false,
        no_python: false,
        env: &[],
        sources: &[
            "xtask/tests/fixtures/purevec/driver.py",
            "src/careerbrew/__init__.py",
            "src/careerbrew/config.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/changes.py",
            "src/careerbrew/calendar.py",
        ],
        fixtures: &[
            "xtask/tests/fixtures/purevec/clock.json",
            "xtask/tests/fixtures/purevec/hash.json",
            "xtask/tests/fixtures/purevec/paths.json",
            "xtask/tests/fixtures/purevec/settings.json",
        ],
    },
    Step {
        // rust-03 ports the native CLI with one read-only pilot: the action catalog.
        id: "rust-03",
        oracle: &["-B", "-m", "careerbrew", "actions", "--json"],
        cases: RUST_03_CASES,
        candidate: Some("target/release/careerbrew-native"),
        // The pilot binary serves no HTTP, no data and no role-checked
        // command: `server.py` stays Python. The catalog it shares with
        // /api/actions is checked offline by `registry_projection` instead.
        http_contract: false,
        data_contract: false,
        role_contract: false,
        registry_projection: true,
        no_python: true,
        // Help text wraps at the terminal width: pin it on both sides.
        env: &[("COLUMNS", "80"), ("LINES", "24")],
        // The pilot crate and the registry it compiles in.
        sources: &[
            "crates/cbcli/Cargo.toml",
            "crates/cbcli/src/lib.rs",
            "crates/cbcli/src/main.rs",
            "crates/cbcli/src/registry.rs",
            "crates/cbcli/src/render.rs",
            "crates/cbcli/src/help.rs",
            "crates/cbcli/src/events.rs",
            "crates/cbcli/registry/registry.json",
            "scripts/gen-registry.py",
            "config/native-routes.txt",
        ],
        fixtures: &[],
    },
    Step {
        // rust-05f/g/h/i is a stateful component workload. The real Python oracle
        // uses only its invocation's synthetic home and injected adapters.
        id: "rust-05",
        oracle: &["-I", "-B", "tests/oracles/rust05.py", "--all"],
        cases: &[],
        candidate: Some("target/release/vault-store"),
        http_contract: false,
        data_contract: true,
        role_contract: false,
        registry_projection: false,
        no_python: true,
        env: &[],
        sources: &[
            "tests/oracles/rust05.py",
            "src/careerbrew/store.py",
            "src/careerbrew/vault.py",
            "src/careerbrew/secrets.py",
            "src/careerbrew/keyfile.py",
            "src/careerbrew/config_io.py",
            "src/careerbrew/events.py",
            "src/careerbrew/approvals.py",
            "src/careerbrew/cli.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/calendar.py",
            "crates/vault/Cargo.toml",
            "crates/vault/src/lib.rs",
            "crates/vault/src/atomic.rs",
            "crates/vault/src/keyfile.rs",
            "crates/vault/src/lock.rs",
            "crates/vault/src/canon.rs",
            "crates/vault/src/json_yaml.rs",
            "crates/vault/src/store.rs",
            "crates/vault/src/settings.rs",
            "crates/vault/src/events.rs",
            "crates/vault/src/helpers.rs",
            "crates/vault/src/bulk.rs",
            "crates/vault/src/synthetic_bulk.rs",
            "crates/vault/src/receipts.rs",
            "crates/vault/src/secrets.rs",
            "crates/vault/src/cli.rs",
            "crates/vault/src/data.rs",
            "crates/vault/src/synthetic.rs",
            "crates/vault/src/bin/vault-store.rs",
            "crates/policy/src/approvals.rs",
            "crates/policy/src/vectors.rs",
            "xtask/src/rust05.rs",
        ],
        fixtures: &["docs/plans/2026-11-05-rust-data-fixture-v1.json"],
    },
    Step {
        // rust-06 is a stateful, component-only backup/sync workload. Its
        // custom verifier also orchestrates both cross-engine restore directions.
        id: "rust-06",
        oracle: &["-I", "-B", "tests/oracles/rust06.py", "--all"],
        cases: &[],
        candidate: Some("target/release/vault-sync"),
        http_contract: false,
        data_contract: false,
        role_contract: false,
        registry_projection: false,
        no_python: true,
        env: &[],
        sources: &[
            "tests/oracles/rust06.py",
            "src/careerbrew/backup.py",
            "src/careerbrew/sync.py",
            "src/careerbrew/cli.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/keyfile.py",
            "src/careerbrew/store.py",
            "src/careerbrew/vault.py",
            "src/careerbrew/machine.py",
            "src/careerbrew/plugins.py",
            "src/careerbrew/goals.py",
            "src/careerbrew/work/promo.py",
            "src/careerbrew/events.py",
            "src/careerbrew/calendar.py",
            "crates/vault/Cargo.toml",
            "crates/vault/src/lib.rs",
            "crates/vault/src/atomic.rs",
            "crates/vault/src/keyfile.rs",
            "crates/vault/src/store.rs",
            "crates/vault/src/bulk.rs",
            "crates/vault/src/canon.rs",
            "crates/vault/src/json_yaml.rs",
            "crates/vault/src/events.rs",
            "crates/vault/src/synthetic.rs",
            "crates/vault/src/backup.rs",
            "crates/vault/src/sync.rs",
            "crates/vault/src/transfer.rs",
            "crates/vault/src/synthetic_transfer.rs",
            "crates/vault/src/bin/vault-sync.rs",
            "xtask/src/rust06.rs",
        ],
        fixtures: &["knowledge/ladders/generic-swe.yaml"],
    },
    Step {
        // rust-04 is a vector step (privacy, policy and approval decisions).
        // Its Python oracle `--all` runs every set over the generator's
        // vectors in one isolated process. `verify` routes the step to the
        // vector-driver kind (`vecdriver::RUST_04`); `measure` times the
        // `policy-vectors --all <file>` candidate over a file the same
        // generator writes (`measure::CANDIDATES`).
        id: "rust-04",
        oracle: &["-I", "-B", "tests/oracles/rust04.py", "--all"],
        cases: &[],
        candidate: Some("target/release/policy-vectors"),
        // As for rust-02: a vector step reaches no CLI contract.
        http_contract: false,
        data_contract: true,
        role_contract: true,
        registry_projection: false,
        no_python: false,
        env: &[],
        sources: &[
            "tests/oracles/rust04.py",
            "tests/oracles/rust04_vectors.py",
            "src/careerbrew/privacy.py",
            "src/careerbrew/approvals.py",
            "src/careerbrew/work/access.py",
        ],
        fixtures: &[],
    },
];

/// rust-03's CLI cases: the `actions` listing in both forms and its filters,
/// two argparse usage errors (exit 2; their stderr is pinned in
/// `crates/cbcli/tests/cli.rs`), and static help at the root, a group, a
/// leaf and `actions` itself. `actions-json` stays first.
pub const RUST_03_CASES: &[(&str, &[&str])] = &[
    ("actions-json", &["actions", "--json"]),
    ("actions-text", &["actions"]),
    ("actions-name-json", &["actions", "config.set", "--json"]),
    ("actions-prefix-json", &["actions", "day", "--json"]),
    ("actions-prefix-text", &["actions", "work"]),
    ("actions-none-json", &["actions", "nosuch", "--json"]),
    ("actions-none-text", &["actions", "nosuch"]),
    ("actions-bad-flag", &["actions", "--bogus"]),
    ("actions-extra-pos", &["actions", "a", "b"]),
    ("help-root", &["--help"]),
    ("help-short", &["-h"]),
    ("help-group", &["day", "--help"]),
    ("help-leaf", &["config", "set", "--help"]),
    ("help-actions", &["actions", "--help"]),
];

/// The contracts a CLI step can scope out, in record order.
pub const SCOPED: &[&str] = &["http", "data", "role"];

impl Step {
    /// The scoped contracts this step leaves out: `verify` lists them as
    /// `missing_contracts`, and `decide` requires that exact list.
    pub fn scoped_out(&self) -> Vec<&'static str> {
        SCOPED
            .iter()
            .zip([self.http_contract, self.data_contract, self.role_contract])
            .filter(|(_, on)| !on)
            .map(|(name, _)| *name)
            .collect()
    }
}

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
/// own sources.
fn hashed_sources(root: &Path, step: &Step) -> Result<Value, String> {
    let mut sources = hashed_sources_with(root, step.sources)?;
    bind_env(&mut sources, step)?;
    Ok(sources)
}

/// The source and fixture identity every producer shares with `decide`.
/// Producer-specific `sources_sha256` fields keep their separate coverage.
pub(crate) fn evidence_hashes(root: &Path, step: &Step) -> Result<(String, String), String> {
    let sources = hashed_sources(root, step)?;
    Ok((
        capture_actions::digest(sources.to_string().as_bytes()),
        fixtures_sha256(root, step)?,
    ))
}

/// Adds the step's extra environment to a source map, so a record's source
/// hash also binds the environment its runs had. A step with no extra
/// environment keeps its map unchanged.
pub(crate) fn bind_env(sources: &mut Value, step: &Step) -> Result<(), String> {
    if step.env.is_empty() {
        return Ok(());
    }
    let pairs: Vec<Value> = step.env.iter().map(|(k, v)| json!([k, v])).collect();
    sources
        .as_object_mut()
        .ok_or("capture sources are not a map")?
        .insert("step-env".to_owned(), json!(pairs));
    Ok(())
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

/// One hash over the HTTP fixtures and the step's registered fixture files.
pub(crate) fn fixtures_sha256(root: &Path, step: &Step) -> Result<String, String> {
    let mut files = capture_actions::files(root, &root.join("tests/fixtures/http"), None)?;
    for name in step.fixtures {
        let bytes = std::fs::read(root.join(name)).map_err(|_| "evidence fixture missing")?;
        files.insert((*name).to_owned(), capture_actions::digest(&bytes));
    }
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
    let fixtures = fixtures_sha256(root, step)?;
    let sources_sha256 = capture_actions::digest(sources.to_string().as_bytes());
    let args = step.oracle.iter().map(OsString::from).collect::<Vec<_>>();
    let (samples, summary, method) =
        sample(root, python, &args, runs, "oracle", "baseline", step.env)?;
    if hashed_sources(root, step)? != sources || fixtures_sha256(root, step)? != fixtures {
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
        "sources_sha256": sources_sha256.clone(),
        "evidence_sources_sha256": sources_sha256,
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
    env: &[(&str, &str)],
) -> Result<(Vec<Value>, Value, &'static str), String> {
    let mut samples = Vec::with_capacity(runs);
    let (mut cpu, mut rss, mut wall) = (Vec::new(), Vec::new(), Vec::new());
    let mut method = "unknown";
    for run in 1..=runs {
        let scratch = tempfile::tempdir().map_err(|_| format!("cannot create {kind} home"))?;
        let mut environment =
            capture_actions::owned_environment(root, &scratch.path().join("owned"))?;
        for (key, value) in env {
            environment.insert((*key).to_owned(), (*value).to_owned());
        }
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
