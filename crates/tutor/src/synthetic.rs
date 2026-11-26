//! Synthetic rust-20 workload over native tutor transactions.

use crate::{
    cards, compact, emit, engine, field, fsrs, mistakes, notion, public, read_json, read_jsonl,
    read_yaml, text, typed, write_json, write_jsonl, write_yaml, Result, TutorError,
};
use chrono::{DateTime, Duration, FixedOffset, TimeZone, Utc};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const CASES: &[&str] = &[
    "depvec-fsrs",
    "fsrs-old-state",
    "cards-and-decks",
    "local-day-stats",
    "public-deck",
    "notion-idempotence",
    "notion-validation-and-partial-writes",
    "providers-and-mistakes",
];
const SECTIONS: &[&str] = &[
    "## In plain words",
    "## How it works",
    "## Example",
    "## Common mistakes",
    "## In the interview",
];

struct Workspace {
    base: PathBuf,
    root: PathBuf,
    public: PathBuf,
}

impl Workspace {
    fn new(base: &Path, name: &str) -> Result<Self> {
        let base = base.join(name);
        let root = base.join("data");
        let public = base.join("public");
        std::fs::create_dir_all(&root)?;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        std::fs::create_dir_all(&public)?;
        std::env::set_var("HOME", base.join("home"));
        std::env::set_var("CAREERBREW_HOME", &root);
        std::env::set_var("CAREERBREW_KEY_FILE", base.join("keys/backup.key"));
        std::env::set_var("CAREERBREW_EVENTS", "off");
        std::env::set_var("CAREERBREW_ENCRYPT", "on");
        std::env::set_var("CAREERBREW_NOW", "2026-10-19T13:00:00+00:00");
        std::env::set_var("CAREERBREW_PUBLIC_CARDS", &public);
        std::env::set_var("CAREERBREW_TEST_OWNER", "1");
        std::env::set_var("TZ", "UTC");
        Ok(Self { base, root, public })
    }
}

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(path)
}

