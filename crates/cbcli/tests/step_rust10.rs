//! rust-10 native handlers: `draft fit` and `draft check` must match the
//! Python console script byte for byte (stdout, exit code, data-root files,
//! event rows) in synthetic homes, and refuse every input they do not port.
//! All thread and draft text here is synthetic.
mod common;

use common::*;

const NOW: &str = "2026-11-20T09:00:00";

const ACK_THREAD: &str = r#"{"messages": [{"ts": "1700000000.000100", "text": "Could you rename the flag?", "mine": false}, {"ts": "1700000000.000200", "text": "thanks!", "mine": false}]}"#;
const DEPLOY_THREAD: &str = r#"{"messages": [{"ts": "1", "text": "Deploy started for the widget service.", "mine": false}, {"ts": "2", "text": "Rolling out to all regions now", "mine": false}]}"#;
const CHANGE_THREAD: &str = r#"[{"ts": 10, "text": "Here is the first version of the parser."}, {"ts": 11, "text": "I left two notes on the tests.", "author": "someone"}]"#;
const MINE_THREAD: &str = r#"{"messages": [{"text": "Can you look at the report?"}, {"text": "Looking now, will reply soon.", "mine": true}]}"#;
const UNICODE_THREAD: &str = "{\"messages\": [{\"ts\": \"1\", \"text\": \"Danke sch\u{f6}n\"}]}";
const FLOAT_THREAD: &str = r#"{"messages": [{"ts": 1.5, "text": "Shipped it."}]}"#;

const DRAFT_CHANGE: &str = "Could you rename it before we merge?\n";
const DRAFT_APPROVE: &str = "Looks good, thanks!\n";
const DRAFT_ACK: &str = "thanks\n";

fn fit_setup() -> Home {
    prepare_setup(&Setup {
        id: "draft-fit",
        cli: &[],
        files: &[
            ("in/ack.json", ACK_THREAD),
            ("in/deploy.json", DEPLOY_THREAD),
            ("in/change.json", CHANGE_THREAD),
            ("in/mine.json", MINE_THREAD),
            ("in/unicode.json", UNICODE_THREAD),
            ("in/float.json", FLOAT_THREAD),
            ("in/bad.json", "{\"messages\": [\n"),
            ("in/draft-change.txt", DRAFT_CHANGE),
            ("in/draft-approve.txt", DRAFT_APPROVE),
            ("in/draft-ack.txt", DRAFT_ACK),
            ("in/draft-crlf.txt", "Looks good\r\n"),
        ],
    })
}

fn path(home: &Home, rel: &str) -> String {
    home.data.join(rel).display().to_string()
}

fn check(home: &Home, words: &[&str], must_serve: bool, label: &str) -> bool {
    assert_same(home, words, must_serve, label)
}

