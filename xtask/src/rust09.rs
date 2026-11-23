//! Custom rust-09 runner: exact component output, semantic project writes,
//! restricted-output privacy and planted write-failure controls.

use crate::{baseline, capture_actions, verify};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const ORACLE: &str = "tests/oracles/rust09.py";
const CASE_IDS: &[&str] = &[
    "project-lifecycle",
    "readiness-gates",
    "priorities-tickets",
    "promise-lifecycle",
    "lock-and-team",
    "cli-handlers",
];
const PRIVATE_MARKERS: &[&str] = &["SYNTHCANARY"];

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

fn normalize_value(value: &mut Value, root: &Path) {
    match value {
        Value::String(text) => {
            let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
            *text = text
                .replace(&root.to_string_lossy().into_owned(), "<owned>")
                .replace(&canonical.to_string_lossy().into_owned(), "<owned>");
        }
        Value::Array(items) => {
            for item in items {
                normalize_value(item, root);
            }
        }
        Value::Object(items) => {
            for (key, item) in items {
                if key == "incarnation_id" {
                    *item = Value::String("<incarnation>".into());
                } else {
                    normalize_value(item, root);
                }
            }
        }
        _ => {}
    }
}

fn observed_files(root: &Path, folder: &Path) -> Result<Trace, String> {
    use std::os::unix::fs::MetadataExt;
    let mut found = BTreeMap::new();
    if !folder.exists() {
        return Ok(found);
    }
    for entry in std::fs::read_dir(folder).map_err(|_| "cannot list rust-09 files")? {
        let path = entry.map_err(|_| "cannot list rust-09 entry")?.path();
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|_| "cannot inspect rust-09 file")?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "rust-09 entry escaped its root")?
            .to_string_lossy()
            .into_owned();
        let semantic = if metadata.is_dir() {
            format!("directory:{:o}", metadata.permissions().mode() & 0o777)
        } else if metadata.is_file() {
            let bytes = std::fs::read(&path).map_err(|_| "cannot read rust-09 file")?;
            let mode = metadata.permissions().mode() & 0o777;
            let content = if bytes.starts_with(b"CBV1\n") {
                "sealed".to_owned()
            } else if relative.ends_with(".yaml") || relative.ends_with(".yml") {
                std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|text| careerbrew_vault::json_yaml::yaml_read(text).ok())
                    .map(|value| {
                        let mut value = careerbrew_setup::value::from_canon(&value);
                        normalize_value(&mut value, root);
                        format!(
                            "yaml:{}",
                            capture_actions::digest(value.to_string().as_bytes())
                        )
                    })
                    .unwrap_or_else(|| capture_actions::digest(&bytes))
            } else if relative.ends_with(".json") {
                std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|text| serde_json::from_str::<Value>(text).ok())
                    .map(|mut value| {
                        normalize_value(&mut value, root);
                        format!(
                            "json:{}",
                            capture_actions::digest(value.to_string().as_bytes())
                        )
                    })
                    .unwrap_or_else(|| capture_actions::digest(&bytes))
            } else if relative.ends_with(".jsonl") {
                std::str::from_utf8(&bytes)
                    .ok()
                    .map(|text| {
                        let rows = text
                            .lines()
                            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                            .map(|mut value| {
                                normalize_value(&mut value, root);
                                value
                            })
                            .collect::<Vec<_>>();
                        format!(
                            "jsonl:{}",
                            capture_actions::digest(Value::Array(rows).to_string().as_bytes())
                        )
                    })
                    .unwrap_or_else(|| capture_actions::digest(&bytes))
            } else {
                capture_actions::digest(&bytes)
            };
            format!("file:{mode:o}:{content}")
        } else if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(&path).map_err(|_| "cannot read rust-09 link")?;
            format!(
                "symlink:{}",
                capture_actions::digest(target.as_os_str().as_bytes())
            )
        } else {
            return Err("unsupported rust-09 entry".into());
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

fn candidate_paths(left: &Writes, right: &Writes) -> Vec<String> {
    left.keys()
        .filter(|path| !right.contains_key(*path))
        .cloned()
        .collect()
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
    let scratch = tempfile::tempdir().map_err(|_| "cannot create rust-09 home")?;
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
            environment.insert("CAREERBREW_RUST09_PLANT_WRITE".into(), plant.into());
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
        let leaked = PRIVATE_MARKERS
            .iter()
            .any(|marker| stdout.contains(marker) || stderr.contains(marker));
        phases.push(Phase {
            id,
            ran: exit == Some(0),
            stdout,
            writes: semantic(&after),
            counts,
            leaked,
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
            "the rust-09 Python oracle failed or leaked at {failed:?}; verify refused"
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
        let extra_paths = candidate_paths(&actual.writes, &expected.writes);
        let missing_paths = candidate_paths(&expected.writes, &actual.writes);
        let passed = matched && extra + missing + changed + duplicates == 0 && !actual.leaked;
        violations.contract += u64::from(!matched);
        violations.side_effect += extra + changed;
        violations.missing_writes += missing;
        violations.duplicate_writes += duplicates;
        violations.privacy += u64::from(actual.leaked);
        cases.push(json!({
            "case": format!("component:{}", expected.id), "kind": "rust-09-component-phase", "passed": passed,
            "exit_matches": actual.ran, "semantic_match": actual.stdout.as_bytes() == expected.stdout.as_bytes(),
            "comparison": "typed-results+semantic-write-map+phase-write-counts", "normalization": ["owned-path", "sealed-record", "incarnation-id"],
            "oracle_sha256": capture_actions::digest(expected.stdout.as_bytes()), "candidate_sha256": capture_actions::digest(actual.stdout.as_bytes()),
            "writes_match": extra + missing + changed == 0, "oracle_writes": expected.writes.len(), "candidate_writes": actual.writes.len(),
            "extra_writes": extra, "missing_writes": missing, "changed_writes": changed,
            "extra_write_paths": extra_paths, "missing_write_paths": missing_paths,
            "changed_write_paths": changed_paths, "duplicate_writes": duplicates,
            "oracle_writes_sha256": writes_sha256(&expected.writes), "candidate_writes_sha256": writes_sha256(&actual.writes), "credential_leaked": actual.leaked,
        }));
    }
    Ok(())
}

fn sources_sha256(root: &Path, step: &baseline::Step) -> Result<String, String> {
    let extra = step
        .sources
        .iter()
        .copied()
        .chain(["xtask/src/rust09.rs", "xtask/src/verify.rs"])
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
    let step = baseline::step("rust-09")?;
    if !python.is_absolute() {
        return Err("--python must be an absolute path".into());
    }
    let candidate = candidate_override
        .map(Path::to_path_buf)
        .or_else(|| step.candidate.map(|path| root.join(path)))
        .ok_or("step rust-09 has no Rust candidate yet")?;
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
    if sources_sha256(root, step)? != hash || baseline::evidence_hashes(root, step)? != evidence {
        return Err("a relevant rust-09 source changed during verify".into());
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
        "schema": verify::SCHEMA, "step": step.id, "source_revision": baseline::revision(root)?, "source_tree_dirty": baseline::dirty(root)?,
        "sources_sha256": hash, "evidence_sources_sha256": evidence.0, "fixtures_sha256": evidence.1,
        "test_candidate": candidate_override.is_some(), "test_oracle": false, "candidate_sha256": candidate_sha256, "test_scope": Value::Null,
        "python_denied": "env", "contracts": ["cli", "projects", "readiness", "priorities", "promises", "locks", "team", "privacy"],
        "case_count": cases.len(), "passed_count": cases.len() - failed.len(), "failed_cases": failed,
        "passed": failed.is_empty() && violations.total() == 0, "checks": checks, "violations": violations.to_json(),
        "unobserved_violations": verify::UNOBSERVED, "missing_contracts": ["http", "data", "role"],
        "side_effects_observed": "semantic-write-map+phase-write-counts", "writes_traced": "every-component-phase", "cases": cases,
    }))
}
