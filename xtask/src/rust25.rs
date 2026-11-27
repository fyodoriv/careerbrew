//! Custom rust-25 runner: the native web front against the Python server.
//!
//! Both sides run in one synthetic data root, so the change version (which
//! hashes inodes, sizes and mtimes) is comparable: the Python oracle seeds it
//! first, then the native `web-component --all` reuses the seed. Each side
//! starts its server on a free OS port, sends every native-owned case of
//! `tests/fixtures/web/rust25-cases.json` and prints one normalized line per
//! case (status, reason, every header but Server and Date, body). A case
//! passes when the two lines are byte-equal. Forwarded routes are compared in
//! `tests/unit/test_rust25_component.py`, which runs both servers as processes.
//!
//! `--test-plant-home-write` takes a planted control that changes one native
//! answer before the comparison: `csp` drops the CSP header of one case,
//! `cookie` changes the set-cookie flags, `body` changes one error text and
//! `missing` drops one case. Each must fail the record.

use crate::{baseline, capture_actions, verify};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

const ORACLE: &str = "tests/oracles/rust25.py";
const FIXTURE: &str = "tests/fixtures/web/rust25-cases.json";
/// The clock both sides read: after the token's creation, inside its 30 days.
const NOW: &str = "2026-11-20T10:00:00+00:00";
pub const PLANTS: &[&str] = &["csp", "cookie", "body", "missing"];

