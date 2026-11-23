//! Custom rust-06 runner: exact semantic component output plus real
//! cross-engine encrypted archive create/verify/restore in both directions.

use crate::{baseline, capture_actions, verify};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const CANARY: &str = "SYNTHETIC-RUST06-CANARY-7Q9";
const ORACLE: &str = "tests/oracles/rust06.py";

const CASE_IDS: &[&str] = &[
    "archive-roundtrip",
    "archive-rejections",
    "archive-paths",
    "archive-corporate",
    "archive-key-guards",
    "archive-targets",
    "sync-roundtrip",
    "sync-rejections",
    "sync-allowlist",
    "sync-plugin-ladder",
    "sync-user-ladder",
    "cli-handlers",
];

#[derive(Clone, PartialEq)]
struct TraceEntry {
    semantic: String,
    inode: u64,
    mtime: i64,
    mtime_nsec: i64,
}

type Trace = BTreeMap<String, TraceEntry>;
type Writes = BTreeMap<String, String>;

struct Phase {
    id: &'static str,
    ran: bool,
    stdout: String,
    writes: Writes,
    counts: BTreeMap<String, u64>,
    leaked: bool,
}

fn scan(folder: &Path, leaks: &mut BTreeSet<String>) -> Result<(), String> {
    for entry in std::fs::read_dir(folder).map_err(|_| "cannot scan synthetic home")? {
        let path = entry.map_err(|_| "cannot scan synthetic entry")?.path();
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|_| "cannot inspect synthetic entry")?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            scan(&path, leaks)?;
        } else if metadata.is_file() {
            let raw = std::fs::read(&path).map_err(|_| "cannot read synthetic entry")?;
            if raw
                .windows(CANARY.len())
                .any(|window| window == CANARY.as_bytes())
            {
                leaks.insert("canary".into());
            }
        }
    }
    Ok(())
}

fn normalized_path(path: &str) -> String {
    path.split('/')
        .map(|part| {
            if part.starts_with("careerbrew-") && part.ends_with(".cbk") {
                "careerbrew-<archive>.cbk"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn observed_files(root: &Path, folder: &Path) -> Result<Trace, String> {
    use std::os::unix::fs::MetadataExt;
    let mut found = BTreeMap::new();
    if !folder.exists() {
        return Ok(found);
    }
    for entry in std::fs::read_dir(folder).map_err(|_| "cannot list owned files")? {
        let path = entry.map_err(|_| "cannot list owned entry")?.path();
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|_| "cannot inspect owned entry")?;
        let relative = normalized_path(
            &path
                .strip_prefix(root)
                .map_err(|_| "entry escaped root")?
                .to_string_lossy(),
        );
        let semantic = if metadata.is_dir() {
            format!("directory:{:o}", metadata.permissions().mode() & 0o777)
        } else if metadata.is_file() {
            let bytes = std::fs::read(&path).map_err(|_| "cannot read owned file")?;
            let mode = metadata.permissions().mode() & 0o777;
            // Registered random-value normalization: sealed records, bundle
            // ciphertext and generated key bytes carry a type marker. Plaintext
            // files keep their exact content digest.
            let content = if bytes.starts_with(b"CBV1\n") {
                "sealed".to_owned()
            } else if relative.ends_with(".cbk")
                || relative.ends_with(".cbsync")
                || relative.ends_with("keys/backup.key")
                || relative.ends_with("keys/pair.key")
            {
                "random".to_owned()
            } else if relative.ends_with(".yaml") || relative.ends_with(".yml") {
                std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|text| careerbrew_vault::json_yaml::yaml_read(text).ok())
                    .map(|value| {
                        format!(
                            "yaml:{}",
                            capture_actions::digest(value.to_json().to_string().as_bytes())
                        )
                    })
                    .unwrap_or_else(|| capture_actions::digest(&bytes))
            } else if relative.ends_with(".json") {
                std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|text| careerbrew_vault::json_yaml::json_read(text).ok())
                    .map(|value| {
                        format!(
                            "json:{}",
                            capture_actions::digest(value.to_json().to_string().as_bytes())
                        )
                    })
                    .unwrap_or_else(|| capture_actions::digest(&bytes))
            } else {
                capture_actions::digest(&bytes)
            };
            format!("file:{mode:o}:{content}")
        } else if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(&path).map_err(|_| "cannot read owned link")?;
            let target = if target.is_absolute() {
                target
                    .strip_prefix(root)
                    .map_err(|_| "absolute link target escaped its owned root")?
                    .as_os_str()
            } else {
                target.as_os_str()
            };
            format!("symlink:{}", capture_actions::digest(target.as_bytes()))
        } else {
            return Err("unsupported owned entry".into());
        };
        found.insert(
            relative,
            TraceEntry {
                semantic,
                inode: metadata.ino(),
                mtime: metadata.mtime(),
                mtime_nsec: metadata.mtime_nsec(),
            },
        );
        if metadata.is_dir() {
            found.extend(observed_files(root, &path)?);
        }
    }
    Ok(found)
}