fn repo_root() -> PathBuf {
    std::env::var_os("CAREERBREW_RUST20_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn t0() -> DateTime<FixedOffset> {
    Utc.with_ymd_and_hms(2026, 10, 19, 9, 0, 0)
        .unwrap()
        .fixed_offset()
}

fn card(id: &str, topic: &str, number: usize) -> Value {
    json!({
        "id": id,
        "question": format!("{topic} question {number}"),
        "answer": format!("synthetic technical answer {number}"),
        "topic": topic,
        "tags": [topic],
    })
}

fn explanation() -> String {
    SECTIONS
        .iter()
        .map(|section| format!("{section}\n{}", vec!["word"; 40].join(" ")))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn public_card(question: &str, updates: Value) -> Value {
    let mut row = json!({
        "source_id": public::qhash(question),
        "question": question,
        "answer": "A tree where each parent is ordered before its children.",
        "topic": "computer science",
        "tags": ["heap"],
        "explanation": explanation(),
    });
    for (key, value) in updates.as_object().into_iter().flatten() {
        row[key] = value.clone();
    }
    row
}

fn error<T>(result: Result<T>) -> String {
    result
        .err()
        .map(|error| error.class)
        .unwrap_or("accepted")
        .to_owned()
}

fn depvec_fsrs(_base: &Path) -> Result<Value> {
    let data = std::fs::read_to_string(fixture("xtask/tests/fixtures/depvec/fsrs.json"))?;
    let fixture: Value = serde_json::from_str(&data)?;
    let mut report = careerbrew_depvec::run("fsrs", &fixture).map_err(TutorError::value)?;
    let count = report["results"].as_array().map_or(0, Vec::len);
    report["vector_count"] = json!(count);
    Ok(report)
}

fn fsrs_old_state(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "old-state")?;
    cards::write_deck(&ws.root, "test", &[card("c1", "coding", 0)])?;
    cards::review(&ws.root, &ws.public, "c1", "good", t0(), None)?;
    cards::review(
        &ws.root,
        &ws.public,
        "c1",
        "good",
        t0() + Duration::minutes(10),
        None,
    )?;
    let mut raw = read_json(&ws.root, "tutor/card-state.json", json!({}))?;
    raw["c1"]["card_id"] = json!(424242);
    raw["c1"]["due"] =
        json!(text(field(&raw["c1"], "due")).replace("09:10:00+00:00", "09:10:00.123456+00:00"));
    write_json(&ws.root, "tutor/card-state.json", &raw)?;
    let old = cards::states(&ws.root)?["c1"].to_json();
    let before = read_json(&ws.root, "tutor/card-state.json", json!({}))?["c1"].clone();
    let review = cards::review(
        &ws.root,
        &ws.public,
        "c1",
        "good",
        t0() + Duration::days(3),
        None,
    )?;
    let new = cards::states(&ws.root)?["c1"].to_json();
    Ok(
        json!({"old": old, "old_file": before.clone(), "old_roundtrip": old == before, "review": review, "new": new}),
    )
}

fn cards_and_decks(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "cards")?;
    let deck = ["coding", "behavioral", "system design"]
        .iter()
        .flat_map(|topic| {
            (0..2).map(move |number| card(&format!("{}{}", &topic[..2], number), topic, number))
        })
        .collect::<Vec<_>>();
    cards::write_deck(&ws.root, "test", &deck)?;
    let first = cards::queue(&ws.root, &ws.public, t0(), Some(4), 50, &[])?;
    let mut reviewed = Vec::new();
    for row in first.iter().take(2) {
        reviewed.push(cards::review(
            &ws.root,
            &ws.public,
            &text(field(row, "id")),
            "good",
            t0(),
            None,
        )?);
    }
    let remaining = cards::queue(
        &ws.root,
        &ws.public,
        t0() + Duration::minutes(1),
        Some(4),
        50,
        &[],
    )?;

    let learn_folder = ws.root.join("learn/rust20");
    let learn_root = learn_folder.join("srs");
    careerbrew_learn::cards::write_deck(&ws.root, &learn_folder, &[card("learn-c1", "learn", 0)])?;
    let learn_review =
        careerbrew_learn::cards::review(&ws.root, &learn_folder, "learn-c1", "good", t0(), None)?;
    let learn_ids = careerbrew_learn::cards::all_cards(&ws.root, &learn_folder)?
        .into_keys()
        .collect::<Vec<_>>();
    let interview_ids = cards::all_cards(&ws.root, &ws.public)?
        .into_keys()
        .collect::<Vec<_>>();
    let isolation = json!({
        "learn_ids": learn_ids,
        "interview_ids": interview_ids,
        "learn_review": learn_review,
        "learn_state": learn_root.join("card-state.json").is_file(),
        "interview_has_learn_state": cards::scheduled(&ws.root)?.contains("learn-c1"),
        "learn_has_interview_deck": learn_root.join("decks/test.yaml").exists(),
    });

    cards::write_deck(&ws.root, "leech", &[card("leech-1", "coding", 0)])?;
    let mut at = t0();
    let mut leech_row = json!({});
    for _ in 0..cards::LEECH_LAPSES {
        cards::review(&ws.root, &ws.public, "leech-1", "good", at, None)?;
        at += Duration::days(30);
        cards::review(&ws.root, &ws.public, "leech-1", "good", at, None)?;
        at += Duration::days(60);
        leech_row = cards::review(&ws.root, &ws.public, "leech-1", "again", at, None)?;
        at += Duration::days(1);
    }
    let suspended_absent =
        !cards::queue(&ws.root, &ws.public, at + Duration::days(30), None, 50, &[])?
            .iter()
            .any(|row| text(field(row, "id")) == "leech-1");
    cards::unsuspend(&ws.root, &ws.public, "leech-1")?;
    let unsuspended_present =
        cards::queue(&ws.root, &ws.public, at + Duration::days(30), None, 50, &[])?
            .iter()
            .any(|row| text(field(row, "id")) == "leech-1");
    Ok(json!({
        "first": first.iter().map(|row| json!({"id": text(field(row, "id")), "topic": text(field(row, "topic")), "mode": text(field(row, "mode"))})).collect::<Vec<_>>(),
        "reviewed": reviewed,
        "remaining": remaining.iter().map(|row| text(field(row, "id"))).collect::<Vec<_>>(),
        "isolation": isolation,
        "leech": leech_row,
        "suspended_absent": suspended_absent,
        "unsuspended_present": unsuspended_present,
        "unknown_rating": error(cards::review(&ws.root, &ws.public, "co0", "great", t0(), None)),
        "unknown_card": error(cards::review(&ws.root, &ws.public, "missing", "good", t0(), None)),
    }))
}

