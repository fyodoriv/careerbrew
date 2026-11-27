//! rust-14 native handlers against Python: `work live stop` and
//! `notify list`, in synthetic homes (`common`).
mod common;

use common::*;

const LIVE_STOP: &str = "work.live.stop";
const NOTIFY_LIST: &str = "notify.list";

const RUNNING: &str = "{\"cursors\": {\"chat\": \"c-7\"}, \"cycles\": 3, \"running\": true}\n";
const STOPPED: &str = "{\"cursors\": {}, \"cycles\": 2, \"running\": false, \"stopped_at\": \"2026-11-19T17:00:00Z\"}\n";

/// Five stored notifications: an open suggestion seen twice, a snoozed one,
/// one whose suggestion was decided, a direct message (closed: no feed holds
/// its ask) and a dismissed one.
const NOTIFICATIONS: &str = r#"{"items": [
 {"id": "n-0001", "key": "suggestion:in-1", "kind": "suggestion", "sub": "goal", "source": "inbox", "headline": "a new goal is suggested", "why": "your top gap has no upcoming goal", "next": "approve it in Today", "priority": 3, "priority_why": "", "item": "in-1", "refs": ["in-1"], "count": 2, "at": "2026-11-20T08:30:00+00:00", "status": "new", "popup": null, "created_at": "2026-11-20T08:30:00+00:00", "updated_at": "2026-11-20T08:40:00+00:00", "acted_at": "", "snooze_until": "", "resolved": ""},
 {"id": "n-0002", "key": "suggestion:in-3", "kind": "suggestion", "sub": "agent-task", "source": "inbox", "headline": "an agent proposes a task", "why": "it is never planned until you accept it", "next": "accept it in Today", "priority": 3, "priority_why": "", "item": "in-3", "refs": ["in-3"], "count": 1, "at": "2026-11-20T07:00:00+00:00", "status": "snoozed", "popup": "sent", "created_at": "2026-11-20T07:00:00+00:00", "updated_at": "2026-11-20T07:10:00+00:00", "acted_at": "2026-11-20T07:10:00+00:00", "snooze_until": "2026-11-20T10:10:00+00:00", "resolved": ""},
 {"id": "n-0003", "key": "suggestion:in-2", "kind": "suggestion", "sub": "new-task", "source": "inbox", "headline": "a ticket assigned to you has no task", "why": "no plan includes work that is assigned to you", "next": "accept it in Today", "priority": 3, "priority_why": "", "item": "in-2", "refs": ["in-2"], "count": 1, "at": "2026-11-20T06:00:00+00:00", "status": "read", "popup": null, "created_at": "2026-11-20T06:00:00+00:00", "updated_at": "2026-11-20T06:00:00+00:00", "acted_at": "", "snooze_until": "", "resolved": ""},
 {"id": "n-0004", "key": "chat:dm-1", "kind": "dm", "sub": "", "source": "chat", "headline": "a direct message asks you", "why": "it was sent to you directly", "next": "reply in chat", "priority": 4, "priority_why": "", "item": "", "refs": ["f-1"], "count": 1, "at": "2026-11-20T08:00:00+00:00", "status": "new", "popup": null, "created_at": "2026-11-20T08:00:00+00:00", "updated_at": "2026-11-20T08:00:00+00:00", "acted_at": "", "snooze_until": "", "resolved": ""},
 {"id": "n-0005", "key": "chat:dm-2", "kind": "dm", "sub": "", "source": "chat", "headline": "a direct message asks you", "why": "it was sent to you directly", "next": "reply in chat", "priority": 4, "priority_why": "", "item": "", "refs": ["f-2"], "count": 1, "at": "2026-11-19T08:00:00+00:00", "status": "dismissed", "popup": null, "created_at": "2026-11-19T08:00:00+00:00", "updated_at": "2026-11-19T09:00:00+00:00", "acted_at": "2026-11-19T09:00:00+00:00", "snooze_until": "", "resolved": "dismissed"}
], "popups": [{"id": "p-1", "at": "2026-11-20T07:00:00+00:00", "launch_state": "launched"}]}
"#;

