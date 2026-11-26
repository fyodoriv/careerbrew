//! Custom rust-20 runner: typed float tolerance, exact due times, semantic
//! writes, plugin resources, privacy, and planted write-failure controls.

use crate::{baseline, capture_actions, verify};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const ORACLE: &str = "tests/oracles/rust20.py";
const CASE_IDS: &[&str] = &[
    "depvec-fsrs",
    "fsrs-old-state",
    "cards-and-decks",
    "local-day-stats",
    "public-deck",
    "notion-idempotence",
    "notion-validation-and-partial-writes",
    "providers-and-mistakes",
];
const PRIVATE_MARKERS: &[&str] = &[
    "RUST20CANARYPERSON",
    "RUST20CANARYEMPLOYER",
    "RUST20CANARYANSWER",
    "RUST20CANARYPLUGIN",
];
const FLOAT_ABS_TOLERANCE: f64 = 0.00001;
const FLOAT_REL_TOLERANCE: f64 = 0.000001;
const DUE_ANCHOR: &str = "2026-10-21T09:10:00.123456+00:00";

#[derive(Clone, PartialEq)]
struct TraceEntry {
    semantic: String,
    inode: u64,
    mtime: i64,
    mtime_nsec: i64,
    event_rows: usize,
}

type Trace = BTreeMap<String, TraceEntry>;
type Writes = BTreeMap<String, String>;

struct Phase {
    id: &'static str,
    ran: bool,
    valid_report: bool,
    stdout: String,
    writes: Writes,
    counts: BTreeMap<String, u64>,
    events: BTreeMap<String, usize>,
    leaked: bool,
}

fn normalized_text(value: &str, root: &Path) -> String {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    value
        .replace(&canonical.to_string_lossy().into_owned(), "<owned>")
        .replace(&root.to_string_lossy().into_owned(), "<owned>")
}

fn normalize_value(value: &mut Value, root: &Path) {
    match value {
        Value::String(text) => *text = normalized_text(text, root),
        Value::Array(items) => {
            for item in items {
                normalize_value(item, root);
            }
        }
        Value::Object(items) => {
            let old = std::mem::take(items);
            let mut normalized = Map::new();
            for (key, mut item) in old {
                normalize_value(&mut item, root);
                normalized.insert(normalized_text(&key, root), item);
            }
            *items = normalized;
        }
        _ => {}
    }
}

fn normalize_event_times(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items {
                normalize_event_times(item);
            }
        }
        Value::Object(items) => {
            for (key, item) in items {
                if key == "ts" {
                    *item = Value::String("<event-time>".into());
                } else {
                    normalize_event_times(item);
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
    for entry in std::fs::read_dir(folder).map_err(|_| "cannot list rust-20 files")? {
        let path = entry.map_err(|_| "cannot list rust-20 entry")?.path();
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|_| "cannot inspect rust-20 file")?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "rust-20 entry escaped its root")?
            .to_string_lossy()
            .into_owned();
        let mut event_rows = 0;
        let semantic = if metadata.is_dir() {
            format!("directory:{:o}", metadata.permissions().mode() & 0o777)
        } else if metadata.is_file() {
            let bytes = std::fs::read(&path).map_err(|_| "cannot read rust-20 file")?;
            let mode = metadata.permissions().mode() & 0o777;
            let content = if relative.ends_with("keys/backup.key") {
                "generated-key".to_owned()
            } else if bytes.starts_with(b"CBV1\n") {
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
                                if relative.ends_with("logs/events.jsonl") {
                                    event_rows += 1;
                                    normalize_event_times(&mut value);
                                }
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
            let target = std::fs::read_link(&path).map_err(|_| "cannot read rust-20 link")?;
            format!(
                "symlink:{}",
                capture_actions::digest(target.as_os_str().as_bytes())
            )
        } else {
            return Err("unsupported rust-20 entry".into());
        };
        found.insert(
            relative,
            TraceEntry {
                semantic,
                inode: metadata.ino(),
                mtime: metadata.mtime(),
                mtime_nsec: metadata.mtime_nsec(),
                event_rows,
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

fn event_counts(trace: &Trace) -> BTreeMap<String, usize> {
    trace
        .iter()
        .filter(|(_, entry)| entry.event_rows > 0)
        .map(|(path, entry)| (path.clone(), entry.event_rows))
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

fn float_value(value: &Value) -> Option<f64> {
    let text = value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_number().map(ToString::to_string))?;
    text.parse::<f64>().ok().filter(|number| number.is_finite())
}

/// Compare full typed reports. Only actual float leaves use the registered
/// tolerance. Every timestamp, including every FSRS due value, stays exact.
fn typed_compare(actual: &Value, expected: &Value, floats: &mut usize) -> bool {
    if expected
        .as_object()
        .is_some_and(|items| items.len() == 1 && items.contains_key("float"))
    {
        let (Some(left), Some(right)) = (
            actual.get("float").and_then(float_value),
            expected.get("float").and_then(float_value),
        ) else {
            return false;
        };
        *floats += 1;
        return (left - right).abs()
            <= FLOAT_ABS_TOLERANCE.max(FLOAT_REL_TOLERANCE * left.abs().max(right.abs()));
    }
    match (actual, expected) {
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, value)| {
                    right
                        .get(key)
                        .is_some_and(|other| typed_compare(value, other, floats))
                })
        }
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(value, other)| typed_compare(value, other, floats))
        }
        _ => actual == expected,
    }
}