#[test]
fn draft_fit_matches_python() {
    strict(NOW);
    let home = fit_setup();
    let p = |rel: &str| path(&home, rel);
    let (ack, deploy, change, mine) = (
        p("in/ack.json"),
        p("in/deploy.json"),
        p("in/change.json"),
        p("in/mine.json"),
    );
    let (d_change, d_approve, d_ack) = (
        p("in/draft-change.txt"),
        p("in/draft-approve.txt"),
        p("in/draft-ack.txt"),
    );
    // "draft.fit": text and JSON forms over each kind of result.
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["draft", "fit", "--thread", &ack], "small ack: emoji"),
        (
            vec!["draft", "fit", "--thread", &ack, "--json"],
            "small ack json",
        ),
        (
            vec![
                "draft",
                "fit",
                "--thread",
                &ack,
                "--seen",
                "1700000000.0001",
            ],
            "stale since seen",
        ),
        (
            vec!["draft", "fit", "--thread", &deploy],
            "deploy started: ack",
        ),
        (
            vec!["draft", "fit", "--thread", &deploy, "--draft", &d_approve],
            "deploy started: keep the approving draft",
        ),
        (
            vec![
                "draft", "fit", "--thread", &deploy, "--draft", &d_change, "--json",
            ],
            "deploy started: a change draft is replaced",
        ),
        (
            vec![
                "draft",
                "fit",
                "--thread",
                &deploy,
                "--major",
                "the totals are wrong",
            ],
            "deploy started with a major issue: stop",
        ),
        (
            vec![
                "draft",
                "fit",
                "--thread",
                &deploy,
                "--major",
                "the totals are wrong",
                "--json",
            ],
            "stop json",
        ),
        (vec!["draft", "fit", "--thread", &change], "reply, no draft"),
        (
            vec!["draft", "fit", "--thread", &change, "--draft", &d_change],
            "reply, draft fits",
        ),
        (
            vec!["draft", "fit", "--thread", &change, "--draft", &d_ack],
            "draft is a small ack",
        ),
        (
            vec![
                "draft", "fit", "--thread", &change, "--draft", &d_change, "--seen", "10",
            ],
            "stale reply draft",
        ),
        (
            vec!["draft", "fit", "--thread", &change, "--draft", "", "--json"],
            "an empty --draft is no draft",
        ),
        (
            vec!["draft", "fit", "--thread", &mine],
            "mine is newest: none",
        ),
        (
            vec!["draft", "fit", "--thread", &mine, "--json"],
            "none json",
        ),
    ];
    for (words, label) in &cases {
        check(&home, words, true, label);
    }
}

#[test]
fn draft_fit_refuses_what_it_does_not_port() {
    strict(NOW);
    let home = fit_setup();
    let p = |rel: &str| path(&home, rel);
    let (ack, unicode, float, bad, missing, crlf) = (
        p("in/ack.json"),
        p("in/unicode.json"),
        p("in/float.json"),
        p("in/bad.json"),
        p("in/missing.json"),
        p("in/draft-crlf.txt"),
    );
    let refused: Vec<(Vec<&str>, &str)> = vec![
        (vec!["draft", "fit", "--thread", &unicode], "non-ASCII text"),
        (vec!["draft", "fit", "--thread", &float], "a float ts"),
        (vec!["draft", "fit", "--thread", &bad], "invalid JSON"),
        (
            vec!["draft", "fit", "--thread", &missing],
            "a missing thread",
        ),
        (
            vec!["draft", "fit", "--thread", "~/thread.json"],
            "a ~ path",
        ),
        (
            vec!["draft", "fit", "--thread", &ack, "--draft", &crlf],
            "a CRLF draft",
        ),
        (
            vec!["draft", "fit", "--thread", &ack, "--seen", "soon"],
            "a ts that is not a number",
        ),
        (
            vec!["draft", "fit", "--thread", &ack, "--major", ""],
            "an empty major (Python's error)",
        ),
    ];
    for (words, label) in &refused {
        assert!(!check(&home, words, false, label), "{label}: served");
    }
    let redacted = prepare_setup(&Setup {
        id: "draft-fit-redaction",
        cli: &[],
        files: &[
            ("profile.yaml", "name: Synthetic Person\n"),
            ("in/ack.json", ACK_THREAD),
        ],
    });
    let ack = path(&redacted, "in/ack.json");
    assert!(
        !check(
            &redacted,
            &["draft", "fit", "--thread", &ack],
            false,
            "redaction input"
        ),
        "redaction input: served"
    );
}

const SENT: &str = "Thanks for the review.\nI fixed the parser.\n";

fn check_setup() -> Home {
    prepare_setup(&Setup {
        id: "draft-check",
        cli: &[],
        files: &[
            ("in/sent.txt", SENT),
            ("in/same.txt", SENT),
            (
                "in/spaces.txt",
                "Thanks for the review.   \r\nI fixed the parser.\r\n\r\n",
            ),
            (
                "in/other.txt",
                "Thanks for the review.\nI fixed the tests.\nMore soon.\n",
            ),
            ("in/short.txt", "Thanks for the review.\n"),
            (
                "in/sep.txt",
                "Thanks for the review.\u{1f}\nI fixed the parser.\n",
            ),
            ("in/dir/inside.txt", "x\n"),
        ],
    })
}

