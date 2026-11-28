//! rust-24 native handlers: parity with the Python CLI in synthetic homes
//! for "time.baseline", "checklist", "work.project.list",
//! "work.project.rules", "work.project.rules-set", "work.project.remove",
//! "work.project.archive", "work.project.unarchive", "work.project.quiz",
//! "work.project.gate", "work.project.update", "work.project.source",
//! "work.project.check" and "work.project.prd".
mod common;

use common::*;

/// One clock for every test in this binary (the variable is process-wide).
const NOW: &str = "2026-11-20T09:00:00";

const ZONE: (&str, &str) = ("settings.yaml", "timezone: Etc/UTC\n");

/// Two plain projects (one with standing rules) and one prep project.
const PROJECTS: &[&[&str]] = &[
    &["work", "project", "add", "alpha"],
    &[
        "work",
        "project",
        "add",
        "beta",
        "--title",
        "Synthetic beta",
    ],
    &["work", "project", "add", "prepper", "--kind", "prep"],
    &[
        "work",
        "project",
        "rules-set",
        "alpha",
        "--text",
        "Synthetic standing rule",
        "--yes",
    ],
];

fn same(home: &Home, args: &[&str], label: &str) {
    assert_same(home, args, true, label);
}

fn refused(home: &Home, args: &[&str], label: &str) {
    assert!(!assert_same(home, args, false, label), "{label}: served");
}

fn empty_home(id: &'static str) -> Home {
    prepare_setup(&Setup {
        id,
        cli: &[],
        files: &[ZONE],
    })
}

fn projects_home(id: &'static str) -> Home {
    prepare_setup(&Setup {
        id,
        cli: PROJECTS,
        files: &[ZONE],
    })
}

fn redact_home(id: &'static str) -> Home {
    prepare_setup(&Setup {
        id,
        cli: PROJECTS,
        files: &[ZONE, ("sensitive-terms.txt", "synthetic\n")],
    })
}

#[test]
fn time_baseline_matches_python() {
    strict(NOW);
    // No timezone setting here: `Etc/UTC` is outside the YAML subset the
    // settings writer serves, so such a home goes to Python.
    let home = prepare_setup(&Setup {
        id: "time-plain",
        cli: &[],
        files: &[],
    });
    for (args, label) in [
        (
            ["time", "baseline", "pm_update", "12"],
            "time.baseline measured",
        ),
        (
            ["time", "baseline", "standup", "0"],
            "time.baseline back to estimate",
        ),
        (
            ["time", "baseline", "inbox", "abc"],
            "time.baseline not a number",
        ),
        (
            ["time", "baseline", "day_log", "601"],
            "time.baseline out of range",
        ),
        (
            ["time", "baseline", "evidence", "7.5"],
            "time.baseline fraction",
        ),
    ] {
        same(&home, &args, label);
    }
    let set = prepare_setup(&Setup {
        id: "time-set",
        cli: &[&["config", "set", "work_start", "08:30"]],
        files: &[],
    });
    same(
        &set,
        &["time", "baseline", "promo_doc", "90"],
        "time.baseline keeps other settings",
    );
    let encrypted = prepare_setup(&Setup {
        id: "time-encrypted",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[],
    });
    refused(
        &encrypted,
        &["time", "baseline", "pm_update", "12"],
        "time.baseline sealed settings",
    );
    // A value error writes nothing, so it is served even when sealed.
    same(
        &encrypted,
        &["time", "baseline", "pm_update", "abc"],
        "time.baseline error in a sealed home",
    );
    refused(
        &empty_home("time-zone"),
        &["time", "baseline", "pm_update", "12"],
        "time.baseline timezone outside the YAML subset",
    );
    refused(
        &redact_home("time-redact"),
        &["time", "baseline", "pm_update", "12"],
        "time.baseline redaction input",
    );
}

