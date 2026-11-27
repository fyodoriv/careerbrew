//! Parity controls for `cargo xtask input-key` (rust-27 slice 2): each test
//! builds a synthetic throwaway Git repo, copies `scripts/input-key.sh` into
//! it, and checks the native command prints the same stdout and exit code.
use std::path::Path;
use std::process::{Command, Output};

const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");

fn script_source() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("scripts/input-key.sh")
}

fn git(repo: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

/// A repo with the script, a tracked file, a task list and an ignore rule.
fn repo(commit: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "ci@example.com"]);
    git(root, &["config", "user.name", "ci"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["config", "core.excludesFile", "/dev/null"]);
    std::fs::create_dir(root.join("scripts")).unwrap();
    std::fs::copy(script_source(), root.join("scripts/input-key.sh")).unwrap();
    std::fs::write(root.join("a.txt"), "alpha\n").unwrap();
    std::fs::write(root.join("TASKS.md"), "# Tasks\n").unwrap();
    std::fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
    if commit {
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", "init"]);
    }
    dir
}

fn env_clean(cmd: &mut Command) {
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE");
}

fn script(root: &Path, specs: &[&str], extra: &[(&str, &Path)]) -> Output {
    let mut cmd = Command::new("bash");
    cmd.arg(root.join("scripts/input-key.sh"))
        .args(specs)
        .env("CAREERBREW_NATIVE_TOOLS", "0");
    env_clean(&mut cmd);
    for (k, v) in extra {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn native(root: &Path, specs: &[&str], extra: &[(&str, &Path)]) -> Output {
    let mut cmd = Command::new(XTASK);
    cmd.arg("input-key").arg("--repo").arg(root);
    if !specs.is_empty() {
        cmd.arg("--").args(specs);
    }
    env_clean(&mut cmd);
    for (k, v) in extra {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

/// Both engines agree on stdout and exit code; returns the key.
fn same(root: &Path, specs: &[&str]) -> String {
    same_env(root, specs, &[])
}

fn same_env(root: &Path, specs: &[&str], extra: &[(&str, &Path)]) -> String {
    let (s, n) = (script(root, specs, extra), native(root, specs, extra));
    assert_eq!(s.status.code(), n.status.code(), "exit codes differ");
    assert_eq!(s.stdout, n.stdout, "stdout differs");
    String::from_utf8(n.stdout).unwrap()
}

fn is_key(text: &str) {
    let key = text.strip_suffix('\n').unwrap();
    assert_eq!(key.len(), 16);
    assert!(key
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
}

#[test]
fn a_clean_committed_repo_gives_the_same_key() {
    let dir = repo(true);
    is_key(&same(dir.path(), &[]));
}

#[test]
fn staging_and_committing_keep_the_key_and_content_moves_it() {
    let dir = repo(true);
    let root = dir.path();
    let base = same(root, &[]);
    std::fs::write(root.join("a.txt"), "beta\n").unwrap();
    let edited = same(root, &[]);
    assert_ne!(base, edited);
    git(root, &["add", "a.txt"]);
    assert_eq!(same(root, &[]), edited);
    git(root, &["commit", "-qm", "edit"]);
    assert_eq!(same(root, &[]), edited);
}

#[test]
fn untracked_files_count_and_ignored_ones_do_not() {
    let dir = repo(true);
    let root = dir.path();
    let base = same(root, &[]);
    std::fs::create_dir(root.join("ignored")).unwrap();
    std::fs::write(root.join("ignored/x"), "x\n").unwrap();
    assert_eq!(same(root, &[]), base);
    std::fs::write(root.join("new.txt"), "n\n").unwrap();
    assert_ne!(same(root, &[]), base);
}

#[test]
fn a_deleted_tracked_file_moves_the_key() {
    let dir = repo(true);
    let root = dir.path();
    let base = same(root, &[]);
    std::fs::remove_file(root.join("a.txt")).unwrap();
    assert_ne!(same(root, &[]), base);
}

#[test]
fn pathspecs_scope_the_key_and_are_part_of_it() {
    let dir = repo(true);
    let root = dir.path();
    let all = same(root, &[]);
    let spec = [":(exclude)TASKS.md"];
    let scoped = same(root, &spec);
    assert_ne!(scoped, all);
    std::fs::write(root.join("TASKS.md"), "# Tasks\n- one\n").unwrap();
    assert_eq!(same(root, &spec), scoped);
    let two = same(root, &["a.txt", "scripts"]);
    assert_ne!(two, scoped);
}

#[test]
fn a_repo_with_no_commit_or_index_gives_the_same_key() {
    let dir = repo(false);
    is_key(&same(dir.path(), &[]));
}

#[test]
fn an_exported_git_dir_does_not_change_the_key() {
    let dir = repo(true);
    let other = repo(true);
    std::fs::write(other.path().join("a.txt"), "other\n").unwrap();
    git(other.path(), &["add", "-A"]);
    let base = same(dir.path(), &[]);
    let git_dir = other.path().join(".git");
    let index = git_dir.join("index");
    let hooked = same_env(
        dir.path(),
        &[],
        &[("GIT_DIR", &git_dir), ("GIT_INDEX_FILE", &index)],
    );
    assert_eq!(hooked, base);
}

#[test]
fn outside_a_checkout_both_exit_1_with_no_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("scripts")).unwrap();
    std::fs::copy(script_source(), root.join("scripts/input-key.sh")).unwrap();
    let ceiling = root.parent().unwrap();
    let extra = [("GIT_CEILING_DIRECTORIES", ceiling)];
    let (s, n) = (script(root, &[], &extra), native(root, &[], &extra));
    assert_eq!(s.status.code(), Some(1));
    assert_eq!(n.status.code(), Some(1));
    assert!(s.stdout.is_empty() && n.stdout.is_empty());
}

#[test]
fn the_index_and_object_store_are_untouched() {
    let dir = repo(true);
    let root = dir.path();
    std::fs::write(root.join("new.txt"), "n\n").unwrap();
    let index = std::fs::read(root.join(".git/index")).unwrap();
    let count = || {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["count-objects", "-v"])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap()
    };
    let before = count();
    let out = native(root, &[], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(count(), before);
}

fn native_raw(args: &[&std::ffi::OsStr]) -> Output {
    let mut cmd = Command::new(XTASK);
    cmd.arg("input-key").args(args);
    env_clean(&mut cmd);
    cmd.output().unwrap()
}

#[test]
fn pathspecs_without_a_separator_give_the_script_key() {
    let dir = repo(true);
    let root = dir.path();
    let spec = ":(exclude)TASKS.md";
    let want = same(root, &[spec]);
    let out = native_raw(&["--repo".as_ref(), root.as_os_str(), spec.as_ref()]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), want);
}

/// Known gap: the script hashes a leading `--` as a pathspec line; the
/// native command treats it as a separator.
#[test]
fn a_leading_double_dash_is_a_separator_only_natively() {
    let dir = repo(true);
    let root = dir.path();
    let plain = same(root, &["a.txt"]);
    let dashed = script(root, &["--", "a.txt"], &[]);
    assert_ne!(String::from_utf8(dashed.stdout).unwrap(), plain);
}

#[test]
fn a_repo_flag_with_no_value_is_a_usage_error() {
    let out = native_raw(&["--repo".as_ref()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn odd_untracked_file_names_give_the_same_key() {
    let dir = repo(true);
    let root = dir.path();
    let base = same(root, &[]);
    std::fs::write(root.join("with space.txt"), "s\n").unwrap();
    std::fs::write(root.join("new\nline.txt"), "n\n").unwrap();
    assert_ne!(same(root, &[]), base);
}

#[test]
fn a_linked_worktree_gives_the_same_key() {
    let dir = repo(true);
    let root = dir.path();
    let holder = tempfile::tempdir().unwrap();
    let wt = holder.path().join("wt");
    git(root, &["worktree", "add", "-q", wt.to_str().unwrap()]);
    std::fs::write(wt.join("new.txt"), "n\n").unwrap();
    is_key(&same(&wt, &[]));
}