fn valid_report(stdout: &str, id: &str) -> bool {
    serde_json::from_str::<Value>(stdout)
        .ok()
        .is_some_and(|report| {
            report["cases"].as_array().is_some_and(|cases| {
                cases.len() == 1
                    && cases[0]["id"] == id
                    && cases[0]["result"]
                        .as_object()
                        .is_some_and(|result| !result.contains_key("error"))
            })
        })
}

fn component_side(
    root: &Path,
    program: &Path,
    native: bool,
    plant: Option<&str>,
) -> Result<Vec<Phase>, String> {
    let scratch = tempfile::tempdir().map_err(|_| "cannot create rust-20 home")?;
    let owned = scratch.path().join("owned");
    let mut environment = capture_actions::owned_environment(root, &owned)?;
    environment.insert(
        "CAREERBREW_RUST20_REPO".into(),
        root.to_string_lossy().into_owned(),
    );
    if plant == Some("event-duplicate") {
        environment.insert("CAREERBREW_RUST20_EVENT_CONTROL".into(), "1".into());
    }
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
            environment.insert("CAREERBREW_RUST20_PLANT_WRITE".into(), plant.into());
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
        let stdout = normalized_text(&stdout, &owned);
        let leaked = PRIVATE_MARKERS
            .iter()
            .any(|marker| stdout.contains(marker) || stderr.contains(marker));
        phases.push(Phase {
            id,
            ran: exit == Some(0),
            valid_report: valid_report(&stdout, id),
            stdout,
            writes: semantic(&after),
            counts,
            events: event_counts(&after),
            leaked,
        });
        state = after;
    }
    if native && plant == Some("duplicate") {
        let mut command = Command::new(program);
        command.args(["--case", "duplicate-write-control"]);
        command.current_dir(root).env_clear().envs(&environment);
        let (_, exit, stdout, stderr) = capture_actions::execute(command)?;
        let after = observed_files(&owned, &owned)?;
        let counts = write_counts(&state, &after);
        let stdout = normalized_text(&stdout, &owned);
        let leaked = PRIVATE_MARKERS
            .iter()
            .any(|marker| stdout.contains(marker) || stderr.contains(marker));
        phases.push(Phase {
            id: "duplicate-write-control",
            ran: exit == Some(0),
            valid_report: true,
            stdout,
            writes: semantic(&after),
            counts,
            events: event_counts(&after),
            leaked,
        });
    }
    Ok(phases)
}

fn duplicate_events(expected: &Phase, actual: &Phase) -> u64 {
    actual
        .events
        .iter()
        .map(|(path, count)| {
            count.saturating_sub(expected.events.get(path).copied().unwrap_or(0)) as u64
        })
        .sum()
}

