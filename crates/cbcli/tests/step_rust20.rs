//! Parity cases for the `rust-20` native handlers (`src/steps/rust20.rs`):
//! each served route key runs in fresh synthetic homes on both sides, plus
//! cases the handler must refuse and one negative control per command.
mod common;
use common::{assert_same, prepare_setup, strict, Setup};

const NOW: &str = "2026-11-20T09:00:00";
const CORPORATE: (&str, &str) = ("machine.yaml", "role: corporate\n");
const TERMS: (&str, &str) = ("sensitive-terms.txt", "zebrafruit\n");
const PLAIN: (&str, &str) = (".plaintext", "");
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
        files: &[PLAIN],
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
    assert_same(
        &home,
        &[
            "cards",
            "add",
            "Qu'est-ce qu'un tas : « heap » ?",
            "A long answer that keeps going past the eighty column fold of the dumper, so the plain scalar wraps onto a second line",
            "--tags",
            "Ärger, spaced ",
        ],
        true,
        "cards.add unicode and folding",
    );
    assert_same(
        &home,
        &["cards", "add", "yes", "12: no #x"],
        true,
        "cards.add quoted scalars",
    );
    let empty = prepare_setup(&Setup {
        id: "add-empty",
        cli: &[],
        files: &[PLAIN],
    });
    assert_same(
        &empty,
        &["cards", "add", "First card", "First answer"],
        true,
        "cards.add first deck",
    );
    let sealed = Setup {
        id: "add-sealed",
        cli: SEED,
        files: &[],
    };
    refuses(
        &sealed,
        &["cards", "add", "A question", "An answer"],
        "cards.add encrypted folder",
    );
    let blank = Setup {
        id: "add-blank",
        cli: SEED,
        files: &[PLAIN],
    };
    refuses(
        &blank,
        &["cards", "add", "A question", "   "],
        "cards.add blank answer",
    );
    let odd = Setup {
        id: "add-odd",
        cli: SEED,
        files: &[PLAIN],
    };
    refuses(
        &odd,
        &["cards", "add", "A\u{1f}question", "An answer"],
        "cards.add separator",
    );
    let terms = Setup {
        id: "add-terms",
        cli: SEED,
        files: &[PLAIN, TERMS],
    };
    refuses(
        &terms,
        &["cards", "add", "A question", "An answer"],
        "cards.add redaction",
    );
    let corporate = Setup {
        id: "add-corp",
        cli: &[],
        files: &[PLAIN, CORPORATE],
    };
    refuses(
        &corporate,
        &["cards", "add", "A question", "An answer"],
        "cards.add corporate",
    );
    // Negative control: a card without an id is a Python KeyError.
    let broken = Setup {
        id: "add-broken",
        cli: &[],
        files: &[
            PLAIN,
            (
                "tutor/decks/mine.yaml",
                "cards:\n- question: q\n  answer: a\n",
            ),
        ],
    };
    refuses(
        &broken,
        &["cards", "add", "A question", "An answer"],
        "cards.add card without id",
    );
}

