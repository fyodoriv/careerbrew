//! Parity cases for the `rust-20` native handlers (`src/steps/rust20.rs`):
//! each served route key runs in fresh synthetic homes on both sides, plus
//! cases the handler must refuse and one negative control per command.
mod common;
use common::{assert_same, prepare_setup, strict, Setup};

const NOW: &str = "2026-11-20T09:00:00";
const CORPORATE: (&str, &str) = ("machine.yaml", "role: corporate\n");
const TERMS: (&str, &str) = ("sensitive-terms.txt", "zebrafruit\n");
const SEED: &[&[&str]] = &[&["cards", "add", "What is a heap?", "A tree with order"]];
const PUBLIC_ID: &str = "kb-b34d21ae5816";

fn refuses(setup: &Setup, args: &[&str], label: &str) {
    let home = prepare_setup(setup);
    assert!(
        !assert_same(&home, args, false, label),
        "{label}: {args:?} served"
    );
}

#[test]
fn cards_next_matches_python() {
    strict(NOW);
    // Route key: "cards.next".
    let home = prepare_setup(&Setup {
        id: "next",
        cli: SEED,
        files: &[],
    });
    assert_same(&home, &["cards", "next"], true, "cards.next default");
    assert_same(
        &home,
        &["cards", "next", "--count", "3"],
        true,
        "cards.next count",
    );
    assert_same(
        &home,
        &["cards", "next", "--json", "--count", "2"],
        true,
        "cards.next json",
    );
    assert_same(
        &home,
        &["cards", "next", "--json", "--count", "0"],
        true,
        "cards.next empty json",
    );
    let reviews = Setup {
        id: "next-bad-review",
        cli: SEED,
        files: &[("tutor/card-reviews.jsonl", "{\"card\": 7}\n")],
    };
    refuses(&reviews, &["cards", "next"], "cards.next bad review row");
    let corporate = Setup {
        id: "next-corp",
        cli: &[],
        files: &[CORPORATE],
    };
    refuses(&corporate, &["cards", "next"], "cards.next corporate");
}

#[test]
fn cards_add_matches_python() {
    strict(NOW);
    // Route key: "cards.add".
    let home = prepare_setup(&Setup {
        id: "add",
        cli: SEED,
        files: &[],
    });
    assert_same(
        &home,
        &[
            "cards",
            "add",
            "  Why sort first? ",
            "Binary search",
            "--tags",
            "Search,,arrays",
        ],
        true,
        "cards.add tags",
    );
    assert_same(
        &home,
        &["cards", "add", "What is a heap?", "Replaced"],
        true,
        "cards.add replace",
    );
    assert_same(
        &home,
        &[
            "cards",
            "add",
            "Q in a new deck",
            "An answer",
            "--deck",
            "Side Deck!",
        ],
        true,
        "cards.add deck",
    );
    let blank = Setup {
        id: "add-blank",
        cli: SEED,
        files: &[],
    };
    refuses(
        &blank,
        &["cards", "add", "A question", "   "],
        "cards.add blank answer",
    );
    let odd = Setup {
        id: "add-odd",
        cli: SEED,
        files: &[],
    };
    refuses(
        &odd,
        &["cards", "add", "A\u{1f}question", "An answer"],
        "cards.add separator",
    );
    let terms = Setup {
        id: "add-terms",
        cli: SEED,
        files: &[TERMS],
    };
    refuses(
        &terms,
        &["cards", "add", "A question", "An answer"],
        "cards.add redaction",
    );
}

