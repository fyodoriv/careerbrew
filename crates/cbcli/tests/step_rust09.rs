//! rust-09 native handlers: `"work.task-set"` and `"work.done"` must match
//! the Python console script byte for byte (stdout, exit code, data-root
//! files, event rows) in synthetic plaintext homes, and refuse every input
//! they do not port. All task text here is synthetic.
mod common;

use common::*;

const NOW: &str = "2026-11-20T09:00:00";

/// The exact text `store.write_yaml` writes for these tasks.
const TASKS: &str = "tasks:\n- id: t001\n  title: Write the parser\n  impact: 3\n  urgency: 3\n  estimate_h: 1.0\n- id: t002\n  title: Review the notes\n  impact: 2\n  urgency: 4\n  estimate_h: 2.5\n  status: done\n  actual_h: 2.0\n- id: t003\n  title: Plan the tests\n  impact: 4\n  urgency: 2\n  estimate_h: 0.1\n  status: accepted\n  source: agent\n";

fn tasks_setup() -> Home {
    prepare_setup(&Setup {
        id: "work-tasks",
        cli: &[],
        files: &[(".plaintext", ""), ("work/tasks.yaml", TASKS)],
    })
}

fn check(home: &Home, words: &[&str], must_serve: bool, label: &str) -> bool {
    assert_same(home, words, must_serve, label)
}

#[test]
fn work_task_set_matches_python() {
    strict(NOW);
    let home = tasks_setup();
    let none = prepare_setup(&Setup {
        id: "work-no-tasks",
        cli: &[],
        files: &[(".plaintext", "")],
    });
    // "work.task-set": each field, all at once, and each error.
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["work", "task-set", "t001", "--impact", "4"], "impact"),
        (
            vec!["work", "task-set", "t001", "--urgency", "5"],
            "urgency",
        ),
        (vec!["work", "task-set", "t002", "--hours", "3"], "hours"),
        (
            vec![
                "work",
                "task-set",
                "t003",
                "--impact",
                "1",
                "--urgency",
                "1",
                "--hours",
                "0.5",
            ],
            "every field",
        ),
        (vec!["work", "task-set", "t001"], "nothing to change"),
        (
            vec!["work", "task-set", "t001", "--impact", "9"],
            "impact out of range",
        ),
        (
            vec!["work", "task-set", "t001", "--urgency", "0"],
            "urgency out of range",
        ),
        (
            vec!["work", "task-set", "t009", "--impact", "2"],
            "unknown task",
        ),
        (
            vec!["work", "task-set", "t001", "--hours", "500"],
            "hours out of range",
        ),
    ];
    for (words, label) in cases {
        check(&home, &words, true, label);
    }
    check(
        &none,
        &["work", "task-set", "t001", "--impact", "2"],
        true,
        "no tasks file: unknown task",
    );
}

#[test]
fn work_done_matches_python() {
    strict(NOW);
    let home = tasks_setup();
    // "work.done": done, a capped ratio, an agent's task, and each error.
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["work", "done", "t001", "1.5"], "done"),
        (vec!["work", "done", "t001", "0.3"], "under the estimate"),
        (
            vec!["work", "done", "t003", "4"],
            "capped ratio, agent source",
        ),
        (vec!["work", "done", "t002", "1"], "already done"),
        (vec!["work", "done", "t009", "1"], "unknown task"),
        (vec!["work", "done", "t001", "0.01"], "hours out of range"),
    ];
    for (words, label) in cases {
        check(&home, &words, true, label);
    }
}

#[test]
fn work_task_commands_refuse_what_they_do_not_port() {
    strict(NOW);
    let sealed = prepare_setup(&Setup {
        id: "work-tasks-sealed",
        cli: &[],
        files: &[("work/tasks.yaml", TASKS)],
    });
    let odd = prepare_setup(&Setup {
        id: "work-tasks-odd",
        cli: &[],
        files: &[
            (".plaintext", ""),
            ("work/tasks.yaml", "tasks: [{id: t001, impact: 3}]\n"),
        ],
    });
    let linked = prepare_setup(&Setup {
        id: "work-tasks-linked",
        cli: &[],
        files: &[
            (".plaintext", ""),
            (
                "work/tasks.yaml",
                "tasks:\n- id: t001\n  impact: 3\n  link: https://example.com/a\n",
            ),
        ],
    });
    let redacted = prepare_setup(&Setup {
        id: "work-tasks-redaction",
        cli: &[],
        files: &[
            (".plaintext", ""),
            ("work/tasks.yaml", TASKS),
            ("profile.yaml", "name: Synthetic Person\n"),
        ],
    });
    for home in [&sealed, &odd, &linked, &redacted] {
        for (words, label) in [
            (
                vec!["work", "task-set", "t001", "--impact", "2"],
                "work.task-set: refused",
            ),
            (vec!["work", "done", "t001", "1"], "work.done: refused"),
        ] {
            assert!(!check(home, &words, false, label), "{label}: served");
        }
    }
}

/// Negative control: two grades differ in stdout and the stored file, so a
/// wrong native answer could not pass as equal.
#[test]
fn task_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = tasks_setup();
    let four = observe(
        &home,
        &["work", "task-set", "t001", "--impact", "4"],
        &Side::Native,
    );
    let five = observe(
        &home,
        &["work", "task-set", "t001", "--impact", "5"],
        &Side::Python,
    );
    assert_ne!(four, five);
    assert_ne!(four.files, five.files);
    assert!(four.stdout.starts_with("t001: impact 4"));
    assert_eq!(four.code, Some(0));
}
