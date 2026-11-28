//! rust-14 native handlers against Python: `work live stop`, `notify list`,
//! `notify status`, `notify act`, `work session list` and the argument
//! error of `work session end`, in synthetic homes (`common`).
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

// ---------- notify status ----------

const NOTIFY_STATUS: &str = "notify.status";

/// A pop-up stamped without an offset: Python reads it in the local zone,
/// the crate cannot, so native must refuse.
const NAIVE_POPUP: &str = r#"{"items": [], "popups": [{"id": "p-1", "at": "2026-11-20T07:00:00", "launch_state": "launched"}]}
"#;

#[test]
fn notify_status_matches_python() {
    strict("2026-11-20T09:00:00");
    let homes = [
        Setup {
            id: "status-off",
            cli: &[],
            files: &[],
        },
        Setup {
            id: "status-on",
            cli: NOTIFY_ON,
            files: &[],
        },
        Setup {
            id: "status-quiet-now",
            cli: &[
                &["config", "set", "notify_events", "true"],
                &["config", "set", "notify_quiet_hours", "08:00-10:00"],
            ],
            files: &[],
        },
        Setup {
            id: "status-quiet-none",
            cli: &[
                &["config", "set", "notify_quiet_hours", "none"],
                &["config", "set", "notify_daily_cap", "2"],
            ],
            files: &[],
        },
        Setup {
            id: "status-alerter",
            cli: &[&["config", "set", "notify_backend", "alerter"]],
            files: &[],
        },
        Setup {
            id: "status-terminal-notifier",
            cli: &[&["config", "set", "notify_backend", "terminal-notifier"]],
            files: &[],
        },
        Setup {
            id: "status-osascript",
            cli: &[&["config", "set", "notify_backend", "osascript"]],
            files: &[],
        },
        Setup {
            id: "status-rows",
            cli: NOTIFY_ON,
            files: &[
                ("day/notifications.json", NOTIFICATIONS),
                ("day/inbox.json", INBOX),
            ],
        },
    ];
    for setup in &homes {
        let home = prepare_setup(setup);
        assert_same(
            &home,
            &["notify", "status"],
            true,
            &format!("{NOTIFY_STATUS} {}", setup.id),
        );
    }
}

#[test]
fn notify_status_refuses_what_it_cannot_copy() {
    strict("2026-11-20T09:00:00");
    let homes = [
        Setup {
            id: "status-redaction",
            cli: NOTIFY_ON,
            files: &[("profile.yaml", "name: Synthetic Person\n")],
        },
        Setup {
            id: "status-naive-popup",
            cli: NOTIFY_ON,
            files: &[("day/notifications.json", NAIVE_POPUP)],
        },
        // Python's `int("lots")` raises: it prints its own error.
        Setup {
            id: "status-bad-cap",
            cli: &[],
            files: &[("settings.yaml", "notify_daily_cap: lots\n")],
        },
        Setup {
            id: "status-bad-file",
            cli: &[],
            files: &[("day/notifications.json", "[1, 2]\n")],
        },
    ];
    for setup in &homes {
        let home = prepare_setup(setup);
        assert!(
            !assert_same(
                &home,
                &["notify", "status"],
                false,
                &format!("{NOTIFY_STATUS} {}", setup.id)
            ),
            "{} must refuse",
            setup.id
        );
    }
}

// ---------- notify act ----------

const NOTIFY_ACT: &str = "notify.act";

const SHORT_SNOOZE: &[&[&str]] = &[
    &["config", "set", "notify_events", "true"],
    &["config", "set", "notify_snooze_minutes", "15"],
];

#[test]
fn notify_act_matches_python() {
    strict("2026-11-20T09:00:00");
    for (id, cli) in [("act-on", NOTIFY_ON), ("act-short-snooze", SHORT_SNOOZE)] {
        let home = prepare_setup(&Setup {
            id,
            cli,
            files: &[
                ("day/notifications.json", NOTIFICATIONS),
                ("day/inbox.json", INBOX),
            ],
        });
        for argv in [
            &["notify", "act", "n-0004", "read"][..],
            &["notify", "act", "n-0004", "snooze"][..],
            &["notify", "act", "n-0004", "dismiss"][..],
            &["notify", "act", "n-0001", "read"][..],
            &["notify", "act", "n-0001", "snooze"][..],
            &["notify", "act", "n-0002", "read"][..],
            &["notify", "act", "n-0002", "snooze"][..],
            &["notify", "act", "n-0005", "read"][..],
        ] {
            assert_same(&home, argv, true, &format!("{NOTIFY_ACT} {id}"));
        }
    }
}

