//! rust-12 native handlers: "learn.list", "learn.note", "learn.status" and
//! "learn.next" (and the wave 4 tour cases below) match Python in synthetic
//! homes (stdout, exit code,
//! data-root files, event rows).
mod common;

use common::*;
use std::path::PathBuf;

/// One clock for every test in this binary (the variable is per process).
const NOW: &str = "2026-11-20T09:00:00+00:00";

const CONCEPTS: &str = "concepts:\n  - {id: queue, section: core, title: The queue, cites: [file:readme], questions: [{bloom: understand, prompt: \"Why a queue?\", answer: Decoupling.}, {bloom: apply, prompt: \"Enqueue twice?\", answer: Runs twice.}]}\n  - {id: worker, section: core, title: The worker, prereqs: [queue], cites: [file:readme], questions: [{bloom: apply, prompt: \"What pulls jobs?\", answer: The worker.}]}\n  - {id: deploy, section: ops, title: Deploys, cites: [file:readme], questions: [{bloom: analyze, prompt: \"How to ship?\", answer: make ship.}]}\n";

const START_ALPHA: &[&str] = &["learn", "start", "alpha", "--file", "readme:README.md"];

/// Runs more Python CLI setup in a prepared home (paths only known at run
/// time), writes files under its HOME, then takes the snapshot again.
fn extend(home: &Home, files: &[(&str, &str)], steps: &[Vec<String>]) {
    extend_at(home, files, steps, None);
}

/// `extend` with the setup commands run at another pinned clock (`at`), so
/// that a retest or a card review is due at `NOW`.
fn extend_at(home: &Home, files: &[(&str, &str)], steps: &[Vec<String>], at: Option<&str>) {
    for (name, text) in files {
        let path = home.home.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    for args in steps {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let mut cmd = python(home, &args);
        cmd.env("CAREERBREW_NATIVE", "off");
        if let Some(at) = at {
            cmd.env("CAREERBREW_NOW", at);
        }
        let done = cmd.output().unwrap();
        assert!(done.status.success(), "setup {args:?}: {done:?}");
    }
    let _ = std::fs::remove_file(home.data.join("logs/events.jsonl"));
    let live = home.home.parent().unwrap().to_path_buf();
    let snap = live.with_file_name("snap");
    std::fs::remove_dir_all(&snap).unwrap();
    copy_tree(&live, &snap);
}

fn words(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

fn in_home(home: &Home, name: &str) -> PathBuf {
    home.home.join(name)
}

/// A project with concepts and answers: one concept mastered, one due.
fn studied() -> Home {
    let home = prepare_setup(&Setup {
        id: "studied",
        cli: &[START_ALPHA],
        files: &[],
    });
    let concepts = in_home(&home, "in/concepts.yaml");
    let concepts = concepts.to_str().unwrap();
    let answer = |concept: &str, result: &str, confidence: &str, level: &str, mode: &str| {
        words(&[
            "learn",
            "answer",
            "alpha",
            concept,
            "--result",
            result,
            "--confidence",
            confidence,
            "--level",
            level,
            "--mode",
            mode,
        ])
    };
    extend(
        &home,
        &[("in/concepts.yaml", CONCEPTS)],
        &[
            words(&["learn", "concepts", "alpha", concepts]),
            answer("queue", "dontknow", "low", "understand", "pretest"),
            answer("queue", "correct", "high", "understand", "teach"),
            answer("queue", "correct", "medium", "apply", "quiz"),
            answer("worker", "wrong", "high", "apply", "teach"),
        ],
    );
    home
}

#[test]
fn learn_list_matches_python() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "empty",
        cli: &[],
        files: &[],
    });
    assert_same(&empty, &["learn", "list"], true, "learn.list empty");

    let staged = prepare_setup(&Setup {
        id: "staged",
        cli: &[
            START_ALPHA,
            &["learn", "start", "Zeta Été", "--file", "readme:README.md"],
            &["learn", "start", "beta", "--file", "readme:README.md"],
        ],
        files: &[],
    });
    assert_same(&staged, &["learn", "list"], true, "learn.list staged");

    let studied = studied();
    assert_same(&studied, &["learn", "list"], true, "learn.list studied");
}

