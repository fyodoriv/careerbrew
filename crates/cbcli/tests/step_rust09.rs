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

// ---------- work priority ----------

/// Open tasks whose titles the priority globs match (and closed ones).
const PRIORITY_TASKS: &str = "tasks:\n- id: t001\n  title: Write the parser\n  impact: 3\n  urgency: 3\n  estimate_h: 1.0\n- id: t002\n  title: Fix the invoice page\n  impact: 3\n  urgency: 3\n  estimate_h: 1.0\n- id: t003\n  title: Write the invoice parser\n  impact: 3\n  urgency: 3\n  estimate_h: 1.0\n  status: done\n- id: t004\n  title: Read the notes\n  impact: 3\n  urgency: 3\n  estimate_h: 1.0\n  status: open\n- id: t005\n  title: Send the bill1\n  impact: 3\n  urgency: 3\n  estimate_h: 1.0\n";

const RANKED: &[&[&str]] = &[&[
    "config",
    "set",
    "project_priority",
    "billing=*invoice*|*bill?, parsers=*parser*, notes",
]];

fn ranked_home(id: &'static str) -> Home {
    prepare_setup(&Setup {
        id,
        cli: RANKED,
        files: &[(".plaintext", ""), ("work/tasks.yaml", PRIORITY_TASKS)],
    })
}

fn plain_home(id: &'static str) -> Home {
    prepare_setup(&Setup {
        id,
        cli: &[],
        files: &[(".plaintext", "")],
    })
}

#[test]
fn work_priority_show_matches_python() {
    strict(NOW);
    // "work.priority.show": no ranking yet, then a ranking with counts.
    let empty = plain_home("priority-empty");
    check(
        &empty,
        &["work", "priority", "show"],
        true,
        "work.priority.show: none yet",
    );
    let ranked = ranked_home("priority-ranked");
    check(
        &ranked,
        &["work", "priority", "show"],
        true,
        "work.priority.show: ranked, with open task counts",
    );
}

#[test]
fn work_priority_set_matches_python() {
    strict(NOW);
    let empty = plain_home("priority-set-empty");
    let ranked = ranked_home("priority-set-ranked");
    // "work.priority.set": a first project, a new one, a move, new globs,
    // and each error Python prints.
    let first: Vec<(Vec<&str>, &str)> = vec![
        (vec!["work", "priority", "set", "acme"], "first project"),
        (
            vec!["work", "priority", "set", "acme", "--match", "*acme*"],
            "first project with a glob",
        ),
    ];
    for (words, label) in first {
        check(&empty, &words, true, label);
    }
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (
            vec!["work", "priority", "set", "tools", "--rank", "2"],
            "new project at rank 2",
        ),
        (
            vec!["work", "priority", "set", "notes", "--rank", "1"],
            "move to the top",
        ),
        (
            vec!["work", "priority", "set", "parsers", "--rank", "9"],
            "rank past the end",
        ),
        (
            vec![
                "work",
                "priority",
                "set",
                "billing",
                "--match",
                "*pay*",
                "--match",
                "*Invoice*",
            ],
            "new globs keep the rank",
        ),
        (vec!["work", "priority", "set", "tools"], "new project last"),
        (
            vec!["work", "priority", "set", "notes", "--rank", "0"],
            "rank 0",
        ),
        (vec!["work", "priority", "set", "Bad"], "upper-case name"),
        (
            vec!["work", "priority", "set", "p", "--match", "x=y"],
            "a glob with =",
        ),
        (
            vec!["work", "priority", "set", "a,notes"],
            "a comma that lists notes twice",
        ),
    ];
    for (words, label) in cases {
        check(&ranked, &words, true, label);
    }
}

#[test]
fn work_priority_remove_matches_python() {
    strict(NOW);
    let ranked = ranked_home("priority-remove-ranked");
    let one = prepare_setup(&Setup {
        id: "priority-remove-one",
        cli: &[&["config", "set", "project_priority", "acme"]],
        files: &[(".plaintext", "")],
    });
    let empty = plain_home("priority-remove-empty");
    // "work.priority.remove": a middle one, the last one, and unknown ones.
    check(
        &ranked,
        &["work", "priority", "remove", "parsers"],
        true,
        "remove one",
    );
    check(
        &one,
        &["work", "priority", "remove", "acme"],
        true,
        "remove the last one",
    );
    check(
        &ranked,
        &["work", "priority", "remove", "nope"],
        true,
        "unknown project",
    );
    check(
        &empty,
        &["work", "priority", "remove", "nope"],
        true,
        "unknown, none yet",
    );
}

#[test]
fn work_priority_refuses_what_it_does_not_port() {
    strict(NOW);
    let sealed = prepare_setup(&Setup {
        id: "priority-sealed",
        cli: &[],
        files: &[],
    });
    let projects = prepare_setup(&Setup {
        id: "priority-projects",
        cli: RANKED,
        files: &[(".plaintext", ""), ("work/projects.yaml", "projects: {}\n")],
    });
    let results = prepare_setup(&Setup {
        id: "priority-results",
        cli: RANKED,
        files: &[(".plaintext", ""), ("day/2026-11-20.jsonl", "")],
    });
    let ticket = prepare_setup(&Setup {
        id: "priority-ticket",
        cli: RANKED,
        files: &[
            (".plaintext", ""),
            (
                "work/tasks.yaml",
                "tasks:\n- id: t001\n  title: Fix PROJ-12\n",
            ),
        ],
    });
    let bracket = prepare_setup(&Setup {
        id: "priority-bracket",
        cli: &[&["config", "set", "project_priority", "acme=[ab]*"]],
        files: &[(".plaintext", "")],
    });
    for home in [&sealed, &projects, &results, &ticket, &bracket] {
        for (words, label) in [
            (
                vec!["work", "priority", "show"],
                "work.priority.show: refused",
            ),
            (
                vec!["work", "priority", "set", "zeta"],
                "work.priority.set: refused",
            ),
        ] {
            assert!(!check(home, &words, false, label), "{label}: served");
        }
    }
    assert!(!check(
        &projects,
        &["work", "priority", "remove", "notes"],
        false,
        "work.priority.remove: refused"
    ));
}

/// Negative control: two rankings differ in stdout and settings.yaml.
#[test]
fn priority_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = ranked_home("priority-control");
    let first = observe(
        &home,
        &["work", "priority", "set", "notes", "--rank", "1"],
        &Side::Native,
    );
    let second = observe(
        &home,
        &["work", "priority", "set", "notes", "--rank", "2"],
        &Side::Python,
    );
    assert_ne!(first, second);
    assert_ne!(first.files, second.files);
    assert_eq!(first.code, Some(0));
}
