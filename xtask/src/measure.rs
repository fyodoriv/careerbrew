//! `migration measure --step <id> --python <path> --pairs <n>`: step 4 of the
//! measurement protocol. For each cache regime (cold, then warm) it runs N
//! pairs of the step's Python oracle and its Rust candidate through the
//! process-tree collector. The trial planner fixes which side runs first in
//! each pair. It writes one content-free `careerbrew-migration-measure-v1`
//! record with a `regimes` list that `decide` reads.
//!
//! The two regimes (each side of each pair always gets a fresh synthetic
//! home, so the data folder is never warm):
//! - cold: no warm-up run, and each oracle run gets its own empty
//!   `PYTHONPYCACHEPREFIX`, so Python compiles every module from source.
//!   Cold changes only the Python side: for the Rust candidate it differs
//!   from warm only by the missing warm-up run, and the OS page cache is
//!   still warm from earlier runs.
//! - warm: one discarded warm-up run per side first. The oracle warm-up may
//!   write bytecode into one regime-owned `PYTHONPYCACHEPREFIX`; the samples
//!   then read it (with `-B`, so they write nothing).
//!
//! Every sample is a real execution: `cached` is always 0.
use crate::trial_plan::{self, Arm, CacheRegime};
use crate::{baseline, baseline_verdict, capture_actions, resources};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "careerbrew-migration-measure-v1";
pub const DEFAULT_PAIRS: usize = 30;
/// The protocol's minimum pair count per regime (the trial planner's);
/// fewer need the test flag.
pub const MIN_PAIRS: usize = trial_plan::MIN_PAIRS as usize;
const HASHED: &[&str] = &["xtask/src/measure.rs", "xtask/src/trial_plan.rs"];

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

/// One side's command: program, arguments, and its warm-up arguments.
struct Side {
    name: &'static str,
    program: PathBuf,
    args: Vec<OsString>,
    warmup_args: Vec<OsString>,
    python: bool,
}

/// One real execution in a fresh synthetic home. `pycache` sets the bytecode
/// folder for the oracle; `write_bytecode` lets the warm-up fill it.
fn execute(
    root: &Path,
    side: &Side,
    args: &[OsString],
    pycache: Option<&Path>,
    write_bytecode: bool,
) -> Result<resources::Usage, String> {
    let scratch = tempfile::tempdir().map_err(|_| "cannot create measure home")?;
    let mut environment: BTreeMap<String, String> =
        capture_actions::owned_environment(root, &scratch.path().join("owned"))?;
    if environment
        .get("CAREERBREW_HOME")
        .is_none_or(|home| home.is_empty())
    {
        return Err("synthetic CAREERBREW_HOME is empty".into());
    }
    if side.python {
        let fresh = scratch.path().join("pycache");
        let folder = pycache.unwrap_or(&fresh);
        if !folder.exists() {
            std::fs::create_dir(folder).map_err(|_| "cannot prepare bytecode folder")?;
        }
        environment.insert(
            "PYTHONPYCACHEPREFIX".into(),
            folder.to_string_lossy().into_owned(),
        );
        if write_bytecode {
            environment.remove("PYTHONDONTWRITEBYTECODE");
        }
    }
    let usage = resources::measure_in(&side.program, args, Some(&environment), Some(root))?;
    if !usage.passed() {
        return Err(format!("{} run did not exit 0; measure refused", side.name));
    }
    Ok(usage)
}

#[derive(Default)]
struct Series {
    cpu: Vec<u64>,
    rss: Vec<u64>,
    wall: Vec<u64>,
}

impl Series {
    fn push(&mut self, usage: &resources::Usage) {
        self.cpu.push(
            usage
                .tree_user_cpu_us
                .saturating_add(usage.tree_system_cpu_us),
        );
        self.rss.push(usage.tree_max_rss_bytes);
        self.wall.push(usage.wall_us);
    }

    fn summary(&self) -> Value {
        json!({
            "cpu_us_p50": baseline::p50(&self.cpu),
            "cpu_us_p95": baseline_verdict::cpu_p95(self.cpu.clone()),
            "tree_max_rss_bytes_p50": baseline::p50(&self.rss),
            "tree_max_rss_bytes_p95": baseline_verdict::cpu_p95(self.rss.clone()),
            "wall_us_p50": baseline::p50(&self.wall),
            "wall_us_p95": baseline_verdict::cpu_p95(self.wall.clone()),
        })
    }
}