/// Fourteen topics over two decks, with ties, so the top-12 cut and the
/// stable order of tied topics both show.
const MANY_TOPICS: &str = "cards:
- {id: m-1, question: q1, answer: a, topic: zeta, tags: []}
- {id: m-2, question: q2, answer: a, topic: alpha, tags: []}
- {id: m-3, question: q3, answer: a, topic: alpha, tags: []}
- {id: m-4, question: q4, answer: a, topic: kappa, tags: []}
- {id: m-5, question: q5, answer: a, topic: beta, tags: []}
- {id: m-6, question: q6, answer: a, topic: gamma, tags: []}
- {id: m-7, question: q7, answer: a, topic: delta, tags: []}
- {id: m-8, question: q8, answer: a, topic: epsilon, tags: []}
- {id: m-9, question: q9, answer: a, topic: eta, tags: []}
- {id: m-10, question: q10, answer: a, topic: theta, tags: []}
- {id: m-11, question: q11, answer: a, topic: iota, tags: []}
- {id: m-12, question: q12, answer: a, topic: lambda, tags: []}
- {id: m-13, question: q13, answer: a, topic: mu, tags: []}
- {id: m-14, question: q14, answer: a, topic: a very long topic name over twenty four, tags: []}
";
const SECOND_DECK: &str = "cards:
- {id: m-2, question: q2 again, answer: a, topic: omega, tags: []}
- {id: s-1, question: s1, answer: a, topic: zeta, tags: []}
";
/// Reviews over several days: a streak, an old row outside 30 days, and
/// retention 1 of 8 (0.125, a rounding tie).
const REVIEWS: &str = r#"{"at": "2026-09-01T10:00:00+00:00", "card": "m-1", "rating": "good", "topic": "zeta"}
{"at": "2026-11-17T10:00:00+00:00", "card": "m-1", "rating": "again", "topic": "zeta"}
{"at": "2026-11-18T10:00:00.250000+00:00", "card": "m-2", "rating": "again", "topic": "omega"}
{"at": "2026-11-19T10:00:00+00:00", "card": "m-3", "rating": "again", "topic": "alpha"}
{"at": "2026-11-19T11:00:00+00:00", "card": "m-4", "rating": "again", "topic": "kappa"}
{"at": "2026-11-19T12:00:00+00:00", "card": "gone", "rating": "good", "topic": "kappa"}
{"at": "2026-11-20T08:00:00+00:00", "card": "m-5", "rating": "again", "topic": "beta"}
{"at": "2026-11-20T08:10:00+00:00", "card": "m-6", "rating": "again", "topic": "gamma"}
{"at": "2026-11-20T08:20:00+00:00", "card": "m-7", "rating": "again", "topic": "delta"}
{"at": "2026-11-20T08:30:00+00:00", "card": "m-8", "rating": "good", "topic": "epsilon", "first": true}
"#;
const STATES: &str = r#"{"m-1": {"card_id": 1, "state": 2, "step": null, "stability": 3.5, "difficulty": 5.0, "due": "2026-11-19T10:00:00+00:00", "last_review": "2026-11-17T10:00:00+00:00"},
 "m-2": {"card_id": 2, "state": 1, "step": 0, "stability": 1.0, "difficulty": 6.0, "due": "2026-11-20T20:00:00+00:00", "last_review": "2026-11-18T10:00:00+00:00"},
 "m-3": {"card_id": 3, "state": 2, "step": null, "stability": 9.0, "difficulty": 4.0, "due": "2026-12-20T20:00:00+00:00", "last_review": "2026-11-19T10:00:00+00:00"},
 "m-4": {"card_id": 4, "state": 2, "step": null, "stability": 2.0, "difficulty": 4.0, "due": "2026-11-20T08:59:59.999999+00:00", "last_review": null}}
"#;
const META: &str = r#"{"suspended": ["m-4", "not-a-card"], "lapse_base": {}}"#;

