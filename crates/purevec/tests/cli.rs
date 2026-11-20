//! The `careerbrew-purevec` binary: `--all` lists the registered sets, an
//! unknown set is refused, and a missing vectors file is a usage error. Its
//! presence also makes `cargo test --all` build the binary that
//! `xtask/tests/purevec.rs` runs.
use std::process::Command;

const PUREVEC: &str = env!("CARGO_BIN_EXE_careerbrew-purevec");

#[test]
fn all_lists_the_registered_sets() {
    let output = Command::new(PUREVEC).arg("--all").output().unwrap();
    assert!(output.status.success());
    let answer: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(answer["sets"].is_array());
}

#[test]
fn no_vector_code_reads_the_wall_clock() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(src).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        for read in ["Utc::now", "Local::now", "SystemTime::now"] {
            assert!(!text.contains(read), "vector code must not call {read}");
        }
    }
}

#[test]
fn an_unknown_set_is_refused() {
    let scratch = std::env::temp_dir().join(format!("careerbrew-purevec-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let file = scratch.join("vectors.json");
    std::fs::write(&file, r#"{"set": "no-such-set", "vectors": []}"#).unwrap();
    let output = Command::new(PUREVEC)
        .arg("no-such-set")
        .arg(&file)
        .output()
        .unwrap();
    std::fs::remove_dir_all(&scratch).unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn a_missing_vectors_file_is_a_usage_error() {
    let output = Command::new(PUREVEC)
        .args(["no-such-set", "/nonexistent/vectors.json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
