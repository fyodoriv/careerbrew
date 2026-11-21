//! `migration verify --step <id>` for a vector step (`rust-01` dependency
//! vectors, `rust-02` pure value vectors). Each vector set is a fixture
//! `<fixtures>/<set>.json` of `{id, op, input}` vectors. The Python driver
//! (careerbrew's own code) and the Rust candidate each map every vector to an
//! output; verify compares them per vector, as case `<set>:<id>`. The oracle
//! must answer every vector; a candidate that fails, prints no valid answer,
//! misses a vector or adds one fails cases.
//!
//! Steps are rows of [`STEPS`]; they share the compare and record code. Sets
//! are found by their fixture files, so this file never changes for a new
//! set. `--test-wrong-vector <set>` plants a wrong vector: the candidate gets
//! the set's first input wrapped in another value, and that case must fail.
//! A step with `count_home_writes` also snapshots the caller's real homes
//! (the repo's `private/` and a set `CAREERBREW_HOME`) before and after both sides and
//! records the count of changed files; any change fails the record.
use crate::{baseline, capture_actions, decide, verify};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// One registered vector step.
pub struct VectorStep {
    pub id: &'static str,
    /// The cargo package that builds the candidate.
    pub package: &'static str,
    /// The candidate binary, relative to the repo root.
    pub candidate: &'static str,
    pub driver: &'static str,
    pub fixtures: &'static str,
    /// The record's `contracts` entry.
    pub contract: &'static str,
    /// Python sources the oracle reads, hashed into `sources_sha256`.
    pub sources: &'static [&'static str],
    /// True while the step has no vector sets yet: verify then records one
    /// failed `no-vector-sets` case instead of refusing.
    pub allow_no_sets: bool,
    /// Count writes to the caller's real homes during verify.
    pub count_home_writes: bool,
}

pub const RUST_01: VectorStep = VectorStep {
    id: "rust-01",
    package: "careerbrew-depvec",
    candidate: "target/debug/careerbrew-depvec",
    driver: "xtask/tests/fixtures/depvec/driver.py",
    fixtures: "xtask/tests/fixtures/depvec",
    contract: "dependency-vectors",
    sources: &["src/careerbrew/store.py"],
    allow_no_sets: false,
    count_home_writes: false,
};

pub const RUST_02: VectorStep = VectorStep {
    id: "rust-02",
    package: "careerbrew-purevec",
    candidate: "target/debug/careerbrew-purevec",
    driver: "xtask/tests/fixtures/purevec/driver.py",
    fixtures: "xtask/tests/fixtures/purevec",
    contract: "pure-vectors",
    sources: &[
        "src/careerbrew/__init__.py",
        "src/careerbrew/config.py",
        "src/careerbrew/paths.py",
        "src/careerbrew/changes.py",
        "src/careerbrew/calendar.py",
    ],
    allow_no_sets: true,
    count_home_writes: true,
};

pub const STEPS: &[VectorStep] = &[RUST_01, RUST_02];

/// The vector step with this id, if it is one.
pub fn step(id: &str) -> Option<&'static VectorStep> {
    STEPS.iter().find(|step| step.id == id)
}

/// The key a planted input is wrapped in.
pub const PLANTED: &str = "planted-wrong-vector";

