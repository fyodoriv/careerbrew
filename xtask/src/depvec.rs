//! `migration verify --step rust-01`: dependency vectors. Each vector set is
//! a fixture `xtask/tests/fixtures/depvec/<set>.json` of `{id, op, input}`
//! vectors. The Python driver (`driver.py`, careerbrew's own code) and the
//! Rust candidate (`careerbrew-depvec`, maintained crates) each map every
//! vector to an output; verify compares them per vector, as case
//! `<set>:<id>`. The oracle must answer every vector; a candidate that
//! fails, prints no valid answer, misses a vector or adds one fails cases.
//!
//! Sets are found by their fixture files, so this file never changes for a
//! new set. `--test-wrong-vector <set>` plants a wrong vector: the
//! candidate gets the set's first input wrapped in another value, and that
//! case must fail.
use crate::{baseline, capture_actions, decide, verify};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

pub const STEP: &str = "rust-01";
/// The candidate binary, relative to the repo root
/// (`cargo build -p careerbrew-depvec`).
pub const CANDIDATE: &str = "target/debug/careerbrew-depvec";
pub const DRIVER: &str = "xtask/tests/fixtures/depvec/driver.py";
pub const FIXTURES: &str = "xtask/tests/fixtures/depvec";
/// The key a planted input is wrapped in.
pub const PLANTED: &str = "planted-wrong-vector";

/// Every vector set: its name (the fixture's file stem) and its vectors,
/// sorted by name. A fixture whose `set` differs from its name, with no
/// vectors, or with an empty or repeated id is refused.
pub fn sets(root: &Path) -> Result<Vec<(String, Vec<Value>)>, String> {
    let dir = root.join(FIXTURES);
    let mut names = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).map_err(|_| "cannot list the vector sets")? {
        let path = entry.map_err(|_| "cannot list the vector sets")?.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let stem = path.file_stem().and_then(|s| s.to_str());
            names.insert(stem.ok_or("vector set name is not UTF-8")?.to_owned());
        }
    }
    if names.is_empty() {
        return Err("no vector sets".into());
    }
    names
        .into_iter()
        .map(|name| {
            let text = std::fs::read_to_string(dir.join(format!("{name}.json")))
                .map_err(|_| "cannot read a vector set")?;
            let fixture: Value =
                serde_json::from_str(&text).map_err(|_| "a vector set is not JSON")?;
            let vectors = check_fixture(&name, &fixture)?;
            Ok((name, vectors))
        })
        .collect()
}

pub(crate) fn check_fixture(name: &str, fixture: &Value) -> Result<Vec<Value>, String> {
    if fixture["set"] != name {
        return Err(format!("vector set {name}: its set field differs"));
    }
    let vectors = fixture["vectors"]
        .as_array()
        .filter(|vectors| !vectors.is_empty())
        .ok_or_else(|| format!("vector set {name} has no vectors"))?;
    let mut ids = BTreeSet::new();
    for vector in vectors {
        let id = vector["id"].as_str().unwrap_or("");
        if id.is_empty() || !ids.insert(id) || vector["op"].as_str().is_none() {
            return Err(format!(
                "vector set {name}: a vector has no op or an empty or repeated id"
            ));
        }
    }
    Ok(vectors.clone())
}

/// Reads `{"set", "results": [{"id", "output"}]}` into id -> output. None
/// when the answer is not that shape, names another set or repeats an id.
pub(crate) fn results(set: &str, stdout: &str) -> Option<BTreeMap<String, Value>> {
    let answer: Value = serde_json::from_str(stdout).ok()?;
    if answer["set"] != set {
        return None;
    }
    let mut map = BTreeMap::new();
    for result in answer["results"].as_array()? {
        let id = result["id"].as_str()?.to_owned();
        if map.insert(id, result["output"].clone()).is_some() {
            return None;
        }
    }
    Some(map)
}

/// One case per vector, plus `<set>:extra-results` when the candidate
/// answers an id the set does not have. The oracle must answer every vector.
pub(crate) fn compare(
    set: &str,
    vectors: &[Value],
    oracle: &BTreeMap<String, Value>,
    candidate: Option<&BTreeMap<String, Value>>,
) -> Result<Vec<Value>, String> {
    let mut cases = Vec::new();
    for vector in vectors {
        let id = vector["id"].as_str().unwrap_or("");
        let expected = oracle
            .get(id)
            .ok_or_else(|| format!("the oracle gave no answer for {set}:{id}; verify refused"))?;
        let actual = candidate.and_then(|map| map.get(id));
        cases.push(json!({
            "case": format!("{set}:{id}"),
            "op": vector["op"],
            "passed": actual == Some(expected),
            "oracle_sha256": capture_actions::digest(expected.to_string().as_bytes()),
            "candidate_sha256": actual.map(|v| capture_actions::digest(v.to_string().as_bytes())),
        }));
    }
    if oracle.len() != vectors.len() {
        return Err(format!(
            "the oracle answered other ids in {set}; verify refused"
        ));
    }
    if let Some(map) = candidate {
        let extra = map.keys().filter(|id| !oracle.contains_key(*id)).count();
        if extra > 0 {
            cases.push(
                json!({"case": format!("{set}:extra-results"), "passed": false, "extra": extra}),
            );
        }
    }
    Ok(cases)
}