#[test]
fn draft_check_matches_python() {
    strict(NOW);
    let home = check_setup();
    let p = |rel: &str| path(&home, rel);
    let (sent, same, spaces, other, short, missing) = (
        p("in/sent.txt"),
        p("in/same.txt"),
        p("in/spaces.txt"),
        p("in/other.txt"),
        p("in/short.txt"),
        p("in/missing.txt"),
    );
    // "draft.check": matches, mismatches (exit 1) and a missing file (exit 2).
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (
            vec!["draft", "check", "--sent", &sent, "--read-back", &same],
            "identical",
        ),
        (
            vec!["draft", "check", "--sent", &sent, "--read-back", &spaces],
            "trailing spaces and CRLF",
        ),
        (
            vec!["draft", "check", "--read-back", &other, "--sent", &sent],
            "a differing line",
        ),
        (
            vec!["draft", "check", "--sent", &sent, "--read-back", &short],
            "a shorter read-back",
        ),
        (
            vec!["draft", "check", "--sent", &missing, "--read-back", &same],
            "a missing sent file",
        ),
        (
            vec!["draft", "check", "--sent", &sent, "--read-back", &missing],
            "a missing read-back",
        ),
    ];
    for (words, label) in &cases {
        check(&home, words, true, label);
    }
}

#[test]
fn draft_check_refuses_what_it_does_not_port() {
    strict(NOW);
    let home = check_setup();
    let p = |rel: &str| path(&home, rel);
    let (sent, sep, dir) = (p("in/sent.txt"), p("in/sep.txt"), p("in/dir"));
    let refused: Vec<(Vec<&str>, &str)> = vec![
        (
            vec!["draft", "check", "--sent", &sent, "--read-back", &dir],
            "a directory",
        ),
        (
            vec!["draft", "check", "--sent", &sep, "--read-back", &sent],
            "a separator Python strips",
        ),
        (
            vec![
                "draft",
                "check",
                "--sent",
                "~/sent.txt",
                "--read-back",
                &sent,
            ],
            "a ~ path",
        ),
        (
            vec!["draft", "check", "--sent", &sent],
            "a missing required flag",
        ),
    ];
    for (words, label) in &refused {
        assert!(!check(&home, words, false, label), "{label}: served");
    }
}

/// Negative control: the comparison sees a planted difference, so a wrong
/// native answer could not pass as equal.
#[test]
fn the_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = check_setup();
    let p = |rel: &str| path(&home, rel);
    let (sent, same, other) = (p("in/sent.txt"), p("in/same.txt"), p("in/other.txt"));
    let matched = observe(
        &home,
        &["draft", "check", "--sent", &sent, "--read-back", &same],
        &Side::Native,
    );
    let differed = observe(
        &home,
        &["draft", "check", "--sent", &sent, "--read-back", &other],
        &Side::Python,
    );
    assert_ne!(matched, differed);
    assert_eq!(matched.code, Some(0));
    assert_eq!(differed.code, Some(1));
    let fit = fit_setup();
    let ack = path(&fit, "in/ack.json");
    let deploy = path(&fit, "in/deploy.json");
    let a = observe(&fit, &["draft", "fit", "--thread", &ack], &Side::Native);
    let b = observe(&fit, &["draft", "fit", "--thread", &deploy], &Side::Python);
    assert_ne!(a.stdout, b.stdout);
    assert!(a.stdout.starts_with("draft: emoji"));
    assert!(b.stdout.starts_with("draft: ack"));
}

// ---------- work agent event, work claim take / release ----------