/// Only closed rows: the dismissed message.
const CLOSED_ONLY: &str = r#"{"items": [
 {"id": "n-0005", "key": "chat:dm-2", "kind": "dm", "sub": "", "source": "chat", "headline": "a direct message asks you", "why": "it was sent to you directly", "next": "reply in chat", "priority": 4, "priority_why": "", "item": "", "refs": ["f-2"], "count": 1, "at": "2026-11-19T08:00:00+00:00", "status": "dismissed", "popup": null, "created_at": "2026-11-19T08:00:00+00:00", "updated_at": "2026-11-19T09:00:00+00:00", "acted_at": "2026-11-19T09:00:00+00:00", "snooze_until": "", "resolved": "dismissed"}
], "popups": []}
"#;

/// A priority Python cannot sort: it crashes, so native must refuse.
const BAD_PRIORITY: &str = r#"{"items": [
 {"id": "n-0009", "key": "chat:dm-9", "kind": "dm", "sub": "", "source": "chat", "headline": "a direct message asks you", "why": "it was sent to you directly", "next": "reply in chat", "priority": "high", "priority_why": "", "item": "", "refs": ["f-9"], "count": 1, "at": "2026-11-20T08:00:00+00:00", "status": "new", "popup": null, "created_at": "", "updated_at": "", "acted_at": "", "snooze_until": "", "resolved": ""}
], "popups": []}
"#;

const INBOX: &str = r#"{"items": [
 {"id": "in-1", "kind": "goal", "status": "open", "text": "Synthetic goal", "data": {"title": "Synthetic goal", "date": "2026-12-01"}},
 {"id": "in-2", "kind": "new-task", "status": "accepted", "text": "Synthetic task", "data": {"title": "Synthetic task"}},
 {"id": "in-3", "kind": "agent-task", "status": "open", "text": "Synthetic agent task", "data": {"task": "t-1"}}
]}
"#;

const FEED_ROW: &str = "{\"at\": \"2026-11-20T08:00:00+00:00\", \"id\": \"f-1\", \"kind\": \"dm\", \"source\": \"chat\", \"title\": \"Synthetic ask\", \"triage\": \"act\"}\n";

const NOTIFY_ON: &[&[&str]] = &[&["config", "set", "notify_events", "true"]];

const LIST_FORMS: &[&[&str]] = &[
    &["notify", "list"],
    &["notify", "list", "--all"],
    &["notify", "list", "--show"],
    &["notify", "list", "--all", "--show"],
];

#[test]
fn work_live_stop_matches_python() {
    strict("2026-11-20T09:00:00");
    let argv = ["work", "live", "stop"];
    for (id, files) in [
        ("live-never-started", &[][..]),
        ("live-running", &[("work/live/state.json", RUNNING)][..]),
        ("live-stopped", &[("work/live/state.json", STOPPED)][..]),
    ] {
        let home = prepare_setup(&Setup {
            id,
            cli: &[],
            files,
        });
        assert_same(&home, &argv, true, &format!("{LIVE_STOP} {id}"));
    }
}

#[test]
fn work_live_stop_refuses_what_it_cannot_copy() {
    strict("2026-11-20T09:00:00");
    let argv = ["work", "live", "stop"];
    for (id, files) in [
        // Python's `say` would redact through the profile's terms.
        (
            "live-redaction",
            &[
                ("work/live/state.json", RUNNING),
                ("profile.yaml", "name: Synthetic Person\n"),
            ][..],
        ),
        // A state file Python cannot read: it prints its own error.
        ("live-corrupt", &[("work/live/state.json", "{\n")][..]),
        // A cycle count Python prints but the native line cannot.
        (
            "live-odd-count",
            &[(
                "work/live/state.json",
                "{\"cycles\": \"3\", \"running\": true}\n",
            )][..],
        ),
    ] {
        let home = prepare_setup(&Setup {
            id,
            cli: &[],
            files,
        });
        assert!(
            !assert_same(&home, &argv, false, &format!("{LIVE_STOP} {id}")),
            "{id} must refuse"
        );
    }
}