fn regime(
    root: &Path,
    sides: &[Side; 2],
    cache: CacheRegime,
    pairs: usize,
    method: &mut &'static str,
) -> Result<Value, String> {
    let warm = cache == CacheRegime::Warm;
    // Warm only: one regime-owned bytecode folder that lives for this regime.
    let shared = if warm {
        Some(tempfile::tempdir().map_err(|_| "cannot create bytecode folder")?)
    } else {
        None
    };
    let pycache = shared.as_ref().map(|folder| folder.path().to_path_buf());
    if warm {
        for side in sides {
            execute(root, side, &side.warmup_args, pycache.as_deref(), true)?;
        }
    }
    // At the protocol minimum the planner itself (with its own refusal)
    // fixes the order; a test-flag run below it uses the same order rule.
    let planned: Vec<(u64, [Arm; 2])> = if pairs >= MIN_PAIRS {
        trial_plan::plan(pairs as u64, cache)?
            .map(|pair| (pair.id, pair.order))
            .collect()
    } else {
        (1..=pairs as u64)
            .map(|id| (id, trial_plan::order(id, cache)))
            .collect()
    };
    let (mut oracle, mut candidate) = (Series::default(), Series::default());
    let mut ids = Vec::with_capacity(pairs);
    // The side order each pair actually ran in, logged at execution time.
    let mut orders: Vec<Vec<&'static str>> = Vec::with_capacity(pairs);
    for (id, order) in planned {
        let mut ran = Vec::with_capacity(2);
        for arm in order {
            let (side, series) = match arm {
                Arm::Baseline => (&sides[0], &mut oracle),
                Arm::Candidate => (&sides[1], &mut candidate),
            };
            let usage = execute(root, side, &side.args, pycache.as_deref(), false)?;
            ran.push(side.name);
            *method = usage.tree_method;
            series.push(&usage);
        }
        orders.push(ran);
        ids.push(id);
    }
    Ok(json!({
        "cache_state": if warm { "warm" } else { "cold" },
        "pair_ids": ids,
        "cached": 0,
        "first": orders.first().cloned().unwrap_or_default(),
        "orders": orders,
        "oracle_cpu_us": oracle.cpu,
        "oracle_tree_max_rss_bytes": oracle.rss,
        "oracle_wall_us": oracle.wall,
        "candidate_cpu_us": candidate.cpu,
        "candidate_tree_max_rss_bytes": candidate.rss,
        "candidate_wall_us": candidate.wall,
        "summary": {"oracle": oracle.summary(), "candidate": candidate.summary()},
    }))
}

/// The inputs one measure binds: the hashed sources, the HTTP fixtures, and
/// the candidate executable file (not files it may load).
#[derive(PartialEq)]
struct Inputs {
    sources: Value,
    fixtures_sha256: String,
    candidate_sha256: String,
}

fn inputs(root: &Path, program: &Path) -> Result<Inputs, String> {
    let binary = std::fs::read(program).map_err(|_| "cannot read the Rust candidate")?;
    Ok(Inputs {
        sources: baseline::hashed_sources_with(root, HASHED)?,
        fixtures_sha256: baseline::fixtures_sha256(root)?,
        candidate_sha256: capture_actions::digest(&binary),
    })
}