fn local_day_stats(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "local-day")?;
    std::env::set_var("TZ", "Etc/GMT+4");
    let deck = (0..5)
        .map(|number| card(&format!("c{number}"), "coding", number))
        .collect::<Vec<_>>();
    cards::write_deck(&ws.root, "test", &deck)?;
    let evening = Utc
        .with_ymd_and_hms(2026, 10, 19, 23, 30, 0)
        .unwrap()
        .fixed_offset();
    let first = cards::queue(&ws.root, &ws.public, evening, Some(3), 50, &[])?;
    for row in &first {
        cards::review(
            &ws.root,
            &ws.public,
            &text(field(row, "id")),
            "good",
            evening,
            None,
        )?;
    }
    let later = evening + Duration::hours(1);
    let queue_later = cards::queue(&ws.root, &ws.public, later, Some(3), 50, &[])?;
    let stats = cards::stats(&ws.root, &ws.public, later)?;
    let next_day = cards::stats(&ws.root, &ws.public, later + Duration::days(1))?;
    Ok(json!({
        "first_count": first.len(),
        "later_modes": queue_later.iter().map(|row| text(field(row, "mode"))).collect::<Vec<_>>(),
        "later_new": queue_later.iter().filter(|row| text(field(row, "mode")) == "new").map(|row| text(field(row, "id"))).collect::<Vec<_>>(),
        "reviewed_today": stats["reviewed_today"],
        "streak_days": stats["streak_days"],
        "next_day_streak": next_day["streak_days"],
        "retention_30d": stats["retention_30d"],
        "by_topic": stats["by_topic"],
    }))
}