fn native_case_ids(root: &Path) -> Result<Vec<String>, String> {
    let text =
        std::fs::read_to_string(root.join(FIXTURE)).map_err(|_| "rust-25 fixture missing")?;
    let fixture: Value = serde_json::from_str(&text).map_err(|_| "rust-25 fixture is not JSON")?;
    Ok(fixture["cases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|case| case["owner"] == "native")
        .filter_map(|case| case["id"].as_str().map(str::to_owned))
        .collect())
}

fn lines(stdout: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for line in stdout.lines() {
        let value: Value = serde_json::from_str(line).map_err(|_| "a rust-25 line is not JSON")?;
        let id = value["case"].as_str().ok_or("a rust-25 line has no case")?;
        if out.insert(id.to_owned(), line.to_owned()).is_some() {
            return Err("a rust-25 case printed twice".into());
        }
    }
    Ok(out)
}

/// Change one native answer the way a broken server would.
fn plant(answers: &mut BTreeMap<String, String>, plant: &str) {
    let edit = |answers: &mut BTreeMap<String, String>, id: &str, from: &str, to: &str| {
        if let Some(line) = answers.get_mut(id) {
            *line = line.replacen(from, to, 1);
        }
    };
    match plant {
        "csp" => {
            if let Some(line) = answers.get_mut("changes-ok") {
                let mut value: Value = serde_json::from_str(line).unwrap_or(Value::Null);
                if let Some(headers) = value["headers"].as_array_mut() {
                    headers.retain(|pair| pair[0] != "content-security-policy");
                }
                *line = value.to_string();
            }
        }
        "cookie" => edit(answers, "session-set-ok", "SameSite=Strict", "SameSite=Lax"),
        "body" => edit(answers, "changes-bad-host", "bad host", "wrong host"),
        "missing" => {
            answers.remove("static-traversal");
        }
        _ => {}
    }
}

fn side(
    root: &Path,
    program: &Path,
    native: bool,
    environment: &BTreeMap<String, String>,
) -> Result<(bool, String, String), String> {
    let mut command = Command::new(program);
    if native {
        command.arg("--all");
    } else {
        command.args(["-I", "-B", ORACLE, "--all"]);
    }
    command.current_dir(root).env_clear().envs(environment);
    let (_, exit, stdout, stderr) = capture_actions::execute(command)?;
    Ok((exit == Some(0), stdout, stderr))
}

fn sources_sha256(root: &Path, step: &baseline::Step) -> Result<String, String> {
    let extra = step
        .sources
        .iter()
        .copied()
        .chain(["xtask/src/rust25.rs", "xtask/src/verify.rs"])
        .collect::<Vec<_>>();
    let sources = baseline::hashed_sources_with(root, &extra)?;
    Ok(capture_actions::digest(sources.to_string().as_bytes()))
}

pub fn record(
    root: &Path,
    python: &Path,
    candidate_override: Option<&Path>,
    planted: Option<&str>,
) -> Result<Value, String> {
    let step = baseline::step("rust-25")?;
    if !python.is_absolute() {
        return Err("--python must be an absolute path".into());
    }
    let candidate = candidate_override
        .map(Path::to_path_buf)
        .or_else(|| step.candidate.map(|path| root.join(path)))
        .ok_or("step rust-25 has no Rust candidate yet")?;
    if !candidate.is_absolute() || !candidate.is_file() {
        return Err("the candidate binary does not exist".into());
    }
    let candidate_sha256 =
        capture_actions::digest(&std::fs::read(&candidate).map_err(|_| "cannot read candidate")?);
    let hash = sources_sha256(root, step)?;
    let evidence = baseline::evidence_hashes(root, step)?;
    let ids = native_case_ids(root)?;
    if ids.is_empty() {
        return Err("the rust-25 fixture has no native cases".into());
    }

    let scratch = tempfile::tempdir().map_err(|_| "cannot create rust-25 home")?;
    let owned = scratch.path().join("owned");
    let mut environment = capture_actions::owned_environment(root, &owned)?;
    environment.insert("CAREERBREW_NOW".into(), NOW.into());
    environment.insert("TZ".into(), "UTC".into());
    let (oracle_ran, oracle_out, _) = side(root, python, false, &environment)?;
    if !oracle_ran {
        return Err("the rust-25 Python oracle failed; verify refused".into());
    }
    let expected = lines(&oracle_out)?;
    if expected.keys().cloned().collect::<Vec<_>>() != {
        let mut sorted = ids.clone();
        sorted.sort();
        sorted
    } {
        return Err("the rust-25 Python oracle did not answer every native case".into());
    }
    // The candidate needs no Python: deny it the same way rust-14 does.
    environment.insert(
        "PYTHONHOME".into(),
        scratch
            .path()
            .join(verify::NO_PYTHON_HOME)
            .to_string_lossy()
            .into_owned(),
    );
    let (candidate_ran, candidate_out, _) = side(root, &candidate, true, &environment)?;
    let mut actual = if candidate_ran {
        lines(&candidate_out)?
    } else {
        BTreeMap::new()
    };
    if let Some(name) = planted {
        plant(&mut actual, name);
    }
    let token = "synthetic-web-token-0123456789abcdef";
    let mut cases = Vec::new();
    let mut violations = verify::Violations::default();
    for id in &ids {
        let want = &expected[id];
        let got = actual.get(id);
        let matched = got.is_some_and(|line| line.as_bytes() == want.as_bytes());
        violations.contract += u64::from(!matched);
        // The token may appear only where the Python server sends it too.
        let leaked = got.is_some_and(|line| line.contains(token) && !want.contains(token));
        violations.privacy += u64::from(leaked);
        cases.push(json!({
            "case": format!("http:{id}"), "kind": "rust-25-http-case", "passed": matched && !leaked,
            "exit_matches": candidate_ran, "semantic_match": matched, "answered": got.is_some(),
            "comparison": "status+reason+headers-in-order+body", "normalization": ["server-header", "date-header", "cookie-port", "tab-key"],
            "oracle_sha256": capture_actions::digest(want.as_bytes()),
            "candidate_sha256": got.map(|line| capture_actions::digest(line.as_bytes())),
            "token_leaked": leaked,
        }));
    }
    let extra = actual
        .keys()
        .filter(|id| !expected.contains_key(*id))
        .count() as u64;
    violations.contract += extra;
    if sources_sha256(root, step)? != hash || baseline::evidence_hashes(root, step)? != evidence {
        return Err("a relevant rust-25 source changed during verify".into());
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
        "test_plant": planted,
        "python_denied": "env", "contracts": ["http-guard", "host-check", "token", "token-expiry", "sign-in-cookie", "sec-fetch-site", "corporate-paths", "change-version", "tab-key", "security-headers", "static-files", "unknown-endpoint"],
        "case_count": cases.len(), "passed_count": cases.len() - failed.len(), "failed_cases": failed, "extra_cases": extra,
        "passed": failed.is_empty() && violations.total() == 0, "checks": checks, "violations": violations.to_json(),
        "unobserved_violations": verify::UNOBSERVED, "missing_contracts": step.scoped_out(),
        "side_effects_observed": "none (read-only routes; the one-time browser link goes to a synthetic stub)", "cases": cases,
    }))
}