#[test]
fn learn_list_refuses_what_it_cannot_repeat() {
    strict(NOW);
    // Python prints through `privacy.redact`, which reads the profile.
    let redact = prepare_setup(&Setup {
        id: "redact",
        cli: &[START_ALPHA],
        files: &[("profile.yaml", "name: Synthetic Person\n")],
    });
    assert!(!assert_same(
        &redact,
        &["learn", "list"],
        false,
        "learn.list redact"
    ));

    // A folder whose name is not its own slug: Python reads another folder.
    let odd = prepare_setup(&Setup {
        id: "odd-folder",
        cli: &[],
        files: &[("learn/Upper/manifest.json", "{\"project\": \"Upper\"}\n")],
    });
    assert!(!assert_same(
        &odd,
        &["learn", "list"],
        false,
        "learn.list odd folder"
    ));

    // A second deck the learn crate does not read.
    let deck = prepare_setup(&Setup {
        id: "extra-deck",
        cli: &[START_ALPHA],
        files: &[("learn/alpha/srs/decks/other.yaml", "cards: []\n")],
    });
    assert!(!assert_same(
        &deck,
        &["learn", "list"],
        false,
        "learn.list extra deck"
    ));
}

#[test]
fn learn_note_matches_python() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "notes",
        cli: &[START_ALPHA],
        files: &[],
    });
    let plain = in_home(&home, "in/note.md");
    let wide = in_home(&home, "in/wide.md");
    extend(
        &home,
        &[
            ("in/note.md", "# Core\n\nThe queue decouples work.\n"),
            ("in/wide.md", "Über café: naïve note\n\n"),
        ],
        &[words(&[
            "learn",
            "note",
            "alpha",
            "02-é/über.md",
            "--set",
            wide.to_str().unwrap(),
        ])],
    );
    let plain = plain.to_str().unwrap();
    assert_same(
        &home,
        &["learn", "note", "alpha", "01-core.md", "--set", plain],
        true,
        "learn.note set",
    );
    assert_same(
        &home,
        &[
            "learn",
            "note",
            "alpha",
            "/01-core.md",
            "--set",
            plain,
            "--consume",
        ],
        true,
        "learn.note set consume",
    );
    assert_same(
        &home,
        &["learn", "note", "alpha", "02-é/über.md"],
        true,
        "learn.note read non-ASCII",
    );
    assert_same(
        &home,
        &["learn", "note", "alpha", "02-é/über.md", "--consume"],
        true,
        "learn.note read ignores consume",
    );
}

#[test]
fn learn_note_refuses_what_it_cannot_repeat() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "note-refusals",
        cli: &[START_ALPHA],
        files: &[],
    });
    let crlf = in_home(&home, "in/crlf.md");
    extend(&home, &[("in/crlf.md", "line one\r\nline two\r\n")], &[]);
    let crlf = crlf.to_str().unwrap();
    let refused = [
        // `Path.read_text` turns `\r\n` into `\n`.
        vec!["learn", "note", "alpha", "01-core.md", "--set", crlf],
        // Python's errors (exit 2) and its own messages.
        vec!["learn", "note", "alpha", "../x.md"],
        vec!["learn", "note", "alpha", "notes.txt"],
        vec!["learn", "note", "alpha", "missing.md"],
        vec!["learn", "note", "nobody", "01-core.md"],
        // `if a.set:` reads an empty value as no value: a read of a missing note.
        vec!["learn", "note", "alpha", "missing.md", "--set", ""],
    ];
    for args in refused {
        assert!(
            !assert_same(&home, &args, false, "learn.note refusal"),
            "{args:?} was served"
        );
    }
}

/// Sections whose first appearance is not in sorted order, one with a
/// non-ASCII name.
const UNSORTED: &str = "concepts:\n  - {id: deploy, section: ops, title: Deploys, cites: [file:readme], questions: [{bloom: analyze, prompt: \"How to ship?\", answer: make ship.}]}\n  - {id: queue, section: core, title: The queue, cites: [file:readme], questions: [{bloom: apply, prompt: \"Why a queue?\", answer: Decoupling.}]}\n  - {id: cafe, section: \"Über\", title: \"Café rules\", cites: [file:readme], questions: [{bloom: apply, prompt: \"Où?\", answer: Ici.}]}\n";