fn semantic(trace: &Trace) -> Writes {
    trace
        .iter()
        .map(|(path, entry)| (path.clone(), entry.semantic.clone()))
        .collect()
}

fn write_counts(before: &Trace, after: &Trace) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for (path, entry) in after {
        if before.get(path) != Some(entry) {
            *counts.entry(path.clone()).or_insert(0) += 1;
        }
    }
    for path in before.keys().filter(|path| !after.contains_key(*path)) {
        *counts.entry(path.clone()).or_insert(0) += 1;
    }
    counts
}

fn diff(oracle: &Writes, candidate: &Writes) -> (u64, u64, u64) {
    let extra = candidate
        .keys()
        .filter(|path| !oracle.contains_key(*path))
        .count() as u64;
    let missing = oracle
        .keys()
        .filter(|path| !candidate.contains_key(*path))
        .count() as u64;
    let changed = oracle
        .iter()
        .filter(|(path, value)| candidate.get(*path).is_some_and(|other| other != *value))
        .count() as u64;
    (extra, missing, changed)
}

fn writes_sha256(writes: &Writes) -> String {
    capture_actions::digest(json!(writes).to_string().as_bytes())
}

fn component_side(
    root: &Path,
    program: &Path,
    native: bool,
    plant: Option<&str>,
) -> Result<Vec<Phase>, String> {
    let scratch = tempfile::tempdir().map_err(|_| "cannot create rust-06 home")?;
    let owned = scratch.path().join("owned");
    let mut environment = capture_actions::owned_environment(root, &owned)?;
    if native {
        environment.remove("PYTHONPATH");
        environment.insert(
            "PYTHONHOME".into(),
            scratch
                .path()
                .join(verify::NO_PYTHON_HOME)
                .to_string_lossy()
                .into_owned(),
        );
        if let Some(plant) = plant {
            environment.insert("CAREERBREW_RUST06_PLANT_WRITE".into(), plant.into());
        }
    }
    let mut phases = Vec::new();
    let mut state = observed_files(&owned, &owned)?;
    for id in CASE_IDS {
        let mut command = Command::new(program);
        if native {
            command.args(["--case", id]);
        } else {
            command.args(["-I", "-B", ORACLE, "--case", id]);
        }
        command.current_dir(root).env_clear().envs(&environment);
        let (_, exit, stdout, stderr) = capture_actions::execute(command)?;
        let after = observed_files(&owned, &owned)?;
        let counts = write_counts(&state, &after);
        let mut leaks = BTreeSet::new();
        scan(&owned, &mut leaks)?;
        if stdout.contains(CANARY) || stderr.contains(CANARY) {
            leaks.insert("stream-canary".into());
        }
        phases.push(Phase {
            id,
            ran: exit == Some(0),
            stdout,
            writes: semantic(&after),
            counts,
            leaked: !leaks.is_empty(),
        });
        state = after;
    }
    Ok(phases)
}