/// A sealed home (personal role) with three live sessions: `sess-1` on
/// `synth` holds a claim on `file:notes.md`, `sess-2` is on the quiet
/// project `calm`, and `sess-3` is on `other`. All names are synthetic.
fn work_setup() -> Home {
    prepare_setup(&Setup {
        id: "work-claims",
        cli: &[
            &["machine", "set", "personal"],
            &["config", "set", "timezone", "America/New_York"],
            &["work", "project", "add", "synth"],
            &["work", "project", "add", "calm"],
            &["work", "project", "add", "other"],
            &["work", "session", "open", "synth", "--id", "sess-1"],
            &["work", "session", "open", "calm", "--id", "sess-2"],
            &["work", "session", "open", "other", "--id", "sess-3"],
            &[
                "work",
                "project",
                "set",
                "calm",
                "--quiet-until",
                "2026-12-01",
            ],
            &[
                "work",
                "claim",
                "take",
                "file:notes.md",
                "--session",
                "sess-1",
            ],
            &["work", "agent", "event", "bot", "started the review"],
        ],
        files: &[],
    })
}

/// Plain stored files: a stale open session with an expired and a live
/// claim, an old ended session the next write prunes, and agent rows old
/// and new.
const OLD_SESSIONS: &str = r#"{"claims": [{"at": "2026-11-19T09:00:00-05:00", "resource": "browser:old", "session": "sess-9", "until": "2026-11-20T08:00:00+00:00"}, {"at": "2026-11-19T09:00:00-05:00", "resource": "browser:kept", "session": "sess-9", "until": "2026-11-20T23:00:00+00:00"}], "sessions": [{"day": "2026-11-19", "ended": "", "id": "sess-9", "project": "synth", "started": "2026-11-19T09:00:00-05:00"}, {"day": "2026-11-01", "ended": "2026-11-01T10:00:00-05:00", "id": "sess-8", "project": "synth", "started": "2026-11-01T09:00:00-05:00"}]}"#;
const OLD_AGENTS: &str = "{\"agent\": \"bot\", \"at\": \"2026-11-01T00:00:00+00:00\", \"project\": \"\", \"run\": \"bot\", \"status\": \"done\", \"title\": \"old\", \"url\": \"\"}\n\n{\"agent\": \"bot\", \"at\": \"2026-11-19T12:00:00+00:00\", \"project\": \"synth\", \"run\": \"r-1\", \"status\": \"running\", \"title\": \"recent\", \"url\": \"\"}\n";

fn old_setup() -> Home {
    prepare_setup(&Setup {
        id: "work-old-records",
        cli: &[&["config", "set", "timezone", "America/New_York"]],
        files: &[
            ("work/sessions.json", OLD_SESSIONS),
            ("work/projects.yaml", "projects:\n  synth: {}\n"),
            ("work/agents/events.jsonl", OLD_AGENTS),
        ],
    })
}

