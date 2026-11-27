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
use crate::{baseline, baseline_verdict, capture_actions, resources, rust19, rust20};
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
/// exit 0. A vector step whose oracle builds its vectors in memory names its
/// tracked generator in `vectors`: measure runs it once (with the measured
/// Python, `-I -B`, in a synthetic home) into a scratch file and appends that
/// file's path to the candidate's arguments, so both sides answer the same
/// vectors. The same file is appended to the oracle's arguments too, so
/// neither side's samples include vector generation. The record shows the
/// path as `<vectors-file>` on both commands.
pub struct Candidate {
    pub step: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub vectors: Option<&'static str>,
}

/// The registered Rust candidates. rust-02's stub runs every vector set in
/// one process, like its oracle (`--all`).
pub const CANDIDATES: &[Candidate] = &[
    Candidate {
        step: "rust-02",
        program: "target/debug/careerbrew-purevec",
        args: &["--all"],
        vectors: None,
    },
    // rust-03's pilot binary is measured as a release build (`cargo build
    // --release -p careerbrew-cli` first); the record names the profile.
    Candidate {
        step: "rust-03",
        program: "target/release/careerbrew-native",
        args: &["actions", "--json"],
        vectors: None,
    },
    // rust-04: both sides run every set of one generated file, the oracle
    // as `rust04.py --all <file>` and the candidate as
    // `policy-vectors --all <file>` (the release build).
    Candidate {
        step: "rust-04",
        program: "target/release/policy-vectors",
        args: &["--all"],
        vectors: Some("tests/oracles/rust04_vectors.py"),
    },
    Candidate {
        step: "rust-05",
        program: "target/release/vault-store",
        args: &["--all"],
        vectors: None,
    },
    Candidate {
        step: "rust-06",
        program: "target/release/vault-sync",
        args: &["--all"],
        vectors: None,
    },
    Candidate {
        step: "rust-07",
        program: "target/release/setup-component",
        args: &["--all"],
        vectors: None,
    },
    Candidate {
        step: "rust-08",
        program: "target/release/sources-component",
        args: &["--all"],
        vectors: None,
    },
    Candidate {
        step: "rust-09",
        program: "target/release/projects-component",
        args: &["--all"],
        vectors: None,
    },
    Candidate {
        step: "rust-10",
        program: "target/release/sessions-component",
        args: &["--all"],
        vectors: None,
    },
    Candidate {
        step: "rust-11",
        program: "target/release/memory-component",
        args: &["--all"],
        vectors: None,
    },
    Candidate {
        step: "rust-12",
        program: "target/release/learn-component",
        args: &["--all"],
        vectors: None,
    },
    Candidate {
        step: "rust-13",
        program: "target/release/feed-component",
        args: &["--all"],
        vectors: None,
    },
    Candidate {
        step: "rust-14",
        program: "target/release/live-component",
        args: &["--all"],
        vectors: None,
    },
    Candidate {
        step: "rust-19",
        program: "target/release/jobs-component",
        args: &["--all"],
        vectors: None,
    },
    Candidate {
        step: "rust-20",
        program: "target/release/tutor-component",
        args: &["--all"],
        vectors: None,
    },
    Candidate {
        step: "rust-15",
        program: "target/release/workday-component",
        args: &["--all"],
        vectors: None,
    },
];

/// The cargo profile a registered candidate was built with, read from its
/// `target/<profile>/` folder (rust-02 measures debug, rust-03 release).
pub fn profile(program: &Path) -> &'static str {
    let parent = program.parent().and_then(Path::file_name);
    match parent.and_then(|name| name.to_str()) {
        Some("release") => "release",
        Some("debug") => "debug",
        _ => "unknown",
    }
}

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

/// The registered candidate's vector generator, if it needs one. A test
/// override never gets one.
fn vector_generator(id: &str, source: &Source) -> Option<&'static str> {
    match source {
        Source::TestOverride(_) => None,
        Source::Registered => CANDIDATES
            .iter()
            .find(|candidate| candidate.step == id)
            .and_then(|candidate| candidate.vectors),
    }
}

/// Runs a step's tracked vector generator once into `folder` (the generator
/// refuses an existing file) and returns the new file's path. It runs in a
/// fresh synthetic home like every side, and must exit 0.
fn generate_vectors(
    root: &Path,
    python: &Path,
    generator: &str,
    folder: &Path,
) -> Result<PathBuf, String> {
    let file = folder.join("vectors.json");
    let side = Side {
        name: "vector generator",
        program: python.to_path_buf(),
        args: Vec::new(),
        warmup_args: Vec::new(),
        python: true,
        env: &[],
    };
    let args = [
        OsString::from("-I"),
        OsString::from("-B"),
        OsString::from(generator),
        file.clone().into_os_string(),
    ];
    execute(root, &side, &args, None, false)?;
    if !file.is_file() {
        return Err("the vector generator wrote no file; measure refused".into());
    }
    Ok(file)
}