fn component_cases(
    root: &Path,
    python: &Path,
    candidate: &Path,
    plant: Option<&str>,
    cases: &mut Vec<Value>,
    violations: &mut verify::Violations,
) -> Result<(), String> {
    let oracle = component_side(root, python, false, None)?;
    if oracle.iter().any(|phase| !phase.ran || phase.leaked) {
        let failed = oracle
            .iter()
            .filter(|phase| !phase.ran || phase.leaked)
            .map(|phase| phase.id)
            .collect::<Vec<_>>();
        return Err(format!(
            "the rust-06 Python oracle failed or leaked at {failed:?}; verify refused"
        ));
    }
    let answer = component_side(root, candidate, true, plant)?;
    for (expected, actual) in oracle.iter().zip(&answer) {
        let matched = actual.ran && actual.stdout.as_bytes() == expected.stdout.as_bytes();
        let (extra, missing, changed) = diff(&expected.writes, &actual.writes);
        let duplicates = actual
            .counts
            .iter()
            .map(|(path, count)| {
                count.saturating_sub(expected.counts.get(path).copied().unwrap_or(0))
            })
            .sum::<u64>();
        let changed_paths = expected
            .writes
            .iter()
            .filter(|(path, value)| {
                actual
                    .writes
                    .get(*path)
                    .is_some_and(|other| other != *value)
            })
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        let passed = matched && extra + missing + changed + duplicates == 0 && !actual.leaked;
        violations.contract += u64::from(!matched);
        violations.side_effect += extra + changed;
        violations.missing_writes += missing;
        violations.duplicate_writes += duplicates;
        violations.privacy += u64::from(actual.leaked);
        if !matched && expected.id == "archive-corporate" {
            violations.role += 1;
        }
        if !matched
            && matches!(
                expected.id,
                "archive-targets"
                    | "sync-rejections"
                    | "sync-allowlist"
                    | "sync-plugin-ladder"
                    | "sync-user-ladder"
            )
        {
            violations.authorization += 1;
        }
        cases.push(json!({
            "case": format!("component:{}", expected.id),
            "kind": "rust-06-component-phase",
            "passed": passed,
            "exit_matches": actual.ran,
            "semantic_match": actual.stdout.as_bytes() == expected.stdout.as_bytes(),
            "comparison": "typed-results+semantic-write-map+phase-write-counts",
            "normalization": ["archive-name", "sealed-record", "bundle-ciphertext", "generated-key"],
            "oracle_sha256": capture_actions::digest(expected.stdout.as_bytes()),
            "candidate_sha256": capture_actions::digest(actual.stdout.as_bytes()),
            "writes_match": extra + missing + changed == 0,
            "oracle_writes": expected.writes.len(),
            "candidate_writes": actual.writes.len(),
            "extra_writes": extra,
            "missing_writes": missing,
            "changed_writes": changed,
            "changed_write_paths": changed_paths,
            "duplicate_writes": duplicates,
            "oracle_writes_sha256": writes_sha256(&expected.writes),
            "candidate_writes_sha256": writes_sha256(&actual.writes),
            "canary_leaked": actual.leaked,
        }));
    }
    Ok(())
}

struct Cross {
    passed: bool,
    writes: usize,
    leaked: bool,
}

fn execute(
    program: &Path,
    args: &[&str],
    environment: &BTreeMap<String, String>,
) -> Result<(Option<i32>, String, String), String> {
    let mut command = Command::new(program);
    command.args(args).env_clear().envs(environment);
    let (_, exit, stdout, stderr) = capture_actions::execute(command)?;
    Ok((exit, stdout, stderr))
}

fn sole_archive(folder: &Path) -> Result<PathBuf, String> {
    let mut files = std::fs::read_dir(folder)
        .map_err(|_| "cross-engine backup folder missing")?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "cbk"))
        .collect::<Vec<_>>();
    if files.len() != 1 {
        return Err("cross-engine create did not write exactly one archive".into());
    }
    Ok(files.pop().unwrap())
}

