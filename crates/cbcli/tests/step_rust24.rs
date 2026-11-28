//! rust-24 native handlers: parity with the Python CLI in synthetic homes
//! for "time.baseline", "checklist", "work.project.list",
//! "work.project.rules", "work.project.rules-set", "work.project.remove",
//! "work.project.archive", "work.project.unarchive", "work.project.quiz",
//! "work.project.gate", "work.project.update", "work.project.source",
//! "work.project.check", "work.project.prd", "work.project.next",
//! "work.project.reference", "work.project.memory", "work.project.set",
//! "work.project.explain", "metrics.time-saved" and "cards.publish".
mod common;

use common::*;

/// One clock for every test in this binary (the variable is process-wide).
const NOW: &str = "2026-11-20T09:00:00";

const ZONE: (&str, &str) = ("settings.yaml", "timezone: Etc/UTC\n");
/// The marker that keeps a data folder plaintext (`Store::encryption_enabled`).
const PLAIN: (&str, &str) = (".plaintext", "");

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
        // Without the marker the data folder may be sealed (the hosted
        // Linux runner), and a sealed write is left to Python.
        files: &[PLAIN],
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
        files: &[PLAIN],
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

/// Plaintext projects with sources, a task, a link and a parent: the
/// writers' homes (wave 4).
const WRITERS: &[&[&str]] = &[
    &[
        "work",
        "project",
        "add",
        "alpha",
        "--memory",
        "/nonexistent-synthetic/alpha-notes.md",
    ],
    &[
        "work",
        "project",
        "source",
        "alpha",
        "url",
        "https://example.com/synthetic",
    ],
    &[
        "work",
        "project",
        "source",
        "alpha",
        "repo",
        "example/synthetic",
    ],
    &[
        "work",
        "task",
        "Synthetic task",
        "--hours",
        "1",
        "--why",
        "synthetic",
    ],
    &[
        "work",
        "project",
        "add",
        "beta",
        "--title",
        "Synthetic beta",
        "--group",
        "syn-team",
        "--link",
        "alpha",
    ],
    &["work", "project", "add", "gamma", "--parent", "alpha"],
    &["work", "project", "next", "beta", "t001"],
];

fn writers_home(id: &'static str) -> Home {
    prepare_setup(&Setup {
        id,
        cli: WRITERS,
        files: &[PLAIN, ZONE],
    })
}

#[test]
fn work_project_next_matches_python() {
    strict(NOW);
    let home = writers_home("next");
    for (args, label) in [
        (
            ["work", "project", "next", "alpha", "t001"],
            "work.project.next links a task",
        ),
        (
            ["work", "project", "next", "beta", "t001"],
            "work.project.next already linked",
        ),
        (
            ["work", "project", "next", "alpha", "t999"],
            "work.project.next unknown task",
        ),
        (
            ["work", "project", "next", "nope", "t001"],
            "work.project.next unknown project",
        ),
    ] {
        same(&home, &args, label);
    }
    same(
        &empty_home("next-empty"),
        &["work", "project", "next", "alpha", "t001"],
        "work.project.next no tasks",
    );
    let sealed = prepare_setup(&Setup {
        id: "next-sealed",
        cli: &[
            &["machine", "set", "personal"],
            &["data", "encrypt"],
            &["work", "project", "add", "alpha"],
            &[
                "work",
                "task",
                "Synthetic",
                "--hours",
                "1",
                "--why",
                "synthetic",
            ],
        ],
        files: &[],
    });
    refused(
        &sealed,
        &["work", "project", "next", "alpha", "t001"],
        "work.project.next sealed write",
    );
    refused(
        &redact_home("next-redact"),
        &["work", "project", "next", "alpha", "t001"],
        "work.project.next redaction input",
    );
}