/// One `learn answer` row: concept, result, confidence, level, mode.
type Answer = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

/// A started project, then (when `concepts` is not empty) its concepts and
/// the answers, recorded at `at` (or `NOW`).
fn learn_home(id: &'static str, concepts: &str, answers: &[Answer], at: Option<&str>) -> Home {
    let home = prepare_setup(&Setup {
        id,
        cli: &[START_ALPHA],
        files: &[],
    });
    if concepts.is_empty() {
        return home;
    }
    let file = in_home(&home, "in/concepts.yaml");
    let mut steps = vec![words(&[
        "learn",
        "concepts",
        "alpha",
        file.to_str().unwrap(),
    ])];
    for (concept, result, confidence, level, mode) in answers {
        steps.push(words(&[
            "learn",
            "answer",
            "alpha",
            concept,
            "--result",
            result,
            "--confidence",
            confidence,
            "--level",
            level,
            "--mode",
            mode,
        ]));
    }
    extend_at(&home, &[("in/concepts.yaml", concepts)], &steps, at);
    home
}

const EARLY: &str = "2026-11-18T09:00:00+00:00";

const PRETESTS: &[Answer] = &[
    ("queue", "dontknow", "low", "understand", "pretest"),
    ("deploy", "dontknow", "low", "understand", "pretest"),
];

#[test]
fn learn_status_and_next_match_python() {
    strict(NOW);
    let quiz: Vec<Answer> = PRETESTS
        .iter()
        .copied()
        .chain([("queue", "wrong", "medium", "understand", "teach")])
        .collect();
    let mastered: &[Answer] = &[
        ("queue", "correct", "high", "understand", "teach"),
        ("queue", "correct", "medium", "apply", "quiz"),
    ];
    let mut all: Vec<Answer> = Vec::new();
    for concept in ["queue", "worker", "deploy"] {
        all.push((concept, "correct", "high", "apply", "quiz"));
        all.push((concept, "correct", "medium", "apply", "quiz"));
    }
    let homes = [
        ("fresh", learn_home("next-fresh", "", &[], None)),
        ("imported", learn_home("next-imported", CONCEPTS, &[], None)),
        ("studied", studied()),
        ("teach", learn_home("next-teach", CONCEPTS, PRETESTS, None)),
        ("quiz", learn_home("next-quiz", CONCEPTS, &quiz, None)),
        (
            "retest",
            learn_home("next-retest", CONCEPTS, mastered, Some(EARLY)),
        ),
        (
            "review",
            learn_home(
                "next-review",
                CONCEPTS,
                &[("worker", "wrong", "high", "apply", "teach")],
                Some(EARLY),
            ),
        ),
        ("done", learn_home("next-done", CONCEPTS, &all, None)),
    ];
    for (label, home) in &homes {
        assert_same(
            home,
            &["learn", "status", "alpha"],
            true,
            &format!("learn.status {label}"),
        );
        assert_same(
            home,
            &["learn", "next", "alpha"],
            true,
            &format!("learn.next {label}"),
        );
    }
    // The project name as typed, which Python prints, reads the same folder.
    let (_, studied) = &homes[2];
    assert_same(
        studied,
        &["learn", "status", "Alpha"],
        true,
        "learn.status typed name",
    );
    // Sections print in the order their first concept appears.
    let unsorted = learn_home(
        "status-unsorted",
        UNSORTED,
        &[("queue", "correct", "high", "apply", "quiz")],
        None,
    );
    assert_same(
        &unsorted,
        &["learn", "status", "alpha"],
        true,
        "learn.status unsorted sections",
    );
}