fn public_deck(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "public")?;
    write_yaml(
        &ws.root,
        "profile.yaml",
        &json!({"name": "RUST20CANARYPERSON", "employer": "RUST20CANARYEMPLOYER"}),
    )?;
    cards::write_deck(
        &ws.root,
        "notion",
        &[
            json!({"id": "n1", "question": "What is a heap?", "answer": "private technical answer", "tags": ["Computer Science"]}),
            json!({"id": "n2", "question": "Tell me about a conflict", "answer": "RUST20CANARYANSWER", "tags": ["Behavioural"]}),
        ],
    )?;
    let initial = public::explain_queue(&ws.root, &ws.public, &repo_root(), None)?
        .into_iter()
        .map(|row| (text(field(&row, "question")), row))
        .collect::<BTreeMap<_, _>>();
    let published = public::publish(
        &ws.root,
        &ws.public,
        &repo_root(),
        &[public_card(
            "What is a heap?",
            json!({"source_id": public::qhash("what is a HEAP")}),
        )],
        false,
    )?;
    let owner_cards = cards::all_cards(&ws.root, &ws.public)?
        .into_keys()
        .collect::<Vec<_>>();
    let explanation_matches = cards::queue(&ws.root, &ws.public, crate::now(), None, 50, &[])?[0]
        ["explanation"]
        .as_str()
        .is_some_and(|text| text.starts_with("## In plain words"));

    let personal_question = "Tell me about a conflict";
    let skipped = public::publish(
        &ws.root,
        &ws.public,
        &repo_root(),
        &[
            json!({"source_id": public::qhash(personal_question), "skip": true, "reason": "personal"}),
        ],
        false,
    )?;
    let private_item = public::explain_queue(&ws.root, &ws.public, &repo_root(), None)?
        .into_iter()
        .find(|row| text(field(row, "question")) == personal_question)
        .ok_or_else(|| TutorError::value("missing private queue item"))?;
    let private_result = public::publish(
        &ws.root,
        &ws.public,
        &repo_root(),
        &[public_card(
            personal_question,
            json!({"source_id": public::qhash(personal_question), "private": true}),
        )],
        false,
    )?;
    let private_sealed =
        std::fs::read(ws.root.join("tutor/private-explanations.yaml"))?.starts_with(b"CBV1\n");
    let public_ids = public::load(&ws.public)?
        .iter()
        .map(|row| text(field(row, "id")))
        .collect::<Vec<_>>();
    let pub_id = public_ids
        .first()
        .cloned()
        .ok_or_else(|| TutorError::value("missing public card"))?;
    public::flag(&ws.root, &ws.public, &pub_id, "the example is wrong")?;
    let flagged = public::explain_queue(&ws.root, &ws.public, &repo_root(), None)?
        .into_iter()
        .find(|row| text(field(row, "rewrite")) == pub_id)
        .ok_or_else(|| TutorError::value("missing flagged queue item"))?;
    public::publish(
        &ws.root,
        &ws.public,
        &repo_root(),
        &[public_card("What is a heap?", json!({"rewrite": pub_id}))],
        false,
    )?;
    let flags_cleared = public::flags(&ws.root)?
        .as_object()
        .is_none_or(Map::is_empty);

    let before = std::fs::read_dir(&ws.public)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "yaml")
        })
        .map(|path| std::fs::read(&path).map(|bytes| (path, bytes)))
        .collect::<std::io::Result<BTreeMap<_, _>>>()?;
    let bad = public_card(
        "What is a stack?",
        json!({"explanation": "## In plain words\ntoo short"}),
    );
    let refused = error(public::publish(
        &ws.root,
        &ws.public,
        &repo_root(),
        &[public_card("What is a trie?", json!({})), bad],
        false,
    ));
    let after = std::fs::read_dir(&ws.public)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "yaml")
        })
        .map(|path| std::fs::read(&path).map(|bytes| (path, bytes)))
        .collect::<std::io::Result<BTreeMap<_, _>>>()?;
    let leaky = public_card(
        "What is a queue?",
        json!({"explanation": format!("{}\nAt RUST20CANARYEMPLOYER we did this.", explanation())}),
    );
    let leak_refusal = error(public::publish(
        &ws.root,
        &ws.public,
        &repo_root(),
        std::slice::from_ref(&leaky),
        false,
    ));
    write_yaml(
        &ws.root,
        "plugins/rust20plug/careerbrew-plugin.yaml",
        &json!({"name": "rust20plug", "version": "0.1.0", "sensitive_terms": "sensitive-terms.txt"}),
    )?;
    crate::write_text(
        &ws.root,
        "plugins/rust20plug/sensitive-terms.txt",
        "RUST20CANARYPLUGIN\n",
    )?;
    write_json(
        &ws.root,
        "plugins.lock.json",
        &json!({"plugins": {"rust20plug": {"origin": "plugin:rust20plug"}}}),
    )?;
    let plugin_leaky = public_card(
        "What is a map?",
        json!({"explanation": format!("{}\nRUST20CANARYPLUGIN", explanation())}),
    );
    let plugin_leak_refusal = error(public::publish(
        &ws.root,
        &ws.public,
        &repo_root(),
        &[plugin_leaky],
        false,
    ));
    Ok(json!({
        "initial_hints": initial.iter().map(|(question, row)| (question.clone(), field(row, "hint").cloned().unwrap_or(Value::Null))).collect::<Map<_, _>>(),
        "published": published,
        "owner_cards": owner_cards,
        "explanation": explanation_matches,
        "skipped": skipped,
        "private_item": private_item,
        "private_result": private_result,
        "private_sealed": private_sealed,
        "public_ids": public_ids,
        "flagged": flagged,
        "flags_cleared": flags_cleared,
        "check": public::check(&ws.public)?,
        "partial_write": {"error": refused, "unchanged": before == after},
        "leak_refusal": leak_refusal,
        "plugin_leak_refusal": plugin_leak_refusal,
        "personal_hits": public::personal_hits(&ws.root, &leaky)?,
        "lint": {
            "local_http": public::lint(&public_card("What is a heap?", json!({"explanation": format!("{}\n`proxy_pass http://localhost:8080;`", explanation()), "id": "kb-local"}))),
            "remote_http": !public::lint(&public_card("What is a heap?", json!({"explanation": format!("{} http://evil.test/x", explanation()), "id": "kb-remote"}))).is_empty(),
        },
        "library_sections": public::library_entries(&ws.root, &ws.public)?.iter().map(|row| text(field(row, "section"))).collect::<Vec<_>>(),
    }))
}