#[test]
fn work_claims_match_python() {
    strict(NOW);
    let home = work_setup();
    // "work.claim.take": taken, by project, renewed, a PR, and its errors.
    let take: Vec<(Vec<&str>, &str)> = vec![
        (
            vec![
                "work",
                "claim",
                "take",
                "browser:sso",
                "--session",
                "sess-1",
            ],
            "take a free resource",
        ),
        (
            vec![
                "work",
                "claim",
                "take",
                "browser:sso",
                "--session",
                "synth",
                "--minutes",
                "45",
            ],
            "take by project name",
        ),
        (
            vec![
                "work",
                "claim",
                "take",
                "file:notes.md",
                "--session",
                "sess-1",
            ],
            "renew its own claim",
        ),
        (
            vec![
                "work",
                "claim",
                "take",
                " browser:sso ",
                "--session",
                "sess-1",
            ],
            "a padded resource is stripped",
        ),
        (
            vec![
                "work",
                "claim",
                "take",
                "pr:Org/Repo#12",
                "--session",
                "sess-3",
            ],
            "a PR claim is lowercased",
        ),
        (
            vec![
                "work",
                "claim",
                "take",
                "browser:sso",
                "--session",
                "sess-1",
                "--minutes",
                "0",
            ],
            "zero minutes",
        ),
        (
            vec![
                "work",
                "claim",
                "take",
                "browser:sso",
                "--session",
                "sess-1",
                "--minutes",
                "481",
            ],
            "too many minutes",
        ),
        (
            vec!["work", "claim", "take", "browser:sso", "--session", "calm"],
            "a quiet project",
        ),
    ];
    // "work.claim.release": released, nothing held, a padded id.
    let release: Vec<(Vec<&str>, &str)> = vec![
        (
            vec![
                "work",
                "claim",
                "release",
                "file:notes.md",
                "--session",
                "sess-1",
            ],
            "release a held claim",
        ),
        (
            vec![
                "work",
                "claim",
                "release",
                "browser:sso",
                "--session",
                "sess-1",
            ],
            "release what it does not hold",
        ),
        (
            vec![
                "work",
                "claim",
                "release",
                "file:notes.md",
                "--session",
                "sess-3",
            ],
            "another session's claim stays",
        ),
        (
            vec![
                "work",
                "claim",
                "release",
                "file:notes.md",
                "--session",
                "sess-1 ",
            ],
            "a padded session id",
        ),
    ];
    for (words, label) in take.iter().chain(&release) {
        check(&home, words, true, label);
    }
    let old = old_setup();
    for (words, label) in [
        (
            vec![
                "work",
                "claim",
                "take",
                "browser:sso",
                "--session",
                "sess-9",
            ],
            "a stale session",
        ),
        (
            vec![
                "work",
                "claim",
                "release",
                "browser:kept",
                "--session",
                "sess-9",
            ],
            "release prunes old records and writes",
        ),
        (
            vec![
                "work",
                "claim",
                "release",
                "browser:none",
                "--session",
                "sess-9",
            ],
            "release nothing still prunes",
        ),
    ] {
        check(&old, &words, true, label);
    }
}

#[test]
fn work_agent_event_matches_python() {
    strict(NOW);
    let home = work_setup();
    let long_title = "a".repeat(201);
    let long_project = "p".repeat(81);
    // "work.agent.event": each status, a session's run, and each error.
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (
            vec!["work", "agent", "event", "bot", "did a thing"],
            "running",
        ),
        (
            vec![
                "work",
                "agent",
                "event",
                "bot",
                "  did   a  thing ",
                "--status",
                "done",
                "--project",
                " synth  core ",
                "--url",
                "https://example.com/x",
                "--run",
                "r-1",
            ],
            "done with every field",
        ),
        (
            vec!["work", "agent", "event", "bot", "x", "--status", "failed"],
            "failed",
        ),
        (
            vec!["work", "agent", "event", "bot", "x", "--session", "sess-1"],
            "a session sets run and project",
        ),
        (
            vec![
                "work",
                "agent",
                "event",
                "bot",
                "x",
                "--session",
                "sess-1",
                "--run",
                "sess-1",
            ],
            "the same run is allowed",
        ),
        (
            vec![
                "work",
                "agent",
                "event",
                "bot",
                "x",
                "--session",
                "sess-1",
                "--run",
                "other",
            ],
            "a conflicting run",
        ),
        (
            vec!["work", "agent", "event", "bot", "x", "--session", "calm"],
            "a quiet session",
        ),
        (
            vec!["work", "agent", "event", "bot", "   "],
            "an empty title",
        ),
        (
            vec!["work", "agent", "event", "bot", &long_title],
            "a long title",
        ),
        (
            vec![
                "work",
                "agent",
                "event",
                "bot",
                "x",
                "--project",
                &long_project,
            ],
            "a long project",
        ),
        (
            vec![
                "work",
                "agent",
                "event",
                "bot",
                "x",
                "--url",
                "ftp://example.com/a",
            ],
            "not http",
        ),
        (
            vec!["work", "agent", "event", "bot", "x", "--url", "https://"],
            "no host",
        ),
        (
            vec![
                "work",
                "agent",
                "event",
                "bot",
                "x",
                "--url",
                "example.com/a",
            ],
            "no scheme",
        ),
    ];
    for (words, label) in &cases {
        check(&home, words, true, label);
    }
    let old = old_setup();
    check(
        &old,
        &["work", "agent", "event", "bot", "next step", "--run", "r-1"],
        true,
        "old rows are dropped at the write",
    );
    check(
        &old,
        &["work", "agent", "event", "bot", "x", "--session", "sess-9"],
        true,
        "a stale session",
    );
}

