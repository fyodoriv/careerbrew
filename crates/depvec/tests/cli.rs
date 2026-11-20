//! The `careerbrew-depvec` binary: it answers a known set and refuses an
//! unknown one. Its presence also makes `cargo test --all` build the binary
//! that `xtask/tests/depvec.rs` runs.
use std::process::Command;

const DEPVEC: &str = env!("CARGO_BIN_EXE_careerbrew-depvec");

fn run(set: &str) -> std::process::Output {
    let scratch = tempfile_dir(set);
    let file = scratch.join("vectors.json");
    std::fs::write(
        &file,
        r#"{"set": "json-yaml", "vectors": [{"id": "a", "op": "json-read", "input": "[1, 2.5]"}, {"id": "b", "op": "json-read", "input": "[1,"}]}"#,
    )
    .unwrap();
    let output = Command::new(DEPVEC).arg(set).arg(&file).output().unwrap();
    std::fs::remove_dir_all(scratch).unwrap();
    output
}

fn tempfile_dir(set: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("careerbrew-depvec-{}-{set}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_known_set_answers_every_vector() {
    let output = run("json-yaml");
    assert!(output.status.success());
    let answer: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        answer["results"][0]["output"]["ok"],
        serde_json::json!({"t": "list", "v": [{"t": "int", "v": "1"}, {"t": "float", "v": "4004000000000000"}]})
    );
    assert_eq!(
        answer["results"][1]["output"],
        serde_json::json!({"error": true})
    );
}

#[test]
fn an_unknown_set_is_refused() {
    assert_eq!(run("no-such-set").status.code(), Some(2));
}
