//! Parity controls for `cargo xtask check-docs` (rust-27b slice 3): each
//! test builds a synthetic temp repo, copies `scripts/check-docs.sh` into
//! it, and checks the native command prints the same stdout and exit code.
use std::path::Path;
use std::process::{Command, Output};

const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A repo whose docs match: two docs, one story, one helper.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("scripts/check-docs.sh");
    std::fs::create_dir(root.join("scripts")).unwrap();
    std::fs::copy(script, root.join("scripts/check-docs.sh")).unwrap();
    write(
        root,
        "docs/README.md",
        "| [Stories](user-stories.md) |\n| [Web](web.md) |\n",
    );
    write(root, "docs/web.md", "# Web\n");
    write(
        root,
        "docs/user-stories.md",
        "US-01 test: `tests/e2e/us-01-start.sh`.\n",
    );
    write(root, "tests/e2e/us-01-start.sh", "# US-01\n");
    write(root, "tests/e2e/lib.sh", "# helper\n");
    dir
}

fn run(mut cmd: Command) -> Output {
    cmd.env("LC_ALL", "C").output().unwrap()
}

fn same(root: &Path) -> Output {
    let mut s = Command::new("bash");
    s.arg(root.join("scripts/check-docs.sh"));
    let script = run(s);
    let mut n = Command::new(XTASK);
    n.arg("check-docs").arg("--repo").arg(root);
    let native = run(n);
    assert_eq!(
        String::from_utf8_lossy(&native.stdout),
        String::from_utf8_lossy(&script.stdout)
    );
    assert_eq!(native.status.code(), script.status.code());
    native
}

#[test]
fn matching_docs_pass_on_both() {
    let dir = repo();
    let out = same(dir.path());
    assert!(out.status.success());
    assert_eq!(out.stdout, b"check-docs: docs match the code\n");
}

#[test]
fn a_doc_with_no_readme_row_fails_on_both() {
    let dir = repo();
    write(dir.path(), "docs/zeta.md", "# Zeta\n");
    write(dir.path(), "docs/alpha.md", "# Alpha\n");
    let out = same(dir.path());
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("check-docs: docs/alpha.md has no row"));
}

#[test]
fn a_named_story_that_is_missing_fails_on_both() {
    let dir = repo();
    write(
        dir.path(),
        "docs/user-stories.md",
        "`tests/e2e/us-01-start.sh` and tests/e2e/us-09-gone.sh. Twice: tests/e2e/us-09-gone.sh\n",
    );
    assert_eq!(same(dir.path()).status.code(), Some(1));
}

#[test]
fn an_unlisted_story_and_a_wrong_id_fail_on_both() {
    let dir = repo();
    write(
        dir.path(),
        "tests/e2e/us-02-extra.sh",
        "# names US-03 only\n",
    );
    write(dir.path(), "tests/e2e/us-notes.txt", "no id\n");
    std::fs::create_dir_all(dir.path().join("tests/e2e/us-dir")).unwrap();
    let out = same(dir.path());
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("us-02-extra.sh does not name its story US-02"));
}

#[test]
fn a_dot_in_a_name_matches_any_character_on_both() {
    let dir = repo();
    // `(webXmd)` satisfies the BRE `(web.md)` in the script too.
    write(dir.path(), "docs/README.md", "(user-stories.md) (webXmd)\n");
    assert!(same(dir.path()).status.success());
}

#[test]
fn missing_index_files_fail_on_both() {
    let dir = repo();
    std::fs::remove_file(dir.path().join("docs/README.md")).unwrap();
    std::fs::remove_file(dir.path().join("docs/user-stories.md")).unwrap();
    assert_eq!(same(dir.path()).status.code(), Some(1));
}

#[test]
fn a_wrong_argument_is_a_usage_error() {
    let out = run({
        let mut c = Command::new(XTASK);
        c.args(["check-docs", "--bogus"]);
        c
    });
    assert_eq!(out.status.code(), Some(2));
}
