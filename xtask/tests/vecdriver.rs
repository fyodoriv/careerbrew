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
    assert!(record.get("same_vector_file").is_none());
    // 2 Unicode probes plus 2 sets, each run on both sides.
    assert_eq!(record["vector_file_hash_checks"], 6);
    assert_eq!(record["candidate"]["kind"], "python-test-script");
    assert_eq!(record["candidate"]["plant"], "none");
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
    assert_eq!(record["candidate"]["plant"], "change-byte");
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

fn refused_without_echo(out: &Path, extra: &[&str], secret: &str) {
    let output = check(out, extra);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(!out.exists(), "a refusal writes no record");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("inside the repo"), "{stderr}");
    assert!(!stderr.contains(secret), "the path must not be echoed");
}

#[test]
fn a_candidate_script_outside_the_repo_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside-canary.py");
    std::fs::write(&outside, "print('x')\n").unwrap();
    let out = dir.path().join("record.json");
    refused_without_echo(
        &out,
        &["--test-candidate-script", outside.to_str().unwrap()],
        "outside-canary",
    );
}

#[test]
fn a_dot_dot_escape_from_the_repo_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("escape-canary.py");
    std::fs::write(&outside, "print('x')\n").unwrap();
    // Climb from the repo root to "/" with "..", then down to the file.
    let depth = root().components().count() - 1;
    let escape = format!(
        "{}{}",
        "../".repeat(depth),
        outside.strip_prefix("/").unwrap().display()
    );
    let out = dir.path().join("record.json");
    refused_without_echo(&out, &["--test-candidate-script", &escape], "escape-canary");
}

#[test]
fn an_existing_out_file_is_refused_before_the_generator_runs() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("record.json");
    std::fs::write(&out, "keep").unwrap();
    // A missing generator would refuse too; a candidate outside the repo
    // would too. --out is checked first, so its own message shows.
    let output = Command::new(XTASK)
        .args(["migration", "vector-check", "--python"])
        .arg(root().join(".venv/bin/python"))
        .args([
            "--generator",
            "absent-generator.py",
            "--driver",
            DRIVER,
            "--out",
        ])
        .arg(&out)
        .args(["--test-candidate-script", DRIVER])
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--out must name a new file"));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "keep");
    let relative = check(
        Path::new("record.json"),
        &["--test-candidate-script", DRIVER],
    );
    assert_eq!(relative.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&relative.stderr).contains("--out must be an absolute path"));
}
