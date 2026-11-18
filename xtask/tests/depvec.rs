//! Controls for `migration verify --step rust-01` (dependency vectors): the
//! built `careerbrew-depvec` candidate agrees with the Python driver on
//! every vector; a planted wrong vector fails exactly its case; a
//! `--test-wrong-vector` that names no set is refused; a candidate that
//! cannot answer fails every case. Each test owns its temp folder.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// The candidate cargo built next to this test's xtask binary (the
/// `careerbrew-depvec` package has an integration test, so `cargo test
/// --all` builds its binary).
fn depvec() -> PathBuf {
    let path = Path::new(XTASK).with_file_name("careerbrew-depvec");
    assert!(
        path.is_file(),
        "build it first: cargo build -p careerbrew-depvec"
    );
    path
}

fn verify(candidate: &Path, out: &Path, extra: &[&str]) -> std::process::Output {
    Command::new(XTASK)
        .args(["migration", "verify", "--step", "rust-01", "--python"])
        .arg(root().join(".venv/bin/python"))
        .arg("--out")
        .arg(out)
        .arg("--test-candidate")
        .arg(candidate)
        .args(extra)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap()
}

fn read(out: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(out).unwrap()).unwrap()
}

/// Every vector set fixture, so a part that adds a set needs no test edit here.
fn fixtures() -> Vec<(String, usize)> {
    let dir = root().join("xtask/tests/fixtures/depvec");
    let mut sets: Vec<(String, usize)> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            let fixture: serde_json::Value = serde_json::from_str(&text).unwrap();
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            (name, fixture["vectors"].as_array().unwrap().len())
        })
        .collect();
    sets.sort();
    sets
}

fn vector_count() -> usize {
    fixtures().iter().map(|(_, n)| n).sum()
}

#[test]
fn the_rust_candidate_agrees_with_python_on_every_vector() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify(&depvec(), &out, &[]);
    let record = read(&out);
    assert!(
        run.status.success(),
        "failed cases: {}",
        record["failed_cases"]
    );
    assert_eq!(record["step"], "rust-01");
    assert_eq!(record["passed"], true);
    assert_eq!(record["case_count"], vector_count());
    let mut sets: Vec<String> = record["vector_sets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    sets.sort();
    let names: Vec<String> = fixtures().into_iter().map(|(n, _)| n).collect();
    assert_eq!(sets, names);
    assert_eq!(record["violations"]["contract"], 0);
}

#[test]
fn a_planted_wrong_vector_fails_its_case() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify(&depvec(), &out, &["--test-wrong-vector", "json-yaml"]);
    assert_eq!(run.status.code(), Some(1));
    let record = read(&out);
    assert_eq!(record["passed"], false);
    assert_eq!(record["planted_wrong_vector"], "json-yaml");
    assert_eq!(
        record["failed_cases"],
        serde_json::json!(["json-yaml:json-read-object-order"])
    );
    assert_eq!(record["violations"]["contract"], 1);
}

#[test]
fn a_wrong_vector_in_no_set_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify(&depvec(), &out, &["--test-wrong-vector", "no-such-set"]);
    assert_eq!(run.status.code(), Some(1));
    assert!(!out.exists());
    assert!(String::from_utf8_lossy(&run.stderr).contains("names no vector set"));
}

#[test]
fn a_candidate_that_cannot_answer_fails_every_case() {
    let scratch = tempfile::tempdir().unwrap();
    let candidate = scratch.path().join("candidate");
    std::fs::write(&candidate, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o700)).unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify(&candidate, &out, &[]);
    assert_eq!(run.status.code(), Some(1));
    let record = read(&out);
    assert_eq!(record["passed_count"], 0);
    assert_eq!(record["case_count"], vector_count());
}

#[test]
fn a_planted_wrong_code_parse_vector_fails_its_case() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let run = verify(&depvec(), &out, &["--test-wrong-vector", "code-parse"]);
    assert_eq!(run.status.code(), Some(1));
    let record = read(&out);
    assert_eq!(record["passed"], false);
    assert_eq!(record["planted_wrong_vector"], "code-parse");
    assert_eq!(
        record["failed_cases"],
        serde_json::json!(["code-parse:python-function-with-types"])
    );
    assert_eq!(record["violations"]["contract"], 1);
}