pub fn record(
    root: &Path,
    id: &str,
    python: &Path,
    source: &Source,
    pairs: usize,
    allow_few_pairs: bool,
) -> Result<Value, String> {
    if pairs == 0 {
        return Err("--pairs must be at least 1".into());
    }
    if pairs < MIN_PAIRS && !allow_few_pairs {
        return Err(format!(
            "--pairs {pairs} is below the protocol minimum of {MIN_PAIRS}"
        ));
    }
    // A test override never makes a protocol record: it needs the test flag.
    let test_override = matches!(source, Source::TestOverride(_));
    if test_override && !allow_few_pairs {
        return Err("--test-candidate needs --test-allow-few-pairs".into());
    }
    let step = baseline::step(id)?;
    let (program, args) = candidate(root, id, source)?;
    if !python.is_absolute() {
        return Err("--python must be an absolute path".into());
    }
    if std::env::var_os("CAREERBREW_HOME").is_some_and(|home| home.is_empty()) {
        return Err("CAREERBREW_HOME is set but empty".into());
    }
    let python_version = capture_actions::version(python)?;
    let bound = inputs(root, &program)?;
    let sources_sha256 = capture_actions::digest(bound.sources.to_string().as_bytes());
    let oracle_args = step.oracle.iter().map(OsString::from).collect::<Vec<_>>();
    let sides = [
        Side {
            name: "oracle",
            program: python.to_path_buf(),
            warmup_args: oracle_args
                .iter()
                .filter(|arg| *arg != "-B")
                .cloned()
                .collect(),
            args: oracle_args,
            python: true,
        },
        Side {
            name: "candidate",
            program: program.clone(),
            args: args.iter().map(OsString::from).collect(),
            warmup_args: args.iter().map(OsString::from).collect(),
            python: false,
        },
    ];
    let mut method = "unknown";
    let regimes = [CacheRegime::Cold, CacheRegime::Warm]
        .into_iter()
        .map(|cache| regime(root, &sides, cache, pairs, &mut method))
        .collect::<Result<Vec<_>, _>>()?;
    if inputs(root, &program).ok().as_ref() != Some(&bound) {
        return Err("a relevant source changed during the measure".into());
    }
    let (os, _) = resources::host_unit()?;
    let mut oracle_command = vec!["<python>".to_owned()];
    oracle_command.extend(step.oracle.iter().map(|arg| (*arg).to_owned()));
    let mut candidate_command = vec!["<candidate>".to_owned()];
    candidate_command.extend(args);
    Ok(json!({
        "schema": SCHEMA,
        "step": id,
        "test_override": test_override,
        "oracle_command": oracle_command,
        "candidate_command": candidate_command,
        "candidate_sha256": bound.candidate_sha256,
        "os": os,
        "arch": std::env::consts::ARCH,
        "rust_toolchain": baseline::toolchain(root)?,
        "python_version": python_version.trim(),
        "source_revision": baseline::revision(root)?,
        "source_tree_dirty": baseline::dirty(root)?,
        "sources_sha256": sources_sha256,
        "fixtures_sha256": bound.fixtures_sha256,
        "meets_protocol_minimum": pairs >= MIN_PAIRS && !test_override,
        "tree_method": method,
        "pairs": pairs,
        "regimes": regimes,
    }))
}

pub fn run(
    root: &Path,
    id: &str,
    python: &Path,
    source: &Source,
    pairs: usize,
    allow_few_pairs: bool,
    output: &Path,
) -> Result<Value, String> {
    if !output.is_absolute() {
        return Err("--out must be an absolute path".into());
    }
    if output.exists() {
        return Err("measure output already exists".into());
    }
    let record = record(root, id, python, source, pairs, allow_few_pairs)?;
    capture_actions::write_receipt(&record, output)?;
    Ok(record)
}

/// In-crate controls for the input binding. They edit a scratch copy of the
/// inputs, never the shared checkout.
#[cfg(test)]
mod controls {
    use super::*;

    fn copy(from: &Path, to: &Path) {
        if from.is_dir() {
            std::fs::create_dir_all(to).unwrap();
            for entry in std::fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                if entry.file_name() == "target" {
                    continue;
                }
                copy(&entry.path(), &to.join(entry.file_name()));
            }
        } else if from.is_file() {
            std::fs::copy(from, to).unwrap();
        }
    }

    fn scratch_root() -> (tempfile::TempDir, PathBuf) {
        let real = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().to_path_buf();
        for name in [
            "src/careerbrew",
            "xtask",
            "tests/fixtures/http",
            "docs/plans",
            "pyproject.toml",
            "uv.lock",
            "Cargo.lock",
        ] {
            let to = root.join(name);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            copy(&real.join(name), &to);
        }
        let candidate = root.join("candidate");
        std::fs::write(&candidate, b"#!/bin/sh\nexit 0\n").unwrap();
        (scratch, candidate)
    }

    fn append(path: &Path) {
        let mut bytes = std::fs::read(path).unwrap_or_default();
        bytes.extend_from_slice(b"\n// changed\n");
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn a_hashed_source_change_breaks_the_input_binding() {
        for name in ["xtask/src/measure.rs", "xtask/src/trial_plan.rs"] {
            let (scratch, candidate) = scratch_root();
            let before = inputs(scratch.path(), &candidate).unwrap();
            assert!(inputs(scratch.path(), &candidate).unwrap() == before);
            append(&scratch.path().join(name));
            assert!(
                inputs(scratch.path(), &candidate).unwrap() != before,
                "{name}"
            );
        }
    }

    #[test]
    fn a_fixture_change_breaks_the_input_binding() {
        let (scratch, candidate) = scratch_root();
        let before = inputs(scratch.path(), &candidate).unwrap();
        append(
            &scratch
                .path()
                .join("tests/fixtures/http/synthetic-added.json"),
        );
        assert!(inputs(scratch.path(), &candidate).unwrap() != before);
    }

    #[test]
    fn a_candidate_change_breaks_the_input_binding() {
        let (scratch, candidate) = scratch_root();
        let before = inputs(scratch.path(), &candidate).unwrap();
        append(&candidate);
        assert!(inputs(scratch.path(), &candidate).unwrap() != before);
    }
}
