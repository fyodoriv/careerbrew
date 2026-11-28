//! rust-13 native handlers: "work.feed.rules", "work.feed.show",
//! "work.feed.links", "work.feed.open" and "work.team.list" match Python in
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
        files: &[("plugins.lock.json", "{}\n")],
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