/// One side's command: program, arguments, and its warm-up arguments.
struct Side {
    name: &'static str,
    program: PathBuf,
    args: Vec<OsString>,
    warmup_args: Vec<OsString>,
    python: bool,
    /// The step's extra environment (`baseline::Step::env`).
    env: &'static [(&'static str, &'static str)],
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
    for (key, value) in side.env {
        environment.insert((*key).to_owned(), (*value).to_owned());
    }
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

fn inputs(root: &Path, step: &baseline::Step, program: &Path) -> Result<Inputs, String> {
    let binary = std::fs::read(program).map_err(|_| "cannot read the Rust candidate")?;
    let hashed: Vec<&str> = HASHED.iter().chain(step.sources).copied().collect();
    let mut sources = baseline::hashed_sources_with(root, &hashed)?;
    baseline::bind_env(&mut sources, step)?;
    Ok(Inputs {
        sources,
        fixtures_sha256: baseline::fixtures_sha256(root, step)?,
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
    let bound = inputs(root, step, &program)?;
    let evidence = baseline::evidence_hashes(root, step)?;
    let sources_sha256 = capture_actions::digest(bound.sources.to_string().as_bytes());
    // Lives until the record is built: every candidate run reads this file.
    let vector_folder = tempfile::tempdir().map_err(|_| "cannot create vector folder")?;
    let mut candidate_args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let mut args = args;
    let mut oracle_args = step.oracle.iter().map(OsString::from).collect::<Vec<_>>();
    let mut oracle_command = vec!["<python>".to_owned()];
    oracle_command.extend(step.oracle.iter().map(|arg| (*arg).to_owned()));
    if let Some(generator) = vector_generator(step.id, source) {
        let file = generate_vectors(root, python, generator, vector_folder.path())?;
        // Both sides read the same file: no sample includes generation.
        oracle_args.push(file.clone().into_os_string());
        candidate_args.push(file.into_os_string());
        // The record stays content-free: no scratch path.
        args.push("<vectors-file>".to_owned());
        oracle_command.push("<vectors-file>".to_owned());
    }
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
            env: step.env,
        },
        Side {
            name: "candidate",
            program: program.clone(),
            warmup_args: candidate_args.clone(),
            args: candidate_args,
            python: false,
            env: step.env,
        },
    ];
    if id == "rust-19" {
        rust19::validate_measure(root, python, &program)?;
    }
    if id == "rust-20" {
        rust20::validate_measure(root, python, &program)?;
    }
    let mut method = "unknown";
    let regimes = [CacheRegime::Cold, CacheRegime::Warm]
        .into_iter()
        .map(|cache| regime(root, &sides, cache, pairs, &mut method))
        .collect::<Result<Vec<_>, _>>()?;
    if inputs(root, step, &program).ok().as_ref() != Some(&bound)
        || baseline::evidence_hashes(root, step)? != evidence
    {
        return Err("a relevant source changed during the measure".into());
    }
    let (os, _) = resources::host_unit()?;
    let profile = if test_override {
        "test-override"
    } else {
        profile(&program)
    };
    let mut candidate_command = vec!["<candidate>".to_owned()];
    candidate_command.extend(args);
    Ok(json!({
        "schema": SCHEMA,
        "step": id,
        "test_override": test_override,
        "oracle_command": oracle_command,
        "candidate_command": candidate_command,
        "candidate_sha256": bound.candidate_sha256,
        "candidate_profile": profile,
        "os": os,
        "arch": std::env::consts::ARCH,
        "rust_toolchain": baseline::toolchain(root)?,
        "python_version": python_version.trim(),
        "source_revision": baseline::revision(root)?,
        "source_tree_dirty": baseline::dirty(root)?,
        "sources_sha256": sources_sha256,
        "evidence_sources_sha256": evidence.0,
        "fixtures_sha256": evidence.1,
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
            "rust-toolchain.toml",
            "crates/cbcli",
            "scripts/gen-registry.py",
            "config/native-routes.txt",
        ] {
            let to = root.join(name);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            copy(&real.join(name), &to);
        }
        let candidate = root.join("candidate");
        std::fs::write(&candidate, b"#!/bin/sh\nexit 0\n").unwrap();
        (scratch, candidate)
    }

    /// The CLI step: these controls hash the shared inputs plus rust-03's
    /// pilot crate sources, which `scratch_root` copies.
    fn cli_step() -> &'static baseline::Step {
        baseline::step("rust-03").unwrap()
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
            let before = inputs(scratch.path(), cli_step(), &candidate).unwrap();
            assert!(inputs(scratch.path(), cli_step(), &candidate).unwrap() == before);
            append(&scratch.path().join(name));
            assert!(
                inputs(scratch.path(), cli_step(), &candidate).unwrap() != before,
                "{name}"
            );
        }
    }