#[test]
fn checklist_matches_python() {
    strict(NOW);
    same(
        &empty_home("checklist-empty"),
        &["checklist"],
        "checklist fresh",
    );
    let set = prepare_setup(&Setup {
        id: "checklist-pm",
        cli: &[],
        files: &[(
            "settings.yaml",
            "timezone: Etc/UTC\npm_slack: '#synthetic'\nfirst_run_checklist: false\n",
        )],
    });
    same(&set, &["checklist"], "checklist pm set and hidden");
    let goals = prepare_setup(&Setup {
        id: "checklist-goal",
        cli: &[],
        files: &[ZONE, ("goals.yaml", "queue: [{title: Synthetic goal}]\n")],
    });
    refused(&goals, &["checklist"], "checklist with a goals file");
}

#[test]
fn work_project_list_matches_python() {
    strict(NOW);
    let empty = empty_home("list-empty");
    same(
        &empty,
        &["work", "project", "list"],
        "work.project.list empty",
    );
    same(
        &empty,
        &["work", "project", "list", "--all"],
        "work.project.list empty all",
    );
    same(
        &empty,
        &["work", "project", "list", "--json"],
        "work.project.list empty json",
    );
    refused(
        &projects_home("list-projects"),
        &["work", "project", "list"],
        "work.project.list with projects",
    );
}

#[test]
fn work_project_rules_matches_python() {
    strict(NOW);
    let home = projects_home("rules");
    same(
        &home,
        &["work", "project", "rules", "alpha"],
        "work.project.rules hidden",
    );
    same(
        &home,
        &["work", "project", "rules", "alpha", "--json"],
        "work.project.rules hidden json",
    );
    same(
        &home,
        &["work", "project", "rules", "beta"],
        "work.project.rules none",
    );
    same(
        &home,
        &["work", "project", "rules", "beta", "--json"],
        "work.project.rules none json",
    );
    same(
        &home,
        &["work", "project", "rules", "nope"],
        "work.project.rules unknown",
    );
    same(
        &empty_home("rules-empty"),
        &["work", "project", "rules", "nope"],
        "work.project.rules no projects",
    );
    refused(
        &redact_home("rules-redact"),
        &["work", "project", "rules", "beta"],
        "work.project.rules redaction input",
    );
}

#[test]
fn work_project_rules_set_matches_python() {
    strict(NOW);
    let home = projects_home("rules-set");
    let long = "x".repeat(1001);
    for (args, label) in [
        (
            vec![
                "work",
                "project",
                "rules-set",
                "alpha",
                "--text",
                "New rule",
            ],
            "work.project.rules-set without yes",
        ),
        (
            vec!["work", "project", "rules-set", "alpha", "--clear"],
            "work.project.rules-set clear without yes",
        ),
        (
            vec!["work", "project", "rules-set", "nope", "--clear", "--yes"],
            "work.project.rules-set unknown",
        ),
        (
            vec![
                "work",
                "project",
                "rules-set",
                "alpha",
                "--text",
                long.as_str(),
                "--yes",
            ],
            "work.project.rules-set too long",
        ),
    ] {
        same(&home, &args, label);
    }
    refused(
        &home,
        &[
            "work",
            "project",
            "rules-set",
            "alpha",
            "--text",
            "New rule",
            "--yes",
        ],
        "work.project.rules-set real write",
    );
    refused(
        &home,
        &[
            "work",
            "project",
            "rules-set",
            "alpha",
            "--text",
            "x",
            "--clear",
        ],
        "work.project.rules-set text and clear",
    );
}

#[test]
fn work_project_remove_and_archive_match_python() {
    strict(NOW);
    let home = projects_home("remove");
    same(
        &home,
        &["work", "project", "remove", "alpha"],
        "work.project.remove without yes",
    );
    same(
        &home,
        &["work", "project", "remove", "nope", "--yes"],
        "work.project.remove unknown",
    );
    refused(
        &home,
        &["work", "project", "remove", "beta", "--yes"],
        "work.project.remove real delete",
    );
    same(
        &home,
        &["work", "project", "archive", "nope"],
        "work.project.archive unknown",
    );
    refused(
        &home,
        &["work", "project", "archive", "beta"],
        "work.project.archive real write",
    );
    same(
        &home,
        &["work", "project", "unarchive", "nope"],
        "work.project.unarchive unknown",
    );
    same(
        &empty_home("archive-empty"),
        &["work", "project", "unarchive", "nope"],
        "work.project.unarchive no projects",
    );
    refused(
        &home,
        &["work", "project", "unarchive", "beta"],
        "work.project.unarchive real write",
    );
}

