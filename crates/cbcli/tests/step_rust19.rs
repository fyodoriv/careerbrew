//! rust-19 native handlers: parity with the Python CLI in synthetic homes
//! for "jobs.choose", "jobs.unchoose", "jobs.log", "jobs.preset",
//! "prep.story", "interview", "interview-patterns", "research.stale" and
//! "jobs.interview-gaps".
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

/// A plan whose tiers name a sourced company twice (in two cases) and two
/// companies with no sourced loop.
const PLAN: (&str, &str) = (
    "jobs/strategy.json",
    "{\"tiers\": {\"dream\": [{\"company\": \"Stripe\", \"want\": 80.5}], \"target\": [{\"company\": \"Synthetic Co\", \"want\": 72}, {\"company\": \"stripe\"}], \"practice\": [{\"company\": \"Example Org\", \"want\": 61.0}]}}",
);

/// A plan naming a company Python casefolds differently from ASCII.
const ODD_PLAN: (&str, &str) = (
    "jobs/strategy.json",
    "{\"tiers\": {\"dream\": [{\"company\": \"Synth\\u00e9tique\", \"want\": 70}]}}",
);

#[test]
fn interview_matches_python() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "interview",
        cli: &[],
        files: &[],
    });
    same(&home, &["interview", "Stripe"], "interview known");
    same(
        &home,
        &["interview", "gitlab", "--show"],
        "interview known show",
    );
    same(
        &home,
        &["interview", "Stripe", "--show", "--no-sources"],
        "interview known no sources",
    );
    same(&home, &["interview", "Acme Co!"], "interview unknown");
    same(
        &home,
        &["interview", "Synthetic Co", "--level", "2", "--show"],
        "interview unknown junior",
    );
    let encrypted = prepare_setup(&Setup {
        id: "interview-encrypted",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[],
    });
    same(&encrypted, &["interview", "Datadog"], "interview encrypted");
    refused(&home, &["interview", ""], "interview empty name");
    refused(
        &home,
        &["interview", "Caf\u{e9}"],
        "interview non-ASCII name",
    );
    refused(
        &corporate("interview-corporate"),
        &["interview", "Stripe"],
        "interview corporate",
    );
}

#[test]
fn interview_patterns_matches_python() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "patterns",
        cli: &[],
        files: &[],
    });
    same(&home, &["interview-patterns"], "interview-patterns text");
    same(
        &home,
        &["interview-patterns", "--json"],
        "interview-patterns json",
    );
    refused(
        &corporate("patterns-corporate"),
        &["interview-patterns"],
        "interview-patterns corporate",
    );
}

#[test]
fn research_stale_matches_python() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "stale",
        cli: &[],
        files: &[],
    });
    same(&empty, &["research", "stale"], "research.stale none due");
    same(
        &empty,
        &["research", "stale", "--days", "10", "--top", "3"],
        "research.stale due",
    );
    same(
        &empty,
        &["research", "stale", "--days", "10", "--top", "0"],
        "research.stale top 0",
    );
    let planned = prepare_setup(&Setup {
        id: "stale-plan",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[PLAN],
    });
    same(
        &planned,
        &["research", "stale", "--days", "30"],
        "research.stale pipeline first",
    );
    let odd = prepare_setup(&Setup {
        id: "stale-odd",
        cli: &[],
        files: &[ODD_PLAN],
    });
    refused(
        &odd,
        &["research", "stale", "--days", "10"],
        "research.stale non-ASCII pipeline",
    );
}

#[test]
fn jobs_interview_gaps_matches_python() {
    strict(NOW);
    let planned = prepare_setup(&Setup {
        id: "gaps",
        cli: &[],
        files: &[PLAN],
    });
    same(
        &planned,
        &["jobs", "interview-gaps"],
        "jobs.interview-gaps plan",
    );
    let encrypted = prepare_setup(&Setup {
        id: "gaps-encrypted",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[PLAN],
    });
    same(
        &encrypted,
        &["jobs", "interview-gaps", "--top", "1"],
        "jobs.interview-gaps encrypted",
    );
    let empty = prepare_setup(&Setup {
        id: "gaps-empty",
        cli: &[],
        files: &[],
    });
    refused(
        &empty,
        &["jobs", "interview-gaps"],
        "jobs.interview-gaps scored fallback",
    );
    let odd = prepare_setup(&Setup {
        id: "gaps-odd",
        cli: &[],
        files: &[ODD_PLAN],
    });
    refused(
        &odd,
        &["jobs", "interview-gaps"],
        "jobs.interview-gaps non-ASCII company",
    );
    refused(
        &corporate("gaps-corporate"),
        &["jobs", "interview-gaps"],
        "jobs.interview-gaps corporate",
    );
}

/// Negative control: two companies print different guides, so the
/// comparison sees a planted difference.
#[test]
fn the_interview_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "interview-control",
        cli: &[],
        files: &[],
    });
    let a = observe(&home, &["interview", "Stripe", "--show"], &Side::Native);
    let b = observe(&home, &["interview", "Google", "--show"], &Side::Python);
    assert_eq!(a.code, Some(0));
    assert_ne!(a, b);
    assert!(a.stdout.starts_with("# Interview loop: Stripe\n"));
}
