//! Python/Rust differential controls for `cargo xtask learnings-table`
//! (rust-27b slice 5). Each test builds a synthetic temp repo, copies
//! `scripts/learnings-table.py` into it, and compares stdout, stderr and the
//! exit code of the Python script and the native command, in both the table
//! mode and `--check`.
use std::path::Path;
use std::process::{Command, Output};

const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("scripts")).unwrap();
    std::fs::copy(
        root().join("scripts/learnings-table.py"),
        dir.path().join("scripts/learnings-table.py"),
    )
    .unwrap();
    dir
}

fn learning(dir: &Path, name: &str, text: &str) {
    write(dir, &format!("docs/learnings/{name}"), text);
}

const GOOD: &str =
    "# A change\n\n## Predicted\n\nfast\n\n## Observed\n\nfaster\n\n## Lesson\n\nmeasure\n";

fn python(dir: &Path, check: bool) -> Output {
    let mut cmd = Command::new(root().join(".venv/bin/python"));
    cmd.arg(dir.join("scripts/learnings-table.py"))
        .env("PYTHONIOENCODING", "utf-8");
    if check {
        cmd.arg("--check");
    }
    cmd.output().unwrap()
}

fn native(dir: &Path, check: bool) -> Output {
    let mut cmd = Command::new(XTASK);
    cmd.arg("learnings-table").arg("--repo").arg(dir);
    if check {
        cmd.arg("--check");
    }
    cmd.output().unwrap()
}

/// Both modes agree on stdout, stderr and exit code; returns the `--check` run.
fn same(dir: &Path) -> Output {
    for check in [false, true] {
        let p = python(dir, check);
        let n = native(dir, check);
        assert_eq!(
            String::from_utf8_lossy(&n.stdout),
            String::from_utf8_lossy(&p.stdout),
            "stdout (check={check})"
        );
        assert_eq!(
            String::from_utf8_lossy(&n.stderr),
            String::from_utf8_lossy(&p.stderr),
            "stderr (check={check})"
        );
        assert_eq!(n.status.code(), p.status.code(), "exit (check={check})");
    }
    native(dir, true)
}

fn text(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

#[test]
fn no_folder_and_no_index_print_an_empty_table() {
    let dir = repo();
    let out = same(dir.path());
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out), "learnings: 0 learning file(s), 0 problem(s)\n");
}

#[test]
fn valid_files_sort_newest_first_then_by_name() {
    let dir = repo();
    learning(dir.path(), "2026-01-02-b.md", GOOD);
    learning(dir.path(), "2026-01-02-a.md", GOOD);
    learning(dir.path(), "2025-12-31-z-9.md", GOOD);
    learning(dir.path(), "2026-03-01-later.md", GOOD);
    let out = same(dir.path());
    assert_eq!(out.status.code(), Some(0));
    let table = text(&native(dir.path(), false));
    assert!(table.lines().nth(2).unwrap().starts_with("| 2026-03-01 |"));
    assert_eq!(table.lines().count(), 6);
}

#[test]
fn bad_names_and_dates_are_problems() {
    let dir = repo();
    for name in [
        ".DS_Store",
        "2026-01-02-Upper.md",
        "2026-01-02-a--b.md",
        "2026-01-02.md",
        "2026-1-02-a.md",
        "notes.md",
        "2026-01-02-a.txt",
    ] {
        learning(dir.path(), name, GOOD);
    }
    learning(dir.path(), "2026-02-30-feb.md", GOOD);
    learning(dir.path(), "0000-01-01-zero.md", GOOD);
    learning(dir.path(), "2024-02-29-leap.md", GOOD);
    learning(dir.path(), "2023-02-29-noleap.md", GOOD);
    learning(dir.path(), "2026-13-01-month.md", GOOD);
    // Python's `$` also matches before one final newline.
    learning(dir.path(), "2026-01-03-newline.md\n", GOOD);
    let out = same(dir.path());
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("2026-02-30 is not a date"));
}

