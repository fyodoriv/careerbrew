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

#[derive(Default)]
struct Observation {
    exit: Option<i32>,
    stdout: String,
    writes: BTreeMap<String, String>,
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

fn observed_files(root: &Path, folder: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut found = BTreeMap::new();
    if !folder.exists() {
        return Ok(found);
    }
    for entry in std::fs::read_dir(folder).map_err(|_| "cannot list owned files")? {
        let path = entry.map_err(|_| "cannot list owned entry")?.path();
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|_| "cannot inspect owned entry")?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "entry escaped root")?
            .to_string_lossy()
            .into_owned();
        if metadata.is_dir() {
            found.insert(
                relative,
                format!("directory:{:o}", metadata.permissions().mode() & 0o777),
            );
            found.extend(observed_files(root, &path)?);
        } else if metadata.is_file() {
            let bytes = std::fs::read(&path).map_err(|_| "cannot read owned file")?;
            found.insert(
                relative,
                format!(
                    "file:{:o}:{}",
                    metadata.permissions().mode() & 0o777,
                    capture_actions::digest(&bytes)
                ),
            );
        } else if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(&path).map_err(|_| "cannot read owned link")?;
            found.insert(
                relative,
                format!(
                    "symlink:{}",
                    capture_actions::digest(target.as_os_str().as_bytes())
                ),
            );
        } else {
            return Err("unsupported owned entry".into());
        }
    }
    Ok(found)
}

fn observe(
    root: &Path,
    program: &Path,
    args: &[&str],
    deny_python: bool,
) -> Result<Observation, String> {
    let scratch = tempfile::tempdir().map_err(|_| "cannot create rust-06 home")?;
    let owned = scratch.path().join("owned");
    let mut environment = capture_actions::owned_environment(root, &owned)?;
    if deny_python {
        environment.remove("PYTHONPATH");
        environment.insert(
            "PYTHONHOME".into(),
            scratch
                .path()
                .join(verify::NO_PYTHON_HOME)
                .to_string_lossy()
                .into_owned(),
        );
    }
    let before = observed_files(&owned, &owned)?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(root)
        .env_clear()
        .envs(&environment);
    let (_, exit, stdout, stderr) = capture_actions::execute(command)?;
    let after = observed_files(&owned, &owned)?;
    let writes = after
        .iter()
        .filter(|(path, value)| before.get(*path) != Some(*value))
        .map(|(path, value)| {
            (
                path.clone(),
                value.split(':').next().unwrap_or("entry").to_owned(),
            )
        })
        .collect();
    let mut leaks = BTreeSet::new();
    if owned.exists() {
        scan(&owned, &mut leaks)?;
    }
    for (stream, text) in [("stdout", &stdout), ("stderr", &stderr)] {
        if text.contains(CANARY) {
            leaks.insert(format!("{stream}-canary"));
        }
    }
    Ok(Observation {
        exit,
        stdout,
        writes,
        leaked: !leaks.is_empty(),
    })
}

fn component_case(
    root: &Path,
    python: &Path,
    candidate: &Path,
    cases: &mut Vec<Value>,
    violations: &mut verify::Violations,
) -> Result<(), String> {
    let oracle = observe(root, python, &["-I", "-B", ORACLE, "--all"], false)?;
    if oracle.exit != Some(0) || oracle.leaked {
        return Err("the rust-06 Python oracle failed or leaked; verify refused".into());
    }
    let answer = observe(root, candidate, &["--all"], true)?;
    let matched = answer.exit == Some(0) && answer.stdout.as_bytes() == oracle.stdout.as_bytes();
    violations.contract += u64::from(!matched);
    violations.privacy += u64::from(answer.leaked);
    cases.push(json!({
        "case": "backup-sync-component",
        "kind": "rust-06-component",
        "passed": matched && !answer.leaked,
        "exit_matches": answer.exit == Some(0),
        "semantic_match": answer.stdout.as_bytes() == oracle.stdout.as_bytes(),
        "comparison": "typed-results+semantic-state+handlers+allowlists",
        "oracle_sha256": capture_actions::digest(oracle.stdout.as_bytes()),
        "candidate_sha256": capture_actions::digest(answer.stdout.as_bytes()),
        "oracle_writes": oracle.writes.len(),
        "candidate_writes": answer.writes.len(),
        "canary_leaked": answer.leaked,
    }));
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
    component_case(root, python, &candidate, &mut cases, &mut violations)?;
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
        "side_effects_observed": "semantic-state+real-cross-engine-restore",
        "writes_traced": "component-and-cross-engine",
        "cases": cases,
    }))
}