#[test]
fn work_project_quiz_and_gate_match_python() {
    strict(NOW);
    let home = projects_home("prep");
    let quiz = |name: &'static str| {
        [
            "work", "project", "quiz", name, "--asked", "3", "--missed", "1",
        ]
    };
    same(&home, &quiz("nope"), "work.project.quiz unknown");
    same(&home, &quiz("alpha"), "work.project.quiz not prep");
    refused(&home, &quiz("prepper"), "work.project.quiz real write");
    same(
        &home,
        &["work", "project", "gate", "nope", "tool"],
        "work.project.gate unknown",
    );
    same(
        &home,
        &["work", "project", "gate", "beta", "questions"],
        "work.project.gate not prep",
    );
    refused(
        &home,
        &["work", "project", "gate", "prepper", "tool"],
        "work.project.gate real write",
    );
}

#[test]
fn work_project_update_source_check_and_prd_match_python() {
    strict(NOW);
    let home = projects_home("update");
    let long = "y".repeat(201);
    for (args, label) in [
        (
            vec!["work", "project", "update", "alpha", "weather", "Synthetic"],
            "work.project.update bad kind",
        ),
        (
            vec!["work", "project", "update", "alpha", "team", "   "],
            "work.project.update empty text",
        ),
        (
            vec!["work", "project", "update", "alpha", "team", long.as_str()],
            "work.project.update long text",
        ),
        (
            vec!["work", "project", "update", "nope", "team", "Synthetic"],
            "work.project.update unknown",
        ),
        (
            vec![
                "work",
                "project",
                "source",
                "alpha",
                "wiki",
                "synthetic-ref",
            ],
            "work.project.source bad kind",
        ),
        (
            vec![
                "work",
                "project",
                "check",
                "alpha",
                "--since",
                "--status",
                "Synthetic",
            ],
            "work.project.check since with status",
        ),
        (
            vec!["work", "project", "prd", "alpha", "--slot", "600"],
            "work.project.prd slot too big",
        ),
        (
            vec!["work", "project", "prd", "alpha", "--slot", "0"],
            "work.project.prd slot zero",
        ),
        (
            vec!["work", "project", "prd", "alpha", "--slot", "520.5"],
            "work.project.prd slot fraction",
        ),
    ] {
        same(&home, &args, label);
    }
    for (args, label) in [
        (
            vec!["work", "project", "update", "alpha", "team", "Synthetic"],
            "work.project.update real write",
        ),
        (
            vec!["work", "project", "source", "alpha", "doc", "synthetic-ref"],
            "work.project.source real write",
        ),
        (
            vec!["work", "project", "check", "alpha"],
            "work.project.check real stamp",
        ),
        (
            vec!["work", "project", "prd", "alpha", "--slot", "2"],
            "work.project.prd scaffold",
        ),
    ] {
        refused(&home, &args, label);
    }
}

/// Negative controls: the comparison sees a real difference between two
/// served results, and a redaction input makes every command refuse.
#[test]
fn the_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = projects_home("control");
    let hidden = observe(&home, &["work", "project", "rules", "alpha"], &Side::Native);
    let none = observe(&home, &["work", "project", "rules", "beta"], &Side::Native);
    assert_ne!(hidden, none);
    assert_eq!(hidden.code, Some(3));
    assert_eq!(none.stdout, "no standing rules\n");
    let unknown = observe(
        &home,
        &["work", "project", "archive", "nope"],
        &Side::Native,
    );
    assert!(unknown.stdout.contains("have: alpha, beta, prepper"));
    assert!(unknown
        .files
        .iter()
        .any(|f| f.starts_with("work/.projects.yaml.lock ")));
    let redact = redact_home("control-redact");
    for args in [
        &["checklist"][..],
        &["work", "project", "remove", "alpha"][..],
        &["work", "project", "prd", "alpha", "--slot", "600"][..],
    ] {
        assert_eq!(
            observe(&redact, args, &Side::Native).code,
            Some(64),
            "{args:?}"
        );
    }
}
