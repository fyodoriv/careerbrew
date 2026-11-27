//! rust-12 native handlers: "learn.list" and "learn.note" match Python in
//! synthetic homes (stdout, exit code, data-root files, event rows).
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
    for (name, text) in files {
        let path = home.home.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    for args in steps {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let done = python(home, &args)
            .env("CAREERBREW_NATIVE", "off")
            .output()
            .unwrap();
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