fn cross_direction(
    root: &Path,
    python: &Path,
    candidate: &Path,
    python_creates: bool,
) -> Result<Cross, String> {
    let scratch = tempfile::tempdir().map_err(|_| "cannot create cross-engine home")?;
    let owned = scratch.path().join("owned");
    let data = owned.join("data");
    let backups = owned.join("backups");
    let restored = owned.join("restored");
    let mut environment = capture_actions::owned_environment(root, &owned)?;
    std::fs::create_dir_all(data.join("work")).map_err(|_| "cannot seed cross-engine data")?;
    std::fs::write(data.join("work/note.md"), b"synthetic cross-engine note\n")
        .map_err(|_| "cannot seed cross-engine note")?;
    environment.insert(
        "HOME".into(),
        owned.join("home").to_string_lossy().into_owned(),
    );
    environment.insert(
        "CAREERBREW_HOME".into(),
        data.to_string_lossy().into_owned(),
    );
    environment.insert(
        "CAREERBREW_KEY_FILE".into(),
        owned.join("keys/backup.key").to_string_lossy().into_owned(),
    );
    environment.insert(
        "CAREERBREW_BACKUP_DIRS".into(),
        backups.to_string_lossy().into_owned(),
    );
    environment.insert("CAREERBREW_EVENTS".into(), "off".into());
    environment.insert("CAREERBREW_ENCRYPT".into(), "on".into());
    let (creator, create_args, restorer, restore_args): (&Path, Vec<&str>, &Path, Vec<&str>) =
        if python_creates {
            (
                python,
                vec!["-I", "-B", "-m", "careerbrew"],
                candidate,
                vec![],
            )
        } else {
            (
                candidate,
                vec![],
                python,
                vec!["-I", "-B", "-m", "careerbrew"],
            )
        };
    let source_mode = std::fs::metadata(data.join("work/note.md"))
        .map_err(|_| "cannot inspect cross-engine source")?
        .permissions()
        .mode()
        & 0o777;
    let mut create_environment = environment.clone();
    if !python_creates {
        create_environment.remove("PYTHONPATH");
        create_environment.insert(
            "PYTHONHOME".into(),
            owned
                .join(verify::NO_PYTHON_HOME)
                .to_string_lossy()
                .into_owned(),
        );
    }
    let mut create = create_args;
    create.extend(["backup", "create"]);
    let (exit, _, _) = execute(creator, &create, &create_environment)?;
    if exit != Some(0) {
        return Ok(Cross {
            passed: false,
            writes: 0,
            leaked: false,
        });
    }
    let archive = sole_archive(&backups)?;
    let archive_text = archive.to_string_lossy().into_owned();
    let restored_text = restored.to_string_lossy().into_owned();
    let mut restore_environment = environment.clone();
    if python_creates {
        restore_environment.remove("PYTHONPATH");
        restore_environment.insert(
            "PYTHONHOME".into(),
            owned
                .join(verify::NO_PYTHON_HOME)
                .to_string_lossy()
                .into_owned(),
        );
    }
    let mut verify_args = restore_args.clone();
    verify_args.extend(["backup", "verify", &archive_text]);
    let (verify_exit, _, _) = execute(restorer, &verify_args, &restore_environment)?;
    let mut restore_args = restore_args;
    restore_args.extend(["backup", "restore", &archive_text, &restored_text]);
    let (restore_exit, _, _) = execute(restorer, &restore_args, &restore_environment)?;
    let content_matches = std::fs::read(restored.join("work/note.md")).ok().as_deref()
        == Some(b"synthetic cross-engine note\n");
    let archive_mode = std::fs::metadata(&archive)
        .map_err(|_| "cannot inspect cross-engine archive")?
        .permissions()
        .mode()
        & 0o777;
    let restored_mode = std::fs::metadata(restored.join("work/note.md"))
        .map_err(|_| "cannot inspect restored file")?
        .permissions()
        .mode()
        & 0o777;
    let mut leaks = BTreeSet::new();
    scan(&owned, &mut leaks)?;
    let files = observed_files(&owned, &owned)?;
    Ok(Cross {
        passed: verify_exit == Some(0)
            && restore_exit == Some(0)
            && content_matches
            && archive_mode == 0o600
            && restored_mode == source_mode,
        writes: files.len(),
        leaked: !leaks.is_empty(),
    })
}

