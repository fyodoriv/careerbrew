//! Parity cases for the `rust-16` native handlers: "day.wrap",
//! "day.question.list" and "day.question.close".
//!
//! Each served case runs Python and the native binary in the same fresh
//! synthetic home and compares stdout, exit code, data-root files (with
//! content digests) and event rows. Homes that a command writes to carry a
//! `.plaintext` marker: an encrypted file differs on every write, on both
//! sides.
mod common;

use common::*;

const NOW: &str = "2026-11-20T09:00:00";

const QUESTIONS_TODAY: &str = concat!(
    "{\"day\": \"2026-11-20\", \"handled\": {}, \"questions\": [",
    "{\"closed\": \"\", \"closedAt\": \"\", \"createdAt\": \"2026-11-20T08:10:00+00:00\", ",
    "\"id\": \"q-aaaa1111\", \"options\": [\"yes\", \"no\"], \"pick\": \"\", ",
    "\"recheck\": \"https://example.com/x\", \"source\": \"plugin:demo\", \"step\": \"inbox\", ",
    "\"text\": \"synthetic ask one\"}, ",
    "{\"closed\": \"answered\", \"closedAt\": \"2026-11-20T08:30:00+00:00\", ",
    "\"createdAt\": \"2026-11-20T08:00:00+00:00\", \"id\": \"q-bbbb2222\", \"options\": [], ",
    "\"pick\": \"\", \"recheck\": \"\", \"source\": \"agent\", \"step\": \"standup\", ",
    "\"text\": \"synthetic ask two\"}], \"rows\": []}"
);

const QUESTIONS_EARLIER: &str = concat!(
    "{\"day\": \"2026-11-17\", \"handled\": {}, \"questions\": [",
    "{\"closed\": \"\", \"closedAt\": \"\", \"createdAt\": \"2026-11-17T10:00:00+00:00\", ",
    "\"id\": \"q-cccc3333\", \"options\": [], \"pick\": \"\", \"recheck\": \"echo hi\", ",
    "\"source\": \"agent\", \"step\": \"review\", \"steps\": [\"review\", \"inbox\"], ",
    "\"text\": \"synthetic ask three\"}], \"rows\": []}"
);

const PLAIN: Setup = Setup {
    id: "plain",
    cli: &[],
    files: &[(".plaintext", "")],
};

const MOVED: Setup = Setup {
    id: "moved",
    cli: &[&["day", "wrap", "--at", "15:00"]],
    files: &[(".plaintext", "")],
};

const SETTINGS: Setup = Setup {
    id: "settings",
    cli: &[],
    files: &[
        (".plaintext", ""),
        (
            "settings.yaml",
            "work_end: '18:00'\nwork_wrap_minutes: 45\ntimezone: America/New_York\n",
        ),
    ],
};

const ENDED: Setup = Setup {
    id: "ended",
    cli: &[],
    files: &[
        (".plaintext", ""),
        (
            "day/work-2026-11-20.json",
            "{\"day\": \"2026-11-20\", \"end\": {\"at\": \"2026-11-20T17:00:00+00:00\"}, \"handled\": {}, \"rows\": []}",
        ),
    ],
};

const QUESTIONS: Setup = Setup {
    id: "questions",
    cli: &[],
    files: &[
        (".plaintext", ""),
        ("day/work-2026-11-20.json", QUESTIONS_TODAY),
        ("day/work-2026-11-17.json", QUESTIONS_EARLIER),
    ],
};

/// Read-only listings also match in an encrypted home.
const QUESTIONS_SEALED: Setup = Setup {
    id: "questions-sealed",
    cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
    files: &[
        ("day/work-2026-11-20.json", QUESTIONS_TODAY),
        ("day/work-2026-11-17.json", QUESTIONS_EARLIER),
    ],
};

const REDACTION: Setup = Setup {
    id: "redaction-input",
    cli: &[],
    files: &[
        (".plaintext", ""),
        ("profile.yaml", "name: Synthetic Person\n"),
    ],
};

const FLOAT_DAY: Setup = Setup {
    id: "float-day",
    cli: &[],
    files: &[
        (".plaintext", ""),
        (
            "day/work-2026-11-20.json",
            "{\"day\": \"2026-11-20\", \"handled\": {}, \"rows\": [{\"step\": \"a\", \"seconds\": 1.5}]}",
        ),
    ],
};

