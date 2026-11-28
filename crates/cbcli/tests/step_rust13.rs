//! rust-13 native handlers: "work.feed.rules", "work.feed.show",
//! "work.feed.links", "work.feed.open" and "work.team.list" (and the wave 4
//! handlers below) match Python in
//! synthetic homes (stdout, exit code, data-root files, event rows).
mod common;

use common::*;

/// One clock for every test in this binary (the variable is per process).
const NOW: &str = "2026-11-20T09:00:00+00:00";

fn empty(id: &'static str) -> Home {
    prepare_setup(&Setup {
        id,
        cli: &[],
        files: &[],
    })
}

#[test]
fn feed_rules_show_links_and_open_match_python() {
    strict(NOW);
    let home = empty("feed-empty");
    let served: [(&str, &[&str]); 6] = [
        ("work.feed.rules", &["work", "feed", "rules"]),
        ("work.feed.show", &["work", "feed", "show"]),
        ("work.feed.show", &["work", "feed", "show", "--top", "3"]),
        ("work.feed.links", &["work", "feed", "links"]),
        ("work.feed.links", &["work", "feed", "links", "--pending"]),
        ("work.feed.open", &["work", "feed", "open", "ev-1"]),
    ];
    for (label, args) in served {
        assert_same(&home, args, true, label);
    }
}

#[test]
fn feed_commands_refuse_what_they_cannot_repeat() {
    strict(NOW);
    // The owner's own rules file: Python prints them first.
    let rules = prepare_setup(&Setup {
        id: "feed-rules-own",
        cli: &[],
        files: &[(
            "work/feed-rules.yaml",
            "rules: [{name: alerts, when: {source: slack}, triage: noise}]\n",
        )],
    });
    assert!(!assert_same(
        &rules,
        &["work", "feed", "rules"],
        false,
        "work.feed.rules own rules"
    ));
    // A stored events file: Python loads and ranks it.
    let events = prepare_setup(&Setup {
        id: "feed-events",
        cli: &[],
        files: &[("work/feed/events.jsonl", "")],
    });
    for (label, args) in [
        ("work.feed.show", ["work", "feed", "show"]),
        ("work.feed.links", ["work", "feed", "links"]),
    ] {
        assert!(!assert_same(&events, &args, false, label));
    }
    // Python prints through `privacy.redact`, which reads the profile.
    let profile = prepare_setup(&Setup {
        id: "feed-profile",
        cli: &[],
        files: &[("profile.yaml", "name: Synthetic Person\n")],
    });
    assert!(!assert_same(
        &profile,
        &["work", "feed", "rules"],
        false,
        "work.feed.rules profile"
    ));
    // A plugin lock: a plugin policy may allow business reading.
    let plugin = prepare_setup(&Setup {
        id: "feed-plugin",
        cli: &[],
        files: &[("plugins.lock.json", "{}\n")],
    });
    assert!(!assert_same(
        &plugin,
        &["work", "feed", "open", "ev-1"],
        false,
        "work.feed.open plugin lock"
    ));
}

const ADD_SAM: &[&str] = &["work", "team", "add", "Sam Lee", "--chat", "U0SAM"];
const ADD_ALEX: &[&str] = &[
    "work",
    "team",
    "add",
    "Alex Kim",
    "--email",
    "alex@example.com",
    "--login",
    "alexk",
];
const ADD_PAT: &[&str] = &["work", "team", "add", "Pat Doe"];

#[test]
fn team_list_matches_python() {
    strict(NOW);
    let none = empty("team-empty");
    assert_same(
        &none,
        &["work", "team", "list"],
        true,
        "work.team.list empty",
    );
    assert_same(
        &none,
        &["work", "team", "list", "--json"],
        true,
        "work.team.list empty json",
    );
    let roster = prepare_setup(&Setup {
        id: "team-roster",
        cli: &[ADD_SAM, ADD_ALEX, ADD_PAT],
        files: &[],
    });
    assert_same(
        &roster,
        &["work", "team", "list"],
        true,
        "work.team.list roster",
    );
    assert_same(
        &roster,
        &["work", "team", "list", "--json"],
        true,
        "work.team.list roster json",
    );
}

