//! The vector-driver kind (`rust-04a`): one generated vector file feeds both
//! drivers. xtask runs the tracked generator once per run into a scratch
//! file, then passes that same path to the Python driver and the Rust
//! driver, set by set. Only the generator is tracked; the vector file is not.
//!
//! Protocol (each program runs in its own fresh synthetic home):
//!
//! - generator: `python -I <generator> <vector-file>` writes
//!   `{"sets": {"<set>": [{"id", "op", "input"}]}}`.
//! - driver probe: `<driver> unicode-version` prints the Unicode table
//!   version it folds and splits with. Python and Rust must agree, or the
//!   run records one failed `unicode-version` case and runs no other case.
//! - driver run: `<driver> <set> <vector-file>` prints
//!   `{"set", "results": [{"id", "output"}]}` (the `depvec` answer shape).
//!
//! Each vector is one case, `<set>:<id>`, compared with the shared
//! `depvec::compare`. The vector file's hash is taken after the generator
//! and again after every driver run; a driver that changes it is refused,
//! and the record counts these checks. The generator, the Python driver and
//! the test candidate script must be files inside the repo root. Test-only plants
//! (`Plant`) change the Rust answer after it is read, and each must fail
//! naming its case.
use crate::{capture_actions, depvec};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// One registered vector-driver step. `migration verify --step <id>` runs it.
pub struct DriverStep {
    pub id: &'static str,
    /// The tracked generator, relative to the repo root.
    pub generator: &'static str,
    /// The Python driver, relative to the repo root.
    pub driver: &'static str,
    /// The Rust driver binary, relative to the repo root.
    pub candidate: &'static str,
    /// The sets the candidate covers so far; the generator's other sets are
    /// left out of the record.
    pub sets: &'static [&'static str],
}

/// rust-04: the `policy-vectors` candidate over every oracle set: the core
/// sets (`rust-04d`) and the canary outputs, public scrub, approvals and
/// access sets (`rust-04e`). The list matches `rust04.SETS` plus
/// `assert-no-echo`, and `careerbrew_policy::vectors::SETS`.
pub const RUST_04: DriverStep = DriverStep {
    id: "rust-04",
    generator: "tests/oracles/rust04_vectors.py",
    driver: "tests/oracles/rust04.py",
    candidate: "target/release/policy-vectors",
    sets: &[
        "canary-core",
        "canary-match",
        "leak-spans",
        "assert-redact",
        "allowlists",
        "public-scrub",
        "private-path",
        "approvals",
        "receipt-validate",
        "access",
        "assert-no-echo",
    ],
};

pub const STEPS: &[DriverStep] = &[RUST_04];

/// The vector-driver step with this id, if it is one.
pub fn step(id: &str) -> Option<&'static DriverStep> {
    STEPS.iter().find(|step| step.id == id)
}

/// How the Rust side runs: a built binary, or (test-only) a Python script
/// run like the oracle, so the harness controls need no built candidate.
pub enum Candidate<'a> {
    Binary(&'a Path),
    PythonScript(&'a Path),
}

/// A test-only planted defect in the Rust answer of one set (the first,
/// unless the caller names one).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Plant {
    None,
    /// Drop the first vector's answer.
    RemoveCase,
    /// Change one byte of the first vector's output.
    ChangeByte,
    /// Report another Unicode table version.
    UnicodeMismatch,
}

impl Plant {
    /// The name the record stores.
    pub fn name(self) -> &'static str {
        match self {
            Plant::None => "none",
            Plant::RemoveCase => "remove-case",
            Plant::ChangeByte => "change-byte",
            Plant::UnicodeMismatch => "unicode-mismatch",
        }
    }

    pub fn parse(name: Option<&str>) -> Result<Plant, String> {
        match name {
            None => Ok(Plant::None),
            Some("remove-case") => Ok(Plant::RemoveCase),
            Some("change-byte") => Ok(Plant::ChangeByte),
            Some("unicode-mismatch") => Ok(Plant::UnicodeMismatch),
            Some(_) => {
                Err("--test-plant must be remove-case, change-byte or unicode-mismatch".into())
            }
        }
    }
}