#[test]
fn work_commands_refuse_what_they_do_not_port() {
    strict(NOW);
    let home = work_setup();
    let refused: Vec<(Vec<&str>, &str)> = vec![
        (
            vec![
                "work",
                "claim",
                "take",
                "browser:sso",
                "--session",
                "nobody",
            ],
            "work.claim.take: an unknown session (repr message)",
        ),
        (
            vec!["work", "claim", "take", "file:a b", "--session", "sess-1"],
            "work.claim.take: a bad resource (repr message)",
        ),
        (
            vec![
                "work",
                "claim",
                "take",
                "file:notes.md",
                "--session",
                "sess-3",
            ],
            "work.claim.take: held by another session",
        ),
        (
            vec![
                "work",
                "claim",
                "release",
                "browser:sso",
                "--session",
                "bad id!",
            ],
            "work.claim.release: a bad session id",
        ),
        (
            vec![
                "work",
                "claim",
                "release",
                "pr:a/b#01",
                "--session",
                "sess-1",
            ],
            "work.claim.release: a bad PR claim",
        ),
        (
            vec!["work", "agent", "event", "bad name!", "x"],
            "work.agent.event: a bad agent name",
        ),
        (
            vec!["work", "agent", "event", "bot", "caf\u{e9}"],
            "work.agent.event: a non-ASCII title",
        ),
        (
            vec![
                "work",
                "agent",
                "event",
                "bot",
                "x",
                "--url",
                "http://[::1]/",
            ],
            "work.agent.event: a bracketed host",
        ),
        (
            vec!["work", "agent", "event", "bot", "x", "--session", "nobody"],
            "work.agent.event: an unknown session",
        ),
    ];
    for (words, label) in &refused {
        assert!(!check(&home, words, false, label), "{label}: served");
    }
    // A store holding a float or a duplicate key refuses.
    let odd = prepare_setup(&Setup {
        id: "work-odd-records",
        cli: &[&["config", "set", "timezone", "America/New_York"]],
        files: &[
            (
                "work/sessions.json",
                r#"{"claims": [], "sessions": [], "sessions": [], "x": 1.5}"#,
            ),
            (
                "work/agents/events.jsonl",
                "{\"at\": \"2026-11-19T12:00:00+00:00\", \"n\": 0.1}\n",
            ),
        ],
    });
    for (words, label) in [
        (
            vec![
                "work",
                "claim",
                "release",
                "browser:sso",
                "--session",
                "sess-1",
            ],
            "work.claim.release: an odd store",
        ),
        (
            vec!["work", "agent", "event", "bot", "x"],
            "work.agent.event: a float in the log",
        ),
    ] {
        assert!(!check(&odd, &words, false, label), "{label}: served");
    }
    let redacted = prepare_setup(&Setup {
        id: "work-redaction",
        cli: &[],
        files: &[("profile.yaml", "name: Synthetic Person\n")],
    });
    assert!(
        !check(
            &redacted,
            &["work", "agent", "event", "bot", "x"],
            false,
            "work.agent.event: redact inputs present",
        ),
        "redaction: served"
    );
}

/// Negative control: a taken and a renewed claim differ in stdout, events
/// and the stored claim, so a wrong native answer could not pass as equal.
#[test]
fn work_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = work_setup();
    let taken = observe(
        &home,
        &[
            "work",
            "claim",
            "take",
            "browser:sso",
            "--session",
            "sess-1",
        ],
        &Side::Native,
    );
    let renewed = observe(
        &home,
        &[
            "work",
            "claim",
            "take",
            "file:notes.md",
            "--session",
            "sess-1",
        ],
        &Side::Python,
    );
    assert_ne!(taken, renewed);
    assert!(taken.stdout.starts_with("claimed browser:sso"));
    assert!(renewed.stdout.starts_with("renewed file:notes.md"));
    assert_eq!(taken.code, Some(0));
}