#[test]
fn team_list_refuses_what_it_cannot_repeat() {
    strict(NOW);
    // A number field: Python's `str(value)` reading is not repeated.
    let odd = prepare_setup(&Setup {
        id: "team-number",
        cli: &[ADD_SAM],
        files: &[],
    });
    let raw = prepare_setup(&Setup {
        id: "team-plain-number",
        cli: &[],
        files: &[("work/team.yaml", "team:\n  - {name: Sam Lee, chat: 1234}\n")],
    });
    assert!(!assert_same(
        &raw,
        &["work", "team", "list"],
        false,
        "work.team.list number field"
    ));
    let plugin = prepare_setup(&Setup {
        id: "team-plugin",
        cli: &[ADD_SAM],
        // A valid empty lock: Python's `work team add` reads `plugins`.
        files: &[("plugins.lock.json", "{\"plugins\": {}}\n")],
    });
    assert!(!assert_same(
        &plugin,
        &["work", "team", "list"],
        false,
        "work.team.list plugin lock"
    ));
    // Still served with one teammate (the control for the refusals above).
    assert_same(&odd, &["work", "team", "list"], true, "work.team.list one");
}

/// Negative control: the comparison sees a difference when one exists.
#[test]
fn a_different_home_gives_a_different_observation() {
    strict(NOW);
    let none = empty("control-empty");
    let roster = prepare_setup(&Setup {
        id: "control-roster",
        cli: &[ADD_SAM],
        files: &[],
    });
    let args = ["work", "team", "list"];
    let got = observe(&none, &args, &Side::Native);
    let want = observe(&roster, &args, &Side::Python);
    assert_ne!(got.stdout, want.stdout);
}

// ---------- wave 4: "work.team.add", "work.team.remove", "work.team.feed",
// "work.feed.retriage", "work.feed.mark" and "work.feed.research" ----------

/// The hosted Linux runner seals a home without this marker: every write
/// case gets it.
const PLAIN: (&str, &str) = (".plaintext", "");

fn plain(id: &'static str, cli: &'static [&'static [&'static str]]) -> Home {
    prepare_setup(&Setup {
        id,
        cli,
        files: &[PLAIN],
    })
}

#[test]
fn team_add_matches_python() {
    strict(NOW);
    let fresh = plain("add-fresh", &[]);
    for args in [
        &[
            "work",
            "team",
            "add",
            "Sam Lee",
            "--chat",
            "U0SAM",
            "--email",
            "Sam@Example.com",
            "--login",
            "@samlee",
        ][..],
        &["work", "team", "add", "  Bo   Rae "],
        &["work", "team", "add", "Pat", "--chat", "W0PAT9"],
    ] {
        assert_same(&fresh, args, true, "work.team.add new");
    }
    let roster = plain("add-roster", &[ADD_SAM, ADD_PAT]);
    for args in [
        // The same ids: no change, no retriage.
        &["work", "team", "add", "sam lee", "--chat", "U0SAM"][..],
        // New ids for a known name.
        &["work", "team", "add", "SAM LEE", "--login", "samlee"],
        &[
            "work",
            "team",
            "add",
            "Alex Kim",
            "--email",
            "alex@example.com",
        ],
    ] {
        assert_same(&roster, args, true, "work.team.add roster");
    }
    // `TeamError` lines, before any lock is taken.
    let long = "N".repeat(81);
    let chat = format!("U{}", "A".repeat(120));
    for args in [
        &["work", "team", "add", ""][..],
        &["work", "team", "add", "   "],
        &["work", "team", "add", long.as_str()],
        &["work", "team", "add", "X", "--chat", "bad"],
        &["work", "team", "add", "X", "--chat", chat.as_str()],
        &["work", "team", "add", "X", "--email", "Nope.Example"],
    ] {
        assert_same(&fresh, args, true, "work.team.add error");
    }
    // Errors are served in a sealed home too: they write nothing sealed.
    let sealed = empty("add-sealed");
    assert_same(
        &sealed,
        &["work", "team", "add", "X", "--chat", "bad"],
        true,
        "work.team.add sealed error",
    );
    // An unchanged teammate in a sealed home: no write.
    let sealed_roster = prepare_setup(&Setup {
        id: "add-sealed-roster",
        cli: &[ADD_SAM],
        files: &[],
    });
    assert_same(
        &sealed_roster,
        &["work", "team", "add", "Sam Lee", "--chat", "U0SAM"],
        true,
        "work.team.add sealed no change",
    );
}