    #[test]
    fn a_fixture_change_breaks_the_input_binding() {
        let (scratch, candidate) = scratch_root();
        let before = inputs(scratch.path(), cli_step(), &candidate).unwrap();
        append(
            &scratch
                .path()
                .join("tests/fixtures/http/synthetic-added.json"),
        );
        assert!(inputs(scratch.path(), cli_step(), &candidate).unwrap() != before);
    }

    #[test]
    fn rust_03_measures_the_release_pilot_binary() {
        let (path, args) = candidate(Path::new("/r"), "rust-03", &Source::Registered).unwrap();
        assert_eq!(path, Path::new("/r/target/release/careerbrew-native"));
        assert_eq!(args, ["actions", "--json"]);
        assert_eq!(profile(&path), "release");
        let (path, _) = candidate(Path::new("/r"), "rust-02", &Source::Registered).unwrap();
        assert_eq!(profile(&path), "debug");
        assert_eq!(profile(Path::new("/x/candidate")), "unknown");
    }

    #[test]
    fn rust_04_measures_policy_vectors_over_a_generated_vector_file() {
        let (path, args) = candidate(Path::new("/r"), "rust-04", &Source::Registered).unwrap();
        assert_eq!(path, Path::new("/r/target/release/policy-vectors"));
        assert_eq!(args, ["--all"]);
        assert_eq!(
            vector_generator("rust-04", &Source::Registered),
            Some("tests/oracles/rust04_vectors.py")
        );
        assert_eq!(vector_generator("rust-03", &Source::Registered), None);
        let fake = Path::new("/x/candidate");
        assert_eq!(
            vector_generator("rust-04", &Source::TestOverride(fake)),
            None
        );
        // The generator is a hashed source of the step.
        let step = baseline::step("rust-04").unwrap();
        assert!(step.sources.contains(&"tests/oracles/rust04_vectors.py"));
        assert_eq!(step.candidate, Some("target/release/policy-vectors"));
        // measure appends the same file to these oracle args, so the oracle
        // must take `--all` last (`rust04.py --all <vectors-file>`).
        assert_eq!(step.oracle.last(), Some(&"--all"));
    }

    #[test]
    fn mixed_pure_vector_fixture_records_are_refused() {
        let (scratch, _) = scratch_root();
        let root = scratch.path();
        let real = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let registered = root.join("target/debug/careerbrew-purevec");
        std::fs::create_dir_all(registered.parent().unwrap()).unwrap();
        copy(&real.join("target/debug/careerbrew-purevec"), &registered);
        let git = |args: &[&str]| {
            let output = std::process::Command::new("/usr/bin/git")
                .args(args)
                .current_dir(root)
                .env_clear()
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}: {output:?}");
        };
        git(&["init", "--quiet"]);
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.com",
            "commit",
            "--quiet",
            "-m",
            "initial",
        ]);
        let python = &real.join(".venv/bin/python");
        let base = baseline::record(root, "rust-02", python, 1, true).unwrap();
        let measured = record(root, "rust-02", python, &Source::Registered, 1, true).unwrap();
        // Whitespace changes the fixture bytes without changing its vectors.
        let fixture = root.join("xtask/tests/fixtures/purevec/hash.json");
        let bytes = std::fs::read_to_string(&fixture).unwrap();
        std::fs::write(&fixture, format!("{bytes}\n")).unwrap();
        let verified = crate::depvec::record(
            root,
            crate::depvec::step("rust-02").unwrap(),
            python,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(base["fixtures_sha256"], measured["fixtures_sha256"]);
        assert_ne!(base["fixtures_sha256"], verified["fixtures_sha256"]);
        assert_eq!(base["source_revision"], verified["source_revision"]);
        assert_eq!(measured["candidate_sha256"], verified["candidate_sha256"]);
        let floor = json!({
            "cpu_floor_us": 0, "cpu_absolute_us": 0,
            "rss_floor_bytes": 0, "rss_absolute_bytes": 0,
        });
        let error = crate::decide::verdict("rust-02", &base, &verified, &measured, Some(&floor))
            .unwrap_err();
        assert_eq!(
            error,
            "the verify record has another fixtures_sha256 than the baseline"
        );
    }

    #[test]
    fn a_candidate_change_breaks_the_input_binding() {
        let (scratch, candidate) = scratch_root();
        let before = inputs(scratch.path(), cli_step(), &candidate).unwrap();
        append(&candidate);
        assert!(inputs(scratch.path(), cli_step(), &candidate).unwrap() != before);
    }
}