/// The scratch vector file's sets, in name order. Each set is checked like
/// a `depvec` fixture: vectors, ids non-empty and unique, an op each.
pub(crate) fn parse_sets(text: &str) -> Result<Vec<(String, Vec<Value>)>, String> {
    let file: Value =
        serde_json::from_str(text).map_err(|_| "the generated vector file is not JSON")?;
    let sets = file["sets"]
        .as_object()
        .filter(|sets| !sets.is_empty())
        .ok_or("the generated vector file has no sets")?;
    let mut out = Vec::new();
    for (name, vectors) in sets {
        let fixture = json!({"set": name, "vectors": vectors});
        out.push((name.clone(), depvec::check_fixture(name, &fixture)?));
    }
    Ok(out)
}

fn bump(byte: &mut u8) {
    *byte = match *byte {
        b'9' => b'0',
        b'z' => b'a',
        b'Z' => b'A',
        other => other + 1,
    };
}

/// Changes one byte of the first leaf value (never an object key): the
/// first ASCII letter or digit of a string or number moves to the next one;
/// a string with none gains one byte; a bool flips; null becomes 0.
/// Returns false when there is no leaf (an empty object or array).
fn change_leaf(value: &mut Value) -> bool {
    match value {
        Value::Object(map) => map.values_mut().any(change_leaf),
        Value::Array(items) => items.iter_mut().any(change_leaf),
        Value::String(text) => {
            let mut bytes = std::mem::take(text).into_bytes();
            match bytes.iter_mut().find(|b| b.is_ascii_alphanumeric()) {
                Some(byte) => bump(byte),
                None => bytes.push(b'x'),
            }
            // Only an ASCII byte changed, so the text stays UTF-8.
            *text = String::from_utf8(bytes).unwrap_or_default();
            true
        }
        Value::Number(number) => {
            let mut bytes = number.to_string().into_bytes();
            if let Some(byte) = bytes.iter_mut().find(|b| b.is_ascii_digit()) {
                bump(byte);
            }
            match serde_json::from_slice(&bytes) {
                Ok(changed) => *value = changed,
                Err(_) => *value = json!(0),
            }
            true
        }
        Value::Bool(flag) => {
            *flag = !*flag;
            true
        }
        Value::Null => {
            *value = json!(0);
            true
        }
    }
}

/// The output with one byte of its first value changed (keys stay as they
/// are). Falls back to wrapping the output when it holds no value.
pub(crate) fn change_byte(output: &Value) -> Value {
    let mut changed = output.clone();
    if change_leaf(&mut changed) && changed != *output {
        changed
    } else {
        json!({ (depvec::PLANTED): output })
    }
}

/// `relative` resolved inside `root`: canonical, a file, and under the
/// canonical root. The message never echoes the path.
pub(crate) fn confine(root: &Path, relative: &Path) -> Result<std::path::PathBuf, String> {
    const REFUSAL: &str =
        "--generator, --driver and --test-candidate-script must name files inside the repo; refused";
    let root = root.canonicalize().map_err(|_| REFUSAL)?;
    let path = root.join(relative).canonicalize().map_err(|_| REFUSAL)?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(REFUSAL.into());
    }
    Ok(path)
}

/// Checks `--out` before anything runs: absolute and not yet there.
pub fn check_out(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("--out must be an absolute path".into());
    }
    if path.symlink_metadata().is_ok() {
        return Err("--out must name a new file".into());
    }
    Ok(())
}