#[test]
fn team_add_refuses_what_it_cannot_repeat() {
    strict(NOW);
    // A sealed write differs on every run.
    let sealed = empty("add-refuse-sealed");
    assert!(!assert_same(
        &sealed,
        &["work", "team", "add", "Sam Lee"],
        false,
        "work.team.add sealed write"
    ));
    // Feed events: the retriage after a change reads them.
    let events = prepare_setup(&Setup {
        id: "add-refuse-events",
        cli: &[],
        files: &[PLAIN, ("work/feed/events.jsonl", "")],
    });
    assert!(!assert_same(
        &events,
        &["work", "team", "add", "Sam Lee"],
        false,
        "work.team.add feed events"
    ));
    // A quote in the email: its repr is not plain single quotes.
    let fresh = plain("add-refuse-quote", &[]);
    assert!(!assert_same(
        &fresh,
        &["work", "team", "add", "X", "--email", "o'x"],
        false,
        "work.team.add quoted email"
    ));
    // A number in the roster: Python's `str(value)` is not repeated.
    let number = prepare_setup(&Setup {
        id: "add-refuse-number",
        cli: &[],
        files: &[
            PLAIN,
            ("work/team.yaml", "team:\n  - {name: Sam Lee, chat: 1234}\n"),
        ],
    });
    assert!(!assert_same(
        &number,
        &["work", "team", "add", "Pat"],
        false,
        "work.team.add number field"
    ));
}

#[test]
fn team_remove_matches_python() {
    strict(NOW);
    let roster = plain("remove-roster", &[ADD_SAM, ADD_ALEX, ADD_PAT]);
    for args in [
        &["work", "team", "remove", "sam   LEE"][..],
        &["work", "team", "remove", "Pat Doe"],
        &["work", "team", "remove", "Nobody"],
    ] {
        assert_same(&roster, args, true, "work.team.remove roster");
    }
    let fresh = empty("remove-empty");
    assert_same(
        &fresh,
        &["work", "team", "remove", "Sam"],
        true,
        "work.team.remove empty",
    );
    // A sealed roster: the error reads it with the key and writes nothing.
    let sealed = prepare_setup(&Setup {
        id: "remove-sealed",
        cli: &[ADD_SAM],
        files: &[],
    });
    assert_same(
        &sealed,
        &["work", "team", "remove", "Nobody"],
        true,
        "work.team.remove sealed error",
    );
    assert!(!assert_same(
        &sealed,
        &["work", "team", "remove", "Sam Lee"],
        false,
        "work.team.remove sealed write"
    ));
}

#[test]
fn team_feed_and_feed_retriage_match_python() {
    strict(NOW);
    let fresh = empty("team-feed-empty");
    for args in [
        &["work", "team", "feed"][..],
        &["work", "team", "feed", "--days", "3"],
        &["work", "team", "feed", "--days", "0", "--top", "2"],
    ] {
        assert_same(&fresh, args, true, "work.team.feed empty");
    }
    assert_same(
        &fresh,
        &["work", "feed", "retriage"],
        true,
        "work.feed.retriage empty",
    );
    let roster = plain("retriage-roster", &[ADD_SAM, ADD_PAT]);
    assert_same(
        &roster,
        &["work", "feed", "retriage"],
        true,
        "work.feed.retriage roster",
    );
    // Refusals: a settings file, stored events, the owner's rules.
    let settings = prepare_setup(&Setup {
        id: "team-feed-settings",
        cli: &[],
        files: &[("settings.yaml", "project_priority: [alpha]\n")],
    });
    assert!(!assert_same(
        &settings,
        &["work", "team", "feed"],
        false,
        "work.team.feed settings"
    ));
    let events = prepare_setup(&Setup {
        id: "team-feed-events",
        cli: &[],
        files: &[("work/feed/events.jsonl", "")],
    });
    for (label, args) in [
        ("work.team.feed events", ["work", "team", "feed"]),
        ("work.feed.retriage events", ["work", "feed", "retriage"]),
    ] {
        assert!(!assert_same(&events, &args, false, label));
    }
    let rules = prepare_setup(&Setup {
        id: "retriage-rules",
        cli: &[],
        files: &[(
            "work/feed-rules.yaml",
            "rules: [{name: alerts, when: {source: slack}, triage: noise}]\n",
        )],
    });
    assert!(!assert_same(
        &rules,
        &["work", "feed", "retriage"],
        false,
        "work.feed.retriage rules"
    ));
}