fn component_cases(
    root: &Path,
    python: &Path,
    candidate: &Path,
    plant: Option<&str>,
    cases: &mut Vec<Value>,
    violations: &mut verify::Violations,
) -> Result<usize, String> {
    let oracle = component_side(
        root,
        python,
        false,
        plant.filter(|_| plant == Some("event-duplicate")),
    )?;
    if oracle
        .iter()
        .any(|phase| !phase.ran || !phase.valid_report || phase.leaked)
    {
        let failed = oracle
            .iter()
            .filter(|phase| !phase.ran || !phase.valid_report || phase.leaked)
            .map(|phase| phase.id)
            .collect::<Vec<_>>();
        return Err(format!(
            "the rust-20 Python oracle failed, returned an error case, or leaked at {failed:?}; verify refused"
        ));
    }
    let answer = component_side(root, candidate, true, plant)?;
    let mut total_floats = 0;
    for (expected, actual) in oracle.iter().zip(&answer) {
        let mut floats = 0;
        let semantic_match = typed_compare(
            &serde_json::from_str(&actual.stdout).unwrap_or(Value::Null),
            &serde_json::from_str(&expected.stdout).unwrap_or(Value::Null),
            &mut floats,
        );
        total_floats += floats;
        let due_anchor = expected.id != "fsrs-old-state"
            || (expected.stdout.contains(DUE_ANCHOR) && actual.stdout.contains(DUE_ANCHOR));
        let matched = actual.ran && actual.valid_report && semantic_match && due_anchor;
        let (extra, missing, changed) = diff(&expected.writes, &actual.writes);
        let duplicates = actual
            .counts
            .iter()
            .map(|(path, count)| {
                count.saturating_sub(expected.counts.get(path).copied().unwrap_or(0))
            })
            .sum::<u64>();
        let event_duplicates = duplicate_events(expected, actual);
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
        let passed = matched
            && extra + missing + changed + duplicates + event_duplicates == 0
            && !actual.leaked;
        violations.contract += u64::from(!matched);
        violations.side_effect += extra + changed;
        violations.missing_writes += missing;
        violations.duplicate_writes += duplicates + event_duplicates;
        violations.privacy += u64::from(actual.leaked);
        cases.push(json!({
            "case": format!("component:{}", expected.id), "kind": "rust-20-component-phase", "passed": passed,
            "exit_matches": actual.ran, "semantic_match": semantic_match && due_anchor,
            "comparison": "typed-results+typed-float-tolerance+exact-due-times+semantic-write-map+phase-write-counts+event-row-counts",
            "normalization": ["owned-path", "sealed-record", "generated-key", "event-timestamp"],
            "float_abs_tolerance": FLOAT_ABS_TOLERANCE, "float_rel_tolerance": FLOAT_REL_TOLERANCE,
            "float_leaves": floats, "due_times_exact": semantic_match && due_anchor,
            "oracle_sha256": capture_actions::digest(expected.stdout.as_bytes()), "candidate_sha256": capture_actions::digest(actual.stdout.as_bytes()),
            "writes_match": extra + missing + changed == 0, "oracle_writes": expected.writes.len(), "candidate_writes": actual.writes.len(),
            "extra_writes": extra, "missing_writes": missing, "changed_writes": changed,
            "extra_write_paths": extra_paths, "missing_write_paths": missing_paths, "changed_write_paths": changed_paths,
            "duplicate_writes": duplicates, "duplicate_event_rows": event_duplicates,
            "oracle_writes_sha256": writes_sha256(&expected.writes), "candidate_writes_sha256": writes_sha256(&actual.writes), "credential_leaked": actual.leaked,
        }));
    }
    if plant == Some("duplicate") {
        let actual = answer
            .last()
            .filter(|phase| phase.id == "duplicate-write-control")
            .ok_or("the duplicate-write control did not run")?;
        let expected = oracle.last().ok_or("the oracle has no final state")?;
        let (extra, missing, changed) = diff(&expected.writes, &actual.writes);
        let duplicates = actual.counts.values().sum::<u64>();
        let event_duplicates = duplicate_events(expected, actual);
        let passed = actual.ran
            && extra + missing + changed + duplicates + event_duplicates == 0
            && !actual.leaked;
        violations.contract += u64::from(!actual.ran);
        violations.side_effect += extra + changed;
        violations.missing_writes += missing;
        violations.duplicate_writes += duplicates + event_duplicates;
        violations.privacy += u64::from(actual.leaked);
        cases.push(json!({
            "case": "component:duplicate-write-control", "kind": "rust-20-component-phase", "passed": passed,
            "exit_matches": actual.ran, "semantic_match": true,
            "comparison": "expected-path+observed-phase-transition+typed-float-tolerance",
            "normalization": ["owned-path", "sealed-record", "generated-key", "event-timestamp"],
            "oracle_sha256": capture_actions::digest(expected.stdout.as_bytes()), "candidate_sha256": capture_actions::digest(actual.stdout.as_bytes()),
            "writes_match": extra + missing + changed == 0, "oracle_writes": expected.writes.len(), "candidate_writes": actual.writes.len(),
            "extra_writes": extra, "missing_writes": missing, "changed_writes": changed,
            "extra_write_paths": candidate_paths(&actual.writes, &expected.writes),
            "missing_write_paths": candidate_paths(&expected.writes, &actual.writes),
            "changed_write_paths": [], "duplicate_writes": duplicates, "duplicate_event_rows": event_duplicates,
            "oracle_writes_sha256": writes_sha256(&expected.writes), "candidate_writes_sha256": writes_sha256(&actual.writes), "credential_leaked": actual.leaked,
        }));
    }
    if total_floats < 8 {
        return Err(format!(
            "rust-20 compared only {total_floats} float leaves; at least 8 are required"
        ));
    }
    Ok(total_floats)
}