#[test]
fn notify_act_refuses_what_it_cannot_copy() {
    strict("2026-11-20T09:00:00");
    let home = prepare_setup(&Setup {
        id: "act-refuse",
        cli: NOTIFY_ON,
        files: &[
            ("day/notifications.json", NOTIFICATIONS),
            ("day/inbox.json", INBOX),
        ],
    });
    for argv in [
        // Accept and Dismiss of a suggestion decide the inbox item.
        &["notify", "act", "n-0001", "accept"][..],
        &["notify", "act", "n-0001", "dismiss"][..],
        // Python's own errors: a closed row, an unknown id, a bad action.
        &["notify", "act", "n-0005", "dismiss"][..],
        &["notify", "act", "n-0004", "accept"][..],
        &["notify", "act", "n-9999", "read"][..],
        &["notify", "act", "n-0004", "open"][..],
    ] {
        assert!(
            !assert_same(&home, argv, false, &format!("{NOTIFY_ACT} refuse")),
            "{argv:?} must refuse"
        );
    }
    let redaction = prepare_setup(&Setup {
        id: "act-redaction",
        cli: NOTIFY_ON,
        files: &[
            ("day/notifications.json", NOTIFICATIONS),
            ("profile.yaml", "name: Synthetic Person\n"),
        ],
    });
    assert!(!assert_same(
        &redaction,
        &["notify", "act", "n-0004", "read"],
        false,
        &format!("{NOTIFY_ACT} redaction")
    ));
}

// ---------- work session list and end ----------

const SESSION_LIST: &str = "work.session.list";
const SESSION_END: &str = "work.session.end";

const OPEN_SESSION: &[&[&str]] = &[
    &["work", "project", "add", "synth-proj"],
    &["work", "session", "open", "synth-proj", "--id", "sess-1"],
];

/// Sessions of every state: one ended past `agent_keep_days`, one ended
/// within it, one from yesterday with a stale handoff, two live ones (one
/// with a fresh handoff), and claims that hold, expired, or belong to an
/// ended session.
const SESSIONS: &str = r#"{"sessions": [
 {"id": "sess-old", "project": "synth-a", "day": "2026-11-01", "started": "2026-11-01T09:00:00+00:00", "ended": "2026-11-01T17:00:00+00:00", "handoff": "old status", "handoff_link": "", "handoff_id": ""},
 {"id": "sess-done", "project": "synth-b", "day": "2026-11-18", "started": "2026-11-18T09:00:00+00:00", "ended": "2026-11-18T17:00:00+00:00", "handoff": "done status", "handoff_link": "", "handoff_id": ""},
 {"id": "sess-stale", "project": "synth-c", "day": "2026-11-19", "started": "2026-11-19T09:00:00+00:00", "ended": "", "handoff": "yesterday status", "handoff_link": "", "handoff_id": "", "wrap_requested_at": "2026-11-19T17:00:00+00:00", "handoff_at": "2026-11-19T16:00:00+00:00"},
 {"id": "sess-live", "project": "synth-d", "day": "2026-11-20", "started": "2026-11-20T08:00:00+00:00", "ended": "", "handoff": "", "handoff_link": "", "handoff_id": ""},
 {"id": "sess-fresh", "project": "synth-e", "day": "2026-11-20", "started": "2026-11-20T08:30:00+00:00", "ended": "", "handoff": "fresh status", "handoff_link": "", "handoff_id": "", "wrap_requested_at": "2026-11-20T08:40:00+00:00", "handoff_at": "2026-11-20T08:50:00+00:00"}
], "claims": [
 {"resource": "thread:t-1", "session": "sess-live", "at": "2026-11-20T08:00:00+00:00", "until": "2026-11-21T23:00:00+00:00"},
 {"resource": "thread:t-2", "session": "sess-live", "at": "2026-11-18T07:00:00+00:00", "until": "2026-11-18T07:30:00+00:00"},
 {"resource": "draft:d-1", "session": "sess-done", "at": "2026-11-18T08:00:00+00:00", "until": "2026-11-21T23:00:00+00:00"}
]}
"#;

const NAIVE_STARTED: &str = r#"{"sessions": [
 {"id": "sess-x", "project": "synth-x", "day": "2026-11-20", "started": "2026-11-20T08:00:00", "ended": ""}
], "claims": []}
"#;

const AGENT_ROW: &str = "{\"agent\": \"synth-agent\", \"at\": \"2026-11-20T08:00:00+00:00\", \"run\": \"sess-live\", \"status\": \"running\", \"title\": \"Synthetic run\"}\n";

const LIST_SESSION_FORMS: &[&[&str]] = &[
    &["work", "session", "list"],
    &["work", "session", "list", "--all"],
    &["work", "session", "list", "--json"],
    &["work", "session", "list", "--all", "--json"],
];