const ODD_RECORD: Setup = Setup {
    id: "odd-record",
    cli: &[],
    files: &[
        (".plaintext", ""),
        (
            "day/work-2026-11-20.json",
            "{\"day\": \"2026-11-20\", \"questions\": [{\"id\": 5, \"step\": \"x\"}]}",
        ),
    ],
};

fn refuses(home: &Home, args: &[&str], label: &str) {
    assert!(
        !assert_same(home, args, false, label),
        "{label}: {args:?} served"
    );
}

#[test]
fn day_wrap_matches_python() {
    strict(NOW);
    let label = "day.wrap";
    for setup in [&PLAIN, &MOVED, &SETTINGS, &ENDED] {
        let home = prepare_setup(setup);
        for args in [
            &["day", "wrap"][..],
            &["day", "wrap", "--json"],
            &["day", "wrap", "--at", "16:30"],
            &["day", "wrap", "--at", "16:30", "--json"],
            &["day", "wrap", "--hold"],
            &["day", "wrap", "--hold", "--json"],
            &["day", "wrap", "--at", "23:50"],
            &["day", "wrap", "--at", "9:5"],
        ] {
            assert_same(&home, args, true, &format!("{label} in {}", setup.id));
        }
    }
}

#[test]
fn day_wrap_refuses_what_it_cannot_match() {
    strict(NOW);
    let label = "day.wrap";
    let plain = prepare_setup(&PLAIN);
    // argparse's mutually exclusive group.
    refuses(&plain, &["day", "wrap", "--at", "16:30", "--hold"], label);
    refuses(&prepare_setup(&REDACTION), &["day", "wrap"], label);
    // The day file is written back whole, and it holds a float.
    let float = prepare_setup(&FLOAT_DAY);
    refuses(&float, &["day", "wrap", "--hold"], label);
    assert_same(&float, &["day", "wrap"], true, label);
}

#[test]
fn day_question_list_matches_python() {
    strict(NOW);
    let label = "day.question.list";
    for setup in [&PLAIN, &QUESTIONS, &QUESTIONS_SEALED] {
        let home = prepare_setup(setup);
        for args in [
            &["day", "question", "list"][..],
            &["day", "question", "list", "--json"],
        ] {
            assert_same(&home, args, true, &format!("{label} in {}", setup.id));
        }
    }
}

#[test]
fn day_question_list_refuses_what_it_cannot_match() {
    strict(NOW);
    let label = "day.question.list";
    refuses(
        &prepare_setup(&REDACTION),
        &["day", "question", "list"],
        label,
    );
    refuses(
        &prepare_setup(&ODD_RECORD),
        &["day", "question", "list"],
        label,
    );
    refuses(
        &prepare_setup(&FLOAT_DAY),
        &["day", "question", "list"],
        label,
    );
}

#[test]
fn day_question_close_matches_python() {
    strict(NOW);
    let label = "day.question.close";
    let home = prepare_setup(&QUESTIONS);
    for id in ["q-aaaa1111", "q-cccc3333", "q-bbbb2222", "q-missing"] {
        assert_same(&home, &["day", "question", "close", id], true, label);
    }
    assert_same(
        &prepare_setup(&PLAIN),
        &["day", "question", "close", "q-missing"],
        true,
        label,
    );
}

#[test]
fn day_question_close_refuses_what_it_cannot_match() {
    strict(NOW);
    let label = "day.question.close";
    refuses(
        &prepare_setup(&REDACTION),
        &["day", "question", "close", "q-aaaa1111"],
        label,
    );
    refuses(
        &prepare_setup(&FLOAT_DAY),
        &["day", "question", "close", "q-aaaa1111"],
        label,
    );
}

/// Negative control: the comparison sees a real difference. Closing an open
/// record and a missing one print, exit and write differently.
#[test]
fn the_rust16_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = prepare_setup(&QUESTIONS);
    let closed = observe(
        &home,
        &["day", "question", "close", "q-aaaa1111"],
        &Side::Native,
    );
    let missing = observe(
        &home,
        &["day", "question", "close", "q-missing"],
        &Side::Python,
    );
    assert_ne!(closed, missing);
    assert_eq!(closed.code, Some(0));
    assert_eq!(missing.code, Some(2));
    let moved = observe(&home, &["day", "wrap", "--at", "16:30"], &Side::Native);
    let held = observe(&home, &["day", "wrap", "--hold"], &Side::Python);
    assert_ne!(moved.stdout, held.stdout);
}