#[test]
fn learn_status_and_next_refuse_what_they_cannot_repeat() {
    strict(NOW);
    let home = learn_home("status-refusals", CONCEPTS, PRETESTS, None);
    let refused: [&[&str]; 6] = [
        // `json.dumps(..., indent=2)` keeps insertion order; not ported.
        &["learn", "status", "alpha", "--json"],
        &["learn", "next", "alpha", "--json"],
        // No manifest: Python's own error (status) or its own reading (next).
        &["learn", "status", "nobody"],
        &["learn", "next", "nobody"],
        &["learn", "status", "alpha", "extra"],
        &["learn", "next"],
    ];
    for args in refused {
        assert!(
            !assert_same(&home, args, false, "learn.status/next refusal"),
            "{args:?} was served"
        );
    }
    // The learn crate breaks ties between sections by name, Python by order.
    let unsorted = learn_home("next-unsorted", UNSORTED, &[], None);
    assert!(!assert_same(
        &unsorted,
        &["learn", "next", "alpha"],
        false,
        "learn.next unsorted sections"
    ));
    // Python prints through `privacy.redact`, which reads the profile.
    let redact = prepare_setup(&Setup {
        id: "status-redact",
        cli: &[START_ALPHA],
        files: &[("profile.yaml", "name: Synthetic Person\n")],
    });
    for args in [["learn", "status", "alpha"], ["learn", "next", "alpha"]] {
        assert!(!assert_same(&redact, &args, false, "learn redact"));
    }
}

/// Negative control: the comparison sees a difference when one exists.
#[test]
fn a_different_home_gives_a_different_observation() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "control-empty",
        cli: &[],
        files: &[],
    });
    let staged = prepare_setup(&Setup {
        id: "control-staged",
        cli: &[START_ALPHA],
        files: &[],
    });
    let want = observe(&empty, &["learn", "list"], &Side::Python);
    let got = observe(&staged, &["learn", "list"], &Side::Native);
    assert_eq!(got.code, Some(0));
    assert_ne!(got.stdout, want.stdout);
    assert_ne!(got.files, want.files);
}

// ---------- wave 4: "learn.tour.approve" and "learn.tour.export" ----------

#[test]
fn tour_approve_and_export_without_a_tour_match_python() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "tour-none",
        cli: &[],
        files: &[(".plaintext", "")],
    });
    let cases: [(&str, &[&str]); 5] = [
        (
            "learn.tour.approve",
            &["learn", "tour", "approve", "alpha", "intro", "--by", "Rev"],
        ),
        (
            "learn.tour.export",
            &["learn", "tour", "export", "alpha", "intro"],
        ),
        (
            "learn.tour.export",
            &["learn", "tour", "export", "Zeta Ete", "a-1"],
        ),
        (
            "learn.tour.export",
            &["learn", "tour", "export", "alpha", "Bad_Name"],
        ),
        (
            "learn.tour.approve",
            &["learn", "tour", "approve", "it's", "BAD", "--by", "Rev"],
        ),
    ];
    for (label, args) in cases {
        assert_same(&home, args, true, label);
    }
}

#[test]
fn tour_approve_and_export_refuse_what_they_cannot_repeat() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "tour-refuse",
        cli: &[],
        files: &[(".plaintext", ""), ("learn/alpha/tours/intro.json", "{}\n")],
    });
    let cases: [(&str, &[&str]); 3] = [
        (
            "learn.tour.export",
            &["learn", "tour", "export", "alpha", "intro"],
        ),
        (
            "learn.tour.approve",
            &["learn", "tour", "approve", "alpha", "intro", "--by", "Rev"],
        ),
        // A quote in the project: the KeyError repr changes its quotes.
        (
            "learn.tour.export",
            &["learn", "tour", "export", "it's", "intro"],
        ),
    ];
    for (label, args) in cases {
        assert!(!assert_same(&home, args, false, label));
    }
}

/// Negative control: a home with the tour and one without give different
/// observations.
#[test]
fn a_present_tour_gives_a_different_export() {
    strict(NOW);
    let none = prepare_setup(&Setup {
        id: "tour-control-none",
        cli: &[],
        files: &[(".plaintext", "")],
    });
    let some = prepare_setup(&Setup {
        id: "tour-control-some",
        cli: &[],
        files: &[(".plaintext", ""), ("learn/alpha/tours/intro.json", "{}\n")],
    });
    let args = ["learn", "tour", "export", "alpha", "intro"];
    let got = observe(&none, &args, &Side::Native);
    let want = observe(&some, &args, &Side::Python);
    assert_ne!(got, want);
}

