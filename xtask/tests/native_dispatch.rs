//! rust-27b: the wrapper scripts run the native tool when it is current.
//!
//! `scripts/input-key.sh`, `scripts/check-docs.sh` and
//! `scripts/browser-test-receipts.sh` run `careerbrew-xtask` when
//! `target/debug/careerbrew-xtask` is newer than every file it is built from,
//! and their old code otherwise. These tests check two things:
//! 1. parity: the same script prints the same stdout, stderr and exit code
//!    with the native tool (`CAREERBREW_XTASK_BIN`) and with the old code
//!    (`CAREERBREW_NATIVE_TOOLS=0`);
//! 2. selection: a planted binary runs only when it is current, never when a
//!    source file is newer, when `CAREERBREW_NATIVE_TOOLS=0`, or for the
//!    leading `--` that only the old input-key code hashes.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

/// A synthetic checkout with copies of both wrapper scripts, docs, stories
/// and a committed Git history.
fn checkout() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    for name in ["input-key.sh", "check-docs.sh"] {
        std::fs::create_dir_all(r.join("scripts")).unwrap();
        std::fs::copy(
            root().join("scripts").join(name),
            r.join("scripts").join(name),
        )
        .unwrap();
    }
    write(
        r,
        "docs/README.md",
        "| [Stories](user-stories.md) |\n| [Web](web.md) |\n",
    );
    write(r, "docs/web.md", "# Web\n");
    write(
        r,
        "docs/user-stories.md",
        "US-01 test: `tests/e2e/us-01-start.sh`.\n",
    );
    write(r, "tests/e2e/us-01-start.sh", "# US-01\n");
    write(r, "TASKS.md", "# Tasks\n");
    git(r, &["init", "-q"]);
    git(r, &["config", "user.email", "ci@example.com"]);
    git(r, &["config", "user.name", "ci"]);
    git(r, &["config", "commit.gpgsign", "false"]);
    git(r, &["config", "core.excludesFile", "/dev/null"]);
    git(r, &["add", "-A"]);
    git(r, &["commit", "-qm", "init"]);
    write(r, "untracked.txt", "new\n");
    dir
}

fn clean(cmd: &mut Command) -> &mut Command {
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LC_ALL", "C")
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .env_remove("CAREERBREW_XTASK_BIN")
        .env_remove("CAREERBREW_NATIVE_TOOLS")
        .env_remove("CAREERBREW_BROWSER_RECEIPT")
}

fn script(path: &Path, args: &[&str], native: Option<&Path>) -> Output {
    let mut cmd = Command::new("bash");
    cmd.arg(path).args(args);
    clean(&mut cmd);
    match native {
        Some(bin) => cmd.env("CAREERBREW_XTASK_BIN", bin),
        None => cmd.env("CAREERBREW_NATIVE_TOOLS", "0"),
    };
    cmd.output().unwrap()
}

/// The script gives the same result through either path; returns it.
fn same(path: &Path, args: &[&str]) -> Output {
    let native = script(path, args, Some(Path::new(XTASK)));
    let old = script(path, args, None);
    assert_eq!(
        String::from_utf8_lossy(&native.stdout),
        String::from_utf8_lossy(&old.stdout),
        "{} {args:?}: stdout",
        path.display()
    );
    assert_eq!(
        String::from_utf8_lossy(&native.stderr),
        String::from_utf8_lossy(&old.stderr),
        "{} {args:?}: stderr",
        path.display()
    );
    assert_eq!(native.status.code(), old.status.code(), "{args:?}: exit");
    native
}

#[test]
fn input_key_gives_the_same_key_through_both_paths() {
    let dir = checkout();
    let s = dir.path().join("scripts/input-key.sh");
    let all = same(&s, &[]);
    assert_eq!(all.status.code(), Some(0));
    assert_eq!(all.stdout.len(), 17, "16 hex characters and a newline");
    let some = same(&s, &[":(exclude)TASKS.md"]);
    assert_ne!(all.stdout, some.stdout, "the pathspec is part of the key");
    same(&s, &["docs", ":(exclude)docs/web.md"]);
    // A pathspec that looks like a native flag follows the native `--`.
    same(&s, &["--repo"]);
}

#[test]
fn input_key_fails_the_same_way_outside_a_checkout() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("scripts")).unwrap();
    let s = dir.path().join("scripts/input-key.sh");
    std::fs::copy(root().join("scripts/input-key.sh"), &s).unwrap();
    let out = same(&s, &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}

#[test]
fn check_docs_gives_the_same_lines_through_both_paths() {
    let dir = checkout();
    let s = dir.path().join("scripts/check-docs.sh");
    let ok = same(&s, &[]);
    assert_eq!(ok.status.code(), Some(0));
    write(dir.path(), "docs/orphan.md", "# no row\n");
    write(dir.path(), "tests/e2e/us-02-unlisted.sh", "# no id\n");
    std::fs::remove_file(dir.path().join("tests/e2e/us-01-start.sh")).unwrap();
    let bad = same(&s, &[]);
    assert_eq!(bad.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&bad.stdout).lines().count(),
        4,
        "a missing row, a missing story, an unlisted one and a wrong id"
    );
}