#[test]
fn feed_mark_and_research_match_python() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "mark-research",
        cli: &[],
        files: &[
            (
                "in/list.json",
                "[{\"url\": \"https://example.com/a\", \"summary\": \"A page.\"}]\n",
            ),
            ("in/object.json", "{\"url\": \"https://example.com/a\"}\n"),
            ("in/empty.json", "[]\n"),
            ("in/text.json", "not json\n"),
        ],
    });
    for value in ["done", "not-ask", "ask", "personal", "work"] {
        assert_same(
            &home,
            &["work", "feed", "mark", "ev-1", value],
            true,
            "work.feed.mark no events",
        );
    }
    let file = |name: &str| home.data.join("in").join(name).display().to_string();
    for name in ["list.json", "object.json", "empty.json"] {
        let path = file(name);
        assert_same(
            &home,
            &["work", "feed", "research", "ev-1", path.as_str()],
            true,
            "work.feed.research no events",
        );
    }
    // Python prints its own "not JSON" line, and its own error for a
    // missing file.
    let text = file("text.json");
    let missing = file("missing.json");
    for path in [&text, &missing] {
        assert!(!assert_same(
            &home,
            &["work", "feed", "research", "ev-1", path.as_str()],
            false,
            "work.feed.research refused"
        ));
    }
    let events = prepare_setup(&Setup {
        id: "mark-events",
        cli: &[],
        files: &[("work/feed/events.jsonl", "")],
    });
    assert!(!assert_same(
        &events,
        &["work", "feed", "mark", "ev-1", "done"],
        false,
        "work.feed.mark events"
    ));
}

/// Negative control for the wave 4 handlers: a home with a teammate and one
/// without give different observations, natively and in Python.
#[test]
fn a_different_roster_gives_a_different_removal() {
    strict(NOW);
    let none = plain("control-remove-empty", &[]);
    let roster = plain("control-remove-roster", &[ADD_SAM]);
    let args = ["work", "team", "remove", "Sam Lee"];
    let got = observe(&none, &args, &Side::Native);
    let want = observe(&roster, &args, &Side::Python);
    assert_ne!(got, want);
}

// ---------- wave 7: "work.feed.add", "work.feed.transcript" and
// "work.feed.cycle" (their FeedError lines) ----------

/// Writes files under the home's HOME (outside the data root), then takes
/// the snapshot again so that every run sees them.
fn with_files(home: &Home, files: &[(&str, &str)]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, text) in files {
        let path = home.home.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        out.push(path.to_str().unwrap().to_owned());
    }
    let live = home.home.parent().unwrap().to_path_buf();
    let snap = live.with_file_name("snap");
    std::fs::remove_dir_all(&snap).unwrap();
    copy_tree(&live, &snap);
    out
}

const TOOLS: &[&str] = &[
    "config",
    "set",
    "feed_tool_sources",
    "mcp__chat__*=chat, mcp__tickets__*=tickets",
];

#[test]
fn feed_add_errors_match_python() {
    strict(NOW);
    let home = empty("feed-add");
    let files = with_files(
        &home,
        &[
            ("in/empty.jsonl", ""),
            ("in/blank.jsonl", "\n  \n\t\n"),
            ("in/words.jsonl", "\n \nhello there\n{\"id\": 1}\n"),
            ("in/first.jsonl", "<not json>"),
        ],
    );
    for file in &files {
        assert_same(&home, &["work", "feed", "add", file], true, "work.feed.add");
    }
    assert_same(
        &home,
        &["work", "feed", "add", &files[0], "--source", ""],
        true,
        "work.feed.add empty source",
    );
}

#[test]
fn feed_transcript_errors_match_python() {
    strict(NOW);
    let home = empty("feed-transcript");
    for args in [
        &["work", "feed", "transcript", "/nonexistent/session.jsonl"][..],
        &["work", "feed", "transcript", "x.jsonl", "--since", "30m"],
    ] {
        assert_same(&home, args, true, "work.feed.transcript no tool map");
    }
    let mapped = plain("feed-transcript-mapped", &[TOOLS]);
    assert_same(
        &mapped,
        &["work", "feed", "transcript", "/nonexistent/session.jsonl"],
        true,
        "work.feed.transcript",
    );
}