// ---------- wave 7: "learn.concepts" ----------

const BAD_CONCEPTS: &str = "concepts:\n  - {id: queue, title: The queue, cites: [file:readme, note:missing.md], questions: [{bloom: remember, prompt: \"What?\", answer: A queue.}, {bloom: remember, prompt: \"Why?\", answer: \"\"}]}\n  - {id: worker, section: core, prereqs: [queue, ghost], cites: [], questions: []}\n  - section: ops\n    title: No id\n    cites: [file:readme]\n    questions:\n      - {bloom: guess, prompt: Pick one, answer: B, options: [A, B, C], misconceptions: [x]}\n";

const CYCLE: &str = "concepts:\n  - {id: a, section: s, title: A, prereqs: [b], cites: [file:readme], questions: [{bloom: apply, prompt: p, answer: x}]}\n  - {id: b, section: s, title: B, prereqs: [a], cites: [file:readme], questions: [{bloom: apply, prompt: p, answer: x}]}\n";

const IMPORT: &str = "# a comment\nconcepts:\n- id: queue\n  section: Core Ideas\n  title: The queue\n  cites: [file:readme]\n  extra: {weight: 2, ok: true, when: 2026-11-20}\n  questions:\n  - {bloom: understand, kind: choice, prompt: \"  Why a queue?  \", answer: Decoupling., options: [a, b], misconceptions: [m]}\n  - {bloom: apply, kind: open, prompt: \"Enqueue twice?\", answer: Runs twice.}\n- id: deploy\n  section: ops\n  title: Deploys\n  cites: [file:readme, file:readme]\n  questions:\n  - {bloom: analyze, kind: choice, prompt: \"How to ship? It is a long question that runs past the eighty column fold of the dumper for sure\", answer: make ship.}\n";

fn concepts_file(home: &Home, name: &str, text: &str) -> String {
    extend(home, &[(name, text)], &[]);
    in_home(home, name).to_str().unwrap().to_owned()
}

#[test]
fn learn_concepts_check_matches_python() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "concepts-check",
        cli: &[START_ALPHA],
        files: &[],
    });
    let good = concepts_file(&home, "in/good.yaml", CONCEPTS);
    let bad = concepts_file(&home, "in/bad.yaml", BAD_CONCEPTS);
    let cycle = concepts_file(&home, "in/cycle.yaml", CYCLE);
    let empty = concepts_file(&home, "in/empty.yaml", "");
    let map = concepts_file(&home, "in/map.yaml", "{}\n");
    let other = concepts_file(&home, "in/other.yaml", "title: no concepts here\n");
    let cases: [(&str, &[&str]); 7] = [
        (
            "learn.concepts",
            &["learn", "concepts", "alpha", &good, "--check"],
        ),
        (
            "learn.concepts problems",
            &["learn", "concepts", "alpha", &bad, "--check"],
        ),
        (
            "learn.concepts cycle",
            &["learn", "concepts", "alpha", &cycle, "--check"],
        ),
        (
            "learn.concepts empty",
            &["learn", "concepts", "alpha", &empty, "--check"],
        ),
        (
            "learn.concepts map",
            &["learn", "concepts", "alpha", &map, "--check"],
        ),
        (
            "learn.concepts other",
            &["learn", "concepts", "alpha", &other, "--check"],
        ),
        // No project folder yet: every citation fails, and the folder is made.
        (
            "learn.concepts new project",
            &["learn", "concepts", "Beta Two", &good, "--check"],
        ),
    ];
    for (label, args) in cases {
        assert_same(&home, args, true, label);
    }
    // The import refuses with the same problems (exit 2), writing nothing.
    assert_same(
        &home,
        &["learn", "concepts", "alpha", &bad, "--consume"],
        true,
        "learn.concepts not imported",
    );
}

