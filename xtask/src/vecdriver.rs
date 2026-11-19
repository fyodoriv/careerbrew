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
//! `depvec::compare`. The vector file's hash is taken before and after the
//! drivers run; a driver that changes it is refused. Test-only plants
//! (`Plant`) change the Rust answer after it is read, and each must fail
//! naming its case.
use crate::{capture_actions, depvec};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

#[allow(dead_code)]
/// One registered vector-driver step. None yet: `rust-04c` registers
/// `rust-04` with its generator and oracle; `rust-04d` adds its candidate.
pub struct DriverStep {
    pub id: &'static str,
    /// The tracked generator, relative to the repo root.
    pub generator: &'static str,
    /// The Python driver, relative to the repo root.
    pub driver: &'static str,
    /// The Rust driver binary, relative to the repo root.
    pub candidate: &'static str,
}

pub const STEPS: &[DriverStep] = &[];

/// The vector-driver step with this id, if it is one.
#[allow(dead_code)]
pub fn step(id: &str) -> Option<&'static DriverStep> {
    STEPS.iter().find(|step| step.id == id)
}

/// How the Rust side runs: a built binary, or (test-only) a Python script
/// run like the oracle, so the harness controls need no built candidate.
pub enum Candidate<'a> {
    Binary(&'a Path),
    PythonScript(&'a Path),
}

/// A test-only planted defect in the Rust answer of the first set.
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

/// The output with one byte changed: the first ASCII letter or digit of its
/// JSON text moves to the next one. Falls back to wrapping the output when
/// the changed text does not parse.
pub(crate) fn change_byte(output: &Value) -> Value {
    let mut bytes = output.to_string().into_bytes();
    if let Some(byte) = bytes.iter_mut().find(|b| b.is_ascii_alphanumeric()) {
        *byte = match *byte {
            b'9' => b'0',
            b'z' => b'a',
            b'Z' => b'A',
            other => other + 1,
        };
    }
    serde_json::from_slice(&bytes)
        .ok()
        .filter(|changed| changed != output)
        .unwrap_or_else(|| json!({ (depvec::PLANTED): output }))
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
}

impl Sides<'_> {
    fn home(&mut self, label: &str) -> std::path::PathBuf {
        self.runs += 1;
        self.scratch.join(format!("{label}-{}", self.runs))
    }

    fn oracle(&mut self, args: &[&std::ffi::OsStr]) -> Result<(Option<i32>, String), String> {
        let home = self.home("python");
        let mut full = vec![std::ffi::OsStr::new("-I"), self.driver.as_os_str()];
        full.extend_from_slice(args);
        run_side(self.root, &home, self.python, &full)
    }

    fn candidate(&mut self, args: &[&std::ffi::OsStr]) -> Result<(Option<i32>, String), String> {
        let home = self.home("rust");
        match self.candidate {
            Candidate::Binary(program) => run_side(self.root, &home, program, args),
            Candidate::PythonScript(script) => {
                let mut full = vec![std::ffi::OsStr::new("-I"), script.as_os_str()];
                full.extend_from_slice(args);
                run_side(self.root, &home, self.python, &full)
            }
        }
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

/// Runs one vector-driver check and returns its record. `generator` and
/// `driver` are paths relative to `root`; `python` must be absolute.
pub fn record(
    root: &Path,
    python: &Path,
    generator: &Path,
    driver: &Path,
    candidate: Candidate,
    plant: Plant,
) -> Result<Value, String> {
    if !python.is_absolute() {
        return Err("--python must be an absolute path".into());
    }
    if let Candidate::Binary(path) = candidate {
        if !path.is_absolute() || !path.is_file() {
            return Err("the Rust driver is missing; build it first".into());
        }
    }
    let generator = root.join(generator);
    let driver = root.join(driver);
    for path in [&generator, &driver] {
        if !path.is_file() {
            return Err("the generator or the Python driver is missing".into());
        }
    }
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
    let sets = parse_sets(&text)?;

    let mut sides = Sides {
        root,
        scratch: scratch.path(),
        python,
        driver: &driver,
        candidate,
        runs: 0,
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
        for (index, (set, vectors)) in sets.iter().enumerate() {
            let args = [std::ffi::OsStr::new(set), vector_file.as_os_str()];
            let (exit, stdout) = sides.oracle(&args)?;
            let oracle = depvec::results(set, &stdout)
                .filter(|_| exit == Some(0))
                .ok_or_else(|| {
                    format!("the Python driver gave no valid answer for {set}; refused")
                })?;
            let (exit, stdout) = sides.candidate(&args)?;
            let mut candidate = depvec::results(set, &stdout).filter(|_| exit == Some(0));
            if index == 0 {
                if let Some(answer) = candidate.as_mut() {
                    plant_results(plant, vectors, answer);
                }
            }
            cases.extend(depvec::compare(set, vectors, &oracle, candidate.as_ref())?);
        }
    }
    let after = std::fs::read(&vector_file).map_err(|_| "the vector file vanished; refused")?;
    if capture_actions::digest(&after) != vector_sha256 {
        return Err("a driver changed the vector file; refused".into());
    }
    let failed: Vec<Value> = cases
        .iter()
        .filter(|case| case["passed"] != true)
        .map(|case| case["case"].clone())
        .collect();
    Ok(json!({
        "kind": "vector-driver",
        "vector_file_sha256": vector_sha256,
        "same_vector_file": true,
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
        assert_ne!(changed, json!({"ok": "x"}));
        assert_eq!(change_byte(&json!(9)), json!(0));
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