#[test]
fn work_session_list_matches_python() {
    strict("2026-11-20T09:00:00");
    let homes = [
        Setup {
            id: "sessions-none",
            cli: &[],
            files: &[],
        },
        Setup {
            id: "sessions-opened",
            cli: OPEN_SESSION,
            files: &[],
        },
        Setup {
            id: "sessions-every-state",
            cli: &[],
            files: &[("work/sessions.json", SESSIONS)],
        },
        Setup {
            id: "sessions-cap-and-keep",
            cli: &[
                &["config", "set", "work_session_cap", "5"],
                &["config", "set", "agent_keep_days", "30"],
            ],
            files: &[("work/sessions.json", SESSIONS)],
        },
    ];
    for setup in &homes {
        let home = prepare_setup(setup);
        for argv in LIST_SESSION_FORMS {
            assert_same(&home, argv, true, &format!("{SESSION_LIST} {}", setup.id));
        }
    }
}

#[test]
fn work_session_list_refuses_what_it_cannot_copy() {
    strict("2026-11-20T09:00:00");
    let homes = [
        // Agent activity and open questions stay with Python.
        Setup {
            id: "sessions-agent-events",
            cli: &[],
            files: &[
                ("work/sessions.json", SESSIONS),
                ("work/agents/events.jsonl", AGENT_ROW),
            ],
        },
        Setup {
            id: "sessions-work-day",
            cli: &[],
            files: &[
                ("work/sessions.json", SESSIONS),
                ("day/work-2026-11-20.json", "{\"questions\": []}\n"),
            ],
        },
        Setup {
            id: "sessions-redaction",
            cli: &[],
            files: &[
                ("work/sessions.json", SESSIONS),
                ("profile.yaml", "name: Synthetic Person\n"),
            ],
        },
        // A store Python refuses with its own error.
        Setup {
            id: "sessions-corrupt",
            cli: &[],
            files: &[("work/sessions.json", "{\"sessions\": [{}]}\n")],
        },
        Setup {
            id: "sessions-naive-started",
            cli: &[],
            files: &[("work/sessions.json", NAIVE_STARTED)],
        },
    ];
    for setup in &homes {
        let home = prepare_setup(setup);
        assert!(
            !assert_same(
                &home,
                &["work", "session", "list"],
                false,
                &format!("{SESSION_LIST} {}", setup.id)
            ),
            "{} must refuse",
            setup.id
        );
    }
}

#[test]
fn work_session_end_argument_error_matches_python() {
    strict("2026-11-20T09:00:00");
    for (id, cli) in [("end-none", &[][..]), ("end-opened", OPEN_SESSION)] {
        let home = prepare_setup(&Setup {
            id,
            cli,
            files: &[],
        });
        for argv in [
            &["work", "session", "end"][..],
            &["work", "session", "end", "--json"][..],
            &["work", "session", "end", "sess-1", "--all"][..],
            &["work", "session", "end", "sess-1", "--all", "--json"][..],
        ] {
            assert_same(&home, argv, true, &format!("{SESSION_END} {id}"));
        }
        // A real end reads and writes the session store: Python.
        for argv in [
            &["work", "session", "end", "sess-1"][..],
            &["work", "session", "end", "--all"][..],
        ] {
            assert!(
                !assert_same(&home, argv, false, &format!("{SESSION_END} {id}")),
                "{argv:?} must refuse"
            );
        }
    }
}

/// Negative controls for the wave 2 handlers: the comparison sees a real
/// difference between two homes and between two argv forms.
#[test]
fn the_comparison_detects_planted_differences_in_wave_two() {
    strict("2026-11-20T09:00:00");
    let off = prepare_setup(&Setup {
        id: "control-status-off",
        cli: &[],
        files: &[],
    });
    let on = prepare_setup(&Setup {
        id: "control-status-on",
        cli: NOTIFY_ON,
        files: &[],
    });
    let native = observe(&off, &["notify", "status"], &Side::Native);
    let python = observe(&on, &["notify", "status"], &Side::Python);
    assert_ne!(native, python);
    assert!(native.stdout.starts_with("notifications: off"));
    let rows = prepare_setup(&Setup {
        id: "control-act",
        cli: NOTIFY_ON,
        files: &[("day/notifications.json", NOTIFICATIONS)],
    });
    let read = observe(&rows, &["notify", "act", "n-0004", "read"], &Side::Native);
    let dismiss = observe(
        &rows,
        &["notify", "act", "n-0004", "dismiss"],
        &Side::Python,
    );
    assert_ne!(read, dismiss);
    assert_eq!(read.stdout, "n-0004: read\n");
    let every = prepare_setup(&Setup {
        id: "control-sessions",
        cli: &[],
        files: &[("work/sessions.json", SESSIONS)],
    });
    let open = observe(&every, &["work", "session", "list"], &Side::Native);
    let all = observe(&every, &["work", "session", "list", "--all"], &Side::Python);
    assert_ne!(open, all);
    assert!(all.stdout.contains("ended  sess-done"), "{}", all.stdout);
    assert!(!open.stdout.contains("sess-done"), "{}", open.stdout);
    assert!(!all.stdout.contains("sess-old"), "{}", all.stdout);
}