#[test]
fn feed_cycle_errors_match_python() {
    strict(NOW);
    let home = empty("feed-cycle");
    for args in [
        &["work", "feed", "cycle"][..],
        &["work", "feed", "cycle", "--json"],
        &["work", "feed", "cycle", "--require", " , ,"],
        &["work", "feed", "cycle", "--require", ""],
    ] {
        assert_same(&home, args, true, "work.feed.cycle");
    }
}

#[test]
fn feed_add_transcript_and_cycle_refuse_what_they_cannot_repeat() {
    strict(NOW);
    let home = plain("feed-refuse", &[]);
    let files = with_files(
        &home,
        &[
            (
                "in/event.jsonl",
                "{\"id\": \"e1\", \"at\": \"2026-11-20T08:00:00Z\", \"title\": \"Hi\"}\n",
            ),
            ("in/raw.json", "{\"tickets\": []}"),
            ("in/number.jsonl", "12 apples\n"),
            ("in/session.jsonl", "{}\n"),
        ],
    );
    let refused: [(&str, &[&str]); 7] = [
        ("work.feed.add event", &["work", "feed", "add", &files[0]]),
        (
            "work.feed.add raw reply",
            &["work", "feed", "add", &files[1]],
        ),
        (
            "work.feed.add number start",
            &["work", "feed", "add", &files[2]],
        ),
        (
            "work.feed.add source",
            &["work", "feed", "add", &files[2], "--source", "chat"],
        ),
        (
            "work.feed.add plugin",
            &["work", "feed", "add", &files[2], "--plugin", "acme"],
        ),
        (
            "work.feed.add missing",
            &["work", "feed", "add", "/nonexistent/x.jsonl"],
        ),
        (
            "work.feed.cycle require",
            &["work", "feed", "cycle", "--require", "chat"],
        ),
    ];
    for (label, args) in refused {
        assert!(!assert_same(&home, args, false, label));
    }
    // A tool map and a transcript that exists: Python reads it.
    let mapped = plain("feed-refuse-mapped", &[TOOLS]);
    let session = with_files(&mapped, &[("in/session.jsonl", "{}\n")]);
    for (label, args) in [
        (
            "work.feed.transcript file",
            &["work", "feed", "transcript", &session[0]][..],
        ),
        ("work.feed.cycle tools", &["work", "feed", "cycle"]),
    ] {
        assert!(!assert_same(&mapped, args, false, label));
    }
    // Owner live sources: Python reads and checks them.
    let live = prepare_setup(&Setup {
        id: "feed-refuse-live",
        cli: &[],
        files: &[PLAIN, ("work/live-sources.yaml", "sources: []\n")],
    });
    assert!(!assert_same(
        &live,
        &["work", "feed", "cycle"],
        false,
        "work.feed.cycle live sources"
    ));
    // Python prints through `privacy.redact`, which reads the profile.
    let redact = prepare_setup(&Setup {
        id: "feed-refuse-redact",
        cli: &[],
        files: &[("profile.yaml", "name: Synthetic Person\n")],
    });
    assert!(!assert_same(
        &redact,
        &["work", "feed", "cycle"],
        false,
        "work.feed.cycle redact"
    ));
}

/// Negative control: an input with no events and one whose first line is
/// not JSON print different errors.
#[test]
fn a_not_json_line_gives_a_different_add_error() {
    strict(NOW);
    let home = empty("feed-add-control");
    let files = with_files(
        &home,
        &[("in/empty.jsonl", ""), ("in/words.jsonl", "hello\n")],
    );
    let got = observe(&home, &["work", "feed", "add", &files[0]], &Side::Native);
    let want = observe(&home, &["work", "feed", "add", &files[1]], &Side::Python);
    assert_ne!(got, want);
}

// ---------- wave 8: day inbox ----------

/// A stored inbox with decided items, one expired plugin ask, and an open
/// question that no fresh candidate keeps.
const INBOX_KEPT: &str = r#"{"items": [
 {"id": "plugin-aaaa1111", "type": "proposal", "kind": "plugin", "status": "dismissed", "origin": "plugin:demo", "data": {"plugin": "demo", "ref": "r1"}, "created_at": "2026-11-18T09:00:00+00:00", "decided_at": "2026-11-18T10:00:00+00:00"},
 {"id": "plugin-bbbb2222", "type": "proposal", "kind": "plugin", "status": "expired", "data": {"plugin": "demo", "ref": "r2"}, "expired": {"what": "PR merged", "url": "", "at": "2026-11-19T09:00:00+00:00"}, "created_at": "2026-11-18T09:00:00+00:00"},
 {"id": "impact-cccc3333", "type": "question", "kind": "impact", "status": "open", "data": {"title": "Synthetic work", "source": "pr"}, "created_at": "2026-11-18T09:00:00+00:00"}
]}
"#;

