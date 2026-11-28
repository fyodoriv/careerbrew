//! rust-18 native handlers: parity with the Python CLI in synthetic homes
//! for "work.note", "work.gaps", "ladder.list", "ladder.levels",
//! "ladder.import", "work.plan" and "work.standup".
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

/// A ladder to import: flow lists, a date, unicode and a line long enough
/// for PyYAML to fold.
const IMPORT_LADDER: &str = "id: imported-ladder\nname: Imported synthetic ladder\nadded: 2026-11-01\nlevels: [one, two]\ncompetencies:\n  - id: craft\n    name: Synthetic craft — done well\n    weight: 2.5\n    signals: {tags: [delivery], keywords: [launch, 'ship it']}\n    expect: {one: 1, two: 3}\n    how_to_close: Ship one synthetic thing that people use every single day, then write down what changed for them.\n    descriptions:\n      two: Ships synthetic things often.\n  - id: care\n    name: Synthetic care\n    weight: 1\n    signals: {tags: []}\n    expect: {one: 0, two: 1}\n";

/// The source file is inside the data root, so both sides see one path.
fn import_args(home: &Home, name: &str) -> Vec<String> {
    let path = home.data.join("import").join(name);
    vec!["ladder".into(), "import".into(), path.display().to_string()]
}

fn same_owned(home: &Home, args: &[String], label: &str) {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    same(home, &args, label);
}

fn refused_owned(home: &Home, args: &[String], label: &str) {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    refused(home, &args, label);
}

#[test]
fn ladder_import_matches_python() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "import",
        cli: &[],
        files: &[
            (".plaintext", ""),
            ("import/good.yaml", IMPORT_LADDER),
            ("import/again.yaml", IMPORT_LADDER),
            ("import/bad.yaml", BAD_LADDER),
            ("import/star.yaml", USER_LADDER),
            (
                "ladders/imported-ladder.yaml",
                "id: imported-ladder\nlevels: [one]\ncompetencies:\n  - {id: a, name: A, weight: 1, signals: {}, expect: {one: 1}}\n",
            ),
        ],
    });
    same_owned(
        &home,
        &import_args(&home, "good.yaml"),
        "ladder.import overwrites a ladder",
    );
    let fresh = prepare_setup(&Setup {
        id: "import-fresh",
        cli: &[],
        files: &[(".plaintext", ""), ("import/good.yaml", IMPORT_LADDER)],
    });
    same_owned(
        &fresh,
        &import_args(&fresh, "good.yaml"),
        "ladder.import new ladder",
    );
    let words = import_args(&fresh, "good.yaml");
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let seen = observe(&fresh, &words, &Side::Native);
    assert_eq!(seen.stdout, "imported ladder imported-ladder\n");
    assert!(seen
        .files
        .iter()
        .any(|line| line.starts_with("ladders/imported-ladder.yaml ")));
    refused_owned(
        &home,
        &import_args(&home, "bad.yaml"),
        "ladder.import invalid ladder",
    );
    refused_owned(
        &home,
        &import_args(&home, "star.yaml"),
        "ladder.import glob keyword (alias syntax)",
    );
    refused_owned(
        &home,
        &import_args(&home, "missing.yaml"),
        "ladder.import missing file",
    );
    let sealed = prepare_setup(&Setup {
        id: "import-sealed",
        cli: &[],
        files: &[("import/good.yaml", IMPORT_LADDER)],
    });
    // A sealed home refuses; either way the result must equal Python's.
    let args = import_args(&sealed, "good.yaml");
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    assert_same(&sealed, &words, false, "ladder.import sealed home");
}

/// Tasks for the plan: tagged and untagged ones, a due date, a done task,
/// a fraction of an hour, and more hours than the default capacity.
const TASKS: (&str, &str) = (
    "work/tasks.yaml",
    "tasks:\n- {id: t1, title: Synthetic design doc, impact: 4, urgency: 2, estimate_h: 2, competencies: [design]}\n- {id: t2, title: Ship synthetic release, impact: 3, urgency: 3, estimate_h: 1.5}\n- {id: t3, title: Synthetic cleanup, impact: 2, urgency: 1, estimate_h: 3, due: 2026-11-21}\n- {id: t4, title: Done synthetic thing, status: done, impact: 5, estimate_h: 1}\n- {id: t5, title: Mentor a synthetic peer, impact: 3, urgency: 4, estimate_h: 0.5, competencies: [leadership, collaboration]}\n- {id: t6, title: Big synthetic migration, impact: 5, urgency: 5, estimate_h: 7}\n",
);