/// Applies a result plant to the Rust answer of one set.
pub(crate) fn plant_results(plant: Plant, vectors: &[Value], answer: &mut BTreeMap<String, Value>) {
    let Some(first) = vectors.first().and_then(|v| v["id"].as_str()) else {
        return;
    };
    match plant {
        Plant::RemoveCase => {
            answer.remove(first);
        }
        Plant::ChangeByte => {
            if let Some(output) = answer.get_mut(first) {
                *output = change_byte(output);
            }
        }
        Plant::None | Plant::UnicodeMismatch => {}
    }
}

/// The failed `unicode-version` case when the two tables differ.
pub(crate) fn unicode_case(python: &str, rust: &str) -> Option<Value> {
    (python != rust).then(|| {
        json!({
            "case": "unicode-version",
            "passed": false,
            "python": python,
            "rust": rust,
        })
    })
}

fn run_side(
    root: &Path,
    home: &Path,
    program: &Path,
    args: &[&std::ffi::OsStr],
) -> Result<(Option<i32>, String), String> {
    let environment = capture_actions::owned_environment(root, home)?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(root)
        .env_clear()
        .envs(&environment);
    let (_, exit, stdout, _) = capture_actions::execute(command)?;
    Ok((exit, stdout))
}

struct Sides<'a> {
    root: &'a Path,
    scratch: &'a Path,
    python: &'a Path,
    driver: &'a Path,
    candidate: Candidate<'a>,
    runs: usize,
    vector_file: &'a Path,
    vector_sha256: String,
    hash_checks: usize,
}

impl Sides<'_> {
    fn home(&mut self, label: &str) -> std::path::PathBuf {
        self.runs += 1;
        self.scratch.join(format!("{label}-{}", self.runs))
    }

    /// The vector file must still hash as the generator wrote it.
    fn recheck(&mut self) -> Result<(), String> {
        let now =
            std::fs::read(self.vector_file).map_err(|_| "the vector file vanished; refused")?;
        if capture_actions::digest(&now) != self.vector_sha256 {
            return Err("a driver changed the vector file; refused".into());
        }
        self.hash_checks += 1;
        Ok(())
    }

    fn oracle(&mut self, args: &[&std::ffi::OsStr]) -> Result<(Option<i32>, String), String> {
        let home = self.home("python");
        let mut full = vec![std::ffi::OsStr::new("-I"), self.driver.as_os_str()];
        full.extend_from_slice(args);
        let answer = run_side(self.root, &home, self.python, &full)?;
        self.recheck()?;
        Ok(answer)
    }

    fn candidate(&mut self, args: &[&std::ffi::OsStr]) -> Result<(Option<i32>, String), String> {
        let home = self.home("rust");
        let answer = match self.candidate {
            Candidate::Binary(program) => run_side(self.root, &home, program, args)?,
            Candidate::PythonScript(script) => {
                let mut full = vec![std::ffi::OsStr::new("-I"), script.as_os_str()];
                full.extend_from_slice(args);
                run_side(self.root, &home, self.python, &full)?
            }
        };
        self.recheck()?;
        Ok(answer)
    }
}

fn version((exit, stdout): (Option<i32>, String), side: &str) -> Result<String, String> {
    let version = stdout.trim();
    if exit != Some(0) || version.is_empty() || version.contains('\n') {
        return Err(format!(
            "the {side} driver gave no Unicode version; refused"
        ));
    }
    Ok(version.to_owned())
}

/// Which sets run and where a plant goes. `only` keeps just those sets of the
/// generated file (all must be there); `plant_set` names the set a plant
/// changes (default: the first set that runs).
#[derive(Default, Clone, Copy)]
pub struct Scope<'a> {
    pub only: Option<&'a [&'a str]>,
    pub plant_set: Option<&'a str>,
}

