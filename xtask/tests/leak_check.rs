//! Controls for `cargo xtask leak-check` (rust-27b slice 1): synthetic
//! canaries in throwaway Git repos. Each test owns its temp folder. Output
//! must carry term ids only, never the canary text.
use std::path::Path;
use std::process::{Command, Output};

const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");
const CANARY: &str = "zqxcanaryterm";
const TERM_ID: &str = "t-"; // checked by prefix; exact ids are unit-tested

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

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.email", "ci@example.com"]);
    git(dir.path(), &["config", "user.name", "ci"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.path().join("README.md"), "clean text\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-qm", "init"]);
    std::fs::write(
        dir.path().join(".terms"),
        format!("# a comment\nqzx\n{CANARY}\n"),
    )
    .unwrap();
    dir
}

fn scan(repo: &Path, extra: &[&str]) -> Output {
    Command::new(XTASK)
        .arg("leak-check")
        .args(extra)
        .arg("--repo")
        .arg(repo)
        .arg("--terms-file")
        .arg(repo.join(".terms"))
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    let text = String::from_utf8(out.stdout.clone()).unwrap();
    assert!(!text.to_lowercase().contains(CANARY), "a term was printed");
    text
}

fn commit(repo: &Path, name: &str, bytes: &[u8]) {
    std::fs::write(repo.join(name), bytes).unwrap();
    git(repo, &["add", name]);
    git(repo, &["commit", "-qm", name]);
}

#[test]
fn a_clean_repo_passes() {
    let r = repo();
    let out = scan(r.path(), &[]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out), "privacy scan: 1 file(s), 0 finding(s)\n");
}

#[test]
fn a_tracked_canary_fails_with_its_id_and_line() {
    let r = repo();
    commit(
        r.path(),
        "notes.md",
        format!("one\nsee {CANARY} here\n").as_bytes(),
    );
    let out = scan(r.path(), &[]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(
        text.contains(&format!("LEAK  notes.md:2: {TERM_ID}")),
        "{text}"
    );
    assert!(text.contains("(working: whole word)"), "{text}");
    assert!(
        text.contains("privacy scan: 2 file(s), 1 finding(s)"),
        "{text}"
    );
    assert!(text.contains("add --show"), "{text}");
}

#[test]
fn a_look_alike_spelling_is_found() {
    let r = repo();
    commit(r.path(), "a.md", "Z-Q-X c4n4ry_t3rm\n".as_bytes());
    let out = scan(r.path(), &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("letters spread across"));
}

#[test]
fn a_short_term_matches_only_literally() {
    let r = repo();
    commit(r.path(), "a.md", b"q-z-x\n");
    assert_eq!(scan(r.path(), &[]).status.code(), Some(0));
}

#[test]
fn a_private_path_is_a_finding() {
    let r = repo();
    std::fs::create_dir(r.path().join("private")).unwrap();
    commit(r.path(), "private/x.txt", b"x\n");
    let out = scan(r.path(), &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("LEAK  private/x.txt: path-under-private"));
}

#[test]
fn a_tracked_binary_or_appended_bytes_file_is_skipped_like_python() {
    let r = repo();
    let mut bytes = format!("{CANARY}\n").into_bytes();
    bytes.extend_from_slice(&[0xff, 0xfe, 0x00]);
    commit(r.path(), "blob.bin", &bytes);
    let out = scan(r.path(), &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("1 file(s), 0 finding(s)"));
}

#[test]
fn a_staged_binary_blob_fails_closed() {
    let r = repo();
    std::fs::write(r.path().join("blob.bin"), [0xffu8, 0xfe, 0x00]).unwrap();
    git(r.path(), &["add", "blob.bin"]);
    let out = scan(r.path(), &["--staged"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("LEAK  Git index or working tree: scan-incomplete"));
}

#[test]
fn staged_mode_reads_the_staged_blob_not_the_working_file() {
    let r = repo();
    std::fs::write(r.path().join("s.md"), format!("{CANARY}\n")).unwrap();
    git(r.path(), &["add", "s.md"]);
    std::fs::write(r.path().join("s.md"), "clean now\n").unwrap();
    let out = scan(r.path(), &["--staged"]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(text.contains("LEAK  s.md:1: t-"), "{text}");
    assert!(text.contains("(staged: whole word)"), "{text}");
    assert!(
        text.contains("privacy scan: 1 file(s), 1 finding(s)"),
        "{text}"
    );
}

#[test]
fn a_missing_terms_file_or_bad_flag_is_a_usage_error() {
    let r = repo();
    std::fs::remove_file(r.path().join(".terms")).unwrap();
    assert_eq!(scan(r.path(), &[]).status.code(), Some(2));
    let out = Command::new(XTASK)
        .args(["leak-check", "--nope"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn a_tracked_file_in_a_deleted_folder_is_skipped_like_python() {
    let r = repo();
    std::fs::create_dir(r.path().join("gone")).unwrap();
    commit(r.path(), "gone/a.md", b"clean\n");
    std::fs::remove_dir_all(r.path().join("gone")).unwrap();
    let out = scan(r.path(), &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("1 file(s), 0 finding(s)"));
}

#[test]
fn a_non_utf8_tracked_name_is_scanned_like_python() {
    use std::os::unix::ffi::OsStrExt;
    let r = repo();
    let name = std::ffi::OsStr::from_bytes(b"n\xffame.md");
    std::fs::write(r.path().join(name), format!("{CANARY}\n")).unwrap();
    let ok = Command::new("git")
        .arg("-C")
        .arg(r.path())
        .args(["-c", "user.email=ci@example.com", "-c", "user.name=ci"])
        .arg("add")
        .arg(name)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap()
        .success();
    assert!(ok);
    git(
        r.path(),
        &[
            "-c",
            "user.email=ci@example.com",
            "-c",
            "user.name=ci",
            "commit",
            "-qm",
            "n",
        ],
    );
    let out = scan(r.path(), &[]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(!text.contains("scan-incomplete"), "{text}");
    assert!(text.contains("2 file(s), 1 finding(s)"), "{text}");
}

#[test]
fn a_terms_file_split_by_a_lone_carriage_return_reads_each_term() {
    let r = repo();
    std::fs::write(r.path().join(".terms"), format!("qzx\r{CANARY}")).unwrap();
    commit(r.path(), "a.md", format!("{CANARY}\n").as_bytes());
    let out = scan(r.path(), &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("LEAK  a.md:1: t-"));
}

#[test]
fn a_working_file_with_carriage_returns_reads_like_python_read_text() {
    let r = repo();
    // A lone CR splits the canary; Python's read_text turns it into a newline.
    commit(r.path(), "crlf.md", b"one\r\ntwo zqxcan\raryterm\r\n");
    let out = scan(r.path(), &[]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(text.contains("LEAK  crlf.md:2: "), "{text}");
    assert!(
        text.contains("(working: letters spread over 2 lines, likely a false match)"),
        "{text}"
    );
}