/// The set's fixture with its first input wrapped, for the planted control.
pub(crate) fn plant(vectors: &[Value]) -> Vec<Value> {
    let mut planted = vectors.to_vec();
    if let Some(first) = planted.first_mut() {
        first["input"] = json!({ (PLANTED): first["input"].take() });
    }
    planted
}

fn run_side(
    root: &Path,
    scratch: &Path,
    label: &str,
    program: &Path,
    args: &[&std::ffi::OsStr],
) -> Result<(Option<i32>, String), String> {
    let environment = capture_actions::owned_environment(root, &scratch.join(label))?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(root)
        .env_clear()
        .envs(&environment);
    let (_, exit, stdout, _) = capture_actions::execute(command)?;
    Ok((exit, stdout))
}

fn sources_sha256(root: &Path, sets: &[(String, Vec<Value>)]) -> Result<String, String> {
    let mut map = serde_json::Map::new();
    let mut names = vec![
        "xtask/src/depvec.rs".to_owned(),
        "xtask/src/verify.rs".to_owned(),
        DRIVER.to_owned(),
        "src/careerbrew/store.py".to_owned(),
    ];
    names.extend(
        sets.iter()
            .map(|(name, _)| format!("{FIXTURES}/{name}.json")),
    );
    for name in names {
        let bytes = std::fs::read(root.join(&name)).map_err(|_| "verify source missing")?;
        map.insert(name, json!(capture_actions::digest(&bytes)));
    }
    Ok(capture_actions::digest(
        Value::Object(map).to_string().as_bytes(),
    ))
}

/// The rust-01 verify record. `candidate` and `test_oracle` are the
/// test-only overrides `verify` takes; `wrong` plants a wrong vector in
/// that set.
pub fn record(
    root: &Path,
    python: &Path,
    candidate: Option<&Path>,
    test_oracle: Option<&Path>,
    wrong: Option<&str>,
) -> Result<Value, String> {
    if !python.is_absolute() {
        return Err("--python must be an absolute path".into());
    }
    if test_oracle.is_some_and(|oracle| !oracle.is_absolute()) {
        return Err("--test-oracle must be an absolute path".into());
    }
    let oracle = test_oracle.unwrap_or(python);
    if std::env::var_os("CAREERBREW_HOME").is_some_and(|home| home.is_empty()) {
        return Err("CAREERBREW_HOME is set but empty".into());
    }
    let test_candidate = candidate.is_some();
    let candidate = candidate.map_or_else(|| root.join(CANDIDATE), Path::to_path_buf);
    if !candidate.is_absolute() {
        return Err("the candidate must be an absolute path".into());
    }
    if !candidate.is_file() {
        return Err(
            "the candidate binary does not exist; run cargo build -p careerbrew-depvec".into(),
        );
    }
    let sets = sets(root)?;
    if wrong.is_some_and(|name| !sets.iter().any(|(set, _)| set == name)) {
        return Err("--test-wrong-vector names no vector set".into());
    }
    let hash = sources_sha256(root, &sets)?;
    let candidate_sha256 = capture_actions::digest(
        &std::fs::read(&candidate).map_err(|_| "cannot read the candidate binary")?,
    );
    let mut cases = Vec::new();
    for (set, vectors) in &sets {
        let scratch = tempfile::tempdir().map_err(|_| "cannot create verify home")?;
        let write = |file: &str, vectors: &[Value]| -> Result<std::path::PathBuf, String> {
            let path = scratch.path().join(file);
            std::fs::write(&path, json!({"set": set, "vectors": vectors}).to_string())
                .map_err(|_| "cannot write the vectors file".to_owned())?;
            Ok(path)
        };
        let oracle_file = write("oracle-vectors.json", vectors)?;
        let candidate_file = if wrong == Some(set.as_str()) {
            write("candidate-vectors.json", &plant(vectors))?
        } else {
            oracle_file.clone()
        };
        let driver = root.join(DRIVER);
        let (exit, stdout) = run_side(
            root,
            scratch.path(),
            "oracle",
            oracle,
            &[
                "-B".as_ref(),
                driver.as_os_str(),
                set.as_ref(),
                oracle_file.as_os_str(),
            ],
        )?;
        let expected = match (exit, results(set, &stdout)) {
            (Some(0), Some(map)) => map,
            _ => {
                return Err(format!(
                    "the oracle failed on vector set {set}; verify refused"
                ))
            }
        };
        let (exit, stdout) = run_side(
            root,
            scratch.path(),
            "candidate",
            &candidate,
            &[set.as_ref(), candidate_file.as_os_str()],
        )?;
        let actual = if exit == Some(0) {
            results(set, &stdout)
        } else {
            None
        };
        cases.extend(compare(set, vectors, &expected, actual.as_ref())?);
    }
    if sources_sha256(root, &sets)? != hash {
        return Err("a relevant source changed during verify".into());
    }
    let failed: Vec<String> = cases
        .iter()
        .filter(|case| case["passed"] != true)
        .filter_map(|case| case["case"].as_str().map(str::to_owned))
        .collect();
    let violations: serde_json::Map<String, Value> = decide::VIOLATIONS
        .iter()
        .map(|name| {
            let count = if *name == "contract" { failed.len() } else { 0 };
            ((*name).to_owned(), json!(count))
        })
        .collect();
    Ok(json!({
        "schema": verify::SCHEMA,
        "step": STEP,
        "source_revision": baseline::revision(root)?,
        "source_tree_dirty": baseline::dirty(root)?,
        "sources_sha256": hash,
        "test_candidate": test_candidate,
        "test_oracle": test_oracle.is_some(),
        "planted_wrong_vector": wrong,
        "candidate_sha256": candidate_sha256,
        "contracts": ["dependency-vectors"],
        "vector_sets": sets.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>(),
        "case_count": cases.len(),
        "passed_count": cases.len() - failed.len(),
        "failed_cases": failed,
        "passed": failed.is_empty(),
        "checks": cases.iter().map(|case| json!({"id": case["case"], "passed": case["passed"]})).collect::<Vec<_>>(),
        "violations": violations,
        "unobserved_violations": verify::UNOBSERVED,
        "missing_contracts": verify::MISSING_CONTRACTS,
        "cases": cases,
    }))
}