#[test]
fn work_plan_matches_python() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "plan-empty",
        cli: &[],
        files: &[ZONE],
    });
    same(&empty, &["work", "plan"], "work.plan empty home");
    let tasks = prepare_setup(&Setup {
        id: "plan-tasks",
        cli: &[&["work", "note", "feat: synthetic launch"]],
        files: &[ZONE, TASKS],
    });
    same(&tasks, &["work", "plan"], "work.plan tasks");
    let seen = observe(&tasks, &["work", "plan"], &Side::Native);
    assert!(seen.stdout.contains(" score ") && seen.stdout.contains(" promo+"));
    assert!(seen.events.iter().any(|row| row.contains("work.plan")));
    same(
        &tasks,
        &["work", "plan", "--hours", "2.5"],
        "work.plan fewer hours",
    );
    same(
        &tasks,
        &["work", "plan", "--hours", "0"],
        "work.plan no hours",
    );
    same(
        &tasks,
        &["work", "plan", "--hours", "40", "--level", "staff"],
        "work.plan staff level",
    );
    let ranked = prepare_setup(&Setup {
        id: "plan-projects",
        cli: &[],
        files: &[
            (
                "settings.yaml",
                "timezone: Etc/UTC\nproject_priority: [synthetic]\n",
            ),
            TASKS,
        ],
    });
    refused(&ranked, &["work", "plan"], "work.plan project priority");
    let old = prepare_setup(&Setup {
        id: "plan-old-link",
        cli: &[],
        files: &[
            ZONE,
            (
                "work/tasks.yaml",
                "tasks:\n- {id: t1, title: Synthetic, link: 'https://example.com/a'}\n",
            ),
        ],
    });
    refused(&old, &["work", "plan"], "work.plan old link field");
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

const PLAIN: (&str, &str) = (".plaintext", "");

const ROWS: &str = "{\"rows\": [{\"title\": \"Synthetic export\", \"status\": \"In review\", \"owner\": \"me\", \"keys\": [\"SYN-12\"], \"repos\": [\"synthetic/shop\"], \"mine\": true}, {\"title\": \"Synthetic search\", \"key\": \"SYN-7\", \"aliases\": \"search\"}], \"groups\": [], \"ticket_projects\": [\"SYN\"], \"saved_at\": \"2026-11-20T08:40:00+00:00\"}";

const ROWS_OLD: &str =
    "{\"rows\": [{\"title\": \"Synthetic export\"}], \"saved_at\": \"2026-11-18T08:40:00+00:00\"}";

const ROWS_CLEARED: &str = "{\"rows\": [], \"saved_at\": \"2026-11-20T08:40:00+00:00\"}";

const ROWS_GROUPED: &str = "{\"rows\": [{\"title\": \"Synthetic export\"}], \"groups\": [{\"label\": \"Tooling\", \"repos\": [\"synthetic-tools\"]}]}";

const EVIDENCE_OLD: &str = "{\"id\": \"note:1\", \"kind\": \"note\", \"title\": \"Synthetic old note\", \"at\": \"2026-11-10T10:00:00+00:00\"}\n{\"id\": \"note:2\", \"kind\": \"note\", \"title\": \"Synthetic naive note\", \"at\": \"2026-11-01T10:00\"}\n{\"id\": \"note:3\", \"kind\": \"note\", \"title\": \"Synthetic personal\", \"at\": \"2026-11-19T12:00:00+00:00\", \"personal\": true}\n";

const FEED_QUIET: &str = "{\"id\": \"slack:1\", \"source\": \"slack\", \"kind\": \"dm\", \"actor\": \"me\", \"title\": \"Synthetic chat\", \"at\": \"2026-11-19T12:00:00+00:00\"}\n{\"id\": \"gh:1\", \"source\": \"github\", \"kind\": \"pr\", \"actor\": \"other\", \"title\": \"Synthetic review\", \"at\": \"2026-11-19T12:00:00+00:00\"}\n";

const EVIDENCE_NEW: &str = "{\"id\": \"note:9\", \"kind\": \"note\", \"title\": \"Synthetic new note\", \"at\": \"2026-11-19T12:00:00+00:00\"}\n";

/// A plaintext home whose `settings.yaml` is `settings`.
fn standup_home(id: &'static str, settings: &'static str) -> Home {
    let files: &'static [(&'static str, &'static str)] =
        Box::leak(vec![("settings.yaml", settings), PLAIN].into_boxed_slice());
    prepare_setup(&Setup {
        id,
        cli: &[],
        files,
    })
}