fn copy_bundle(destination: &Path) -> Result<()> {
    let source = fixture("tests/fixtures/rust20-notion-bundle");
    std::fs::create_dir_all(destination)?;
    for path in std::fs::read_dir(source)?
        .flatten()
        .map(|entry| entry.path())
    {
        std::fs::copy(
            &path,
            destination.join(path.file_name().unwrap_or_default()),
        )?;
    }
    Ok(())
}

fn seed_non_notion_rows(root: &Path) -> Result<()> {
    write_jsonl(
        root,
        "jobs/applications.jsonl",
        &[
            json!({"company": "User company", "tier": "target", "stage": "applied", "at": "2026-01-01T00:00:00+00:00", "origin": "user"}),
        ],
    )?;
    write_jsonl(
        root,
        "work/evidence.jsonl",
        &[
            json!({"id": "user-1", "kind": "note", "title": "User evidence", "at": "2026-01-01T00:00:00+00:00", "origin": "user"}),
        ],
    )?;
    write_yaml(
        root,
        "prep/stories.yaml",
        &json!({"stories": [{"id": "user-story", "title": "User story", "origin": "user"}]}),
    )
}

fn notion_idempotence(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "notion")?;
    let bundle = ws.root.join("imports/notion");
    copy_bundle(&bundle)?;
    write_yaml(
        &ws.root,
        "profile.yaml",
        &json!({"name": "Synthetic", "skills": {"python": 4}}),
    )?;
    seed_non_notion_rows(&ws.root)?;
    let first = notion::run(&ws.root, &bundle, &repo_root(), false)?;
    cards::review(&ws.root, &ws.public, "notion-f1", "good", t0(), None)?;
    let before = json!({
        "apps": read_jsonl(&ws.root, "jobs/applications.jsonl")?.len(),
        "evidence": read_jsonl(&ws.root, "work/evidence.jsonl")?.len(),
        "stories": field(&read_yaml(&ws.root, "prep/stories.yaml", json!({}))?, "stories").and_then(Value::as_array).map_or(0, Vec::len),
    });
    let second = notion::run(&ws.root, &bundle, &repo_root(), false)?;
    let after = json!({
        "apps": read_jsonl(&ws.root, "jobs/applications.jsonl")?.len(),
        "evidence": read_jsonl(&ws.root, "work/evidence.jsonl")?.len(),
        "stories": field(&read_yaml(&ws.root, "prep/stories.yaml", json!({}))?, "stories").and_then(Value::as_array).map_or(0, Vec::len),
    });
    let mut app_origins = read_jsonl(&ws.root, "jobs/applications.jsonl")?
        .iter()
        .map(|row| text(field(row, "origin")))
        .collect::<Vec<_>>();
    let mut evidence_origins = read_jsonl(&ws.root, "work/evidence.jsonl")?
        .iter()
        .map(|row| text(field(row, "origin")))
        .collect::<Vec<_>>();
    let mut story_origins = field(
        &read_yaml(&ws.root, "prep/stories.yaml", json!({}))?,
        "stories",
    )
    .and_then(Value::as_array)
    .cloned()
    .unwrap_or_default()
    .iter()
    .map(|row| text(field(row, "origin")))
    .collect::<Vec<_>>();
    app_origins.sort();
    evidence_origins.sort();
    story_origins.sort();
    let origins =
        json!({"apps": app_origins, "evidence": evidence_origins, "stories": story_origins});
    let profile = read_yaml(&ws.root, "profile.yaml", json!({}))?;
    let sealed = std::fs::read_dir(&bundle)?
        .flatten()
        .map(|entry| entry.path())
        .map(|path| std::fs::read(path).map(|bytes| bytes.starts_with(b"CBV1\n")))
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .all(|sealed| sealed);
    Ok(json!({
        "first": first,
        "second": second,
        "before": before,
        "after": after,
        "origins": origins,
        "check": notion::check(&ws.root, &bundle, &ws.public)?,
        "reviewed_today": cards::stats(&ws.root, &ws.public, t0())?["reviewed_today"],
        "scheduled_kept": cards::scheduled(&ws.root)?.contains("notion-f1"),
        "deck_count": cards::all_cards(&ws.root, &ws.public)?.values().filter(|row| text(field(row, "deck")) == "notion").count(),
        "skills": profile["skills"],
        "sealed": sealed,
        "library_count": notion::library(&ws.root)?.len(),
        "plain": [notion::plain("**What is a \\^heap?**"), notion::plain("__Why__ `useMemo`?")],
    }))
}