/// Every vector set: its name (the fixture's file stem) and its vectors,
/// sorted by name. A fixture whose `set` differs from its name, with no
/// vectors, or with an empty or repeated id is refused.
pub fn sets(root: &Path, step: &VectorStep) -> Result<Vec<(String, Vec<Value>)>, String> {
    let dir = root.join(step.fixtures);
    let mut names = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).map_err(|_| "cannot list the vector sets")? {
        let path = entry.map_err(|_| "cannot list the vector sets")?.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let stem = path.file_stem().and_then(|s| s.to_str());
            names.insert(stem.ok_or("vector set name is not UTF-8")?.to_owned());
        }
    }
    if names.is_empty() && !step.allow_no_sets {
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

fn sources_sha256(
    root: &Path,
    step: &VectorStep,
    sets: &[(String, Vec<Value>)],
) -> Result<String, String> {
    let mut map = serde_json::Map::new();
    let mut names = vec![
        "xtask/src/depvec.rs".to_owned(),
        "xtask/src/verify.rs".to_owned(),
        step.driver.to_owned(),
    ];
    names.extend(step.sources.iter().map(|name| (*name).to_owned()));
    names.extend(
        sets.iter()
            .map(|(name, _)| format!("{}/{name}.json", step.fixtures)),
    );
    for name in names {
        let bytes = std::fs::read(root.join(&name)).map_err(|_| "verify source missing")?;
        map.insert(name, json!(capture_actions::digest(&bytes)));
    }
    Ok(capture_actions::digest(
        Value::Object(map).to_string().as_bytes(),
    ))
}

/// One file's state in a home snapshot: its size and modification time.
/// Directories are listed but carry no state, so a new file counts once.
type FileState = Option<(u64, Option<SystemTime>)>;

/// The caller's real homes: careerbrew's default data folder (`private/`
/// in the repo root, as `paths.data_root` uses when `CAREERBREW_HOME` is
/// unset) and a set `CAREERBREW_HOME`. A home inside another one is left
/// out, so one write counts once.
pub(crate) fn real_homes(root: &Path) -> Vec<PathBuf> {
    let mut homes = vec![root.join("private")];
    if let Some(home) = std::env::var_os("CAREERBREW_HOME").filter(|home| !home.is_empty()) {
        homes.push(PathBuf::from(home));
    }
    let homes: Vec<PathBuf> = homes
        .into_iter()
        .map(|home| home.canonicalize().unwrap_or(home))
        .collect();
    let mut kept: Vec<PathBuf> = Vec::new();
    for home in &homes {
        let inside_other = homes
            .iter()
            .any(|other| other != home && home.starts_with(other));
        if !inside_other && !kept.contains(home) {
            kept.push(home.clone());
        }
    }
    kept
}

/// Every path under `homes` with its state. It never follows a symbolic
/// link; a folder it cannot list stays one entry. A missing home is empty.
pub(crate) fn snapshot(homes: &[PathBuf]) -> BTreeMap<PathBuf, FileState> {
    let mut seen = BTreeMap::new();
    let mut stack: Vec<PathBuf> = homes.to_vec();
    while let Some(path) = stack.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                stack.extend(entries.filter_map(|entry| entry.ok().map(|e| e.path())));
            }
            seen.insert(path, None);
        } else {
            seen.insert(path, Some((meta.len(), meta.modified().ok())));
        }
    }
    seen
}

/// The count of paths added, removed or changed between two snapshots.
pub(crate) fn changed(
    before: &BTreeMap<PathBuf, FileState>,
    after: &BTreeMap<PathBuf, FileState>,
) -> usize {
    let added_or_changed = after
        .iter()
        .filter(|(path, state)| before.get(*path) != Some(*state))
        .count();
    let removed = before
        .keys()
        .filter(|path| !after.contains_key(*path))
        .count();
    added_or_changed + removed
}

