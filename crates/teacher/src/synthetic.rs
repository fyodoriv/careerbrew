//! Synthetic rust-21 workload: the Rust side of `tests/oracles/rust21.py`.
//!
//! Each case mirrors the oracle line by line, in fresh invocation-owned
//! workspaces under `CAREERBREW_HOME/rust21-workload/`.

use crate::seams::{self, Seams};
use crate::{goals, now_utc, teacher, Ctx, Result};
use careerbrew_tutor::{
    cards, mistakes, read_json, read_jsonl, read_yaml, typed, write_json, write_jsonl, write_yaml,
};
use chrono::{Duration, NaiveDate};
use serde_json::{json, Map, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const CASES: &[&str] = &[
    "goals-queue",
    "goals-focus-blocks",
    "teacher-local-day",
    "teacher-flow",
    "teacher-cards-validate-resume",
    "teacher-improvements",
    "teacher-drafts",
];
const NOW: &str = "2026-10-19T13:00:00+00:00";
const FUTURE: &str = "2099-01-01T00:00:00+00:00";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// One workspace: the environment the oracle's `Workspace` sets, and its context.
fn workspace(base: &Path, name: &str) -> Result<Ctx> {
    let base = base.join(name);
    let root = base.join("data");
    let public = base.join("public");
    std::fs::create_dir_all(&root)?;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    std::fs::create_dir_all(&public)?;
    let repo = repo();
    std::env::set_var("HOME", base.join("home"));
    std::env::set_var("CAREERBREW_HOME", &root);
    std::env::set_var("CAREERBREW_KEY_FILE", base.join("keys/backup.key"));
    std::env::set_var("CAREERBREW_EVENTS", "off");
    std::env::set_var("CAREERBREW_ENCRYPT", "on");
    std::env::set_var("CAREERBREW_NOW", NOW);
    std::env::set_var("CAREERBREW_PUBLIC_CARDS", &public);
    std::env::set_var(
        "CAREERBREW_EXERCISES",
        repo.join("tests/fixtures/rust21/exercises"),
    );
    std::env::set_var("CAREERBREW_TEST_OWNER", "1");
    std::env::set_var("TZ", "UTC");
    write_yaml(&root, "settings.yaml", &json!({"timezone": "UTC"}))?;
    let seams = Seams::synthetic(&repo)?;
    Ctx::from_env(&repo, seams)
}

fn error_of<T>(result: Result<T>) -> Value {
    match result {
        Ok(_) => json!("accepted"),
        Err(error) => json!([error.class, error.message]),
    }
}

/// A step without its wall-clock start, recursively through `next`.
fn view(step: Value) -> Value {
    let Value::Object(map) = step else {
        return step;
    };
    let mut out = map
        .into_iter()
        .filter(|(key, _)| key != "started")
        .collect::<Map<_, _>>();
    if let Some(next) = out.remove("next") {
        out.insert("next".into(), view(next));
    }
    Value::Object(out)
}

fn done(ctx: &Ctx, step: &Value, result: Value) -> Result<Value> {
    let mut step = step.clone();
    step["started"] = json!(FUTURE);
    Ok(view(teacher::complete(ctx, &step, &result)?))
}

fn goals_file(ctx: &Ctx) -> Result<Value> {
    read_yaml(&ctx.root, "goals.yaml", json!({}))
}

fn queue(ctx: &Ctx, rows: Value) -> Result<Value> {
    goals::set_queue(ctx, rows.as_array().map_or(&[][..], Vec::as_slice))
}

fn goals_queue(base: &Path) -> Result<Value> {
    let ctx = workspace(base, "goals")?;
    let c = &ctx;
    let mut bad = Map::new();
    for (name, rows) in [
        ("kind", json!([{"kind": "vacation", "title": "x"}])),
        ("title", json!([{"kind": "learn", "title": "  "}])),
        (
            "minutes_text",
            json!([{"title": "x", "minutes_per_day": "ten"}]),
        ),
        (
            "minutes_none",
            json!([{"title": "x", "minutes_per_day": null}]),
        ),
        ("minutes_low", json!([{"title": "x", "minutes_per_day": 9}])),
        (
            "minutes_high",
            json!([{"title": "x", "minutes_per_day": "601"}]),
        ),
        ("date", json!([{"title": "x", "date": "2026-13-01"}])),
        (
            "duplicate",
            json!([{"title": "Same goal"}, {"title": "same  GOAL"}]),
        ),
    ] {
        bad.insert(name.into(), error_of(queue(c, rows)));
    }
    let untouched = ctx.root.join("goals.yaml").exists();
    let first = queue(
        c,
        json!([
            {"kind": "learn", "title": "  Learn   Arrays  ", "topics": ["Arrays and Hashing", " ", "Heaps"], "minutes_per_day": "45", "date": "2026-11-01"},
            {"kind": "interview", "title": "SynCo onsite", "company": "  SynCo ", "date": "2026-10-25", "minutes_per_day": 60.9},
            {"kind": "custom", "title": "Read more ✓ books!", "id": ""},
            {"kind": "promotion", "title": "Past goal", "date": "2026-10-18"},
        ]),
    )?;
    let upcoming = goals::load(c)?["upcoming"].clone();
    let replaced = goals::set_upcoming(
        c,
        Some(
            &json!({"kind": "learn", "title": "Learn trees", "date": "2026-10-21", "minutes_per_day": 30}),
        ),
    )?;
    let removed = goals::remove(c, "learn-trees")?;
    let emptied = goals::set_upcoming(c, None)?;
    let long_term = goals::set_long_term(
        c,
        &[
            json!({"text": "  Become   staff  "}),
            json!({"text": ""}),
            json!({"text": "Write a book", "by": "2027-06-01", "id": "book"}),
        ],
    )?;
    let bad_by = error_of(goals::set_long_term(
        c,
        &[json!({"text": "x", "by": "soon"})],
    ));
    write_yaml(
        &ctx.root,
        "goals.yaml",
        &json!({"upcoming": {"id": "legacy", "kind": "learn", "title": "Legacy", "date": "2026-12-01", "minutes_per_day": 40}}),
    )?;
    let legacy = goals::load(c)?;
    let many = queue(
        c,
        json!((0..22)
            .map(|n| json!({"title": format!("Goal {n}"), "date": format!("2026-11-{:02}", n + 1)}))
            .collect::<Vec<_>>()),
    )?;
    for n in 0..21 {
        goals::remove(c, &format!("goal-{n}"))?;
    }
    let history = goals_file(c)?["history"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let shapes = [
        Value::Null,
        json!({}),
        json!(["x"]),
        json!({"queue": [{"kind": "interview"}, {"kind": "learn"}, "x"], "history": [{"kind": "job-search"}], "upcoming": {"kind": "interview"}}),
    ]
    .iter()
    .map(|data| json!(goals::job_hunt_goals(data)))
    .collect::<Vec<_>>();
    let days = json!({
        "none": goals::days_left(c, Some(&json!({})), None)?,
        "undated": goals::days_left(c, Some(&json!({"title": "x"})), None)?,
        "ahead": goals::days_left(c, Some(&json!({"date": "2026-10-25"})), None)?,
        "past": goals::days_left(c, Some(&json!({"date": "2026-10-01"})), None)?,
    });
    let ctx = workspace(base, "goals-corporate")?;
    careerbrew_setup::machine::set_role("corporate", "")?;
    let corporate = error_of(queue(
        &ctx,
        json!([{"kind": "job-search", "title": "Find a job"}]),
    ));
    let corporate_learn =
        queue(&ctx, json!([{"kind": "learn", "title": "Learn graphs"}]))?["upcoming"]["id"].clone();
    let file = goals_file(&ctx)?;
    Ok(json!({
        "bad": bad,
        "untouched": untouched,
        "first": first,
        "upcoming": upcoming,
        "replaced": replaced,
        "removed": removed,
        "emptied": emptied,
        "long_term": long_term,
        "bad_by": bad_by,
        "legacy": legacy,
        "many_upcoming": many["upcoming"]["id"],
        "history": history.iter().map(|row| json!([row["id"], row["replaced_at"]])).collect::<Vec<_>>(),
        "history_count": history.len(),
        "shapes": shapes,
        "days": days,
        "corporate": corporate,
        "corporate_learn": corporate_learn,
        "file": file,
    }))
}

fn goals_focus_blocks(base: &Path) -> Result<Value> {
    let ctx = workspace(base, "focus")?;
    let c = &ctx;
    let goal_rows = [
        ("none", None),
        (
            "interview-known",
            Some(
                json!({"kind": "interview", "title": "a", "company": "SynCo", "topics": ["two pointers", "graphs", "kafka"]}),
            ),
        ),
        (
            "interview-quiet",
            Some(json!({"kind": "interview", "title": "a", "company": "QuietCo"})),
        ),
        (
            "interview-design",
            Some(json!({"kind": "interview", "title": "a", "company": "DesignCo"})),
        ),
        (
            "interview-unnamed",
            Some(json!({"kind": "interview", "title": "a", "topics": ["rust"]})),
        ),
        (
            "learn-patterns",
            Some(json!({"kind": "learn", "title": "a", "topics": ["trees", "rust"]})),
        ),
        (
            "learn-plain",
            Some(json!({"kind": "learn", "title": "a", "topics": ["rust"]})),
        ),
        (
            "job-search",
            Some(json!({"kind": "job-search", "title": "a", "topics": ["go"]})),
        ),
        (
            "promotion",
            Some(json!({"kind": "promotion", "title": "a"})),
        ),
        (
            "custom",
            Some(json!({"kind": "custom", "title": "a", "topics": ["x"]})),
        ),
    ];
    let mut focus = Map::new();
    for (name, goal) in goal_rows {
        focus.insert(name.into(), goals::focus(c, goal.as_ref())?);
    }
    let mut blocks = Map::new();
    blocks.insert("empty".into(), json!(goals::study_blocks(c, None)?));
    write_jsonl(
        &ctx.root,
        "tutor/exercise-attempts.jsonl",
        &[
            json!({"exercise": "syn-001", "at": NOW, "seconds": 30, "hints": 0, "correct": true, "score": 1.0, "passed": 3, "total": 3, "first": true, "complexity_ok": true, "code": "pass"}),
        ],
    )?;
    let plans = [
        (
            "interview",
            json!([{"kind": "interview", "title": "Onsite", "company": "SynCo", "date": "2026-10-23", "minutes_per_day": 95, "topics": ["arrays and hashing"]}]),
        ),
        (
            "learn-today",
            json!([{"kind": "learn", "title": "Today", "date": "2026-10-19", "minutes_per_day": 50}]),
        ),
        (
            "learn-undated",
            json!([{"kind": "learn", "title": "Someday", "minutes_per_day": 25, "topics": ["trees", "heap", "rust"]}]),
        ),
        (
            "promotion",
            json!([{"kind": "promotion", "title": "Promo", "date": "2026-12-01", "minutes_per_day": 120, "topics": ["impact", "scope", "x"]}]),
        ),
        (
            "job-search",
            json!([{"kind": "job-search", "title": "Search", "date": "2026-11-19", "minutes_per_day": 50}]),
        ),
        (
            "all-solved",
            json!([{"kind": "learn", "title": "Solved", "date": "2026-10-29", "topics": ["arrays and hashing"], "minutes_per_day": 10}]),
        ),
    ];
    for (name, rows) in plans {
        queue(c, rows)?;
        blocks.insert(name.into(), json!(goals::study_blocks(c, None)?));
    }
    write_jsonl(
        &ctx.root,
        "tutor/exercise-attempts.jsonl",
        &["syn-001", "syn-002"]
            .iter()
            .map(|e| json!({"exercise": e, "at": NOW, "seconds": 1, "correct": true, "score": 1.0}))
            .collect::<Vec<_>>(),
    )?;
    blocks.insert(
        "all-solved-later".into(),
        json!(goals::study_blocks(c, None)?),
    );
    let other = NaiveDate::from_ymd_opt(2026, 10, 30).expect("a fixed date");
    blocks.insert(
        "other-day".into(),
        json!(goals::study_blocks(c, Some(other))?),
    );
    let ctx = workspace(base, "focus-corporate")?;
    careerbrew_setup::machine::set_role("corporate", "")?;
    write_yaml(
        &ctx.root,
        "goals.yaml",
        &json!({"queue": [{"id": "x", "kind": "interview", "title": "x", "date": "2026-10-30"}]}),
    )?;
    blocks.insert("corporate".into(), json!(goals::study_blocks(&ctx, None)?));
    Ok(json!({"focus": focus, "blocks": blocks}))
}

fn teacher_local_day(base: &Path) -> Result<Value> {
    let ctx = workspace(base, "local-day")?;
    let c = &ctx;
    std::env::set_var("TZ", "Asia/Tokyo");
    write_yaml(
        &ctx.root,
        "settings.yaml",
        &json!({"timezone": "America/New_York"}),
    )?;
    std::env::set_var("CAREERBREW_NOW", "2026-10-20T03:30:00+00:00");
    write_jsonl(
        &ctx.root,
        "teach/steps.jsonl",
        &[
            json!({"at": "2026-10-19T03:00:00+00:00", "kind": "card", "outcome": "good", "seconds": 900, "ref": "c0", "rules": "x"}),
            json!({"at": "2026-10-19T14:00:00+00:00", "kind": "card", "outcome": "good", "seconds": 600, "ref": "c1", "rules": "x"}),
            json!({"at": "2026-10-20T03:00:00+00:00", "kind": "lesson", "outcome": "confirmed", "seconds": 1259, "ref": "trees", "rules": "x"}),
            json!({"at": "2026-10-20T05:00:00+00:00", "kind": "card", "outcome": "again", "seconds": 300, "ref": "c2", "rules": "x"}),
        ],
    )?;
    queue(
        c,
        json!([
            {"kind": "learn", "title": "Yesterday in UTC", "date": "2026-10-19", "minutes_per_day": 30},
            {"kind": "learn", "title": "Learn trees", "date": "2026-10-25", "minutes_per_day": 30, "topics": ["trees"]},
        ]),
    )?;
    let upcoming = goals::load(c)?["upcoming"]["id"].clone();
    let summary = teacher::summary(c)?;
    let finished = view(teacher::next_step(c)?);
    queue(
        c,
        json!([{"kind": "learn", "title": "Learn trees", "date": "2026-10-25", "minutes_per_day": 31, "topics": ["trees"]}]),
    )?;
    let under = view(teacher::next_step(c)?);
    std::env::set_var("CAREERBREW_NOW", "2026-10-20T05:30:00+00:00");
    let late = teacher::summary(c)?;
    Ok(
        json!({"upcoming": upcoming, "summary": summary, "done": finished, "under_budget": under, "after_midnight": late}),
    )
}

fn teacher_flow(base: &Path) -> Result<Value> {
    let ctx = workspace(base, "flow")?;
    let c = &ctx;
    write_json(
        &ctx.root,
        "approvals.json",
        &json!([{"id": "ap000001", "title": "Synthetic approval", "detail": "", "by": "agent", "at": "2026-10-19T12:00:00+00:00", "status": "pending", "note": ""}]),
    )?;
    let mut out = Map::new();
    let step = teacher::next_step(c)?;
    out.insert("approve".into(), view(step.clone()));
    out.insert(
        "approve_done".into(),
        done(
            c,
            &step,
            json!({"decision": "approved", "note": "  synthetic ok  "}),
        )?,
    );
    out.insert(
        "approvals".into(),
        read_json(&ctx.root, "approvals.json", json!([]))?,
    );
    queue(
        c,
        json!([{"kind": "learn", "title": "Learn arrays", "topics": ["arrays and hashing", "heaps"], "minutes_per_day": 30, "date": "2026-11-01"}]),
    )?;
    mistakes::record(
        &ctx.root,
        &ctx.public,
        "syn-001",
        "arrays and hashing",
        "edge-case",
        "empty input",
        None,
    )?;
    mistakes::record(
        &ctx.root,
        &ctx.public,
        "syn-001",
        "arrays and hashing",
        "edge-case",
        "empty input again",
        None,
    )?;
    let step = teacher::next_step(c)?;
    out.insert("mistake".into(), view(step.clone()));
    out.insert("mistake_done".into(), done(c, &step, json!({}))?);
    let lesson = teacher::next_step(c)?;
    out.insert(
        "lesson_not_yet".into(),
        done(c, &lesson, json!({"confirmed": false}))?,
    );
    let again = teacher::next_step(c)?;
    out.insert("lesson_again".into(), view(again.clone()));
    teacher::save_draft(c, "syn-001", "def pair_count(nums):\n    return 0\n")?;
    out.insert(
        "lesson_confirmed".into(),
        done(c, &again, json!({"confirmed": true}))?,
    );
    for code in ["fail-edge", "fail-wrong", "fail-error"] {
        let step = teacher::next_step(c)?;
        out.insert(
            format!("exercise_{code}"),
            done(c, &step, json!({"code": code}))?,
        );
    }
    let step = teacher::next_step(c)?;
    out.insert("second_exercise".into(), view(step.clone()));
    out.insert(
        "second_pass".into(),
        done(c, &step, json!({"code": "pass"}))?,
    );
    std::env::set_var("CAREERBREW_NOW", "2026-10-23T13:00:00+00:00");
    let step = teacher::next_step(c)?;
    out.insert("retry".into(), view(step.clone()));
    out.insert(
        "retry_done".into(),
        done(c, &step, json!({"code": "pass-slow"}))?,
    );
    let step = teacher::next_step(c)?;
    out.insert(
        "two_pointers_confirmed".into(),
        done(c, &step, json!({"confirmed": true}))?,
    );
    let step = teacher::next_step(c)?;
    out.insert(
        "two_pointers_timeout".into(),
        done(c, &step, json!({"code": "fail-timeout"}))?,
    );
    let step = teacher::next_step(c)?;
    out.insert(
        "two_pointers_plain".into(),
        done(c, &step, json!({"code": "pass-plain"}))?,
    );
    out.insert("state".into(), teacher::state(c)?);
    out.insert("log".into(), json!(teacher::log(c)?));
    out.insert("attempts".into(), json!(seams::attempts(c, None)?));
    out.insert("mistakes".into(), mistakes::summary(&ctx.root)?);
    out.insert("summary".into(), teacher::summary(c)?);
    Ok(Value::Object(out))
}

fn card(id: &str, question: &str, answer: &str, topic: &str) -> Value {
    json!({"id": id, "question": question, "answer": answer, "topic": topic, "tags": [topic]})
}

fn teacher_cards_validate_resume(base: &Path) -> Result<Value> {
    let ctx = workspace(base, "cards")?;
    let c = &ctx;
    write_yaml(
        &ctx.root,
        "settings.yaml",
        &json!({"timezone": "UTC", "cards_new_per_day": 0}),
    )?;
    queue(
        c,
        json!([{"kind": "promotion", "title": "Promo case", "minutes_per_day": 60}]),
    )?;
    cards::write_deck(
        &ctx.root,
        "mine",
        &[
            card("m1", "Synthetic mine one?", "One.", "promo"),
            card("m2", "Synthetic mine two?", "Two.", "promo"),
        ],
    )?;
    cards::write_deck(
        &ctx.root,
        "notion",
        &[card("n1", "Synthetic notion?", "Notion.", "notes")],
    )?;
    cards::review(
        &ctx.root,
        &ctx.public,
        "m1",
        "good",
        now_utc(&ctx.root) - Duration::days(30),
        None,
    )?;
    let mut out = Map::new();
    let step = teacher::next_step(c)?;
    out.insert("card".into(), view(step.clone()));
    out.insert(
        "card_done".into(),
        done(c, &step, json!({"rating": "again"}))?,
    );
    for (decision, extra) in [
        ("keep", json!({})),
        ("fix", json!({"answer": "A fixed synthetic answer."})),
        ("drop", json!({})),
    ] {
        let step = teacher::next_step(c)?;
        out.insert(format!("validate_{decision}"), view(step.clone()));
        let mut result = json!({"decision": decision});
        for (key, value) in extra.as_object().into_iter().flatten() {
            result[key] = value.clone();
        }
        out.insert(format!("validate_{decision}_done"), done(c, &step, result)?);
    }
    write_json(
        &ctx.root,
        "jobs/strategy.json",
        &json!({"tiers": {"practice": [{"posting_id": "p1", "company": "SynCo", "skills": ["a", "b", "c", "d", "e", "f"]}], "target": [{"posting_id": "p2", "company": "OtherCo"}], "dream": [{"posting_id": "p3", "company": "synco"}]}}),
    )?;
    let mut step = Value::Null;
    // Capped: a state that never changes must not hang the component.
    for _ in 0..10 {
        step = teacher::next_step(c)?;
        if step["kind"] != "validate" {
            break;
        }
        done(c, &step, json!({"decision": "keep"}))?;
    }
    out.insert("resume".into(), view(step.clone()));
    out.insert("resume_sent".into(), done(c, &step, json!({"sent": true}))?);
    let step = teacher::next_step(c)?;
    out.insert(
        "resume_skip".into(),
        done(c, &step, json!({"sent": false}))?,
    );
    out.insert(
        "applications".into(),
        json!(read_jsonl(&ctx.root, "jobs/applications.jsonl")?),
    );
    out.insert("state".into(), teacher::state(c)?);
    let ordered = teacher::ordered_cards(c)?;
    let mut decks = Map::new();
    for deck in ["mine", "notion"] {
        decks.insert(
            deck.into(),
            json!(ordered
                .iter()
                .filter(|card| card["deck"] == deck)
                .map(|card| card["id"].clone())
                .collect::<Vec<_>>()),
        );
    }
    out.insert("decks".into(), Value::Object(decks));
    out.insert(
        "suspended".into(),
        cards::stats(&ctx.root, &ctx.public, careerbrew_tutor::now())?["suspended"].clone(),
    );
    Ok(Value::Object(out))
}

fn teacher_improvements(base: &Path) -> Result<Value> {
    let ctx = workspace(base, "improve")?;
    let c = &ctx;
    let mut rows = [60, 60, 61, 59, 60, 400]
        .iter()
        .enumerate()
        .map(|(n, s)| json!({"at": NOW, "kind": "card", "outcome": "good", "seconds": s, "ref": format!("c{n}"), "rules": "x"}))
        .collect::<Vec<_>>();
    rows.extend(["trees", "heap", "graphs", "stack"].iter().map(|r| {
        json!({"at": NOW, "kind": "lesson", "outcome": "not-yet", "seconds": 30, "ref": r, "rules": "x"})
    }));
    rows.extend((0..2).map(|_| {
        json!({"at": NOW, "kind": "resume", "outcome": "skipped", "seconds": 5, "ref": "", "rules": "x"})
    }));
    write_jsonl(&ctx.root, "teach/steps.jsonl", &rows)?;
    write_jsonl(
        &ctx.root,
        "tutor/exercise-attempts.jsonl",
        &[
            ("syn-001", false),
            ("syn-002", false),
            ("syn-001", false),
            ("syn-001", false),
            ("syn-002", true),
            ("syn-002", false),
        ]
        .iter()
        .map(|(e, ok)| json!({"exercise": e, "at": NOW, "correct": ok, "score": 0.0}))
        .collect::<Vec<_>>(),
    )?;
    for detail in ["first", "second", "third"] {
        mistakes::record(
            &ctx.root,
            &ctx.public,
            "syn-003",
            "two pointers",
            "exception",
            detail,
            None,
        )?;
    }
    mistakes::record(
        &ctx.root,
        &ctx.public,
        "lesson:trees",
        "trees",
        "concept",
        "not yet",
        None,
    )?;
    mistakes::record(
        &ctx.root,
        &ctx.public,
        "lesson:trees",
        "trees",
        "concept",
        "not yet",
        None,
    )?;
    mistakes::record(
        &ctx.root,
        &ctx.public,
        "syn-002",
        "arrays and hashing",
        "complexity",
        "expected O(n)",
        None,
    )?;
    cards::write_deck(&ctx.root, "mine", &[card("m1", "Leech?", "Yes.", "x")])?;
    cards::suspend(&ctx.root, &ctx.public, "m1")?;
    let feedback = vec![
        teacher::feedback(c, "  Too   many\ncards  ", "card", Some(1))?,
        teacher::feedback(c, &"x".repeat(2100), &"c".repeat(40), None)?,
        teacher::feedback(c, "Cards again", "card", Some(2))?,
        teacher::feedback(c, "Old complaint", "lesson", Some(1))?,
    ];
    let mut written = read_jsonl(&ctx.root, "teach/feedback.jsonl")?;
    written.push(
        json!({"at": "2026-09-01T00:00:00+00:00", "text": "old", "step": "lesson", "rating": 1}),
    );
    write_jsonl(&ctx.root, "teach/feedback.jsonl", &written)?;
    let improvements = teacher::improvements(c)?;
    let low = teacher::low_rated_kinds(c)?.into_iter().collect::<Vec<_>>();
    queue(
        c,
        json!([{"kind": "learn", "title": "Learn arrays", "topics": ["arrays and hashing"], "minutes_per_day": 600}]),
    )?;
    cards::write_deck(&ctx.root, "review", &[card("r1", "Due?", "Due.", "x")])?;
    cards::review(
        &ctx.root,
        &ctx.public,
        "r1",
        "good",
        now_utc(&ctx.root) - Duration::days(30),
        None,
    )?;
    write_yaml(
        &ctx.root,
        "settings.yaml",
        &json!({"timezone": "UTC", "cards_new_per_day": 0}),
    )?;
    teacher::save_state(
        c,
        &json!({"confirmed_patterns": ["arrays and hashing"], "last_alarm": "9999"}),
    )?;
    let moved = view(teacher::next_step(c)?);
    let watch = teacher::watch_out(c, &seams::get(c, "syn-003")?)?;
    Ok(json!({
        "feedback": feedback,
        "improvements": improvements,
        "low": low,
        "moved_later": moved,
        "watch_out": watch,
        "rules_version": teacher::rules_version(c)?,
    }))
}

fn teacher_drafts(base: &Path) -> Result<Value> {
    let ctx = workspace(base, "drafts")?;
    let c = &ctx;
    let errors = vec![
        error_of(teacher::save_draft(c, "", "x")),
        error_of(teacher::save_draft(c, "syn-001", &"x".repeat(100_001))),
    ];
    teacher::save_draft(c, "syn-001", "first")?;
    teacher::save_draft(c, "syn-002", &"x".repeat(100_000))?;
    teacher::save_draft(c, "syn-001", "second ✓")?;
    // Never reported: the privacy canary.
    teacher::save_draft(c, "syn-003", "RUST21CANARYCODE")?;
    let drafts = teacher::drafts(c)?;
    let ids = drafts.keys().cloned().collect::<Vec<_>>();
    Ok(json!({
        "errors": errors,
        "ids": ids,
        "first": drafts["syn-001"],
        "long": drafts["syn-002"].as_str().map_or(0, |text| text.chars().count()),
        "no_goal": view(teacher::next_step(c)?),
    }))
}

fn call(cases: &mut Vec<Value>, name: &str, result: Result<Value>) {
    let value = match result {
        Ok(value) => value,
        Err(error) => json!({"error": error.class, "message": error.message}),
    };
    cases.push(json!({"id": name, "result": typed(&value)}));
}

pub fn run_all(selected: Option<&str>) -> Result<String> {
    let base = careerbrew_tutor::root()?.join("rust21-workload");
    std::fs::create_dir_all(&base)?;
    let mut cases = Vec::new();
    for name in CASES {
        if selected.is_some_and(|selected| selected != *name) {
            continue;
        }
        let result = match *name {
            "goals-queue" => goals_queue(&base),
            "goals-focus-blocks" => goals_focus_blocks(&base),
            "teacher-local-day" => teacher_local_day(&base),
            "teacher-flow" => teacher_flow(&base),
            "teacher-cards-validate-resume" => teacher_cards_validate_resume(&base),
            "teacher-improvements" => teacher_improvements(&base),
            "teacher-drafts" => teacher_drafts(&base),
            _ => Err(crate::err("ValueError", "unknown synthetic case")),
        };
        call(&mut cases, name, result);
    }
    match std::env::var("CAREERBREW_RUST21_PLANT_WRITE").as_deref() {
        Ok("extra") => write_json(
            &careerbrew_tutor::root()?,
            "planted-extra.json",
            &json!({"synthetic": true}),
        )?,
        Ok("missing") if selected == Some("teacher-drafts") => {
            std::fs::remove_file(base.join("drafts/data/teach/drafts.json"))?;
        }
        _ => {}
    }
    Ok(format!(
        "{}\n",
        careerbrew_tutor::compact(&json!({"cases": cases}))?
    ))
}
