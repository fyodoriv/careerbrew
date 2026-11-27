//! rust-18 native handlers: parity with the Python CLI in synthetic homes
//! for "work.note", "work.gaps", "ladder.list" and "ladder.levels".
mod common;

use common::*;

/// One clock for every test in this binary (the variable is process-wide).
const NOW: &str = "2026-11-20T09:00:00+00:00";

const ZONE: (&str, &str) = ("settings.yaml", "timezone: Etc/UTC\n");

const USER_LADDER: &str = "id: synthetic-ladder\nname: Synthetic ladder\nlevels: [one, two]\ncompetencies:\n  - id: craft\n    name: Synthetic craft\n    weight: 2\n    signals: {tags: [delivery], keywords: [ship*]}\n    expect: {one: 1, two: 3}\n    how_to_close: Ship one synthetic thing.\n    descriptions:\n      two: Ships synthetic things — often.\n    examples:\n      two:\n        looks_like: [a synthetic launch]\n        doesnt_look_like: []\n";

const BAD_LADDER: &str = "id: broken-ladder\nlevels: [one]\n";

fn same(home: &Home, args: &[&str], label: &str) {
    assert_same(home, args, true, label);
}

fn refused(home: &Home, args: &[&str], label: &str) {
    assert!(!assert_same(home, args, false, label), "{label}: served");
}

#[test]
fn work_note_matches_python() {
    strict(NOW);
    let plain = prepare_setup(&Setup {
        id: "note-plain",
        cli: &[],
        files: &[ZONE],
    });
    same(
        &plain,
        &[
            "work",
            "note",
            "Synthetic design review",
            "--tags",
            "delivery,design",
            "--impact",
            "cut p99",
        ],
        "work.note tags and impact",
    );
    same(
        &plain,
        &["work", "note", "feat: synthetic thing"],
        "work.note auto tags",
    );
    same(
        &plain,
        &["work", "note", "Synthetic", "--tags", "a,,b"],
        "work.note empty tag part",
    );
    let encrypted = prepare_setup(&Setup {
        id: "note-encrypted",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[ZONE],
    });
    same(
        &encrypted,
        &["work", "note", "Synthetic incident fix"],
        "work.note encrypted",
    );
    let redact = prepare_setup(&Setup {
        id: "note-redact",
        cli: &[],
        files: &[ZONE, ("sensitive-terms.txt", "synthetic\n")],
    });
    refused(
        &redact,
        &["work", "note", "Synthetic secret"],
        "work.note redaction input",
    );
}

#[test]
fn work_gaps_matches_python() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "gaps-empty",
        cli: &[],
        files: &[ZONE],
    });
    same(&empty, &["work", "gaps"], "work.gaps empty home");
    same(
        &empty,
        &["work", "gaps", "--level", "staff"],
        "work.gaps another level",
    );
    let noted = prepare_setup(&Setup {
        id: "gaps-noted",
        cli: &[
            &["work", "note", "feat: synthetic launch"],
            &["work", "note", "Synthetic design rfc", "--tags", "design"],
            &["work", "note", "Mentor a synthetic peer"],
        ],
        files: &[ZONE],
    });
    same(&noted, &["work", "gaps"], "work.gaps with evidence");
    let user = prepare_setup(&Setup {
        id: "gaps-user-ladder",
        cli: &[&["work", "note", "Ship synthetic thing"]],
        files: &[
            (
                "settings.yaml",
                "timezone: Etc/UTC\nladder: synthetic-ladder\ntarget_level: two\n",
            ),
            ("ladders/synthetic-ladder.yaml", USER_LADDER),
        ],
    });
    same(&user, &["work", "gaps"], "work.gaps settings ladder");
    refused(
        &empty,
        &["work", "gaps", "--ladder", "no-such-ladder"],
        "work.gaps unknown ladder",
    );
}

#[test]
fn ladder_list_matches_python() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "list-empty",
        cli: &[],
        files: &[],
    });
    same(&empty, &["ladder", "list"], "ladder.list base");
    let user = prepare_setup(&Setup {
        id: "list-user",
        cli: &[],
        files: &[("ladders/synthetic-ladder.yaml", USER_LADDER)],
    });
    same(&user, &["ladder", "list"], "ladder.list user ladder");
    let bad = prepare_setup(&Setup {
        id: "list-bad",
        cli: &[],
        files: &[("ladders/broken-ladder.yaml", BAD_LADDER)],
    });
    refused(&bad, &["ladder", "list"], "ladder.list invalid ladder");
}

#[test]
fn ladder_levels_matches_python() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "levels-empty",
        cli: &[],
        files: &[],
    });
    same(&empty, &["ladder", "levels"], "ladder.levels default");
    same(
        &empty,
        &["ladder", "levels", "--json"],
        "ladder.levels json",
    );
    same(
        &empty,
        &[
            "ladder",
            "levels",
            "--ladder",
            "generic-swe",
            "--level",
            "staff",
        ],
        "ladder.levels staff",
    );
    same(
        &empty,
        &["ladder", "levels", "--ladder", "no-such-ladder"],
        "ladder.levels unknown ladder",
    );
    same(
        &empty,
        &["ladder", "levels", "--level", "principal"],
        "ladder.levels unknown level",
    );
    let user = prepare_setup(&Setup {
        id: "levels-user",
        cli: &[],
        files: &[
            (
                "settings.yaml",
                "ladder: synthetic-ladder\ntarget_level: two\n",
            ),
            ("ladders/synthetic-ladder.yaml", USER_LADDER),
        ],
    });
    same(&user, &["ladder", "levels"], "ladder.levels user ladder");
    same(
        &user,
        &["ladder", "levels", "--json"],
        "ladder.levels user json",
    );
    let odd = prepare_setup(&Setup {
        id: "levels-odd-setting",
        cli: &[],
        files: &[("settings.yaml", "ladder: 5\n")],
    });
    refused(
        &odd,
        &["ladder", "levels"],
        "ladder.levels non-text setting",
    );
}

/// Negative control: two homes that differ give different results, so the
/// comparison can see a planted difference.
#[test]
fn the_comparison_detects_a_planted_difference() {
    strict(NOW);
    let senior = prepare_setup(&Setup {
        id: "control-senior",
        cli: &[],
        files: &[],
    });
    let staff = prepare_setup(&Setup {
        id: "control-staff",
        cli: &[],
        files: &[("settings.yaml", "target_level: staff\n")],
    });
    let args = ["ladder", "levels", "--json"];
    let a = observe(&senior, &args, &Side::Native);
    let b = observe(&staff, &args, &Side::Python);
    assert_eq!(a.code, Some(0));
    assert_ne!(a, b);
    assert!(a.stdout.contains("\"level\": \"senior\""));
    assert!(b.stdout.contains("\"level\": \"staff\""));
}
