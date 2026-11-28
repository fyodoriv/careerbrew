//! Parity cases for the `rust-17` native handlers: "day.results",
//! "work.promise.list", and (wave 8) the others named below.
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

// ---------- wave 8: "day.pm-sent", "work.promise.outcomes",
// "work.promise.stats", "work.promise.accept" ----------

const SENT: &str = "Today\n- shipped the synthetic thing\n\nTomorrow\n- review the demo\n";

const PM_EMPTY: Setup = Setup {
    id: "pm-empty",
    cli: &[],
    files: &[(".plaintext", ""), ("sent.txt", SENT)],
};

const PM_SEALED: Setup = Setup {
    id: "pm-sealed",
    cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
    files: &[("sent.txt", SENT)],
};

const PM_FALSY_SHOWN: Setup = Setup {
    id: "pm-falsy-shown",
    cli: &[],
    files: &[
        (".plaintext", ""),
        ("sent.txt", SENT),
        ("day/pm-shown-2026-11-20.json", "{}"),
        ("day/pm-shown-2026-11-19.json", "[]"),
    ],
};

const PM_SHOWN: Setup = Setup {
    id: "pm-shown",
    cli: &[],
    files: &[
        (".plaintext", ""),
        ("sent.txt", SENT),
        (
            "day/pm-shown-2026-11-20.json",
            "{\"raw\": \"Synthetic: one\", \"shown\": \"Synthetic: one\"}",
        ),
    ],
};

const PM_BAD_TEXT: Setup = Setup {
    id: "pm-bad-text",
    cli: &[],
    files: &[(".plaintext", ""), ("sent.txt", "\u{fffd}")],
};

fn sent_args<'a>(home: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["day", "pm-sent", home];
    args.extend_from_slice(extra);
    args
}

#[test]
fn day_pm_sent_with_no_shown_draft_matches_python() {
    strict(NOW);
    let label = "day.pm-sent";
    for setup in [&PM_EMPTY, &PM_SEALED, &PM_FALSY_SHOWN] {
        let home = prepare_setup(setup);
        let file = home.data.join("sent.txt").display().to_string();
        for extra in [
            &[][..],
            &["--day", "2026-11-19"],
            &["--day", "2026-11-18"],
            // Python does not check the day; any plain form is served.
            &["--day", "2026-13-45"],
        ] {
            let args = sent_args(&file, extra);
            assert_same(&home, &args, true, &format!("{label} in {}", setup.id));
        }
    }
}

#[test]
fn day_pm_sent_refuses_what_it_cannot_match() {
    strict(NOW);
    let label = "day.pm-sent";
    // A shown draft: Python learns from it (history, promises, receipts),
    // and two Python runs differ (a saved stamp), so only the native side
    // is checked: it refuses and writes nothing.
    let shown = prepare_setup(&PM_SHOWN);
    let file = shown.data.join("sent.txt").display().to_string();
    let mut before = Vec::new();
    walk(&shown.data, &shown.data, &mut before);
    before.sort();
    let got = observe(&shown, &sent_args(&file, &[]), &Side::Native);
    assert_eq!(got.code, Some(64), "{label}: a shown draft served");
    assert!(got.stdout.is_empty() && got.events.is_empty(), "{label}");
    assert_eq!(got.files, before, "{label}: a refusal wrote");
    // Another day of the same home has no shown draft: served.
    assert_same(
        &shown,
        &sent_args(&file, &["--day", "2026-11-19"]),
        true,
        label,
    );
    let empty = prepare_setup(&PM_EMPTY);
    let file = empty.data.join("sent.txt").display().to_string();
    refuses(&empty, &sent_args(&file, &["--day", "yesterday"]), label);
    let missing = empty.data.join("missing.txt").display().to_string();
    refuses(&empty, &sent_args(&missing, &[]), label);
    refuses(&empty, &sent_args("~/sent.txt", &[]), label);
    let bad = prepare_setup(&PM_BAD_TEXT);
    let path = bad.data.join("sent.txt");
    std::fs::write(&path, [0xff_u8, 0xfe, 0x00]).unwrap();
    let snap = bad
        .home
        .parent()
        .unwrap()
        .with_file_name("snap")
        .join("data/sent.txt");
    std::fs::write(snap, [0xff_u8, 0xfe, 0x00]).unwrap();
    refuses(&bad, &sent_args(&path.display().to_string(), &[]), label);
    let redacted = prepare_setup(&REDACTION);
    refuses(&redacted, &sent_args("/dev/null", &[]), label);
}