fn write_snapshot(root: &Path) -> Value {
    json!({
        "library": root.join("library/notion/index.json").exists(),
        "board": root.join("jobs/notion-board.json").exists(),
        "applications": root.join("jobs/applications.jsonl").exists(),
        "deck": root.join("tutor/decks/notion.yaml").exists(),
        "profile": root.join("profile.yaml").exists(),
        "stories": root.join("prep/stories.yaml").exists(),
        "evidence": root.join("work/evidence.jsonl").exists(),
        "terms": root.join("sensitive-terms.txt").exists(),
    })
}

fn notion_validation_and_partial_writes(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "partial")?;
    let bad = ws.base.join("bad");
    copy_bundle(&bad)?;
    let mut manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(bad.join("manifest.json"))?)?;
    manifest["counts"]
        .as_object_mut()
        .expect("manifest counts")
        .remove("stories");
    std::fs::write(bad.join("manifest.json"), serde_json::to_string(&manifest)?)?;
    let missing_kind = error(notion::run(&ws.root, &bad, &repo_root(), false));
    let missing_kind_writes = write_snapshot(&ws.root);

    let bundle = ws.base.join("bundle");
    copy_bundle(&bundle)?;
    let partial_error = error(notion::run(&ws.root, &bundle, &repo_root(), true));
    let mut partial = write_snapshot(&ws.root);
    let bundle_sealed = std::fs::read_dir(&bundle)?
        .flatten()
        .map(|entry| entry.path())
        .map(|path| std::fs::read(path).map(|bytes| bytes.starts_with(b"CBV1\n")))
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .all(|sealed| sealed);
    partial["bundle_sealed"] = json!(bundle_sealed);
    let recovered = notion::run(&ws.root, &bundle, &repo_root(), false)?;
    let recovered_check = notion::check(&ws.root, &bundle, &ws.public)?;
    let recovered_counts = json!({
        "apps": read_jsonl(&ws.root, "jobs/applications.jsonl")?.len(),
        "evidence": read_jsonl(&ws.root, "work/evidence.jsonl")?.len(),
        "stories": field(&read_yaml(&ws.root, "prep/stories.yaml", json!({}))?, "stories").and_then(Value::as_array).map_or(0, Vec::len),
    });

    let corporate = Workspace::new(base, "corporate")?;
    careerbrew_setup::machine::set_role("corporate", "")?;
    let role_error = error(notion::run(
        &corporate.root,
        &fixture("tests/fixtures/rust20-notion-bundle"),
        &repo_root(),
        false,
    ));
    let role_writes = write_snapshot(&corporate.root);
    Ok(json!({
        "missing_kind": missing_kind,
        "missing_kind_writes": missing_kind_writes,
        "partial_error": partial_error,
        "partial": partial,
        "recovered": recovered,
        "recovered_check": recovered_check,
        "recovered_counts": recovered_counts,
        "role_error": role_error,
        "role_writes": role_writes,
    }))
}