/// Self-test controls on synthetic answers: equal answers pass, a planted
/// wrong output fails only its case, a missing answer fails, an extra answer
/// fails, and a planted input changes only the first vector.
pub mod controls {
    use super::*;

    pub fn run() -> usize {
        let cases: [fn(); 5] = [
            equal_answers_pass,
            a_wrong_output_fails_its_case,
            a_missing_answer_fails,
            an_extra_answer_fails,
            a_planted_input_changes_only_the_first_vector,
        ];
        for case in cases {
            case();
        }
        cases.len()
    }

    fn vectors() -> Vec<Value> {
        vec![
            json!({"id": "a", "op": "json-read", "input": "1"}),
            json!({"id": "b", "op": "json-read", "input": "2"}),
        ]
    }

    fn answers() -> BTreeMap<String, Value> {
        BTreeMap::from([
            ("a".to_owned(), json!({"ok": {"t": "int", "v": "1"}})),
            ("b".to_owned(), json!({"ok": {"t": "int", "v": "2"}})),
        ])
    }

    fn failed(cases: &[Value]) -> Vec<&str> {
        cases
            .iter()
            .filter(|case| case["passed"] != true)
            .filter_map(|case| case["case"].as_str())
            .collect()
    }

    fn equal_answers_pass() {
        let cases = compare("s", &vectors(), &answers(), Some(&answers())).unwrap();
        assert!(
            cases.len() == 2 && failed(&cases).is_empty(),
            "equal answers must pass"
        );
    }

    fn a_wrong_output_fails_its_case() {
        let mut wrong = answers();
        wrong.insert(
            "b".into(),
            json!({"ok": {"t": "float", "v": "4000000000000000"}}),
        );
        let cases = compare("s", &vectors(), &answers(), Some(&wrong)).unwrap();
        assert_eq!(failed(&cases), ["s:b"], "a wrong output must fail its case");
    }

    fn a_missing_answer_fails() {
        let mut missing = answers();
        missing.remove("a");
        let cases = compare("s", &vectors(), &answers(), Some(&missing)).unwrap();
        assert_eq!(failed(&cases), ["s:a"], "a missing answer must fail");
        let cases = compare("s", &vectors(), &answers(), None).unwrap();
        assert_eq!(
            failed(&cases),
            ["s:a", "s:b"],
            "no answer must fail every case"
        );
        assert!(
            compare("s", &vectors(), &missing, Some(&answers())).is_err(),
            "an oracle that misses a vector must be refused"
        );
    }

    fn an_extra_answer_fails() {
        let mut extra = answers();
        extra.insert("c".into(), json!({"error": true}));
        let cases = compare("s", &vectors(), &answers(), Some(&extra)).unwrap();
        assert_eq!(
            failed(&cases),
            ["s:extra-results"],
            "an extra answer must fail"
        );
    }

    fn a_planted_input_changes_only_the_first_vector() {
        let planted = plant(&vectors());
        assert_eq!(planted[0]["input"], json!({(PLANTED): "1"}));
        assert_eq!(planted[1], vectors()[1], "the plant must touch one vector");
        assert!(
            check_fixture(
                "s",
                &json!({"set": "s", "vectors": [vectors()[0], vectors()[0]]})
            )
            .is_err(),
            "a repeated id must be refused"
        );
    }
}