#[test]
fn work_standup_matches_python() {
    strict(NOW);
    let empty = prepare_setup(&Setup {
        id: "standup-empty",
        cli: &[],
        files: &[ZONE, PLAIN],
    });
    same(&empty, &["work", "standup"], "work.standup empty home");
    same(
        &empty,
        &["work", "standup", "--show"],
        "work.standup --show",
    );
    for (id, settings, label) in [
        (
            "standup-monday",
            "timezone: Etc/UTC\nstandup_days: [0]\nstandup_time: '08:30'\n",
            "work.standup Monday only",
        ),
        (
            "standup-since-last",
            "timezone: Etc/UTC\nstandup_window: since-last\nstandup_time: '08:00'\nstandup_minutes: 30\n",
            "work.standup since-last today",
        ),
        (
            "standup-zone",
            "timezone: Etc/UTC\nstandup_tz: America/New_York\n",
            "work.standup standup zone",
        ),
    ] {
        same(&standup_home(id, settings), &["work", "standup"], label);
    }
    let rows = prepare_setup(&Setup {
        id: "standup-rows",
        cli: &[],
        files: &[
            ZONE,
            PLAIN,
            ("work/standup.json", ROWS),
            ("work/evidence.jsonl", EVIDENCE_OLD),
            ("work/feed/events.jsonl", FEED_QUIET),
        ],
    });
    same(&rows, &["work", "standup"], "work.standup rows all hidden");
    same(
        &rows,
        &["work", "standup", "--show"],
        "work.standup rows --show",
    );
    let old = prepare_setup(&Setup {
        id: "standup-rows-old",
        cli: &[],
        files: &[ZONE, PLAIN, ("work/standup.json", ROWS_OLD)],
    });
    same(
        &old,
        &["work", "standup"],
        "work.standup rows saved days ago",
    );
    let cleared = prepare_setup(&Setup {
        id: "standup-cleared",
        cli: &[],
        files: &[ZONE, PLAIN, ("work/standup.json", ROWS_CLEARED)],
    });
    same(&cleared, &["work", "standup"], "work.standup rows cleared");
}

#[test]
fn work_standup_refuses_what_it_cannot_match() {
    strict(NOW);
    let new = prepare_setup(&Setup {
        id: "standup-new-evidence",
        cli: &[],
        files: &[
            ZONE,
            PLAIN,
            ("work/standup.json", ROWS),
            ("work/evidence.jsonl", EVIDENCE_NEW),
        ],
    });
    refused(
        &new,
        &["work", "standup"],
        "work.standup evidence in the window",
    );
    let notes = prepare_setup(&Setup {
        id: "standup-notes",
        cli: &[],
        files: &[
            ZONE,
            PLAIN,
            ("work/standup.json", ROWS),
            ("work/standup-notes.json", "{}"),
        ],
    });
    refused(
        &notes,
        &["work", "standup"],
        "work.standup agent notes file",
    );
    let grouped = prepare_setup(&Setup {
        id: "standup-groups",
        cli: &[],
        files: &[ZONE, PLAIN, ("work/standup.json", ROWS_GROUPED)],
    });
    refused(&grouped, &["work", "standup"], "work.standup groups");
    refused(
        &grouped,
        &["work", "standup", "--rows", "-", "--talk", "-"],
        "work.standup two stdin inputs",
    );
    let input = prepare_setup(&Setup {
        id: "standup-input",
        cli: &[],
        files: &[ZONE, PLAIN, ("rows.json", "[]")],
    });
    let file = input.data.join("rows.json").display().to_string();
    refused(
        &input,
        &["work", "standup", "--rows", &file],
        "work.standup --rows",
    );
}

/// Negative control for "work.standup": another standup time gives another
/// window, and the comparison sees it.
#[test]
fn work_standup_comparison_detects_a_planted_difference() {
    strict(NOW);
    let ten = prepare_setup(&Setup {
        id: "standup-control-ten",
        cli: &[],
        files: &[ZONE, PLAIN],
    });
    let eight = standup_home(
        "standup-control-eight",
        "timezone: Etc/UTC\nstandup_time: '08:00'\n",
    );
    let args = ["work", "standup"];
    let a = observe(&ten, &args, &Side::Native);
    let b = observe(&eight, &args, &Side::Python);
    assert_eq!(a.code, Some(0));
    assert_ne!(a, b);
    assert!(a.stdout.contains("10:00"));
    assert!(b.stdout.contains("08:00"));
}