fn providers_and_mistakes(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "providers")?;
    if std::env::var("CAREERBREW_RUST20_EVENT_CONTROL").as_deref() == Ok("1") {
        std::env::set_var("CAREERBREW_EVENTS", "on");
    }
    write_yaml(
        &ws.root,
        "plugins/rust20plug/careerbrew-plugin.yaml",
        &json!({
            "name": "rust20plug",
            "version": "0.1.0",
            "provides": {"knowledge": "knowledge", "rubrics": "rubrics"},
        }),
    )?;
    write_yaml(
        &ws.root,
        "plugins/rust20plug/knowledge/plugin.yaml",
        &json!({"items": [{
            "id": "plugin-rust-1",
            "topic": "Plugin topic",
            "round": "skill",
            "rubric": "plugin-rubric",
            "teach": "Synthetic plugin lesson",
            "prompt": "Synthetic plugin prompt",
        }]}),
    )?;
    write_yaml(
        &ws.root,
        "plugins/rust20plug/rubrics/plugin.yaml",
        &json!({"id": "plugin-rubric", "criteria": [{"id": "plugin-criterion", "text": "Synthetic plugin criterion"}]}),
    )?;
    write_json(
        &ws.root,
        "plugins.lock.json",
        &json!({"plugins": {"rust20plug": {"origin": "plugin:rust20plug"}}}),
    )?;
    let knowledge = engine::resources(&ws.root, &repo_root(), "knowledge")?;
    let rubric_resources = engine::resources(&ws.root, &repo_root(), "rubrics")?;
    let plugin_knowledge = knowledge
        .into_iter()
        .filter(|(origin, _)| origin != "base")
        .collect::<Vec<_>>();
    let added = engine::add_gap_items(&ws.root, &["Rust".into()])?;
    let repeated = engine::add_gap_items(&ws.root, &["Rust".into()])?;
    let all_items = engine::items(&ws.root, &repo_root(), &plugin_knowledge)?;
    let item = all_items["gap-rust-1"].clone();
    let plugin_item = all_items["plugin-rust-1"].clone();
    let all_rubrics = engine::rubrics(&ws.root, &repo_root(), &rubric_resources)?;
    let rubric = all_rubrics["skill"].clone();
    let plugin_rubric_criteria = field(&all_rubrics["plugin-rubric"], "criteria")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|row| text(field(row, "id")))
        .collect::<Vec<_>>();
    let criteria = field(&rubric, "criteria")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|row| text(field(row, "id")))
        .collect::<Vec<_>>();
    let mut seeded = engine::cards(&ws.root)?;
    seeded.insert("gap-rust-1".into(), fsrs::CardState::new(424243, t0()));
    engine::save_cards(&ws.root, &seeded)?;
    let row = engine::record(
        &ws.root,
        &repo_root(),
        &plugin_knowledge,
        &rubric_resources,
        "gap-rust-1",
        &criteria,
        engine::Attempt {
            seconds: None,
            at: t0(),
            graded_by: "self",
        },
    )?;
    if std::env::var("CAREERBREW_RUST20_PLANT_WRITE").as_deref() == Ok("event-duplicate") {
        emit(
            &ws.root,
            "tutor.attempt",
            vec![("status", careerbrew_vault::canon::Canon::Str("good".into()))],
        )?;
    }
    let old_cards = engine::cards(&ws.root)?
        .iter()
        .map(|(id, card)| (id.clone(), card.to_json()))
        .collect::<Map<_, _>>();
    let attempts = engine::attempts(&ws.root)?;
    let next_review = engine::next_item(
        &ws.root,
        &repo_root(),
        &plugin_knowledge,
        &["Rust".into()],
        t0() + Duration::days(20),
    )?;
    let mastery = engine::mastery(
        &ws.root,
        &repo_root(),
        &plugin_knowledge,
        &rubric_resources,
        t0(),
    )?
    .get("Rust")
    .cloned()
    .unwrap_or(Value::Null);
    let readiness = engine::readiness(
        &ws.root,
        &repo_root(),
        &plugin_knowledge,
        &rubric_resources,
        &["Rust".into()],
        t0(),
    )?;

    let first = mistakes::record(
        &ws.root,
        &ws.public,
        "ex-1",
        "arrays",
        "wrong-answer",
        "synthetic wrong answer",
        None,
    )?;
    let second = mistakes::record(
        &ws.root,
        &ws.public,
        "ex-1",
        "arrays",
        "wrong-answer",
        "synthetic repeated answer",
        None,
    )?;
    let exercise = json!({"id": "ex-2", "pattern": "trees", "pitfalls": {}, "tests": [{"note": "empty input"}]});
    let outcome = json!({"results": [{"ok": false, "i": 0}]});
    let found = mistakes::from_exercise(&ws.root, &ws.public, &exercise, &outcome, &json!({}))?;
    let summary = mistakes::summary(&ws.root)?;
    let rating_thresholds = [0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]
        .iter()
        .map(|score| match fsrs::score_to_rating(*score) {
            1 => "Again",
            2 => "Hard",
            3 => "Good",
            _ => "Easy",
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "added": added,
        "repeated": repeated,
        "item": item,
        "criteria": criteria,
        "record": row,
        "cards": old_cards,
        "attempt_count": attempts.len(),
        "next_review": next_review,
        "mastery": mastery,
        "readiness": readiness,
        "now": crate::stamp_at(engine::now()),
        "rating_thresholds": rating_thresholds,
        "plugin_item": plugin_item,
        "plugin_rubric_criteria": plugin_rubric_criteria,
        "mistakes": {
            "first_repeat": field(&first, "repeat"),
            "second_repeat": field(&second, "repeat"),
            "from_exercise": found.iter().map(|row| text(field(row, "kind"))).collect::<Vec<_>>(),
            "deck_count": cards::all_cards(&ws.root, &ws.public)?.values().filter(|row| text(field(row, "deck")) == "mistakes").count(),
            "summary": summary,
        },
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
    let base = crate::root()?.join("rust20-workload");
    std::fs::create_dir_all(&base)?;
    let mut cases = Vec::new();
    for name in CASES {
        if selected.is_some_and(|selected| selected != *name) {
            continue;
        }
        let result = match *name {
            "depvec-fsrs" => depvec_fsrs(&base),
            "fsrs-old-state" => fsrs_old_state(&base),
            "cards-and-decks" => cards_and_decks(&base),
            "local-day-stats" => local_day_stats(&base),
            "public-deck" => public_deck(&base),
            "notion-idempotence" => notion_idempotence(&base),
            "notion-validation-and-partial-writes" => notion_validation_and_partial_writes(&base),
            "providers-and-mistakes" => providers_and_mistakes(&base),
            _ => Err(TutorError::value("unknown synthetic case")),
        };
        call(&mut cases, name, result);
    }
    if selected == Some("duplicate-write-control") {
        std::env::set_var("CAREERBREW_HOME", base.join("providers/data"));
        std::env::set_var(
            "CAREERBREW_KEY_FILE",
            base.join("providers/keys/backup.key"),
        );
        let path = base.join("providers/data/tutor/gap-items.yaml");
        let current = crate::read_yaml_abs(&path, json!({}))?;
        crate::write_yaml_abs(&path, &current)?;
    }
    if std::env::var("CAREERBREW_RUST20_PLANT_WRITE").as_deref() == Ok("extra") {
        write_json(
            &crate::root()?,
            "planted-extra.json",
            &json!({"synthetic": true}),
        )?;
    }
    if std::env::var("CAREERBREW_RUST20_PLANT_WRITE").as_deref() == Ok("missing")
        && selected == Some("fsrs-old-state")
    {
        std::fs::remove_file(base.join("old-state/data/tutor/card-state.json"))?;
    }
    Ok(format!("{}\n", compact(&json!({"cases": cases}))?))
}