#[test]
fn learn_concepts_import_matches_python() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "concepts-import",
        cli: &[START_ALPHA],
        files: &[(".plaintext", "")],
    });
    let good = concepts_file(&home, "in/good.yaml", CONCEPTS);
    let wide = concepts_file(&home, "in/wide.yaml", IMPORT);
    let empty = concepts_file(&home, "in/empty.yaml", "concepts: []\n");
    for (label, args) in [
        (
            "learn.concepts import",
            &["learn", "concepts", "alpha", &good][..],
        ),
        (
            "learn.concepts import consume",
            &["learn", "concepts", "alpha", &wide, "--consume"],
        ),
        (
            "learn.concepts import empty",
            &["learn", "concepts", "alpha", &empty],
        ),
        (
            "learn.concepts plaintext check",
            &["learn", "concepts", "alpha", &wide, "--check"],
        ),
    ] {
        assert_same(&home, args, true, label);
    }
}

#[test]
fn learn_concepts_refuses_what_it_cannot_repeat() {
    strict(NOW);
    // A sealed data folder: the import's writes differ on every run.
    let sealed = prepare_setup(&Setup {
        id: "concepts-sealed",
        cli: &[START_ALPHA],
        files: &[],
    });
    let good = concepts_file(&sealed, "in/good.yaml", CONCEPTS);
    let anchors = concepts_file(
        &sealed,
        "in/anchor.yaml",
        "concepts:\n  - &one {id: a, section: s, title: A, cites: [file:readme], questions: [{bloom: apply, prompt: p, answer: x}]}\n  - *one\n",
    );
    let list = concepts_file(&sealed, "in/list.yaml", "- {id: a}\n");
    let repeated = concepts_file(
        &sealed,
        "in/repeat.yaml",
        "concepts:\n  - {id: a}\n  - {id: a}\n",
    );
    let number = concepts_file(
        &sealed,
        "in/number.yaml",
        "concepts:\n  - {id: 7, section: s}\n",
    );
    let cases: [(&str, &[&str]); 6] = [
        (
            "learn.concepts sealed import",
            &["learn", "concepts", "alpha", &good],
        ),
        (
            "learn.concepts anchors",
            &["learn", "concepts", "alpha", &anchors, "--check"],
        ),
        (
            "learn.concepts list",
            &["learn", "concepts", "alpha", &list, "--check"],
        ),
        (
            "learn.concepts repeated",
            &["learn", "concepts", "alpha", &repeated, "--check"],
        ),
        (
            "learn.concepts number id",
            &["learn", "concepts", "alpha", &number, "--check"],
        ),
        (
            "learn.concepts missing file",
            &[
                "learn",
                "concepts",
                "alpha",
                "/nonexistent/x.yaml",
                "--check",
            ],
        ),
    ];
    for (label, args) in cases {
        assert!(!assert_same(&sealed, args, false, label));
    }
    // Python prints through `privacy.redact`, which reads the profile.
    let redact = prepare_setup(&Setup {
        id: "concepts-redact",
        cli: &[START_ALPHA],
        files: &[("profile.yaml", "name: Synthetic Person\n")],
    });
    let good = concepts_file(&redact, "in/good.yaml", CONCEPTS);
    assert!(!assert_same(
        &redact,
        &["learn", "concepts", "alpha", &good, "--check"],
        false,
        "learn.concepts redact"
    ));
}

/// Negative control: a passing and a failing concepts file give different
/// observations.
#[test]
fn a_bad_concepts_file_gives_a_different_check() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "concepts-control",
        cli: &[START_ALPHA],
        files: &[],
    });
    let good = concepts_file(&home, "in/good.yaml", CONCEPTS);
    let bad = concepts_file(&home, "in/bad.yaml", BAD_CONCEPTS);
    let got = observe(
        &home,
        &["learn", "concepts", "alpha", &good, "--check"],
        &Side::Native,
    );
    let want = observe(
        &home,
        &["learn", "concepts", "alpha", &bad, "--check"],
        &Side::Python,
    );
    assert_ne!(got, want);
}