/// Runs one vector-driver check and returns its record. `generator` and
/// `driver` (and a test candidate script) are paths inside `root`;
/// `python` must be absolute.
pub fn record(
    root: &Path,
    python: &Path,
    generator: &Path,
    driver: &Path,
    candidate: Candidate,
    plant: Plant,
    scope: Scope,
) -> Result<Value, String> {
    if !python.is_absolute() {
        return Err("--python must be an absolute path".into());
    }
    if let Candidate::Binary(path) = candidate {
        if !path.is_absolute() || !path.is_file() {
            return Err("the Rust driver is missing; build it first".into());
        }
    }
    let generator = confine(root, generator)?;
    let driver = confine(root, driver)?;
    let script;
    let (candidate, candidate_kind) = match candidate {
        Candidate::Binary(path) => (Candidate::Binary(path), "rust-binary"),
        Candidate::PythonScript(path) => {
            script = confine(root, path)?;
            (Candidate::PythonScript(&script), "python-test-script")
        }
    };
    let scratch = tempfile::tempdir().map_err(|_| "cannot make a scratch folder")?;
    let vector_file = scratch.path().join("vectors.json");
    let (exit, _) = run_side(
        root,
        &scratch.path().join("generator"),
        python,
        &[
            std::ffi::OsStr::new("-I"),
            generator.as_os_str(),
            vector_file.as_os_str(),
        ],
    )?;
    if exit != Some(0) {
        return Err("the vector generator failed; refused".into());
    }
    let bytes = std::fs::read(&vector_file).map_err(|_| "cannot read the generated vector file")?;
    let vector_sha256 = capture_actions::digest(&bytes);
    let text = String::from_utf8(bytes).map_err(|_| "the generated vector file is not UTF-8")?;
    let mut sets = parse_sets(&text)?;
    if let Some(only) = scope.only {
        if only
            .iter()
            .any(|name| !sets.iter().any(|(set, _)| set == name))
        {
            return Err("the generated vector file lacks a registered set; refused".into());
        }
        sets.retain(|(set, _)| only.contains(&set.as_str()));
    }
    let plant_set = match scope.plant_set {
        Some(name) if !sets.iter().any(|(set, _)| set == name) => {
            return Err("--test-wrong-vector names no vector set of this step".into());
        }
        Some(name) => name.to_owned(),
        None => sets.first().map(|(set, _)| set.clone()).unwrap_or_default(),
    };

    let mut sides = Sides {
        root,
        scratch: scratch.path(),
        python,
        driver: &driver,
        candidate,
        runs: 0,
        vector_file: &vector_file,
        vector_sha256: vector_sha256.clone(),
        hash_checks: 0,
    };
    let probe = [std::ffi::OsStr::new("unicode-version")];
    let python_unicode = version(sides.oracle(&probe)?, "Python")?;
    let mut rust_unicode = version(sides.candidate(&probe)?, "Rust")?;
    if plant == Plant::UnicodeMismatch {
        rust_unicode.push_str("-planted");
    }

    let mut cases = Vec::new();
    if let Some(case) = unicode_case(&python_unicode, &rust_unicode) {
        // Refused before any vector case runs.
        cases.push(case);
    } else {
        for (set, vectors) in &sets {
            let args = [std::ffi::OsStr::new(set), vector_file.as_os_str()];
            let (exit, stdout) = sides.oracle(&args)?;
            let oracle = depvec::results(set, &stdout)
                .filter(|_| exit == Some(0))
                .ok_or_else(|| {
                    format!("the Python driver gave no valid answer for {set}; refused")
                })?;
            let (exit, stdout) = sides.candidate(&args)?;
            let mut candidate = depvec::results(set, &stdout).filter(|_| exit == Some(0));
            if *set == plant_set {
                if let Some(answer) = candidate.as_mut() {
                    plant_results(plant, vectors, answer);
                }
            }
            cases.extend(depvec::compare(set, vectors, &oracle, candidate.as_ref())?);
        }
    }
    let hash_checks = sides.hash_checks;
    let failed: Vec<Value> = cases
        .iter()
        .filter(|case| case["passed"] != true)
        .map(|case| case["case"].clone())
        .collect();
    Ok(json!({
        "kind": "vector-driver",
        "vector_file_sha256": vector_sha256,
        // Every driver run was followed by a hash check of the one vector
        // file against the generator's hash; this counts them.
        "vector_file_hash_checks": hash_checks,
        "candidate": {"kind": candidate_kind, "plant": plant.name()},
        "unicode": {"python": python_unicode, "rust": rust_unicode},
        "sets": sets.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>(),
        "case_count": cases.len(),
        "passed_count": cases.len() - failed.len(),
        "failed_cases": failed,
        "passed": failed.is_empty(),
        "cases": cases,
    }))
}