#[test]
fn work_project_reference_matches_python() {
    strict(NOW);
    let home = writers_home("reference");
    same(
        &home,
        &[
            "work",
            "project",
            "reference",
            "alpha",
            "https://example.com/synthetic",
        ],
        "work.project.reference on",
    );
    same(
        &home,
        &[
            "work",
            "project",
            "reference",
            "alpha",
            "example/synthetic",
            "--off",
        ],
        "work.project.reference off",
    );
    same(
        &home,
        &["work", "project", "reference", "nope", "x"],
        "work.project.reference unknown project",
    );
    refused(
        &home,
        &["work", "project", "reference", "alpha", "no-such-ref"],
        "work.project.reference no literal match",
    );
}

#[test]
fn work_project_memory_matches_python() {
    strict(NOW);
    let home = writers_home("memory");
    same(
        &home,
        &[
            "work",
            "project",
            "memory",
            "gamma",
            "/nonexistent-synthetic/gamma.md",
        ],
        "work.project.memory a file",
    );
    same(
        &home,
        &["work", "project", "memory", "alpha", ""],
        "work.project.memory cleared",
    );
    same(
        &home,
        &[
            "work",
            "project",
            "memory",
            "nope",
            "/nonexistent-synthetic/x.md",
        ],
        "work.project.memory unknown project",
    );
    for (path, label) in [
        ("/tmp", "work.project.memory a folder"),
        (
            "/nonexistent-synthetic/MEMORY.md",
            "work.project.memory an index",
        ),
        ("~/synthetic.md", "work.project.memory home path"),
    ] {
        refused(&home, &["work", "project", "memory", "gamma", path], label);
    }
}

#[test]
fn work_project_set_matches_python() {
    strict(NOW);
    let home = writers_home("set");
    let long = "z".repeat(201);
    for (args, label) in [
        (
            vec!["work", "project", "set", "beta", "--title", "New title"],
            "work.project.set title",
        ),
        (
            vec!["work", "project", "set", "beta", "--group", "none"],
            "work.project.set clear group",
        ),
        (
            vec![
                "work",
                "project",
                "set",
                "gamma",
                "--title",
                "  Padded  ",
                "--group",
                "syn-group",
            ],
            "work.project.set title and group",
        ),
        (
            vec!["work", "project", "set", "alpha"],
            "work.project.set nothing",
        ),
        (
            vec!["work", "project", "set", "beta", "--title", long.as_str()],
            "work.project.set long title",
        ),
        (
            vec!["work", "project", "set", "nope", "--title", "x"],
            "work.project.set unknown project",
        ),
    ] {
        same(&home, &args, label);
    }
    refused(
        &home,
        &["work", "project", "set", "beta", "--parent", "alpha"],
        "work.project.set parent",
    );
    refused(
        &home,
        &["work", "project", "set", "beta", "--link", "gamma"],
        "work.project.set link",
    );
}

#[test]
fn work_project_explain_matches_python() {
    strict(NOW);
    let home = writers_home("explain");
    let example = "skills/careerbrew-project-explainer/references/example-careerbrew.md";
    for (args, label) in [
        (
            vec!["work", "project", "explain", "gamma"],
            "work.project.explain scaffold",
        ),
        (
            vec!["work", "project", "explain", "gamma", "--short"],
            "work.project.explain scaffold short",
        ),
        (
            vec!["work", "project", "explain", "gamma", "--deep"],
            "work.project.explain scaffold deep",
        ),
        (
            vec!["work", "project", "explain", "nope"],
            "work.project.explain unknown project",
        ),
        (
            vec!["work", "project", "explain", "gamma", "--check", example],
            "work.project.explain check passes",
        ),
        (
            vec![
                "work", "project", "explain", "gamma", "--check", example, "--short",
            ],
            "work.project.explain check short fails",
        ),
        (
            vec![
                "work",
                "project",
                "explain",
                "gamma",
                "--check",
                "config/native-routes.txt",
            ],
            "work.project.explain check fails",
        ),
    ] {
        same(&home, &args, label);
    }
    refused(
        &home,
        &["work", "project", "explain", "alpha"],
        "work.project.explain project with sources",
    );
    let inside = home.data.join("settings.yaml").display().to_string();
    refused(
        &home,
        &["work", "project", "explain", "gamma", "--check", &inside],
        "work.project.explain check inside the data root",
    );
}

