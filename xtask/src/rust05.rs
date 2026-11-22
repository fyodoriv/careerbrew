//! Custom rust-05 runner: semantic encrypted-state comparison, expected CLI
//! refusals 1/3, synthetic privacy scans, then the shared ten DATA cases.

use crate::{baseline, capture_actions, verify};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

const CANARY: &str = "SYNTHETIC-RUST05-CANARY-7Q9";
const SECRET: &str = "SYNTHETIC-RUST05-SECRET-NOT-AN-AUTHENTICATION-VALUE";
const ORACLE: &str = "tests/oracles/rust05.py";

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
            for (id, term) in [("canary", CANARY), ("secret", SECRET)] {
                if raw
                    .windows(term.len())
                    .any(|window| window == term.as_bytes())
                {
                    leaks.insert(id.to_owned());
                }
            }
        }
    }
    Ok(())
}

fn observe(
    root: &Path,
    program: &Path,
    args: &[&str],
    deny_python: bool,
) -> Result<Observation, String> {
    let scratch = tempfile::tempdir().map_err(|_| "cannot create rust-05 home")?;
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
    let before = capture_actions::files(&owned, &owned, None)?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(root)
        .env_clear()
        .envs(&environment);
    let (_, exit, stdout, stderr) = capture_actions::execute(command)?;
    let after = capture_actions::files(&owned, &owned, None)?;
    let writes = after
        .iter()
        .filter(|(path, value)| before.get(*path) != Some(*value))
        .map(|(path, value)| {
            let mode = value.split(':').next().unwrap_or("entry").to_owned();
            (path.clone(), mode)
        })
        .collect();
    let mut leaks = BTreeSet::new();
    if owned.exists() {
        scan(&owned, &mut leaks)?;
    }
    for (stream, text) in [("stdout", &stdout), ("stderr", &stderr)] {
        for (kind, term) in [("canary", CANARY), ("secret", SECRET)] {
            if text.contains(term) {
                leaks.insert(format!("{stream}-{kind}"));
            }
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
        return Err("the rust-05 Python oracle failed or leaked; verify refused".into());
    }
    let answer = observe(root, candidate, &["--all"], true)?;
    let matched = answer.exit == Some(0) && answer.stdout.as_bytes() == oracle.stdout.as_bytes();
    violations.contract += u64::from(!matched);
    violations.privacy += u64::from(answer.leaked);
    cases.push(json!({
        "case": "storage-receipts-cli",
        "kind": "rust-05-component",
        "passed": matched && !answer.leaked,
        "exit_matches": answer.exit == Some(0),
        "semantic_match": answer.stdout.as_bytes() == oracle.stdout.as_bytes(),
        "comparison": "typed-values+semantic-ciphertext+exact-noop-identity",
        "oracle_sha256": capture_actions::digest(oracle.stdout.as_bytes()),
        "candidate_sha256": capture_actions::digest(answer.stdout.as_bytes()),
        "canary_leaked": answer.leaked,
    }));
    Ok(())
}

fn refusal_case(
    root: &Path,
    python: &Path,
    candidate: &Path,
    name: &str,
    args: &[&str],
    cases: &mut Vec<Value>,
    violations: &mut verify::Violations,
) -> Result<(), String> {
    let mut oracle_args = vec!["-B", "-m", "careerbrew"];
    oracle_args.extend_from_slice(args);
    let oracle = observe(root, python, &oracle_args, false)?;
    if !matches!(oracle.exit, Some(1 | 3)) || oracle.leaked {
        return Err(format!(
            "oracle case {name} did not produce its expected refusal"
        ));
    }
    let answer = observe(root, candidate, args, true)?;
    let matched = answer.exit == oracle.exit
        && answer.stdout == oracle.stdout
        && answer.writes == oracle.writes;
    violations.contract +=
        u64::from(!(answer.exit == oracle.exit && answer.stdout == oracle.stdout));
    violations.side_effect += u64::from(answer.writes != oracle.writes);
    violations.privacy += u64::from(answer.leaked);
    cases.push(json!({
        "case": name,
        "kind": "rust-05-cli-refusal",
        "passed": matched && !answer.leaked,
        "expected_exit": oracle.exit,
        "exit_matches": answer.exit == oracle.exit,
        "stdout_matches": answer.stdout == oracle.stdout,
        "writes_match": answer.writes == oracle.writes,
        "oracle_stdout_sha256": capture_actions::digest(oracle.stdout.as_bytes()),
        "candidate_stdout_sha256": capture_actions::digest(answer.stdout.as_bytes()),
        "canary_leaked": answer.leaked,
    }));
    Ok(())
}

fn sources_sha256(root: &Path, step: &baseline::Step) -> Result<String, String> {
    let extra: Vec<&str> = step
        .sources
        .iter()
        .copied()
        .chain([
            "xtask/src/rust05.rs",
            "xtask/src/verify.rs",
            "xtask/tests/fixtures/capture/data_driver.py",
            "docs/plans/2026-11-05-rust-data-fixture-v1.json",
        ])
        .collect();
    let sources = baseline::hashed_sources_with(root, &extra)?;
    Ok(capture_actions::digest(sources.to_string().as_bytes()))
}

pub fn record(
    root: &Path,
    python: &Path,
    candidate_override: Option<&Path>,
) -> Result<Value, String> {
    let step = baseline::step("rust-05")?;
    if !python.is_absolute() {
        return Err("--python must be an absolute path".into());
    }
    let candidate = candidate_override
        .map(Path::to_path_buf)
        .or_else(|| step.candidate.map(|path| root.join(path)))
        .ok_or("step rust-05 has no Rust candidate yet")?;
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
    refusal_case(
        root,
        python,
        &candidate,
        "data-edit-missing",
        &["data", "edit", "absent.yaml"],
        &mut cases,
        &mut violations,
    )?;
    refusal_case(
        root,
        python,
        &candidate,
        "secret-set-nonterminal",
        &["secret", "set", "synthetic.service"],
        &mut cases,
        &mut violations,
    )?;
    verify::data_cases(root, python, &candidate, true, &mut cases, &mut violations)?;
    if sources_sha256(root, step)? != hash || baseline::evidence_hashes(root, step)? != evidence {
        return Err("a relevant rust-05 source changed during verify".into());
    }
    if capture_actions::digest(&std::fs::read(&candidate).map_err(|_| "cannot read candidate")?)
        != candidate_sha256
    {
        return Err("the candidate binary changed during verify".into());
    }
    let failed: Vec<String> = cases
        .iter()
        .filter(|case| case["passed"] != true)
        .filter_map(|case| case["case"].as_str().map(str::to_owned))
        .collect();
    let checks: Vec<Value> = cases
        .iter()
        .map(|case| json!({"id": case["case"], "passed": case["passed"]}))
        .collect();
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
        "contracts": ["cli", "side-effect", "data", "storage-semantics", "privacy"],
        "case_count": cases.len(),
        "passed_count": cases.len() - failed.len(),
        "failed_cases": failed,
        "passed": failed.is_empty() && violations.total() == 0,
        "checks": checks,
        "violations": violations.to_json(),
        "unobserved_violations": verify::UNOBSERVED,
        "missing_contracts": ["http", "role"],
        "side_effects_observed": "semantic-state+exact-noop-identity",
        "writes_traced": "per-phase",
        "cases": cases,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn planted_stdout_canary_is_a_privacy_leak() {
        let scratch = tempfile::tempdir().unwrap();
        let script = scratch.path().join("stdout-control.sh");
        std::fs::write(&script, format!("#!/bin/sh\nprintf '%s\\n' '{CANARY}'\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let observed = observe(root, &script, &[], false).unwrap();
        assert_eq!(observed.exit, Some(0));
        assert!(observed.leaked, "stdout canary escaped the privacy scan");
    }

    #[test]
    fn planted_stderr_canary_is_a_privacy_leak() {
        let scratch = tempfile::tempdir().unwrap();
        let script = scratch.path().join("stderr-control.sh");
        std::fs::write(
            &script,
            format!("#!/bin/sh\nprintf '%s\\n' '{CANARY}' >&2\n"),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let observed = observe(root, &script, &[], false).unwrap();
        assert_eq!(observed.exit, Some(0));
        assert!(observed.leaked, "stderr canary escaped the privacy scan");
    }
}
