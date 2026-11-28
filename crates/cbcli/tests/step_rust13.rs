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