#[test]
fn notify_list_matches_python() {
    strict("2026-11-20T09:00:00");
    let homes = [
        Setup {
            id: "notify-off-empty",
            cli: &[],
            files: &[],
        },
        Setup {
            id: "notify-on-empty",
            cli: NOTIFY_ON,
            files: &[],
        },
        Setup {
            id: "notify-on-rows",
            cli: NOTIFY_ON,
            files: &[
                ("day/notifications.json", NOTIFICATIONS),
                ("day/inbox.json", INBOX),
            ],
        },
        Setup {
            id: "notify-off-rows",
            cli: &[],
            files: &[
                ("day/notifications.json", NOTIFICATIONS),
                ("day/inbox.json", INBOX),
            ],
        },
        Setup {
            id: "notify-on-closed-only",
            cli: NOTIFY_ON,
            files: &[("day/notifications.json", CLOSED_ONLY)],
        },
        Setup {
            id: "notify-off-closed-only",
            cli: &[],
            files: &[("day/notifications.json", CLOSED_ONLY)],
        },
    ];
    for setup in &homes {
        let home = prepare_setup(setup);
        for argv in LIST_FORMS {
            assert_same(&home, argv, true, &format!("{NOTIFY_LIST} {}", setup.id));
        }
    }
}

#[test]
fn notify_list_refuses_what_it_cannot_copy() {
    strict("2026-11-20T09:00:00");
    let homes = [
        // A work feed decides which asks are open; it stays with Python.
        Setup {
            id: "notify-feed",
            cli: NOTIFY_ON,
            files: &[
                ("day/notifications.json", NOTIFICATIONS),
                ("day/inbox.json", INBOX),
                ("work/feed/events.jsonl", FEED_ROW),
            ],
        },
        Setup {
            id: "notify-redaction",
            cli: NOTIFY_ON,
            files: &[
                ("day/notifications.json", NOTIFICATIONS),
                ("profile.yaml", "name: Synthetic Person\n"),
            ],
        },
        Setup {
            id: "notify-bad-priority",
            cli: NOTIFY_ON,
            files: &[("day/notifications.json", BAD_PRIORITY)],
        },
        Setup {
            id: "notify-bad-file",
            cli: NOTIFY_ON,
            files: &[("day/notifications.json", "[1, 2]\n")],
        },
    ];
    for setup in &homes {
        let home = prepare_setup(setup);
        assert!(
            !assert_same(
                &home,
                &["notify", "list"],
                false,
                &format!("{NOTIFY_LIST} {}", setup.id)
            ),
            "{} must refuse",
            setup.id
        );
    }
}

/// Negative controls: the comparison sees a real difference between two
/// argv forms and between two homes.
#[test]
fn the_comparison_detects_a_planted_difference() {
    strict("2026-11-20T09:00:00");
    let rows = prepare_setup(&Setup {
        id: "control-rows",
        cli: NOTIFY_ON,
        files: &[
            ("day/notifications.json", NOTIFICATIONS),
            ("day/inbox.json", INBOX),
        ],
    });
    let open = observe(&rows, &["notify", "list"], &Side::Native);
    let every = observe(&rows, &["notify", "list", "--all"], &Side::Python);
    assert_ne!(open, every);
    assert!(open.stdout.contains("n-0001 [new] p3"), "{}", open.stdout);
    assert!(!open.stdout.contains("n-0005"), "{}", open.stdout);
    assert!(
        every.stdout.contains("n-0005 [dismissed]"),
        "{}",
        every.stdout
    );
    let running = prepare_setup(&Setup {
        id: "control-running",
        cli: &[],
        files: &[("work/live/state.json", RUNNING)],
    });
    let never = prepare_setup(&Setup {
        id: "control-never",
        cli: &[],
        files: &[],
    });
    let stopped = observe(&running, &["work", "live", "stop"], &Side::Native);
    let idle = observe(&never, &["work", "live", "stop"], &Side::Python);
    assert_ne!(stopped, idle);
    assert!(stopped
        .stdout
        .starts_with("live feed stopped after 3 cycle(s)"));
    assert_eq!(idle.stdout, "the live feed was not running\n");
}