fn sources_sha256(root: &Path, step: &baseline::Step) -> Result<String, String> {
    let extra = step
        .sources
        .iter()
        .copied()
        .chain(["xtask/src/rust20.rs", "xtask/src/verify.rs"])
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
    let step = baseline::step("rust-20")?;
    if !python.is_absolute() {
        return Err("--python must be an absolute path".into());
    }
    let candidate = candidate_override
        .map(Path::to_path_buf)
        .or_else(|| step.candidate.map(|path| root.join(path)))
        .ok_or("step rust-20 has no Rust candidate yet")?;
    if !candidate.is_absolute() || !candidate.is_file() {
        return Err("the candidate binary does not exist".into());
    }
    let candidate_sha256 =
        capture_actions::digest(&std::fs::read(&candidate).map_err(|_| "cannot read candidate")?);
    let hash = sources_sha256(root, step)?;
    let evidence = baseline::evidence_hashes(root, step)?;
    let mut cases = Vec::new();
    let mut violations = verify::Violations::default();
    let float_leaves =
        component_cases(root, python, &candidate, plant, &mut cases, &mut violations)?;
    if sources_sha256(root, step)? != hash || baseline::evidence_hashes(root, step)? != evidence {
        return Err("a relevant rust-20 source changed during verify".into());
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
        "python_denied": "env", "contracts": ["cli", "fsrs", "old-state", "cards", "local-day", "public-deck", "notion", "plugin-resources", "providers", "mistakes", "privacy"],
        "case_count": cases.len(), "passed_count": cases.len() - failed.len(), "failed_cases": failed,
        "float_leaves": float_leaves, "float_abs_tolerance": FLOAT_ABS_TOLERANCE, "float_rel_tolerance": FLOAT_REL_TOLERANCE,
        "passed": failed.is_empty() && violations.total() == 0, "checks": checks, "violations": violations.to_json(),
        "unobserved_violations": verify::UNOBSERVED, "missing_contracts": ["http", "data", "role"],
        "side_effects_observed": "semantic-write-map+phase-write-counts+event-row-counts", "writes_traced": "every-component-phase", "cases": cases,
    }))
}
