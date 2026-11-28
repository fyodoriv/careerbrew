//! Parity cases for the `rust-16` native handlers: "day.wrap",
//! "day.question.list", "day.question.close", "day.loop" (status, add and
//! stop), "day.history", "work.promise.done", "work.promise.dropped",
//! "work.promise.step", "work.promise.scan" (nothing new) and
//! "work.promise.add" (its errors).
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

/// A wrap length Python's `timedelta` or `datetime` overflows on.
const HUGE_WRAP: Setup = Setup {
    id: "huge-wrap",
    cli: &[],
    files: &[
        (".plaintext", ""),
        ("settings.yaml", "work_wrap_minutes: 99999999999\n"),
    ],
};

/// A record field holding an object with unsorted keys: `json.loads`
/// keeps their order.
const OBJECT_FIELD: Setup = Setup {
    id: "object-field",
    cli: &[],
    files: &[
        (".plaintext", ""),
        (
            "day/work-2026-11-20.json",
            "{\"day\": \"2026-11-20\", \"questions\": [{\"id\": \"q-dddd4444\", \"step\": \"x\", \"steps\": {\"b\": 1, \"a\": 2}}]}",
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
    refuses(&prepare_setup(&HUGE_WRAP), &["day", "wrap"], label);
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
    let object = prepare_setup(&OBJECT_FIELD);
    refuses(&object, &["day", "question", "list", "--json"], label);
    assert_same(&object, &["day", "question", "list"], true, label);
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

const LOOPS_FILE: &str = concat!(
    "{\"inputs\": {}, \"last\": {\"status\": \"ok\", \"seconds\": 41}, ",
    "\"loops\": [",
    "{\"day\": \"2026-11-20\", \"id\": \"c1\", \"kind\": \"cron\", \"started\": \"2026-11-20T08:15\", ",
    "\"step\": \"/standup\", \"stopped\": \"\"}, ",
    "{\"day\": \"2026-11-18\", \"id\": \"m.old\", \"kind\": \"monitor\", \"started\": \"2026-11-18T10:00\", ",
    "\"step\": \"\", \"stopped\": \"\"}, ",
    "{\"day\": \"2026-11-19\", \"id\": \"a2\", \"kind\": \"agent\", \"started\": \"2026-11-19T11:00\", ",
    "\"step\": \"\", \"stopped\": \"2026-11-19T12:00\"}], ",
    "\"next_at\": \"2026-11-20T09:05:00+00:00\", \"passes\": [41, 50.5, 38]}"
);

const LOOPS: Setup = Setup {
    id: "loops",
    cli: &[],
    files: &[(".plaintext", ""), ("day/loops.json", LOOPS_FILE)],
};

/// A stopped stamp `datetime.fromisoformat` reads but this handler does not.
const ODD_LOOP: Setup = Setup {
    id: "odd-loop",
    cli: &[],
    files: &[
        (".plaintext", ""),
        (
            "day/loops.json",
            "{\"loops\": [{\"day\": \"2026-11-20\", \"id\": \"c1\", \"kind\": \"cron\", \"started\": \"2026-11-20T08:15Z\", \"step\": \"\", \"stopped\": \"\"}]}",
        ),
    ],
};

const BAD_LOOPS: Setup = Setup {
    id: "bad-loops",
    cli: &[],
    files: &[(".plaintext", ""), ("day/loops.json", "{\"loops\": {}}")],
};

const HISTORY_FILE: &str = concat!(
    "{\"days\": {\"2026-11-10\": {\"/old\": {\"seconds\": 5, \"status\": \"done\"}}, ",
    "\"2026-11-18\": {\"/careerbrew\": {\"seconds\": 30, \"status\": \"done\"}, ",
    "\"/zeta\": {\"seconds\": 3700, \"status\": \"failed\"}, \"/alpha\": {\"seconds\": 90, \"status\": \"done\"}}, ",
    "\"2026-11-19\": {\"/careerbrew\": {\"seconds\": 50, \"status\": \"done\"}, ",
    "\"/alpha\": {\"seconds\": 150, \"status\": \"blocked\"}, \"/zeta\": {\"seconds\": null, \"status\": \"failed\"}}, ",
    "\"2026-11-20\": {\"/careerbrew\": {\"seconds\": 41, \"status\": \"done\"}, ",
    "\"/beta\": {\"seconds\": 1.5, \"status\": \"running\"}, \"junk\": 3}, \"2026-11-16\": []}}"
);

const HISTORY: Setup = Setup {
    id: "history",
    cli: &[],
    files: &[(".plaintext", ""), ("day/work-history.json", HISTORY_FILE)],
};

/// Another file `plan.build()` may read (a project list): refused.
const HISTORY_PROJECTS: Setup = Setup {
    id: "history-projects",
    cli: &[],
    files: &[
        (".plaintext", ""),
        ("day/work-history.json", HISTORY_FILE),
        ("work/projects.yaml", "projects: []\n"),
    ],
};

/// A `work_steps` setting changes which steps keep their names: refused.
const HISTORY_STEPS: Setup = Setup {
    id: "history-steps",
    cli: &[],
    files: &[
        (".plaintext", ""),
        ("day/work-history.json", HISTORY_FILE),
        ("settings.yaml", "work_steps: []\n"),
    ],
};

/// `True` is an `int` in Python: refused rather than counted.
const HISTORY_BOOL: Setup = Setup {
    id: "history-bool",
    cli: &[],
    files: &[
        (".plaintext", ""),
        (
            "day/work-history.json",
            "{\"days\": {\"2026-11-20\": {\"/a\": {\"seconds\": true, \"status\": \"done\"}}}}",
        ),
    ],
};

#[test]
fn day_loop_matches_python() {
    strict(NOW);
    let label = "day.loop";
    for setup in [&PLAIN, &LOOPS] {
        let home = prepare_setup(setup);
        for args in [
            &["day", "loop", "status"][..],
            &["day", "loop", "status", "--json"],
            &[
                "day", "loop", "add", "--kind", "cron", "--id", "c9", "--step", "standup",
            ],
            &["day", "loop", "add", "--kind", "agent", "--id", "a.2"],
            &[
                "day",
                "loop",
                "add",
                "--kind",
                "session",
                "--id",
                "s:1",
                "--step",
                " /review ",
            ],
            &["day", "loop", "add", "--kind", "agent"],
            &["day", "loop", "add", "--id", "x1"],
            &["day", "loop", "add", "--kind", "agent", "--id", "bad id"],
            &[
                "day", "loop", "add", "--kind", "agent", "--id", "ok", "--step", "bad step",
            ],
            &[
                "day", "loop", "add", "--kind", "agent", "--id", "ok", "--step", "a..b",
            ],
            &["day", "loop", "stop", "--id", "nope"],
            &["day", "loop", "stop", "--id", "c1"],
            &["day", "loop", "stop", "--id", "a2"],
            &["day", "loop", "stop"],
        ] {
            assert_same(&home, args, true, label);
        }
    }
}

#[test]
fn day_loop_refuses_what_it_cannot_match() {
    strict(NOW);
    let home = prepare_setup(&PLAIN);
    // Long-running, subprocess or not ported actions stay on Python.
    // `pass` and `run` print and store their wall time, so two Python runs
    // differ: check only the native refusal for them.
    for action in ["pass", "run"] {
        let args = ["day", "loop", action];
        let got = observe(&home, &args, &Side::Native);
        assert_eq!(got.code, Some(64), "day.loop: {args:?} served");
        assert!(
            got.stdout.is_empty() && got.events.is_empty(),
            "day.loop: {args:?}"
        );
    }
    for action in ["end", "catch-up"] {
        refuses(&home, &["day", "loop", action], "day.loop");
    }
    // An id or step whose Python repr this handler does not spell.
    refuses(
        &home,
        &["day", "loop", "add", "--kind", "agent", "--id", "it's"],
        "day.loop",
    );
    refuses(
        &home,
        &[
            "day",
            "loop",
            "add",
            "--kind",
            "agent",
            "--id",
            "ok",
            "--step",
            "caf\u{e9}",
        ],
        "day.loop",
    );
    refuses(&home, &["day", "loop", "stop", "--id", "it's"], "day.loop");
    for setup in [&ODD_LOOP, &BAD_LOOPS, &REDACTION] {
        let home = prepare_setup(setup);
        refuses(&home, &["day", "loop", "status"], "day.loop");
        refuses(
            &home,
            &["day", "loop", "add", "--kind", "cron", "--id", "c2"],
            "day.loop",
        );
    }
}

#[test]
fn day_history_matches_python() {
    strict(NOW);
    let label = "day.history";
    for setup in [&PLAIN, &HISTORY] {
        let home = prepare_setup(setup);
        for args in [
            &["day", "history"][..],
            &["day", "history", "--json"],
            &["day", "history", "--days", "2"],
            &["day", "history", "--days", "1", "--json"],
            &["day", "history", "--days", "120"],
        ] {
            assert_same(&home, args, true, label);
        }
    }
}

#[test]
fn day_history_refuses_what_it_cannot_match() {
    strict(NOW);
    for setup in [&HISTORY_PROJECTS, &HISTORY_STEPS, &HISTORY_BOOL, &REDACTION] {
        let home = prepare_setup(setup);
        refuses(&home, &["day", "history"], "day.history");
    }
    let home = prepare_setup(&HISTORY);
    for days in ["0", "121", "+3", "x"] {
        refuses(&home, &["day", "history", "--days", days], "day.history");
    }
}

/// Negative control: adding a loop and reading the empty status differ in
/// output and in the files they leave.
#[test]
fn the_loop_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = prepare_setup(&PLAIN);
    let added = observe(
        &home,
        &["day", "loop", "add", "--kind", "cron", "--id", "c1"],
        &Side::Native,
    );
    let status = observe(&home, &["day", "loop", "status"], &Side::Python);
    assert_ne!(added, status);
    let short = observe(
        &prepare_setup(&HISTORY),
        &["day", "history", "--days", "1"],
        &Side::Native,
    );
    let long = observe(&prepare_setup(&HISTORY), &["day", "history"], &Side::Python);
    assert_ne!(short.stdout, long.stdout);
}

// ---------- work promise done, dropped and step ----------

/// `work/projects.yaml` as `store.write_yaml` writes it: an open promise
/// with a link, one with a step, a removal mark and an old `closed` key, a
/// closed one, and a plain project.
const PROJECTS_YAML: &str = "projects:\n  promise-aaaa1111:\n    incarnation_id: '00000000000000000000000000000000'\n    created_at: '2026-11-19T10:00:00+00:00'\n    memory: ''\n    stale_after_days: 14\n    sources:\n    - kind: url\n      ref: https://example.com/t/1\n      checked_at: null\n      status: ''\n      reference: true\n    tasks: []\n    kind: promise\n    title: Send the synthetic report\n    promise:\n      event: ''\n      to: Sam\n      due: '2026-11-21'\n      made_at: '2026-11-19T10:00:00+00:00'\n      steps: {}\n  promise-bbbb2222:\n    incarnation_id: '11111111111111111111111111111111'\n    created_at: '2026-11-18T10:00:00+00:00'\n    memory: ''\n    stale_after_days: 14\n    sources: []\n    tasks: []\n    kind: promise\n    title: Review the synthetic plan\n    promise:\n      event: ev1\n      to: ''\n      due: ''\n      made_at: '2026-11-18T10:00:00+00:00'\n      steps:\n        research: '2026-11-18T11:00:00+00:00'\n      pm_removed: '2026-11-19'\n      closed: x\n    updates:\n    - at: '2026-11-18T11:00:00+00:00'\n      kind: project\n      text: 'research: looked'\n  promise-cccc3333:\n    incarnation_id: '22222222222222222222222222222222'\n    created_at: '2026-11-17T10:00:00+00:00'\n    memory: ''\n    stale_after_days: 14\n    sources: []\n    tasks: []\n    kind: promise\n    title: Closed one\n    promise:\n      event: ''\n      to: ''\n      due: ''\n      made_at: '2026-11-17T10:00:00+00:00'\n      steps: {}\n      closed: done\n    archived_at: '2026-11-18T10:00:00+00:00'\n  synthetic-proj:\n    incarnation_id: '33333333333333333333333333333333'\n    created_at: '2026-11-17T10:00:00+00:00'\n    memory: ''\n    stale_after_days: 14\n    sources: []\n    tasks: []\n";

const PROMISES: Setup = Setup {
    id: "promises",
    cli: &[],
    files: &[(".plaintext", ""), ("work/projects.yaml", PROJECTS_YAML)],
};

const PROMISES_REDACTION: Setup = Setup {
    id: "promises-redaction",
    cli: &[],
    files: &[
        (".plaintext", ""),
        ("profile.yaml", "name: Synthetic Person\n"),
        ("work/projects.yaml", PROJECTS_YAML),
    ],
};

/// Records in flow style: not what `safe_dump` writes back.
const PROMISES_FLOW: Setup = Setup {
    id: "promises-flow",
    cli: &[],
    files: &[
        (".plaintext", ""),
        (
            "work/projects.yaml",
            "{projects: {promise-aaaa1111: {kind: promise, title: x, promise: {steps: {}}}}}\n",
        ),
    ],
};

/// No `.plaintext` marker: every write is sealed and differs.
const PROMISES_SEALED: Setup = Setup {
    id: "promises-sealed",
    cli: &[],
    files: &[("work/projects.yaml", PROJECTS_YAML)],
};

#[test]
fn work_promise_done_and_dropped_match_python() {
    strict(NOW);
    for label in ["work.promise.done", "work.promise.dropped"] {
        let verb = label.rsplit('.').next().unwrap();
        let home = prepare_setup(&PROMISES);
        for name in [
            "promise-aaaa1111",
            "promise-bbbb2222",
            "promise-cccc3333",
            "promise-missing",
            "synthetic-proj",
        ] {
            assert_same(&home, &["work", "promise", verb, name], true, label);
        }
        let empty = prepare_setup(&PLAIN);
        assert_same(
            &empty,
            &["work", "promise", verb, "promise-aaaa1111"],
            true,
            label,
        );
    }
}

#[test]
fn work_promise_step_matches_python() {
    strict(NOW);
    let label = "work.promise.step";
    let home = prepare_setup(&PROMISES);
    let long = "x".repeat(201);
    let edge = "y".repeat(200);
    let cases: Vec<(&str, &str, &str)> = vec![
        ("promise-aaaa1111", "research", "read the synthetic thread"),
        ("promise-aaaa1111", "status", "  told the owner  "),
        ("promise-bbbb2222", "draft", "drafted a reply"),
        ("promise-bbbb2222", "research", "looked again"),
        ("promise-cccc3333", "research", "too late"),
        ("promise-missing", "research", "nothing"),
        ("synthetic-proj", "draft", "not a promise"),
        ("promise-aaaa1111", "draft", "   "),
        ("promise-aaaa1111", "draft", "two\nlines"),
        ("promise-aaaa1111", "draft", &long),
        ("promise-aaaa1111", "draft", &edge),
    ];
    for (name, which, text) in cases {
        assert_same(
            &home,
            &["work", "promise", "step", name, which, text],
            true,
            label,
        );
    }
    let empty = prepare_setup(&PLAIN);
    assert_same(
        &empty,
        &["work", "promise", "step", "promise-aaaa1111", "draft", "x"],
        true,
        label,
    );
}

#[test]
fn work_promise_writers_refuse_what_they_cannot_match() {
    strict(NOW);
    let cases: [(&str, &[&str]); 3] = [
        (
            "work.promise.done",
            &["work", "promise", "done", "promise-aaaa1111"],
        ),
        (
            "work.promise.dropped",
            &["work", "promise", "dropped", "promise-aaaa1111"],
        ),
        (
            "work.promise.step",
            &[
                "work",
                "promise",
                "step",
                "promise-aaaa1111",
                "research",
                "x",
            ],
        ),
    ];
    for (label, args) in cases {
        refuses(&prepare_setup(&PROMISES_REDACTION), args, label);
        refuses(&prepare_setup(&PROMISES_FLOW), args, label);
        refuses(&prepare_setup(&PROMISES_SEALED), args, label);
    }
    // argparse's choices.
    refuses(
        &prepare_setup(&PROMISES),
        &["work", "promise", "step", "promise-aaaa1111", "review", "x"],
        "work.promise.step",
    );
}

/// Negative control: closing a promise and closing a missing one print,
/// exit and write differently, and done and dropped write different files.
#[test]
fn the_promise_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = prepare_setup(&PROMISES);
    let done = observe(
        &home,
        &["work", "promise", "done", "promise-aaaa1111"],
        &Side::Native,
    );
    let missing = observe(
        &home,
        &["work", "promise", "done", "promise-missing"],
        &Side::Python,
    );
    assert_ne!(done, missing);
    assert_ne!(done.files, missing.files);
    let dropped = observe(
        &home,
        &["work", "promise", "dropped", "promise-aaaa1111"],
        &Side::Python,
    );
    assert_ne!(done.files, dropped.files);
}

// ---------- work promise scan and add (wave 6) ----------

/// Own sent events: ev-1 (kept with text, tracked below), ev-2 (kept, no
/// text), ev-3 from someone else, ev-4 personal, ev-5 a page, ev-6 not kept.
const FEED_EVENTS: &str = concat!(
    "{\"actor\": \"me\", \"at\": \"2026-11-19T10:00:00Z\", \"channel\": \"eng\", \"id\": \"ev-1\", \"kind\": \"message\", \"source\": \"slack\"}\n",
    "{\"actor\": \"me\", \"at\": \"2026-11-19T10:05:00Z\", \"id\": \"ev-2\", \"kind\": \"dm\", \"source\": \"slack\"}\n",
    "{\"actor\": \"other\", \"at\": \"2026-11-19T10:06:00Z\", \"from\": \"Sam\", \"id\": \"ev-3\", \"kind\": \"message\", \"source\": \"slack\"}\n",
    "{\"actor\": \"me\", \"at\": \"2026-11-19T10:07:00Z\", \"id\": \"ev-4\", \"kind\": \"dm\", \"personal\": true, \"source\": \"slack\"}\n",
    "\n",
    "{\"actor\": \"me\", \"at\": \"2026-11-19T10:08:00Z\", \"id\": \"ev-5\", \"kind\": \"page\", \"source\": \"slack\"}\n",
    "{\"actor\": \"me\", \"at\": \"2026-11-19T10:09:00Z\", \"id\": \"ev-6\", \"kind\": \"reply\", \"source\": \"slack\"}\n",
);

const FEED_ORIGINALS: &str = concat!(
    "{\"id\": \"ev-1\", \"text\": \"I'll send the synthetic report tomorrow\"}\n",
    "{\"id\": \"ev-2\", \"text\": \"\"}\n",
    "{\"id\": \"ev-3\", \"text\": \"I will look at it today\"}\n",
    "{\"id\": \"ev-4\", \"text\": \"I'll call home tomorrow\"}\n",
);

/// `name_for("ev-1")` is tracked already; `safe_dump` writes exactly this.
const TRACKED_YAML: &str = "projects:\n  promise-e8bd59a7:\n    kind: promise\n    title: Send the synthetic report\n  synthetic-proj:\n    title: Plain\n";

const FEED_TRACKED: Setup = Setup {
    id: "feed-tracked",
    cli: &[],
    files: &[
        (".plaintext", ""),
        ("work/feed/events.jsonl", FEED_EVENTS),
        ("work/feed/originals.jsonl", FEED_ORIGINALS),
        ("work/projects.yaml", TRACKED_YAML),
    ],
};

/// The same feed with nothing tracked: a scan would make a new record.
const FEED_NEW: Setup = Setup {
    id: "feed-new",
    cli: &[],
    files: &[
        (".plaintext", ""),
        ("work/feed/events.jsonl", FEED_EVENTS),
        ("work/feed/originals.jsonl", FEED_ORIGINALS),
    ],
};

/// Kept events with no own candidate, one line ending in `\r\n`.
const FEED_CRLF: Setup = Setup {
    id: "feed-crlf",
    cli: &[],
    files: &[
        (".plaintext", ""),
        (
            "work/feed/events.jsonl",
            "{\"actor\": \"other\", \"id\": \"ev-3\", \"kind\": \"message\"}\r\n",
        ),
        ("work/feed/originals.jsonl", FEED_ORIGINALS),
    ],
};

const FEED_REDACTION: Setup = Setup {
    id: "feed-redaction",
    cli: &[],
    files: &[
        (".plaintext", ""),
        ("profile.yaml", "name: Synthetic Person\n"),
        ("work/feed/events.jsonl", FEED_EVENTS),
        ("work/feed/originals.jsonl", FEED_ORIGINALS),
        ("work/projects.yaml", TRACKED_YAML),
    ],
};

/// The tracked feed in a folder with no `.plaintext` marker.
const FEED_SEALED: Setup = Setup {
    id: "feed-sealed",
    cli: &[],
    files: &[
        ("work/feed/events.jsonl", FEED_EVENTS),
        ("work/feed/originals.jsonl", FEED_ORIGINALS),
        ("work/projects.yaml", TRACKED_YAML),
    ],
};

const SEALED_EMPTY: Setup = Setup {
    id: "sealed-empty",
    cli: &[],
    files: &[],
};

/// The native binary refuses (exit 64) with no output, no event row and no
/// file change. For a case Python serves by writing a record with a random
/// id, where two Python runs differ.
fn native_refuses(home: &Home, args: &[&str], label: &str) {
    restore(home);
    let mut before = Vec::new();
    walk(&home.data, &home.data, &mut before);
    before.sort();
    let got = observe(home, args, &Side::Native);
    assert_eq!(got.code, Some(64), "{label}: {args:?} served");
    assert!(
        got.stdout.is_empty() && got.events.is_empty(),
        "{label}: {args:?}"
    );
    assert_eq!(got.files, before, "{label}: {args:?} wrote");
}

#[test]
fn work_promise_scan_matches_python() {
    strict(NOW);
    let label = "work.promise.scan";
    let args = &["work", "promise", "scan"];
    for setup in [&PLAIN, &FEED_TRACKED, &SEALED_EMPTY] {
        assert_same(&prepare_setup(setup), args, true, label);
    }
}

#[test]
fn work_promise_scan_refuses_what_it_cannot_match() {
    strict(NOW);
    let label = "work.promise.scan";
    let args = &["work", "promise", "scan"];
    for setup in [&FEED_CRLF, &FEED_REDACTION, &FEED_SEALED] {
        refuses(&prepare_setup(setup), args, label);
    }
    // A new promise: Python writes a record with a random id.
    native_refuses(&prepare_setup(&FEED_NEW), args, label);
}

#[test]
fn work_promise_add_errors_match_python() {
    strict(NOW);
    let label = "work.promise.add";
    let cases: [&[&str]; 11] = [
        &["work", "promise", "add", "send it", "--due", "tomorrow"],
        &["work", "promise", "add", "send it", "--due", "2026/11/03"],
        &["work", "promise", "add", "send it", "--due", " 2026-11-3 "],
        &["work", "promise", "add", "send it", "--due", "2026-13-01"],
        &["work", "promise", "add", "send it", "--due", "0000-01-01"],
        &["work", "promise", "add", "send it", "--due", "   "],
        &[
            "work", "promise", "add", "send it", "--due", "nope", "--link", "ftp://x",
        ],
        &[
            "work",
            "promise",
            "add",
            "send it",
            "--link",
            "ftp://example.com/x",
        ],
        &["work", "promise", "add", "   "],
        &["work", "promise", "add", "", "--parent", "missing-proj"],
        &[
            "work",
            "promise",
            "add",
            "send it",
            "--parent",
            "missing-proj",
            "--to",
            "Sam",
        ],
    ];
    for setup in [&PLAIN, &PROMISES, &FEED_TRACKED] {
        let home = prepare_setup(setup);
        for args in cases {
            assert_same(&home, args, true, label);
        }
    }
}

#[test]
fn work_promise_add_refuses_what_it_cannot_match() {
    strict(NOW);
    let label = "work.promise.add";
    // An error `say` would redact, a quote `repr` escapes, and project files
    // this module does not read (flow style, sealed).
    refuses(
        &prepare_setup(&PROMISES_REDACTION),
        &["work", "promise", "add", "x", "--due", "tomorrow"],
        label,
    );
    refuses(
        &prepare_setup(&PLAIN),
        &["work", "promise", "add", "x", "--due", "it's"],
        label,
    );
    refuses(
        &prepare_setup(&PROMISES_FLOW),
        &["work", "promise", "add", "   "],
        label,
    );
    refuses(
        &prepare_setup(&PROMISES_SEALED),
        &["work", "promise", "add", "x", "--parent", "missing-proj"],
        label,
    );
    // Valid adds write a record with a random `incarnation_id`.
    let home = prepare_setup(&PROMISES);
    for args in [
        &["work", "promise", "add", "send the synthetic report"][..],
        &["work", "promise", "add", "x", "--due", "2026-11-23"],
        &["work", "promise", "add", "x", "--due", "20261123"],
        &["work", "promise", "add", "x", "--parent", "synthetic-proj"],
    ] {
        native_refuses(&home, args, label);
    }
}

/// Negative control: two add errors print different lines, a blank text
/// makes the lock file a due-day error does not, and a scan of another
/// feed prints another count.
#[test]
fn the_promise_add_and_scan_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = prepare_setup(&PROMISES);
    let due = observe(
        &home,
        &["work", "promise", "add", "x", "--due", "tomorrow"],
        &Side::Native,
    );
    let blank = observe(&home, &["work", "promise", "add", "   "], &Side::Python);
    assert_ne!(due.stdout, blank.stdout);
    assert_ne!(due.files, blank.files);
    let empty = observe(
        &prepare_setup(&PLAIN),
        &["work", "promise", "scan"],
        &Side::Native,
    );
    let tracked = observe(
        &prepare_setup(&FEED_TRACKED),
        &["work", "promise", "scan"],
        &Side::Python,
    );
    assert_ne!(empty.stdout, tracked.stdout);
}

/// "day.handoff" stays with Python: its two optional positionals (section,
/// text) are a form the argument reader never accepts, so every argv is
/// refused before a handler could run.
#[test]
fn day_handoff_is_refused_before_any_handler() {
    strict(NOW);
    let home = prepare_setup(&PLAIN);
    for args in [
        &["day", "handoff", "show"][..],
        &["day", "handoff", "add", "lessons", "synthetic lesson"],
    ] {
        let got = observe(&home, args, &Side::Native);
        assert_eq!(got.code, Some(64), "{args:?}");
    }
}