#[test]
fn cards_stats_matches_python() {
    strict(NOW);
    // Route key: "cards.stats".
    let empty = prepare_setup(&Setup {
        id: "stats-empty",
        cli: &[],
        files: &[PLAIN],
    });
    assert_same(&empty, &["cards", "stats"], true, "cards.stats empty home");
    let graded = prepare_setup(&Setup {
        id: "stats-graded",
        cli: &[
            &["cards", "add", "What is a heap?", "A tree with order"],
            &["cards", "grade", PUBLIC_ID, "again"],
        ],
        files: &[PLAIN],
    });
    assert_same(&graded, &["cards", "stats"], true, "cards.stats graded");
    let full = prepare_setup(&Setup {
        id: "stats-full",
        cli: &[],
        files: &[
            PLAIN,
            ("tutor/decks/mine.yaml", MANY_TOPICS),
            ("tutor/decks/side.yaml", SECOND_DECK),
            ("tutor/card-reviews.jsonl", REVIEWS),
            ("tutor/card-state.json", STATES),
            ("tutor/card-meta.json", META),
        ],
    });
    assert_same(&full, &["cards", "stats"], true, "cards.stats many topics");
    let corporate = Setup {
        id: "stats-corp",
        cli: &[],
        files: &[PLAIN, CORPORATE],
    };
    refuses(&corporate, &["cards", "stats"], "cards.stats corporate");
    let terms = Setup {
        id: "stats-terms",
        cli: &[],
        files: &[PLAIN, TERMS],
    };
    refuses(&terms, &["cards", "stats"], "cards.stats redaction");
    let naive = Setup {
        id: "stats-naive",
        cli: &[],
        files: &[
            PLAIN,
            ("tutor/decks/mine.yaml", MANY_TOPICS),
            (
                "tutor/card-reviews.jsonl",
                "{\"at\": \"2026-11-19T10:00:00\", \"card\": \"m-1\", \"rating\": \"good\", \"topic\": \"zeta\"}\n",
            ),
        ],
    };
    refuses(
        &naive,
        &["cards", "stats"],
        "cards.stats naive review stamp",
    );
    // Negative control: a recent row with no topic is a Python KeyError.
    let topicless = Setup {
        id: "stats-topicless",
        cli: &[],
        files: &[
            PLAIN,
            ("tutor/decks/mine.yaml", MANY_TOPICS),
            (
                "tutor/card-reviews.jsonl",
                "{\"at\": \"2026-11-19T10:00:00+00:00\", \"card\": \"m-1\", \"rating\": \"good\"}\n",
            ),
        ],
    };
    refuses(
        &topicless,
        &["cards", "stats"],
        "cards.stats recent row without topic",
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

/// The files a `Setup` writes.
type Files = &'static [(&'static str, &'static str)];

const GAPS: (&str, &str) = (
    "jobs/strategy.json",
    "{\"skill_gaps\": [[\"kafka\", 2], [\"queues\", 1]]}",
);
const GAP_ITEMS: (&str, &str) = (
    "tutor/gap-items.yaml",
    "items:
- id: aaa-gap-kafka-1
  topic: kafka
  round: skill
  rubric: skill
  teach: \"Study K\u{e4}fka \u{2014} a log\"
  prompt: Explain kafka
  extra: [1, true, null, {a: b}]
- id: arrays-two-pointers-1
  topic: data structures
  round: coding
  rubric: coding
  origin: hand
  prompt: A replaced synthetic prompt
",
);
const OFF_FOCUS: (&str, &str) = (
    "tutor/gap-items.yaml",
    "items:\n- id: aaa-off-1\n  topic: gardening\n  round: skill\n  rubric: skill\n  prompt: Off focus\n",
);

#[test]
fn drill_next_matches_python() {
    strict(NOW);
    // Route key: "drill.next".
    let empty = prepare_setup(&Setup {
        id: "next-empty",
        cli: &[],
        files: &[],
    });
    assert_same(&empty, &["drill", "next"], true, "drill.next empty");
    assert_same(
        &empty,
        &["drill", "next", "--json"],
        true,
        "drill.next empty json",
    );
    let gaps = prepare_setup(&Setup {
        id: "next-gaps",
        cli: &[],
        files: &[GAPS, GAP_ITEMS],
    });
    assert_same(&gaps, &["drill", "next"], true, "drill.next gaps");
    assert_same(
        &gaps,
        &["drill", "next", "--json"],
        true,
        "drill.next gaps json",
    );
    // A generated item that replaces a base one keeps its place and wins.
    let replaced = prepare_setup(&Setup {
        id: "next-replaced",
        cli: &[],
        files: &[GAP_ITEMS],
    });
    assert_same(
        &replaced,
        &["drill", "next", "--json"],
        true,
        "drill.next replaced json",
    );
    let off = prepare_setup(&Setup {
        id: "next-off-focus",
        cli: &[],
        files: &[OFF_FOCUS],
    });
    assert_same(&off, &["drill", "next"], true, "drill.next off focus");
    let encrypted = prepare_setup(&Setup {
        id: "next-encrypted",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[GAPS, GAP_ITEMS],
    });
    assert_same(&encrypted, &["drill", "next"], true, "drill.next encrypted");
    let refused: [(&str, Files, &[&str]); 6] = [
        (
            "next-cards",
            &[("tutor/cards.json", "{}")],
            &["drill", "next"],
        ),
        (
            "next-attempts",
            &[("tutor/attempts.jsonl", "")],
            &["drill", "next"],
        ),
        ("next-corp", &[CORPORATE], &["drill", "next"]),
        ("next-terms", &[TERMS], &["drill", "next"]),
        (
            "next-float",
            &[(
                "tutor/gap-items.yaml",
                "items:\n- id: aaa-f-1\n  topic: behavioral\n  round: x\n  weight: 1.5\n",
            )],
            &["drill", "next", "--json"],
        ),
        (
            "next-no-rubric",
            &[(
                "tutor/gap-items.yaml",
                "items:\n- id: aaa-r-1\n  topic: behavioral\n  round: x\n  prompt: P\n",
            )],
            &["drill", "next"],
        ),
    ];
    for (id, files, args) in refused {
        refuses(
            &Setup {
                id,
                cli: &[],
                files,
            },
            args,
            id,
        );
    }
}

/// Negative control: the comparison sees the generated items.
#[test]
fn drill_next_output_follows_the_items() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "control-empty",
        cli: &[],
        files: &[],
    });
    let gaps = prepare_setup(&Setup {
        id: "control-gaps",
        cli: &[],
        files: &[GAPS, GAP_ITEMS],
    });
    let a = common::observe(&empty, &["drill", "next"], &common::Side::Native);
    let b = common::observe(&gaps, &["drill", "next"], &common::Side::Native);
    assert_ne!(a, b);
    assert!(a.stdout.starts_with("[learn] arrays-two-pointers-1 "));
    assert!(b.stdout.starts_with("[learn] aaa-gap-kafka-1 "));
}