fn cross_engine_case(
    root: &Path,
    python: &Path,
    candidate: &Path,
    cases: &mut Vec<Value>,
    violations: &mut verify::Violations,
) -> Result<(), String> {
    let python_rust = cross_direction(root, python, candidate, true)?;
    let rust_python = cross_direction(root, python, candidate, false)?;
    let passed =
        python_rust.passed && rust_python.passed && !python_rust.leaked && !rust_python.leaked;
    violations.contract += u64::from(!(python_rust.passed && rust_python.passed));
    violations.privacy += u64::from(python_rust.leaked || rust_python.leaked);
    cases.push(json!({
        "case": "cross-engine-archive-restore",
        "kind": "rust-06-cross-engine",
        "passed": passed,
        "python_create_rust_restore": python_rust.passed,
        "rust_create_python_restore": rust_python.passed,
        "writes_observed": python_rust.writes + rust_python.writes,
        "canary_leaked": python_rust.leaked || rust_python.leaked,
        "comparison": "real-archives+verify+restore+content+modes",
    }));
    Ok(())
}

fn sources_sha256(root: &Path, step: &baseline::Step) -> Result<String, String> {
    let extra = step
        .sources
        .iter()
        .copied()
        .chain(["xtask/src/rust06.rs", "xtask/src/verify.rs"])
        .collect::<Vec<_>>();
    let sources = baseline::hashed_sources_with(root, &extra)?;
    Ok(capture_actions::digest(sources.to_string().as_bytes()))
}

pub fn record(
    root: &Path,
    python: &Path,
    candidate_override: Option<&Path>,
    plant: Option<&str>,
) -> Result<Value, String> {
    let step = baseline::step("rust-06")?;
    if !python.is_absolute() {
        return Err("--python must be an absolute path".into());
    }
    let candidate = candidate_override
        .map(Path::to_path_buf)
        .or_else(|| step.candidate.map(|path| root.join(path)))
        .ok_or("step rust-06 has no Rust candidate yet")?;
    if !candidate.is_absolute() || !candidate.is_file() {
        return Err("the candidate binary does not exist".into());
    }
    let candidate_sha256 =
        capture_actions::digest(&std::fs::read(&candidate).map_err(|_| "cannot read candidate")?);
    let hash = sources_sha256(root, step)?;
    let evidence = baseline::evidence_hashes(root, step)?;
    let mut cases = Vec::new();
    let mut violations = verify::Violations::default();
    component_cases(root, python, &candidate, plant, &mut cases, &mut violations)?;
    cross_engine_case(root, python, &candidate, &mut cases, &mut violations)?;
    if sources_sha256(root, step)? != hash || baseline::evidence_hashes(root, step)? != evidence {
        return Err("a relevant rust-06 source changed during verify".into());
    }
    if capture_actions::digest(&std::fs::read(&candidate).map_err(|_| "cannot read candidate")?)
        != candidate_sha256
    {
        return Err("the candidate binary changed during verify".into());
    }
    let failed = cases
        .iter()
        .filter(|case| case["passed"] != true)
        .filter_map(|case| case["case"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let checks = cases
        .iter()
        .map(|case| json!({"id": case["case"], "passed": case["passed"]}))
        .collect::<Vec<_>>();
    Ok(json!({
        "schema": verify::SCHEMA,
        "step": step.id,
        "source_revision": baseline::revision(root)?,
        "source_tree_dirty": baseline::dirty(root)?,
        "sources_sha256": hash,
        "evidence_sources_sha256": evidence.0,
        "fixtures_sha256": evidence.1,
        "test_candidate": candidate_override.is_some(),
        "test_oracle": false,
        "candidate_sha256": candidate_sha256,
        "test_scope": Value::Null,
        "python_denied": "env",
        "contracts": ["cli", "side-effect", "archive-semantics", "cross-engine", "privacy"],
        "case_count": cases.len(),
        "passed_count": cases.len() - failed.len(),
        "failed_cases": failed,
        "passed": failed.is_empty() && violations.total() == 0,
        "checks": checks,
        "violations": violations.to_json(),
        "unobserved_violations": verify::UNOBSERVED,
        "missing_contracts": ["http", "data", "role"],
        "side_effects_observed": "semantic-write-map+phase-write-counts+real-cross-engine-restore",
        "writes_traced": "every-component-phase",
        "cases": cases,
    }))
}
