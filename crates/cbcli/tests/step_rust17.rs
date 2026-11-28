//! Parity cases for the `rust-17` native handlers: "day.results" and
//! "work.promise.list".
//!
//! Both only read, so encrypted homes are compared too.
mod common;

use common::*;

const NOW: &str = "2026-11-20T09:00:00";

const RESULTS: &str = concat!(
    "{\"id\": \"a\", \"kind\": \"pr\", \"title\": \"Synthetic one\"}\n",
    "{\"id\": \"b\", \"kind\": \"mail\", \"title\": \"Synthetic two\"}\n",
    "\n",
    "{\"id\": \"c\", \"title\": \"Synthetic three\"}\n",
    "{\"id\": \"d\", \"kind\": \"pr\"}\n",
);

const PROJECTS: &str = "projects:
  p-one:
    kind: promise
    promise:
      due: '2026-11-19'
      made_at: '2026-11-18T10:00:00+00:00'
      steps:
        draft: done
        research: done
  p-two:
    kind: promise
    archived_at: '2026-11-19'
    sources:
    - kind: chat
      ref: https://example.com/b
    promise:
      closed: dropped
      due: '2026-11-21'
      steps:
        status: x
        draft: y
        research: z
  w-three:
    kind: work
    title: Other synthetic work
";

const EMPTY: Setup = Setup {
    id: "empty",
    cli: &[],
    files: &[],
};

const DAY_ROWS: Setup = Setup {
    id: "day-rows",
    cli: &[],
    files: &[
        ("day/2026-11-20.jsonl", RESULTS),
        (
            "day/2026-11-19.jsonl",
            "{\"id\": \"e\", \"kind\": \"page\"}\n",
        ),
    ],
};

const DAY_ROWS_SEALED: Setup = Setup {
    id: "day-rows-sealed",
    cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
    files: &[("day/2026-11-20.jsonl", RESULTS)],
};

const ODD_BREAK: Setup = Setup {
    id: "odd-break",
    cli: &[],
    files: &[(
        "day/2026-11-20.jsonl",
        "{\"id\": \"a\", \"kind\": \"pr\"}\r\n",
    )],
};

const PROMISES: Setup = Setup {
    id: "promises",
    cli: &[
        &[
            "work",
            "promise",
            "add",
            "Send the synthetic report",
            "--due",
            "2026-11-18",
            "--to",
            "Sam Example",
        ],
        &[
            "work",
            "promise",
            "add",
            "Review the demo",
            "--due",
            "2026-11-25",
        ],
        &[
            "work",
            "promise",
            "add",
            "Look into the thing",
            "--link",
            "https://example.com/t/1",
        ],
        &[
            "work",
            "promise",
            "add",
            "Old synthetic one",
            "--due",
            "2026-11-10",
        ],
    ],
    files: &[],
};

const PROMISE_RECORDS: Setup = Setup {
    id: "promise-records",
    cli: &[],
    files: &[(".plaintext", ""), ("work/projects.yaml", PROJECTS)],
};

const PROMISE_SEALED: Setup = Setup {
    id: "promise-sealed",
    cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
    files: &[("work/projects.yaml", PROJECTS)],
};

/// An unquoted YAML date reads as a date, which Python prints differently.
const PROMISE_DATE: Setup = Setup {
    id: "promise-date",
    cli: &[],
    files: &[(
        "work/projects.yaml",
        "projects:\n  p-one:\n    kind: promise\n    promise:\n      due: 2026-11-19\n",
    )],
};

const REDACTION: Setup = Setup {
    id: "redaction-input",
    cli: &[],
    files: &[("profile.yaml", "name: Synthetic Person\n")],
};

fn refuses(home: &Home, args: &[&str], label: &str) {
    assert!(
        !assert_same(home, args, false, label),
        "{label}: {args:?} served"
    );
}

#[test]
fn day_results_matches_python() {
    strict(NOW);
    let label = "day.results";
    for setup in [&EMPTY, &DAY_ROWS, &DAY_ROWS_SEALED] {
        let home = prepare_setup(setup);
        for args in [
            &["day", "results"][..],
            &["day", "results", "--show"],
            &["day", "results", "--date", "2026-11-19"],
            &["day", "results", "--date", "2026-11-20", "--show"],
        ] {
            assert_same(&home, args, true, &format!("{label} in {}", setup.id));
        }
    }
}

#[test]
fn day_results_refuses_what_it_cannot_match() {
    strict(NOW);
    let label = "day.results";
    let empty = prepare_setup(&EMPTY);
    // `date.fromisoformat` reads this basic form; only the plain form is served.
    refuses(&empty, &["day", "results", "--date", "20261119"], label);
    // Year 0: chrono reads it, `date.fromisoformat` raises.
    refuses(&empty, &["day", "results", "--date", "0000-01-01"], label);
    refuses(&prepare_setup(&ODD_BREAK), &["day", "results"], label);
    refuses(&prepare_setup(&REDACTION), &["day", "results"], label);
}

#[test]
fn work_promise_list_matches_python() {
    strict(NOW);
    let label = "work.promise.list";
    for setup in [&EMPTY, &PROMISES, &PROMISE_RECORDS, &PROMISE_SEALED] {
        let home = prepare_setup(setup);
        for args in [
            &["work", "promise", "list"][..],
            &["work", "promise", "list", "--json"],
            &["work", "promise", "list", "--all"],
            &["work", "promise", "list", "--all", "--json"],
        ] {
            assert_same(&home, args, true, &format!("{label} in {}", setup.id));
        }
    }
}

#[test]
fn work_promise_list_refuses_what_it_cannot_match() {
    strict(NOW);
    let label = "work.promise.list";
    refuses(
        &prepare_setup(&PROMISE_DATE),
        &["work", "promise", "list"],
        label,
    );
    refuses(
        &prepare_setup(&REDACTION),
        &["work", "promise", "list"],
        label,
    );
}

/// Negative control: the comparison sees a real difference. A home with
/// promises and an empty one list different rows.
#[test]
fn the_rust17_comparison_detects_a_planted_difference() {
    strict(NOW);
    let empty = observe(
        &prepare_setup(&EMPTY),
        &["work", "promise", "list"],
        &Side::Native,
    );
    let some = observe(
        &prepare_setup(&PROMISES),
        &["work", "promise", "list"],
        &Side::Python,
    );
    assert_ne!(empty, some);
    assert_eq!(empty.stdout, "promises: none open\n");
    let none = observe(&prepare_setup(&EMPTY), &["day", "results"], &Side::Native);
    let rows = observe(
        &prepare_setup(&DAY_ROWS),
        &["day", "results"],
        &Side::Python,
    );
    assert_ne!(none.stdout, rows.stdout);
}