/// Negative control for the wave-4 writers: two served writes differ.
#[test]
fn the_writers_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = writers_home("writers-control");
    let on = observe(
        &home,
        &["work", "project", "reference", "alpha", "example/synthetic"],
        &Side::Native,
    );
    let off = observe(
        &home,
        &[
            "work",
            "project",
            "reference",
            "alpha",
            "example/synthetic",
            "--off",
        ],
        &Side::Native,
    );
    assert_eq!(on.code, Some(0));
    assert_ne!(on.files, off.files);
    assert_ne!(on.stdout, off.stdout);
}

/// Writes one data-root file into the live home and its snapshot, after
/// `prepare` (which deletes the event log).
fn plant(home: &Home, rel: &str, text: &str) {
    let live = home.home.parent().unwrap();
    for root in [live.to_path_buf(), live.with_file_name("snap")] {
        let path = root.join("data").join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

/// Timed rows over two ISO weeks, a measured baseline's action included,
/// and a row the reader skips unparsed.
const TIMED_LOG: &str = concat!(
    "{\"event\": \"cli.run\", \"status\": \"ok\", \"saves\": \"day_log\", \"ms\": 30000, \"ts\": \"2026-11-10T10:00:00Z\"}\n",
    "{\"event\": \"api.read\", \"route\": \"today\", \"ts\": \"2026-11-17T09:00:00Z\"}\n",
    "{\"event\": \"cli.run\", \"status\": \"ok\", \"saves\": \"pm_update\", \"ms\": 90000, \"ts\": \"2026-11-17T10:00:00Z\"}\n",
    "{\"event\": \"cli.run\", \"status\": \"fail\", \"saves\": \"pm_update\", \"ms\": 1000, \"ts\": \"2026-11-17T10:05:00Z\"}\n",
    "{\"event\": \"web.timing\", \"saves\": \"standup\", \"step\": \"open\", \"seconds\": 0, \"ts\": \"2026-11-18T10:00:00Z\"}\n",
    "{\"event\": \"web.timing\", \"saves\": \"standup\", \"step\": \"copy\", \"seconds\": 15, \"ts\": \"2026-11-18T10:00:15Z\"}\n",
    "{\"event\": \"web.timing\", \"saves\": \"inbox\", \"step\": \"open\", \"seconds\": 0, \"ts\": \"2026-11-19T10:00:00Z\"}\n",
);

fn time_home(id: &'static str) -> Home {
    prepare_setup(&Setup {
        id,
        cli: &[],
        files: &[
            (
                "settings.yaml",
                "timezone: Etc/UTC\ntime_baseline_pm_update: 12\n",
            ),
            PLAIN,
        ],
    })
}

#[test]
fn metrics_time_saved_matches_python() {
    strict(NOW);
    // Route key: "metrics.time-saved".
    let empty = prepare_setup(&Setup {
        id: "time-saved-empty",
        cli: &[],
        files: &[ZONE, PLAIN],
    });
    same(
        &empty,
        &["metrics", "time-saved"],
        "metrics.time-saved empty",
    );
    same(
        &empty,
        &["metrics", "time-saved", "--json"],
        "metrics.time-saved empty json",
    );
    let home = time_home("time-saved-log");
    plant(&home, "logs/events.jsonl", TIMED_LOG);
    same(&home, &["metrics", "time-saved"], "metrics.time-saved log");
    same(
        &home,
        &["metrics", "time-saved", "--json"],
        "metrics.time-saved log json",
    );
    // An earlier report is rewritten in place.
    let stale = time_home("time-saved-stale");
    plant(&stale, "logs/events.jsonl", TIMED_LOG);
    plant(&stale, "out/time-saved.json", "{}\n");
    same(
        &stale,
        &["metrics", "time-saved"],
        "metrics.time-saved stale",
    );
}

#[test]
fn metrics_time_saved_refuses_what_it_cannot_prove() {
    strict(NOW);
    refused(
        &redact_home("time-saved-redact"),
        &["metrics", "time-saved"],
        "metrics.time-saved redact inputs",
    );
    // A NaN run time: Python sums it; the native reader never guesses.
    let home = time_home("time-saved-nan");
    plant(
        &home,
        "logs/events.jsonl",
        "{\"event\": \"cli.run\", \"status\": \"ok\", \"saves\": \"pm_update\", \"ms\": NaN, \"ts\": \"2026-11-17T10:00:00Z\"}\n",
    );
    let native = observe(&home, &["metrics", "time-saved"], &Side::Native);
    assert_eq!(native.code, Some(64), "NaN row served");
    assert!(native.stdout.is_empty());
    // A stamp outside the one strict shape.
    let home = time_home("time-saved-stamp");
    plant(
        &home,
        "logs/events.jsonl",
        "{\"event\": \"cli.run\", \"status\": \"ok\", \"saves\": \"pm_update\", \"ms\": 1, \"ts\": \"2026-11-17 10:00:00\"}\n",
    );
    let native = observe(&home, &["metrics", "time-saved"], &Side::Native);
    assert_eq!(native.code, Some(64), "odd stamp served");
}

/// Negative control: an empty log and a timed log differ in what is written.
#[test]
fn the_time_saved_comparison_detects_a_planted_difference() {
    strict(NOW);
    let empty = time_home("time-saved-control-empty");
    let full = time_home("time-saved-control-full");
    plant(&full, "logs/events.jsonl", TIMED_LOG);
    let a = observe(&empty, &["metrics", "time-saved"], &Side::Native);
    let b = observe(&full, &["metrics", "time-saved"], &Side::Native);
    assert_eq!(a.code, Some(0));
    assert_eq!(b.code, Some(0));
    assert_ne!(a.stdout, b.stdout);
    assert_ne!(a.files, b.files);
}

/// An explanation that meets the bar: five sections in order, 150+ words.
fn explanation() -> String {
    let words = "a synthetic queue keeps each job in order so that workers take the oldest job first and every result comes back to the caller with its own id and a short status line that names the step";
    let mut out = String::new();
    for section in [
        "## In plain words",
        "## How it works",
        "## Example",
        "## Common mistakes",
        "## In the interview",
    ] {
        out.push_str(&format!("    {section}\n    {words}\n"));
    }
    out
}

fn card(question: &str, extra: &str) -> String {
    format!(
        "- question: \"{question}\"\n  answer: \"A synthetic queue hands out the oldest job first.\"\n  topic: Synthetic Queues\n{extra}  explanation: |\n{}",
        explanation()
    )
}

const CARDS_FILE: &str = "cards-in.yaml";

fn cards_home(id: &'static str, text: &str) -> Home {
    let home = prepare_setup(&Setup {
        id,
        cli: &[],
        files: &[ZONE, PLAIN],
    });
    plant(&home, CARDS_FILE, text);
    home
}

fn cards_path(home: &Home) -> String {
    home.data.join(CARDS_FILE).display().to_string()
}

#[test]
fn cards_publish_dry_run_matches_python() {
    strict(NOW);
    // Route key: "cards.publish".
    let one = cards_home(
        "cards-one",
        &card("What does a synthetic queue widget do?", ""),
    );
    let path = cards_path(&one);
    same(
        &one,
        &["cards", "publish", &path, "--dry-run"],
        "cards.publish dry run one card",
    );
    let mixed = format!(
        "{}{}- skip: true\n  source_id: synth01\n  reason: personal\n- skip: true\n{}",
        card("What does a synthetic queue widget do?", ""),
        card(
            "How would you explain a synthetic retry loop?",
            "  private: true\n  source_id: synth02\n  tags: [queues]\n"
        ),
        card(
            "What does a synthetic queue widget do?",
            "  source_id: synth03\n"
        ),
    );
    let mixed = cards_home("cards-mixed", &mixed);
    let path = cards_path(&mixed);
    same(
        &mixed,
        &["cards", "publish", &path, "--dry-run"],
        "cards.publish dry run skips and private",
    );
    let wrapped = format!(
        "cards:\n{}",
        card("What does a synthetic stack widget do?", "")
            .lines()
            .map(|line| format!("  {line}\n"))
            .collect::<String>()
    );
    let wrapped = cards_home("cards-wrapped", &wrapped);
    let path = cards_path(&wrapped);
    same(
        &wrapped,
        &["cards", "publish", &path, "--dry-run"],
        "cards.publish dry run cards map",
    );
    let empty = cards_home("cards-missing", "");
    let missing = empty.data.join("no-such.yaml").display().to_string();
    same(
        &empty,
        &["cards", "publish", &missing, "--dry-run"],
        "cards.publish dry run missing file",
    );
}

#[test]
fn cards_publish_failing_cards_match_python() {
    strict(NOW);
    let text = format!(
        "{}- question: \"Too short?\"\n  answer: \"short\"\n  explanation: \"## In plain words only\"\n- answer: \"no question here at all, synthetic\"\n",
        card("What does a synthetic queue widget do?", "")
    );
    let home = cards_home("cards-failing", &text);
    let path = cards_path(&home);
    same(
        &home,
        &["cards", "publish", &path, "--dry-run"],
        "cards.publish dry run failing cards",
    );
    // A failing card raises before any write, publish or not.
    same(
        &home,
        &["cards", "publish", &path],
        "cards.publish failing cards",
    );
}

#[test]
fn cards_publish_refuses_what_it_cannot_prove() {
    strict(NOW);
    let text = card("What does a synthetic queue widget do?", "");
    let redact = prepare_setup(&Setup {
        id: "cards-redact",
        cli: &[],
        files: &[ZONE, PLAIN, ("sensitive-terms.txt", "synthetic\n")],
    });
    plant(&redact, CARDS_FILE, &text);
    let path = cards_path(&redact);
    refused(
        &redact,
        &["cards", "publish", &path, "--dry-run"],
        "cards.publish redact inputs",
    );
    let corporate = prepare_setup(&Setup {
        id: "cards-corporate",
        cli: &[],
        files: &[ZONE, PLAIN, ("machine.yaml", "role: corporate\n")],
    });
    plant(&corporate, CARDS_FILE, &text);
    let path = cards_path(&corporate);
    refused(
        &corporate,
        &["cards", "publish", &path, "--dry-run"],
        "cards.publish corporate role",
    );
    // A YAML anchor: the reader is not checked on it.
    let anchored = cards_home(
        "cards-anchor",
        "- question: &q \"What is a synthetic anchor?\"\n  answer: *q\n",
    );
    let path = cards_path(&anchored);
    refused(
        &anchored,
        &["cards", "publish", &path, "--dry-run"],
        "cards.publish yaml anchor",
    );
    // A real publish writes the repo deck: never served natively, and never
    // run here on the Python side.
    let one = cards_home("cards-real", &text);
    let path = cards_path(&one);
    let native = observe(&one, &["cards", "publish", &path], &Side::Native);
    assert_eq!(native.code, Some(64), "a real publish served");
}

/// Negative control: a passing and a failing card differ.
#[test]
fn the_cards_comparison_detects_a_planted_difference() {
    strict(NOW);
    let good = cards_home(
        "cards-control-good",
        &card("What does a synthetic queue widget do?", ""),
    );
    let bad = cards_home(
        "cards-control-bad",
        "- question: \"Too short?\"\n  answer: \"short\"\n",
    );
    let a = observe(
        &good,
        &["cards", "publish", &cards_path(&good), "--dry-run"],
        &Side::Native,
    );
    let b = observe(
        &bad,
        &["cards", "publish", &cards_path(&bad), "--dry-run"],
        &Side::Native,
    );
    assert_eq!(a.code, Some(0));
    assert_eq!(b.code, Some(1));
    assert_ne!(a.stdout, b.stdout);
}