#[test]
fn cards_unsuspend_matches_python() {
    strict(NOW);
    // Route key: "cards.unsuspend".
    let home = prepare_setup(&Setup {
        id: "unsuspend",
        cli: SEED,
        files: &[],
    });
    assert_same(
        &home,
        &["cards", "unsuspend", PUBLIC_ID],
        true,
        "cards.unsuspend public",
    );
    let meta = Setup {
        id: "unsuspend-meta",
        cli: SEED,
        files: &[
            (
                "tutor/card-meta.json",
                "{\"suspended\": [\"kb-b34d21ae5816\", \"zz-other\"], \"lapse_base\": {\"zz-other\": 2}}",
            ),
            (
                "tutor/card-reviews.jsonl",
                "{\"card\": \"kb-b34d21ae5816\", \"lapse\": true}\n{\"card\": \"kb-b34d21ae5816\", \"lapse\": false}\n",
            ),
        ],
    };
    let home = prepare_setup(&meta);
    assert_same(
        &home,
        &["cards", "unsuspend", PUBLIC_ID],
        true,
        "cards.unsuspend meta",
    );
    let unknown = Setup {
        id: "unsuspend-unknown",
        cli: SEED,
        files: &[],
    };
    refuses(
        &unknown,
        &["cards", "unsuspend", "mine-0000000000"],
        "cards.unsuspend unknown",
    );
    let corporate = Setup {
        id: "unsuspend-corp",
        cli: &[],
        files: &[CORPORATE],
    };
    refuses(
        &corporate,
        &["cards", "unsuspend", PUBLIC_ID],
        "cards.unsuspend corporate",
    );
}

#[test]
fn cards_public_check_matches_python() {
    strict(NOW);
    // Route key: "cards.public-check".
    let home = prepare_setup(&Setup {
        id: "public-check",
        cli: &[],
        files: &[],
    });
    assert_same(
        &home,
        &["cards", "public-check"],
        true,
        "cards.public-check",
    );
    let terms = Setup {
        id: "public-check-terms",
        cli: &[],
        files: &[TERMS],
    };
    refuses(
        &terms,
        &["cards", "public-check"],
        "cards.public-check redaction",
    );
}

const INDEX: &str = "[{\"id\": \"a1b2c3d4e5f6\", \"section\": \"guides\", \"title\": \"Graph notes\", \"file\": \"graph.md\"}, {\"id\": \"f0e1d2c3\", \"section\": \"interview\", \"title\": \"Heap tips\", \"file\": \"missing.md\"}]";

#[test]
fn library_matches_python() {
    strict(NOW);
    // Route keys: "library.list" and "library.show".
    let files: &[(&str, &str)] = &[
        ("library/notion/index.json", INDEX),
        ("library/notion/graph.md", "BFS uses a queue.\n"),
    ];
    let home = prepare_setup(&Setup {
        id: "library",
        cli: &[],
        files,
    });
    assert_same(&home, &["library", "list"], true, "library.list");
    assert_same(
        &home,
        &["library", "show", "a1b2c3d4e5f6"],
        true,
        "library.show exact",
    );
    assert_same(
        &home,
        &["library", "show", "f0e1"],
        true,
        "library.show prefix, no file",
    );
    let empty = prepare_setup(&Setup {
        id: "library-empty",
        cli: &[],
        files: &[],
    });
    assert_same(&empty, &["library", "list"], true, "library.list empty");
    let miss = Setup {
        id: "library-miss",
        cli: &[],
        files,
    };
    refuses(&miss, &["library", "show", "zz"], "library.show unknown");
    let bad = Setup {
        id: "library-bad",
        cli: &[],
        files: &[("library/notion/index.json", "[{\"id\": 3}]")],
    };
    refuses(&bad, &["library", "list"], "library.list bad entry");
    let corporate = Setup {
        id: "library-corp",
        cli: &[],
        files: &[CORPORATE],
    };
    refuses(&corporate, &["library", "list"], "library.list corporate");
}

#[test]
fn drill_list_matches_python() {
    strict(NOW);
    // Route key: "drill.list".
    let home = prepare_setup(&Setup {
        id: "drill",
        cli: &[],
        files: &[],
    });
    assert_same(&home, &["drill", "list"], true, "drill.list base");
    let gap = Setup {
        id: "drill-gap",
        cli: &[],
        files: &[(
            "tutor/gap-items.yaml",
            "items:\n- id: gap-queues-1\n  topic: queues\n  round: technical\n",
        )],
    };
    let home = prepare_setup(&gap);
    assert_same(&home, &["drill", "list"], true, "drill.list generated");
    let bad = Setup {
        id: "drill-bad",
        cli: &[],
        files: &[(
            "tutor/gap-items.yaml",
            "items:\n- id: gap-x-1\n  round: technical\n",
        )],
    };
    refuses(&bad, &["drill", "list"], "drill.list missing topic");
    let terms = Setup {
        id: "drill-terms",
        cli: &[],
        files: &[TERMS],
    };
    refuses(&terms, &["drill", "list"], "drill.list redaction");
}