/// Decisions with no safe day and observations Python skips without a
/// sent-day read; plus promises for the stats window.
const DECISIONS: &str = "projects:
  p-a:
    kind: promise
    archived_at: '2026-11-19T10:00:00+00:00'
    promise:
      source: pm-update
      closed: done
      due: '2026-11-19'
      made_at: '2026-11-18T00:00:00'
      pm_decision:
        day: not-a-day
        state: moved
        due: '2026-11-21'
  p-b:
    kind: promise
    promise:
      source: pm-update
      due: 2026-11-19
      made_at: '2026-11-17T00:00:00'
      pm_decision:
        state: other
        day: '2026-02-30'
  p-c:
    kind: promise
    archived_at: '2026-11-19'
    promise:
      made_at: '2026-11-10T08:00:00+00:00'
      closed: dropped
      pm_observation:
        day: '2026-11-19'
        state: done
  p-d:
    kind: promise
    promise:
      made_at: '2026-11-19T23:30:00Z'
      pm_moved: true
      due: '2026-11-20'
      pm_observation:
        day: bad
        state: moved
  p-e:
    kind: promise
    archived_at: '2026-11-20T08:00:00+00:00'
    promise:
      made_at: '2026-11-15'
      closed: done
      due: '2026-11-18'
  p-f:
    kind: promise
    archived_at: '2026-11-20T08:00:00+00:00'
    promise:
      made_at: '2026-11-16T12:00:00+00:00'
      closed: done
      original_due: '2026-11-25'
      due: '2026-11-10'
  p-g:
    kind: promise
    archived_at: '2026-11-20T08:00:00+00:00'
    promise:
      made_at: '2026-11-19T12:00:00+00:00'
      closed: done
      manual_kept_day: '2026-11-19'
      due: ''
  p-h:
    kind: promise
    promise:
      made_at: '2026-11-19T12:00:00+00:00'
      pm_removed: true
  p-i:
    kind: promise
    promise:
      made_at: ''
  p-j:
    kind: promise
    promise:
      made_at: '2026-12-01T00:00:00'
  w-k:
    kind: work
    title: Other synthetic work
    promise:
      made_at: '2026-11-19T00:00:00'
";

const DECISION_HOME: Setup = Setup {
    id: "decisions",
    cli: &[],
    files: &[(".plaintext", ""), ("work/projects.yaml", DECISIONS)],
};

const DECISION_SEALED: Setup = Setup {
    id: "decisions-sealed",
    cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
    files: &[("work/projects.yaml", DECISIONS)],
};

const DECISION_ZONE: Setup = Setup {
    id: "decisions-tokyo",
    cli: &[],
    files: &[
        (".plaintext", ""),
        ("settings.yaml", "timezone: Asia/Tokyo\n"),
        ("work/projects.yaml", DECISIONS),
    ],
};

/// An open record with a current-day observation: Python reads the sent
/// text under the sent-day lock.
const OBSERVED: Setup = Setup {
    id: "observed",
    cli: &[],
    files: &[(
        "work/projects.yaml",
        "projects:\n  p-one:\n    kind: promise\n    promise:\n      pm_observation:\n        day: '2026-11-19'\n        state: done\n",
    )],
};

/// A decision with a safe day: Python compares its receipt revision.
const DECIDED: Setup = Setup {
    id: "decided",
    cli: &[],
    files: &[(
        "work/projects.yaml",
        "projects:\n  p-one:\n    kind: promise\n    promise:\n      pm_decision:\n        day: '2026-11-19'\n        state: done\n",
    )],
};

/// A made_at form Python reads that is not served.
const ODD_STAMP: Setup = Setup {
    id: "odd-stamp",
    cli: &[],
    files: &[(
        "work/projects.yaml",
        "projects:\n  p-one:\n    kind: promise\n    promise:\n      made_at: '2026-11-19 09:00:00'\n",
    )],
};