fn report(executed: bool, id: &str) -> Value {
    let test = if executed {
        json!({"projectId": "chromium", "expectedStatus": "passed", "status": "expected",
               "results": [{"status": "passed", "retry": 0}]})
    } else {
        json!({"projectId": "chromium", "expectedStatus": "passed"})
    };
    json!({"errors": [], "suites": [{"specs": [{"id": id, "tests": [test]}], "suites": []}]})
}

fn receipts(dir: &Path, discovery: &Value, execution: &Value) -> Output {
    std::fs::write(dir.join("d.json"), serde_json::to_vec(discovery).unwrap()).unwrap();
    std::fs::write(dir.join("e.json"), serde_json::to_vec(execution).unwrap()).unwrap();
    // Relative paths: the wrapper must keep the caller's folder.
    let s = root().join("scripts/browser-test-receipts.sh");
    let run = |native: Option<&Path>| {
        let mut cmd = Command::new("bash");
        cmd.arg(&s).args(["d.json", "e.json"]).current_dir(dir);
        clean(&mut cmd);
        match native {
            Some(bin) => cmd.env("CAREERBREW_XTASK_BIN", bin),
            None => cmd.env("CAREERBREW_NATIVE_TOOLS", "0"),
        };
        cmd.output().unwrap()
    };
    let native = run(Some(Path::new(XTASK)));
    let old = run(None);
    assert_eq!(native.stdout, old.stdout);
    assert_eq!(native.stderr, old.stderr);
    assert_eq!(native.status.code(), old.status.code());
    native
}

#[test]
fn browser_test_receipts_give_the_same_result_through_both_paths() {
    let dir = tempfile::tempdir().unwrap();
    let pass = receipts(dir.path(), &report(false, "a"), &report(true, "a"));
    assert_eq!(pass.status.code(), Some(0));
    let missing = receipts(dir.path(), &report(false, "a"), &report(true, "b"));
    assert_eq!(missing.status.code(), Some(1));
    let unrun = receipts(dir.path(), &report(false, "a"), &report(false, "a"));
    assert_eq!(unrun.status.code(), Some(1));
}

/// A checkout with a planted `target/debug/careerbrew-xtask` that prints
/// `planted native` and the build inputs the wrappers compare it with.
fn planted() -> (tempfile::TempDir, PathBuf) {
    let dir = checkout();
    let r = dir.path();
    write(r, "xtask/Cargo.toml", "[package]\n");
    write(r, "xtask/src/main.rs", "fn main() {}\n");
    write(r, "Cargo.lock", "# lock\n");
    let bin = r.join("target/debug/careerbrew-xtask");
    write(
        r,
        "target/debug/careerbrew-xtask",
        "#!/bin/sh\necho planted native\n",
    );
    let mut perms = std::fs::metadata(&bin).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(&bin, perms).unwrap();
    let old = SystemTime::now() - Duration::from_secs(3600);
    for rel in ["xtask/Cargo.toml", "xtask/src/main.rs", "Cargo.lock"] {
        let f = std::fs::File::options()
            .write(true)
            .open(r.join(rel))
            .unwrap();
        f.set_modified(old).unwrap();
    }
    (dir, bin)
}

fn plain(path: &Path, args: &[&str], env: &[(&str, &str)]) -> String {
    let mut cmd = Command::new("bash");
    cmd.arg(path).args(args);
    clean(&mut cmd);
    for (k, v) in env {
        cmd.env(k, v);
    }
    String::from_utf8(cmd.output().unwrap().stdout).unwrap()
}

#[test]
fn a_current_binary_runs_and_a_stale_or_disabled_one_does_not() {
    let (dir, _bin) = planted();
    let key = dir.path().join("scripts/input-key.sh");
    let docs = dir.path().join("scripts/check-docs.sh");
    assert_eq!(plain(&key, &[], &[]), "planted native\n");
    assert_eq!(plain(&key, &["docs"], &[]), "planted native\n");
    assert_eq!(plain(&docs, &[], &[]), "planted native\n");
    // Only the old code hashes a leading `--` as a pathspec line.
    assert_ne!(plain(&key, &["--", "docs"], &[]), "planted native\n");
    // The opt-out.
    let off = [("CAREERBREW_NATIVE_TOOLS", "0")];
    assert_eq!(plain(&docs, &[], &off), "check-docs: docs match the code\n");
    assert_ne!(plain(&key, &[], &off), "planted native\n");
    // A source edited after the build makes the binary stale.
    let f = std::fs::File::options()
        .write(true)
        .open(dir.path().join("xtask/src/main.rs"))
        .unwrap();
    f.set_modified(SystemTime::now() + Duration::from_secs(60))
        .unwrap();
    assert_eq!(plain(&docs, &[], &[]), "check-docs: docs match the code\n");
    assert_eq!(plain(&key, &[], &[]).len(), 17, "the old code's key");
}

#[test]
fn a_binary_without_its_build_inputs_never_runs() {
    let (dir, _bin) = planted();
    std::fs::remove_file(dir.path().join("xtask/Cargo.toml")).unwrap();
    let docs = dir.path().join("scripts/check-docs.sh");
    assert_eq!(plain(&docs, &[], &[]), "check-docs: docs match the code\n");
}
