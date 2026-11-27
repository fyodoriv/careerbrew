//! rust-19 native handlers: parity with the Python CLI in synthetic homes
//! for "jobs.choose", "jobs.unchoose", "jobs.log", "jobs.preset" and
//! "prep.story".
mod common;

use common::*;

/// One clock for every test in this binary (the variable is process-wide).
const NOW: &str = "2026-11-20T23:30:00-05:00";

const POSTINGS: (&str, &str) = (
    "jobs/postings.json",
    "{\"p1\": {\"id\": \"p1\", \"company\": \"Synthetic Co\", \"title\": \"Synthetic engineer\", \"location\": \"Remote\", \"remote\": true, \"url\": \"https://example.com/p1\", \"description\": \"Synthetic.\"}, \"p2\": {\"id\": \"p2\", \"company\": \"Example Org\", \"title\": \"Synthetic lead\", \"location\": \"Remote\", \"remote\": false, \"url\": \"https://example.com/p2\", \"description\": \"Synthetic.\", \"seniority\": 3}}",
);

const ODD_POSTINGS: (&str, &str) = (
    "jobs/postings.json",
    "{\"p1\": {\"id\": \"p1\", \"company\": \"Synthetic Co\", \"surprise\": 1}}",
);

const CHOSEN: (&str, &str) = ("jobs/chosen.json", "{\"p2\": \"target\"}");

const SOURCES: (&str, &str) = (
    "jobs/sources.yaml",
    "sources:\n- kind: hn\n- kind: wwr\n  feed: programming\n",
);

fn same(home: &Home, args: &[&str], label: &str) {
    assert_same(home, args, true, label);
}

fn refused(home: &Home, args: &[&str], label: &str) {
    assert!(!assert_same(home, args, false, label), "{label}: served");
}

/// A corporate home. `machine set corporate` refuses while job-hunt data is
/// present, so it holds none.
fn corporate(id: &'static str) -> Home {
    prepare_setup(&Setup {
        id,
        cli: &[&["machine", "set", "corporate"]],
        files: &[],
    })
}

#[test]
fn jobs_choose_matches_python() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "choose",
        cli: &[],
        files: &[POSTINGS],
    });
    same(&home, &["jobs", "choose", "p1", "dream"], "jobs.choose new");
    same(
        &home,
        &["jobs", "choose", "nope", "target"],
        "jobs.choose unknown id",
    );
    let picked = prepare_setup(&Setup {
        id: "choose-picked",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[POSTINGS, CHOSEN],
    });
    same(
        &picked,
        &["jobs", "choose", "p2", "practice"],
        "jobs.choose again",
    );
    same(
        &picked,
        &["jobs", "choose", "p1", "target"],
        "jobs.choose add",
    );
    let odd = prepare_setup(&Setup {
        id: "choose-odd",
        cli: &[],
        files: &[ODD_POSTINGS],
    });
    refused(
        &odd,
        &["jobs", "choose", "p1", "dream"],
        "jobs.choose odd posting",
    );
    refused(
        &corporate("choose-corporate"),
        &["jobs", "choose", "p1", "dream"],
        "jobs.choose corporate",
    );
}

#[test]
fn jobs_unchoose_matches_python() {
    strict(NOW);
    let picked = prepare_setup(&Setup {
        id: "unchoose",
        cli: &[],
        files: &[POSTINGS, CHOSEN],
    });
    same(&picked, &["jobs", "unchoose", "p2"], "jobs.unchoose chosen");
    same(
        &picked,
        &["jobs", "unchoose", "p1"],
        "jobs.unchoose not chosen",
    );
    let empty = prepare_setup(&Setup {
        id: "unchoose-empty",
        cli: &[],
        files: &[],
    });
    same(
        &empty,
        &["jobs", "unchoose", "p1"],
        "jobs.unchoose empty home",
    );
    refused(
        &corporate("unchoose-corporate"),
        &["jobs", "unchoose", "p2"],
        "jobs.unchoose corporate",
    );
}

#[test]
fn jobs_log_matches_python() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "log",
        cli: &[],
        files: &[],
    });
    same(
        &home,
        &["jobs", "log", "Synthetic Co", "practice", "applied"],
        "jobs.log first",
    );
    let encrypted = prepare_setup(&Setup {
        id: "log-encrypted",
        cli: &[
            &["machine", "set", "personal"],
            &["data", "encrypt"],
            &["jobs", "log", "Example Org", "target", "screen"],
        ],
        files: &[],
    });
    same(
        &encrypted,
        &["jobs", "log", "Example Org", "target", "onsite"],
        "jobs.log encrypted",
    );
    refused(
        &home,
        &["jobs", "log", "Synthetic Co", "practice", "lunch"],
        "jobs.log bad stage",
    );
    refused(
        &corporate("log-corporate"),
        &["jobs", "log", "Synthetic Co", "practice", "applied"],
        "jobs.log corporate",
    );
}

#[test]
fn jobs_preset_matches_python() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "preset-empty",
        cli: &[],
        files: &[],
    });
    same(&empty, &["jobs", "preset"], "jobs.preset empty home");
    let listed = prepare_setup(&Setup {
        id: "preset-listed",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[SOURCES],
    });
    same(&listed, &["jobs", "preset"], "jobs.preset no force");
    same(&listed, &["jobs", "preset", "--force"], "jobs.preset force");
    let crit = prepare_setup(&Setup {
        id: "preset-criteria",
        cli: &[],
        files: &[("jobs/criteria.yaml", "remote_ok: true\n")],
    });
    refused(&crit, &["jobs", "preset"], "jobs.preset criteria kept");
}

#[test]
fn prep_story_matches_python() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "story-empty",
        cli: &[],
        files: &[],
    });
    same(
        &empty,
        &[
            "prep",
            "story",
            "Shipped synthetic thing",
            "--competencies",
            "design,,delivery",
        ],
        "prep.story first",
    );
    same(
        &empty,
        &[
            "prep",
            "story",
            "Led a synthetic launch",
            "--situation",
            "A synthetic team",
            "--result",
            "It shipped",
        ],
        "prep.story fields",
    );
    let encrypted = prepare_setup(&Setup {
        id: "story-encrypted",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[],
    });
    same(
        &encrypted,
        &["prep", "story", "Synthetic story"],
        "prep.story encrypted",
    );
    refused(
        &empty,
        &["prep", "story", "Synthetic: with a colon"],
        "prep.story quoted title",
    );
    let banked = prepare_setup(&Setup {
        id: "story-banked",
        cli: &[&["prep", "story", "First synthetic story"]],
        files: &[],
    });
    refused(
        &banked,
        &["prep", "story", "Second synthetic story"],
        "prep.story second story",
    );
    refused(
        &corporate("story-corporate"),
        &["prep", "story", "Synthetic story"],
        "prep.story corporate",
    );
}

/// Negative control: choosing different tiers writes different files, so
/// the comparison sees a planted difference.
#[test]
fn the_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "control",
        cli: &[],
        files: &[POSTINGS],
    });
    let a = observe(&home, &["jobs", "choose", "p1", "dream"], &Side::Native);
    let b = observe(&home, &["jobs", "choose", "p1", "target"], &Side::Python);
    assert_eq!(a.code, Some(0));
    assert_ne!(a, b);
    assert!(a.stdout.starts_with("chosen: p1 as dream"));
}