#[test]
fn work_promise_outcomes_matches_python() {
    strict(NOW);
    let label = "work.promise.outcomes";
    for setup in [
        &EMPTY,
        &PROMISES,
        &PROMISE_RECORDS,
        &PROMISE_SEALED,
        &DECISION_HOME,
        &DECISION_SEALED,
    ] {
        let home = prepare_setup(setup);
        for args in [
            &["work", "promise", "outcomes"][..],
            &["work", "promise", "outcomes", "--json"],
        ] {
            assert_same(&home, args, true, &format!("{label} in {}", setup.id));
        }
    }
}

#[test]
fn work_promise_outcomes_refuses_what_it_cannot_match() {
    strict(NOW);
    let label = "work.promise.outcomes";
    for setup in [&OBSERVED, &DECIDED, &REDACTION] {
        refuses(
            &prepare_setup(setup),
            &["work", "promise", "outcomes", "--json"],
            label,
        );
    }
}

#[test]
fn work_promise_stats_matches_python() {
    strict(NOW);
    let label = "work.promise.stats";
    for setup in [
        &EMPTY,
        &PROMISES,
        &PROMISE_RECORDS,
        &DECISION_HOME,
        &DECISION_SEALED,
        &DECISION_ZONE,
    ] {
        let home = prepare_setup(setup);
        for args in [
            &["work", "promise", "stats", "--days", "1"][..],
            &["work", "promise", "stats", "--days", "2", "--json"],
            &["work", "promise", "stats", "--days", "5"],
            &["work", "promise", "stats", "--days", "7", "--json"],
            &["work", "promise", "stats", "--days", "30"],
            &["work", "promise", "stats", "--days", "3660", "--json"],
            &["work", "promise", "stats", "--days", "0"],
            &["work", "promise", "stats", "--days", "3661", "--json"],
        ] {
            assert_same(&home, args, true, &format!("{label} in {}", setup.id));
        }
    }
}

#[test]
fn work_promise_stats_refuses_what_it_cannot_match() {
    strict(NOW);
    let label = "work.promise.stats";
    for setup in [&OBSERVED, &DECIDED, &ODD_STAMP, &REDACTION] {
        refuses(
            &prepare_setup(setup),
            &["work", "promise", "stats", "--days", "30"],
            label,
        );
    }
}

#[test]
fn work_promise_accept_without_yes_matches_python() {
    strict(NOW);
    let label = "work.promise.accept";
    for setup in [&EMPTY, &DECISION_HOME] {
        let home = prepare_setup(setup);
        for args in [
            &["work", "promise", "accept", "pm-synthetic"][..],
            &["work", "promise", "accept", "pm-resolution-synthetic"],
        ] {
            assert_same(&home, args, true, &format!("{label} in {}", setup.id));
        }
    }
}

#[test]
fn work_promise_accept_refuses_what_it_cannot_match() {
    strict(NOW);
    let label = "work.promise.accept";
    // With --yes Python checks for a terminal, which the child cannot see.
    let home = prepare_setup(&DECISION_HOME);
    refuses(
        &home,
        &["work", "promise", "accept", "pm-synthetic", "--yes"],
        label,
    );
    refuses(
        &prepare_setup(&REDACTION),
        &["work", "promise", "accept", "pm-synthetic"],
        label,
    );
}

/// Negative control for wave 8: the comparison sees a planted difference.
#[test]
fn the_wave8_comparison_detects_a_planted_difference() {
    strict(NOW);
    let none = observe(
        &prepare_setup(&EMPTY),
        &["work", "promise", "outcomes", "--json"],
        &Side::Native,
    );
    let some = observe(
        &prepare_setup(&DECISION_HOME),
        &["work", "promise", "outcomes", "--json"],
        &Side::Python,
    );
    assert_eq!(none.stdout, "{\"outcomes\": []}\n");
    assert_ne!(none.stdout, some.stdout);
    let few = observe(
        &prepare_setup(&EMPTY),
        &["work", "promise", "stats", "--days", "30"],
        &Side::Native,
    );
    let many = observe(
        &prepare_setup(&DECISION_HOME),
        &["work", "promise", "stats", "--days", "30"],
        &Side::Python,
    );
    assert_ne!(few.stdout, many.stdout);
}
