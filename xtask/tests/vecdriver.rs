//! Controls for `migration vector-check` (rust-04a, the vector-driver kind):
//! one generated vector file feeds both drivers; a clean run passes; each
//! planted defect (a removed case, a changed output byte, a Unicode version
//! mismatch) fails and names its case; a missing driver is refused. The
//! synthetic driver fixture plays both sides. Each test owns its temp folder.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");
const GENERATOR: &str = "xtask/tests/fixtures/vecdriver/generator.py";
const DRIVER: &str = "xtask/tests/fixtures/vecdriver/driver.py";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn check(out: &Path, extra: &[&str]) -> Output {
    Command::new(XTASK)
        .args(["migration", "vector-check", "--python"])
        .arg(root().join(".venv/bin/python"))
        .args(["--generator", GENERATOR, "--driver", DRIVER, "--out"])
        .arg(out)
        .args(extra)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap()
}

fn read(out: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(out).unwrap()).unwrap()
}

fn failed(record: &serde_json::Value) -> Vec<String> {
    record["failed_cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case.as_str().unwrap().to_owned())
        .collect()
}

fn planted(plant: &str) -> (Output, serde_json::Value) {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("record.json");
    let output = check(
        &out,
        &["--test-candidate-script", DRIVER, "--test-plant", plant],
    );
    (output, read(&out))
}

#[test]
fn one_generated_file_feeds_both_drivers_and_every_case_passes() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("record.json");
    let output = check(&out, &["--test-candidate-script", DRIVER]);
    assert!(output.status.success(), "{output:?}");
    let record = read(&out);
    assert_eq!(record["kind"], "vector-driver");
    assert_eq!(record["same_vector_file"], true);
    assert_eq!(record["case_count"], 5);
    assert_eq!(record["sets"], serde_json::json!(["fold", "split"]));
    assert_eq!(record["unicode"]["python"], record["unicode"]["rust"]);
    assert!(failed(&record).is_empty());
}

#[test]
fn a_removed_case_fails_naming_it() {
    let (output, record) = planted("remove-case");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(failed(&record), ["fold:ascii"]);
}

#[test]
fn a_changed_output_byte_fails_naming_it() {
    let (output, record) = planted("change-byte");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(failed(&record), ["fold:ascii"]);
}

#[test]
fn a_unicode_version_mismatch_fails_before_any_case_runs() {
    let (output, record) = planted("unicode-mismatch");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(failed(&record), ["unicode-version"]);
    assert_eq!(record["case_count"], 1, "no vector case may run");
}

#[test]
fn a_missing_rust_driver_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("record.json");
    let missing = dir.path().join("absent-driver");
    let output = check(&out, &["--candidate", missing.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!out.exists(), "a refusal writes no record");
}