/// Writes the record to a new file; never overwrites one.
pub fn write_new(path: &Path, record: &Value) -> Result<(), String> {
    use std::io::Write;
    if !path.is_absolute() {
        return Err("--out must be an absolute path".into());
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "--out must name a new file")?;
    let text = serde_json::to_string_pretty(record).map_err(|_| "cannot write the record")?;
    file.write_all(format!("{text}\n").as_bytes())
        .map_err(|_| "cannot write the record".into())
}

pub mod controls {
    use super::*;

    pub fn run() -> usize {
        let cases: [fn(); 4] = [
            a_removed_case_fails_by_name,
            a_changed_byte_fails_by_name,
            a_unicode_mismatch_is_one_named_case,
            a_generated_file_is_checked_like_a_fixture,
        ];
        for case in cases {
            case();
        }
        cases.len()
    }

    fn vectors() -> Vec<Value> {
        vec![
            json!({"id": "a", "op": "fold", "input": "x"}),
            json!({"id": "b", "op": "fold", "input": "y"}),
        ]
    }

    fn answers() -> BTreeMap<String, Value> {
        BTreeMap::from([
            ("a".to_owned(), json!({"ok": "x"})),
            ("b".to_owned(), json!({"ok": "y"})),
        ])
    }

    fn failed(plant: Plant) -> Vec<String> {
        let mut candidate = answers();
        plant_results(plant, &vectors(), &mut candidate);
        depvec::compare("s", &vectors(), &answers(), Some(&candidate))
            .unwrap()
            .iter()
            .filter(|case| case["passed"] != true)
            .filter_map(|case| case["case"].as_str().map(str::to_owned))
            .collect()
    }

    fn a_removed_case_fails_by_name() {
        assert!(failed(Plant::None).is_empty(), "equal answers must pass");
        assert_eq!(failed(Plant::RemoveCase), ["s:a"]);
    }

    fn a_changed_byte_fails_by_name() {
        assert_eq!(failed(Plant::ChangeByte), ["s:a"]);
        let changed = change_byte(&json!({"ok": "x"}));
        assert_eq!(
            changed,
            json!({"ok": "y"}),
            "the value changes, not the key"
        );
        assert_eq!(change_byte(&json!(9)), json!(0));
        assert_eq!(change_byte(&json!({"ok": ""})), json!({"ok": "x"}));
        assert_eq!(change_byte(&json!({"ok": ["a b"]})), json!({"ok": ["b b"]}));
    }

    fn a_unicode_mismatch_is_one_named_case() {
        assert!(unicode_case("15.1.0", "15.1.0").is_none());
        let case = unicode_case("15.1.0", "16.0.0").unwrap();
        assert_eq!(case["case"], "unicode-version");
        assert_eq!(case["passed"], false);
    }

    fn a_generated_file_is_checked_like_a_fixture() {
        let good = json!({"sets": {"s": vectors()}}).to_string();
        assert_eq!(parse_sets(&good).unwrap().len(), 1);
        let repeated = json!({"sets": {"s": [vectors()[0], vectors()[0]]}}).to_string();
        assert!(
            parse_sets(&repeated).is_err(),
            "a repeated id must be refused"
        );
        assert!(
            parse_sets("{\"sets\": {}}").is_err(),
            "no sets must be refused"
        );
        assert!(Plant::parse(Some("other")).is_err());
        assert!(STEPS.iter().all(|s| crate::depvec::step(s.id).is_none()));
    }
}