#[test]
fn section_rules_match() {
    let dir = repo();
    let cases = [
        (
            "2026-01-01-order.md",
            "# T\n## Observed\nx\n## Predicted\ny\n## Lesson\nz\n",
        ),
        (
            "2026-01-02-dup.md",
            "# T\n## Predicted\nx\n## Predicted\ny\n## Observed\nz\n## Lesson\nw\n",
        ),
        (
            "2026-01-03-unknown.md",
            "# T\n## Predicted\nx\n##  Extra  \ny\n## Observed\nz\n## Lesson\nw\n",
        ),
        (
            "2026-01-04-empty.md",
            "# T\n## Predicted\n\n   \n## Observed\nz\n## Lesson\nw\n",
        ),
        (
            "2026-01-05-outside.md",
            "intro\n# T\n## Predicted\nx\n## Observed\nz\n## Lesson\nw\n",
        ),
        (
            "2026-01-06-notitle.md",
            "## Predicted\nx\n## Observed\nz\n## Lesson\nw\n",
        ),
        (
            "2026-01-07-blanktitle.md",
            "#  \n# Real\n## Predicted\nx\n## Observed\nz\n## Lesson\nw\n",
        ),
        (
            "2026-01-08-missing.md",
            "# T\n## Predicted\nx\n## Lesson\nw\n",
        ),
        (
            "2026-01-09-second.md",
            "# T\n# Again\n## Predicted\nx\n## Observed\nz\n## Lesson\nw\n",
        ),
        ("2026-01-10-empty-file.md", ""),
    ];
    for (name, body) in cases {
        learning(dir.path(), name, body);
    }
    let out = same(dir.path());
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn pipes_whitespace_and_line_breaks_match_python() {
    let dir = repo();
    learning(
        dir.path(),
        "2026-02-01-pipes.md",
        "# a | b \\| c\n## Predicted\n`x|y` and \\|\n## Observed\n\u{a0} spaced \u{3000}\n\n  second\u{1c}\n## Lesson\nlast|\n",
    );
    learning(
        dir.path(),
        "2026-02-02-breaks.md",
        "# Crlf\r\n## Predicted\r\nx\ry\u{2028}z\u{85}w\n## Observed\u{0b}o\u{0c}p\n## Lesson\nl\u{1d}m\u{1e}n\u{2029}q",
    );
    learning(
        dir.path(),
        "2026-02-03-unicode.md",
        "# Ünïcode ✓ 🙂\n## Predicted\n\u{1f}tab\u{1f}\n## Observed\n\u{feff}bom\n## Lesson\nok\n",
    );
    let out = same(dir.path());
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn stray_index_rows_are_problems_and_headers_are_not() {
    let dir = repo();
    learning(dir.path(), "2026-01-01-ok.md", GOOD);
    let long = format!("| 2026-01-01 | {} | p | o | l |", "é".repeat(80));
    write(
        dir.path(),
        "docs/learnings.md",
        &format!(
            "# Learnings\n| Date | Change |\n|---|---|\n|  - | x |\n|\u{a0}Date\u{3000}| y |\n| 2026-01-01 | row | p | o | l |\n{long}\n|Dates| z |\n"
        ),
    );
    let out = same(dir.path());
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out).matches("a table row").count(), 3);
}

#[test]
fn an_unreadable_learning_fails_with_the_same_stdout_and_exit() {
    let dir = repo();
    learning(dir.path(), "2026-01-01-ok.md", GOOD);
    // Not UTF-8. The folder and the name are joined here, so no literal
    // path names a file the public release tree leaves out (surface S21).
    let bad = dir
        .path()
        .join("docs")
        .join("learnings")
        .join("2026-01-02-bad.md");
    std::fs::write(bad, b"# \xff\n").unwrap();
    for check in [false, true] {
        let p = python(dir.path(), check);
        let n = native(dir.path(), check);
        // Python prints a traceback; the native command a short line (a known gap).
        assert_eq!(n.stdout, p.stdout);
        assert_eq!(n.status.code(), p.status.code());
        assert_eq!(n.status.code(), Some(1));
        assert!(!n.stderr.is_empty());
    }
}

#[test]
fn a_wrong_argument_is_a_usage_error() {
    let out = Command::new(XTASK)
        .args(["learnings-table", "--split"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

/// Negative control: a planted change in the Python script (one character
/// of the summary line) must make the comparison fail.
#[test]
fn a_planted_script_change_is_caught() {
    let dir = repo();
    learning(dir.path(), "2026-01-01-ok.md", GOOD);
    let script = dir.path().join("scripts/learnings-table.py");
    let source = std::fs::read_to_string(&script).unwrap();
    let planted = source.replace("learning file(s)", "learning file(z)");
    assert_ne!(planted, source, "the plant must change the script");
    std::fs::write(&script, planted).unwrap();
    let caught = std::panic::catch_unwind(|| same(dir.path()));
    assert!(
        caught.is_err(),
        "the differential accepted a planted defect"
    );
}