/// The verify record of a vector step. `candidate` and `test_oracle` are the
/// test-only overrides `verify` takes; `wrong` plants a wrong vector in that
/// set; `plant_write` (test-only) writes one file of that name into the
/// caller's `CAREERBREW_HOME` between the snapshots, which must be counted.
pub fn record(
    root: &Path,
    step: &VectorStep,
    python: &Path,
    candidate: Option<&Path>,
    test_oracle: Option<&Path>,
    wrong: Option<&str>,
    plant_write: Option<&str>,
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
    let planted_home = match plant_write {
        None => None,
        Some(_) if !step.count_home_writes => {
            return Err(
                "--test-plant-home-write applies only to a step that counts home writes".into(),
            )
        }
        Some(name) => {
            if name.is_empty() || name.contains('/') || name == "." || name == ".." {
                return Err("--test-plant-home-write must be a plain file name".into());
            }
            let home = std::env::var_os("CAREERBREW_HOME")
                .map(PathBuf::from)
                .filter(|home| home.is_absolute() && home.is_dir())
                .ok_or("--test-plant-home-write needs an absolute CAREERBREW_HOME folder")?;
            let path = home.join(name);
            if std::fs::symlink_metadata(&path).is_ok() {
                return Err("--test-plant-home-write names an existing file".into());
            }
            Some(path)
        }
    };
    let test_candidate = candidate.is_some();
    let candidate = candidate.map_or_else(|| root.join(step.candidate), Path::to_path_buf);
    if !candidate.is_absolute() {
        return Err("the candidate must be an absolute path".into());
    }
    if !candidate.is_file() {
        return Err(format!(
            "the candidate binary does not exist; run cargo build -p {}",
            step.package
        ));
    }
    let sets = sets(root, step)?;
    if wrong.is_some_and(|name| !sets.iter().any(|(set, _)| set == name)) {
        return Err("--test-wrong-vector names no vector set".into());
    }
    let hash = sources_sha256(root, step, &sets)?;
    let evidence = baseline::step(step.id)
        .ok()
        .map(|registered| baseline::evidence_hashes(root, registered))
        .transpose()?;
    let candidate_sha256 = capture_actions::digest(
        &std::fs::read(&candidate).map_err(|_| "cannot read the candidate binary")?,
    );
    let homes = real_homes(root);
    let before = step.count_home_writes.then(|| snapshot(&homes));
    let mut cases = Vec::new();
    for (set, vectors) in &sets {
        let scratch = tempfile::tempdir().map_err(|_| "cannot create verify home")?;
        let write = |file: &str, vectors: &[Value]| -> Result<PathBuf, String> {
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
        let driver = root.join(step.driver);
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
    if sets.is_empty() {
        // Only a step with `allow_no_sets` gets here: zero vectors prove
        // nothing, so the record fails rather than passing trivially.
        cases.push(json!({"case": "no-vector-sets", "passed": false}));
    }
    if let Some(path) = &planted_home {
        use std::io::Write as _;
        // create_new: never truncate a file this run did not create.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| "--test-plant-home-write names an existing file")?;
        file.write_all(b"planted home write\n")
            .map_err(|_| "cannot plant the home write")?;
    }
    let home_writes = before.map(|before| changed(&before, &snapshot(&homes)));
    if let Some(path) = &planted_home {
        std::fs::remove_file(path).map_err(|_| "cannot remove the planted home write")?;
    }
    if home_writes.is_some_and(|count| count > 0) {
        cases.push(json!({"case": "real-home-writes", "passed": false}));
    }
    let current_evidence = baseline::step(step.id)
        .ok()
        .map(|registered| baseline::evidence_hashes(root, registered))
        .transpose()?;
    if sources_sha256(root, step, &sets)? != hash || current_evidence != evidence {
        return Err("a relevant source changed during verify".into());
    }
    let current_candidate_sha256 = capture_actions::digest(
        &std::fs::read(&candidate).map_err(|_| "cannot read the candidate binary")?,
    );
    if current_candidate_sha256 != candidate_sha256 {
        return Err("the candidate binary changed during verify".into());
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
    let mut record = json!({
        "schema": verify::SCHEMA,
        "step": step.id,
        "source_revision": baseline::revision(root)?,
        "source_tree_dirty": baseline::dirty(root)?,
        "sources_sha256": hash,
        "test_candidate": test_candidate,
        "test_oracle": test_oracle.is_some(),
        "planted_wrong_vector": wrong,
        "candidate_sha256": candidate_sha256,
        "contracts": [step.contract],
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
    });
    if let Some((sources, fixtures)) = evidence {
        record["evidence_sources_sha256"] = json!(sources);
        record["fixtures_sha256"] = json!(fixtures);
    }
    if let Some(count) = home_writes {
        record["real_home_writes"] = json!(count);
        record["real_homes_checked"] = json!(homes.len());
        record["planted_home_write"] = json!(plant_write.is_some());
    }
    Ok(record)
}

/// Self-test controls on synthetic answers: equal answers pass, a planted
/// wrong output fails only its case, a missing answer fails, an extra answer
/// fails, a planted input changes only the first vector, a planted home
/// write is counted, and every vector step is registered once.
pub mod controls {
    use super::*;

    pub fn run() -> usize {
        let cases: [fn(); 7] = [
            equal_answers_pass,
            a_wrong_output_fails_its_case,
            a_missing_answer_fails,
            an_extra_answer_fails,
            a_planted_input_changes_only_the_first_vector,
            a_planted_home_write_is_counted,
            each_vector_step_is_registered_once,
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

    fn a_planted_home_write_is_counted() {
        let home = tempfile::tempdir().expect("control home");
        std::fs::create_dir(home.path().join("work")).expect("control folder");
        std::fs::write(home.path().join("work/kept.txt"), b"x").expect("control file");
        let homes = [home.path().to_path_buf()];
        let before = snapshot(&homes);
        assert_eq!(changed(&before, &snapshot(&homes)), 0, "no write counts 0");
        std::fs::write(home.path().join("work/planted.txt"), b"y").expect("planted");
        let after = snapshot(&homes);
        assert_eq!(changed(&before, &after), 1, "a planted write must count 1");
        std::fs::remove_file(home.path().join("work/kept.txt")).expect("remove");
        assert_eq!(
            changed(&before, &snapshot(&homes)),
            2,
            "a removed file must count too"
        );
        let missing = [home.path().join("absent")];
        assert!(snapshot(&missing).is_empty(), "a missing home is empty");
    }

    fn each_vector_step_is_registered_once() {
        let ids: BTreeSet<&str> = STEPS.iter().map(|step| step.id).collect();
        assert_eq!(ids.len(), STEPS.len(), "a vector step is registered twice");
        for id in ["rust-01", "rust-02"] {
            let found = step(id).expect("a vector step is missing");
            // rust-01 has no product path to time, so no baseline step.
            if id != "rust-01" {
                assert!(baseline::step(id).is_ok(), "{id} needs a baseline step");
            }
            assert!(found.candidate.ends_with(found.package));
        }
        assert!(step("rust-03").is_none(), "rust-03 is a CLI step");
    }
}