#[test]
fn day_inbox_matches_python() {
    strict(NOW);
    let fresh = plain("inbox-fresh", &[]);
    assert_same(&fresh, &["day", "inbox"], true, "day.inbox");
    assert_same(
        &fresh,
        &["day", "inbox", "--group", "grp-0000000000"],
        true,
        "day.inbox group",
    );
    // A second run: the stored open goal gets the fresh words.
    let again = plain("inbox-again", &[&["day", "inbox"]]);
    assert_same(&again, &["day", "inbox"], true, "day.inbox again");
    // The goal dismissed: nothing open, the decision kept.
    let decided = plain(
        "inbox-decided",
        &[&["day", "inbox"], &["day", "dismiss", "goal-c0a5abd6"]],
    );
    assert_same(&decided, &["day", "inbox"], true, "day.inbox decided");
    let kept = prepare_setup(&Setup {
        id: "inbox-kept",
        cli: &[],
        files: &[PLAIN, ("day/inbox.json", INBOX_KEPT)],
    });
    assert_same(&kept, &["day", "inbox"], true, "day.inbox kept");
    assert_same(
        &kept,
        &["day", "inbox", "--group", "grp-x"],
        true,
        "day.inbox kept group",
    );
    let staff = prepare_setup(&Setup {
        id: "inbox-staff",
        cli: &[],
        files: &[PLAIN, ("settings.yaml", "target_level: staff\n")],
    });
    assert_same(&staff, &["day", "inbox"], true, "day.inbox staff");
}

#[test]
fn day_inbox_refuses_what_it_cannot_repeat() {
    strict(NOW);
    let grouped = r#"{"items": [{"id": "goal-c0a5abd6", "type": "proposal", "kind": "goal", "status": "open", "group": "grp-1", "data": {}}]}"#;
    let cases: [(&'static str, Vec<(&'static str, &'static str)>); 6] = [
        // An encrypted data folder: the written file differs on every run.
        ("inbox-sealed", vec![]),
        // An open task: task and competency candidates stay Python.
        (
            "inbox-task",
            vec![
                PLAIN,
                (
                    "work/tasks.yaml",
                    "tasks:\n  - {id: t1, title: Synthetic task}\n",
                ),
            ],
        ),
        // A saved goal: `goals.load` stays Python.
        ("inbox-goals", vec![PLAIN, ("goals.yaml", "queue: []\n")]),
        // A setting `config.get` reads.
        (
            "inbox-config",
            vec![PLAIN, ("settings.yaml", "inbox_expire_done: false\n")],
        ),
        // An unknown level raises `LadderError` in Python.
        (
            "inbox-level",
            vec![PLAIN, ("settings.yaml", "target_level: nobody\n")],
        ),
        // A stored open goal that carries a group.
        ("inbox-grouped", vec![PLAIN, ("day/inbox.json", grouped)]),
    ];
    for (id, files) in cases {
        let files: &'static [(&'static str, &'static str)] = Box::leak(files.into_boxed_slice());
        let home = prepare_setup(&Setup {
            id,
            cli: &[],
            files,
        });
        assert!(
            !assert_same(&home, &["day", "inbox"], false, id),
            "{id} served"
        );
    }
}

/// Negative control: a run on a stored inbox prints another listing than a
/// run on a fresh one.
#[test]
fn a_kept_inbox_lists_differently() {
    strict(NOW);
    let fresh = plain("inbox-control-fresh", &[]);
    let kept = prepare_setup(&Setup {
        id: "inbox-control-kept",
        cli: &[],
        files: &[PLAIN, ("day/inbox.json", INBOX_KEPT)],
    });
    let a = observe(&fresh, &["day", "inbox"], &Side::Native);
    let b = observe(&kept, &["day", "inbox"], &Side::Native);
    assert_ne!(a.stdout, b.stdout);
}
