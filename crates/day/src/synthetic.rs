//! Synthetic rust-17 differential workload over native component functions.
//! It mirrors `tests/oracles/rust17.py` step for step, so writes match.

use crate::promises::LockOnlyPromises;
use crate::{
    difflib, field, handoff, pm_learn, reset_write_count, results, text, write_count, Result,
};
use chrono::NaiveDate;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const CASES: &[&str] = &[
    "difflib-vectors",
    "results-records",
    "handoff-records",
    "pm-sent-receipts",
    "pm-lessons",
];
const CANARY: &str = "SYNTHCANARY-rust17-private";
const PROMISES: LockOnlyPromises = LockOnlyPromises;

struct Workspace {
    base: PathBuf,
    root: PathBuf,
}

impl Workspace {
    fn new(base: &Path, name: &str) -> Result<Self> {
        let base = base.join(name);
        let root = base.join("data");
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(base.join("home"))?;
        std::env::set_var("HOME", base.join("home"));
        std::env::set_var("CAREERBREW_HOME", &root);
        std::env::set_var("CAREERBREW_KEY_FILE", base.join("keys/backup.key"));
        std::env::set_var("CAREERBREW_EVENTS", "off");
        std::env::set_var("CAREERBREW_ENCRYPT", "on");
        std::env::set_var("CAREERBREW_NOW", "2026-11-02T09:00:00+00:00");
        std::env::set_var("CAREERBREW_TEST_OWNER", "1");
        std::env::set_var("CAREERBREW_RUST13_WRITE_TRACE", base.join("writes.log"));
        std::env::set_var("TZ", "UTC");
        std::env::remove_var("CAREERBREW_TEST_BUSINESS");
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../profile/examples");
        for relative in [
            "day/calendars.yaml",
            "jobs/criteria.yaml",
            "jobs/sources.yaml",
            "mail.yaml",
            "profile.yaml",
            "sensitive-terms.txt",
            "settings.yaml",
            "work/sources.yaml",
        ] {
            let value = std::fs::read_to_string(examples.join(relative))?;
            careerbrew_feed::write_text(&root, relative, &value)?;
        }
        reset_write_count(&base)?;
        Ok(Self { base, root })
    }
}

fn plant() -> String {
    std::env::var("CAREERBREW_RUST17_PLANT_WRITE").unwrap_or_default()
}

fn at(value: &str) {
    std::env::set_var("CAREERBREW_NOW", value);
}

fn day(value: &str) -> NaiveDate {
    crate::iso_day(value).expect("fixture day")
}

fn error_of<T>(result: Result<T>) -> String {
    result
        .err()
        .map(|error| error.class)
        .unwrap_or("accepted")
        .to_owned()
}

fn set_setting(root: &Path, key: &str, value: &str) -> Result<()> {
    let mut settings = careerbrew_feed::read_yaml(root, "settings.yaml", json!({}))?;
    if !settings.is_object() {
        settings = json!({});
    }
    settings[key] = json!(value);
    careerbrew_feed::write_yaml(root, "settings.yaml", &settings)
}

fn sealed(path: &Path) -> Result<bool> {
    Ok(std::fs::read(path)?.starts_with(b"CBV1\n"))
}

fn plain_absent(path: &Path) -> Result<bool> {
    let bytes = std::fs::read(path)?;
    Ok(!bytes
        .windows(CANARY.len())
        .any(|part| part == CANARY.as_bytes()))
}

fn hide(value: &str) -> String {
    value.replace(CANARY, "<canary>")
}

fn hide_value(value: &Value) -> Value {
    match value {
        Value::String(item) => Value::String(hide(item)),
        Value::Array(items) => Value::Array(items.iter().map(hide_value).collect()),
        Value::Object(items) => Value::Object(
            items
                .iter()
                .map(|(key, item)| (hide(key), hide_value(item)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn opt(value: Option<String>) -> Value {
    value.map(Value::String).unwrap_or(Value::Null)
}

fn difflib_vectors(_base: &Path) -> Result<Value> {
    let long_a = "the quick brown fox jumps over the lazy dog ".repeat(6);
    let long_b = "the quick brown cat jumps over the lazy dog ".repeat(5) + "and then naps";
    let edge = "x".repeat(120) + "y" + &"x".repeat(129);
    let pairs: Vec<(String, String)> = vec![
        (String::new(), String::new()),
        ("abc".into(), String::new()),
        (
            "Billing: shipped invoices".into(),
            "Billing: shipped the invoices".into(),
        ),
        (
            "Search: tuned ranking".into(),
            "Discovery: tuned ranking".into(),
        ),
        ("abcd".into(), "bcda".into()),
        ("éa\u{2003}b".into(), "a\u{2003}bé".into()),
        (long_a.clone(), long_b.clone()),
        (long_b, long_a),
        ("x".repeat(250), edge),
    ];
    let ratios = pairs
        .iter()
        .map(|(a, b)| json!(difflib::ratio(a, b)))
        .collect::<Vec<_>>();
    let blocks = pairs
        .iter()
        .map(|(a, b)| {
            difflib::blocks(a, b)
                .iter()
                .map(|m| json!([m.a, m.b, m.size]))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let to_lines = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let ops = |a: &[String], b: &[String]| {
        difflib::line_opcodes(a, b)
            .iter()
            .map(|op| json!([op.tag, op.i1, op.i2, op.j1, op.j2]))
            .collect::<Vec<_>>()
    };
    let opcodes = ops(
        &to_lines(&["a", "b", "c", "d", "e"]),
        &to_lines(&["a", "x", "c", "e", "f", "g"]),
    );
    let opcodes_b = ops(
        &to_lines(&["one", "two", "three", "two"]),
        &to_lines(&["two", "one", "two", "four"]),
    );
    Ok(json!({"ratios": ratios, "blocks": blocks, "opcodes": opcodes, "opcodes_b": opcodes_b}))
}

fn results_records(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "results-records")?;
    let root = &ws.root;
    let today = day("2026-11-02");
    let first = vec![
        json!({"id": "pr:example/tools#1", "at": "2026-11-02T08:00:00+00:00", "kind": "pr", "title": format!("Fix login {CANARY}"), "source": "github", "origin": "user"}),
        json!({"id": "review:example/tools#2", "at": "2026-11-02T08:10:00+00:00", "kind": "review", "title": "Review search", "source": "github", "origin": "user"}),
        json!({"id": "review:example/tools#2", "at": "2026-11-02T08:11:00+00:00", "kind": "review", "title": "Review search v2", "source": "github", "origin": "user"}),
    ];
    let added = results::append(root, &first, today)?;
    reset_write_count(&ws.base)?;
    let repeat = results::append(root, &first, today)?;
    let repeat_writes = write_count(&ws.base);
    let mut moved = first[0].clone();
    moved["at"] = json!("2026-11-02T08:30:00+00:00");
    let moved_at = results::append(root, &[moved], today)?;
    let mut fixed = first[0].clone();
    fixed["title"] = json!(format!("Fix login flow {CANARY}"));
    let corrected = results::append(
        root,
        &[
            fixed,
            json!({"id": "note:1", "at": "2026-11-02T08:40:00+00:00", "kind": "note", "title": "Wrote notes", "source": "manual", "origin": "user"}),
        ],
        today,
    )?;
    let mut draft_row = first[1].clone();
    draft_row["title"] = json!("Review search v2");
    draft_row["draft"] = json!(true);
    let draft = results::append(root, &[draft_row], today)?;
    let loaded_rows = results::load(root, today)?;
    let loaded = loaded_rows
        .iter()
        .map(|row| {
            let mut row = row.clone();
            if let Some(map) = row.as_object_mut() {
                map.remove("title");
            }
            row
        })
        .collect::<Vec<_>>();
    let items = vec![
        json!({"id": "e1", "at": "2026-11-02T10:00:00Z", "kind": "pr", "title": "Zulu", "draft": false}),
        json!({"id": "e2", "at": "2026-11-02T23:30:00-05:00", "title": "Next day in UTC"}),
        json!({"id": "e3", "at": "2026-11-02T07:00:00", "kind": "note", "origin": "agent", "state": "open", "merged_at": "", "feed_source": "slack"}),
        json!({"id": "e4", "at": "2026-11-01T23:59:59+00:00", "kind": "pr"}),
        json!({"id": "e5", "at": "not-a-date", "kind": "pr"}),
        json!({"id": "e6", "at": "2026-11-02", "kind": "pr"}),
        json!({"id": "e7", "kind": "pr"}),
    ];
    let evidence = results::evidence_rows(&items, today);
    if plant() == "extra" {
        std::fs::write(root.join("planted-extra.txt"), "synthetic")?;
    }
    let path = root.join(results::relative(today));
    Ok(json!({
        "added": added,
        "repeat": repeat,
        "repeat_writes": repeat_writes,
        "moved_at": moved_at,
        "corrected": corrected,
        "draft": draft,
        "loaded": loaded,
        "titles_plain": loaded_rows.iter().map(|row| hide(&text(field(row, "title")))).collect::<Vec<_>>(),
        "summary": results::summary(root, today)?,
        "empty_summary": results::summary(root, day("2026-11-03"))?,
        "evidence": evidence,
        "sealed": sealed(&path)?,
        "plain_absent": plain_absent(&path)?,
    }))
}

fn handoff_records(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "handoff-records")?;
    let root = &ws.root;
    let today = day("2026-11-02");
    let errors = vec![
        error_of(handoff::item("unknown", "text", "", &[])),
        error_of(handoff::item("prs", "  \u{2003} ", "", &[])),
        error_of(handoff::item("prs", &"x".repeat(201), "", &[])),
    ];
    let edge = handoff::item("prs", &"é".repeat(200), "", &[])?;
    let extra = handoff::item(
        "questions",
        "  Ask   the\u{2003}team  ",
        "https://example.com/q",
        &[
            ("recheck", json!("true")),
            ("empty", json!("")),
            ("fixed", Value::Null),
        ],
    )?;
    let review = format!("Review {CANARY}");
    let link = "https://github.com/example/tools/pull/1";
    let first = handoff::add(root, "prs", &review, link, today)?;
    reset_write_count(&ws.base)?;
    let again = handoff::add(root, "prs", &review, link, today)?;
    let repeat_writes = write_count(&ws.base);
    let second = handoff::add(root, "waiting", "Waiting on design", "", today)?;
    let owned = vec![
        handoff::item("lessons", "Rule one", "", &[])?,
        handoff::item("gotchas", "Gotcha two", "", &[])?,
    ];
    handoff::replace_owned(root, "wrap", &owned, today)?;
    reset_write_count(&ws.base)?;
    handoff::replace_owned(root, "wrap", &owned, today)?;
    let owned_repeat_writes = write_count(&ws.base);
    handoff::replace_owned(
        root,
        "wrap",
        &[
            handoff::item("lessons", "Rule one", "", &[])?,
            handoff::item("waiting", "Waiting on design", "", &[])?,
        ],
        today,
    )?;
    let after_replace = handoff::load(root, today)?
        .iter()
        .map(|row| json!({"id": text(field(row, "id")), "section": text(field(row, "section")), "owner": text(field(row, "owner"))}))
        .collect::<Vec<_>>();
    let done_known = handoff::done(root, &text(field(&second, "id")), today)?;
    let done_missing = handoff::done(root, "h-missing", today)?;
    if plant() == "operation-duplicate" {
        let relative = handoff::relative(today);
        let document = careerbrew_feed::read_json(root, &relative, json!({}))?;
        careerbrew_feed::write_json(root, &relative, &document)?;
    }
    handoff::add(root, "calendar", "Standup 09:30", "", day("2026-10-30"))?;
    handoff::add(root, "calendar", "Empty later", "", day("2026-10-31"))?;
    let empty_id = text(field(
        &handoff::item("calendar", "Empty later", "", &[])?,
        "id",
    ));
    handoff::done(root, &empty_id, day("2026-10-31"))?;
    let (found_day, found) = handoff::latest_before(root, day("2026-11-02"))?;
    let (later_day, later) = handoff::latest_before(root, day("2026-11-03"))?;
    let (none_day, none) = handoff::latest_before(root, day("2026-10-30"))?;
    let format = |value: Option<NaiveDate>| opt(value.map(|d| d.format("%Y-%m-%d").to_string()));
    let path = root.join(handoff::relative(today));
    Ok(json!({
        "errors": errors,
        "edge_len": text(field(&edge, "text")).chars().count(),
        "extra": extra,
        "first_id": text(field(&first, "id")),
        "again_same": again == first,
        "repeat_writes": repeat_writes,
        "second": second,
        "owned_repeat_writes": owned_repeat_writes,
        "after_replace": after_replace,
        "done": [done_known, done_missing],
        "latest": [format(found_day), found.iter().map(|row| text(field(row, "id"))).collect::<Vec<_>>()],
        "latest_later": [format(later_day), later.len()],
        "latest_none": [format(none_day), none],
        "sealed": sealed(&path)?,
        "plain_absent": plain_absent(&path)?,
    }))
}

fn history_view(root: &Path) -> Result<Value> {
    Ok(json!(careerbrew_feed::read_jsonl(root, pm_learn::HISTORY)?
        .iter()
        .map(|row| json!([
            text(field(row, "day")),
            text(field(row, "from")),
            hide(&text(field(row, "body")))
        ]))
        .collect::<Vec<_>>()))
}

fn pm_sent_receipts(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "pm-sent-receipts")?;
    let root = &ws.root;
    let (manual_day, receipt_day, invalid_day, channel_day, dm_day, none_day) = (
        "2026-10-26",
        "2026-10-27",
        "2026-10-28",
        "2026-10-29",
        "2026-10-30",
        "2026-10-31",
    );
    let days = [
        manual_day,
        receipt_day,
        invalid_day,
        channel_day,
        dm_day,
        none_day,
    ];
    let draft = "Billing: shipped invoices.\nSearch: tuned ranking.";
    for item in days {
        pm_learn::remember(root, item, draft, draft)?;
    }
    let events = vec![
        json!({"id": "slack:C0PM:100.1", "source": "slack", "actor": "me", "channel": "pm-updates", "at": "2026-10-26T15:00:00+00:00"}),
        json!({"id": "slack:C0SYN:200.5", "source": "slack", "actor": "me", "channel": "dm", "at": "2026-10-27T15:00:00+00:00"}),
        json!({"id": "slack:C0PM:300.1", "source": "slack", "actor": "me", "channel": "PM-Updates", "at": "2026-10-29T14:00:00Z"}),
        json!({"id": "slack:C0PM:300.2", "source": "slack", "actor": "me", "channel": "pm-updates", "at": "2026-10-29T16:00:00+00:00"}),
        json!({"id": "slack:C0PM:300.3", "source": "slack", "actor": "other", "channel": "pm-updates", "at": "2026-10-29T16:30:00+00:00"}),
        json!({"id": "slack:C0PM:300.4", "source": "slack", "actor": "me", "channel": "pm-updates", "personal": true, "at": "2026-10-29T16:40:00+00:00"}),
        json!({"id": "slack:C0PM:300.5", "source": "slack", "actor": "me", "channel": "random", "at": "2026-10-29T16:50:00+00:00"}),
        json!({"id": "slack:D0PM:400.1", "source": "slack", "actor": "me", "at": "2026-10-30T15:00:00+00:00"}),
        json!({"id": "slack:D0XX:400.2", "source": "slack", "actor": "me", "at": "2026-10-30T15:10:00+00:00"}),
        json!({"id": "slack:G0MP:400.3", "source": "slack", "actor": "me", "at": "2026-10-30T15:20:00+00:00"}),
        json!({"id": "github:example/tools#9", "source": "github", "actor": "me", "at": "2026-10-30T15:30:00+00:00"}),
    ];
    let originals = vec![
        json!({"id": "slack:C0PM:100.1", "text": "Billing: shipped invoices.\nSearch: from the channel."}),
        json!({"id": "slack:C0SYN:200.5", "text": format!("Billing: shipped invoices (edited {CANARY}).\nSearch: tuned ranking.")}),
        json!({"id": "slack:C0PM:300.1", "text": "Lunch plans for the team?"}),
        json!({"id": "slack:C0PM:300.2", "text": "Billing: shipped invoices today.\nSearch: tuned ranking more."}),
        json!({"id": "slack:C0PM:300.3", "text": draft}),
        json!({"id": "slack:C0PM:300.4", "text": draft}),
        json!({"id": "slack:C0PM:300.5", "text": draft}),
        json!({"id": "slack:D0PM:400.1", "text": "Billing: shipped invoices.\nSearch: DM to the right person.", "original": {"channel": {"is_im": true, "name": "pm-person"}}}),
        json!({"id": "slack:D0XX:400.2", "text": draft, "original": {"channel": {"is_im": true, "name": "other-person"}}}),
        json!({"id": "slack:G0MP:400.3", "text": draft, "original": {"channel": {"is_im": true, "is_mpim": true, "name": "pm-person"}}}),
        json!({"id": "github:example/tools#9", "text": draft}),
    ];
    careerbrew_feed::write_jsonl(root, "work/feed/events.jsonl", &events)?;
    careerbrew_feed::write_jsonl(root, "work/feed/originals.jsonl", &originals)?;
    reset_write_count(&ws.base)?;
    let no_target = vec![
        opt(pm_learn::pm_sent_text(root, channel_day)?),
        opt(pm_learn::pm_sent_text(root, dm_day)?),
    ];
    let no_target_writes = write_count(&ws.base);
    set_setting(root, "pm_slack", "#pm-updates")?;
    pm_learn::save_sent(
        root,
        manual_day,
        &format!("  Manual {CANARY} update\n\n"),
        None,
        &PROMISES,
    )?;
    let manual = pm_learn::pm_sent_text(root, manual_day)?;
    pm_learn::save_sent(
        root,
        receipt_day,
        "Billing: shipped invoices.",
        Some(&pm_learn::Receipt {
            channel: "C0SYN",
            ts: "200.5",
        }),
        &PROMISES,
    )?;
    let receipt_id =
        careerbrew_feed::read_json(root, &pm_learn::receipt_relative(receipt_day), Value::Null)?;
    let receipt_text = pm_learn::pm_sent_text(root, receipt_day)?;
    let learned_receipt = pm_learn::learn_sent(root, receipt_day, &PROMISES)?;
    reset_write_count(&ws.base)?;
    let learned_again = pm_learn::learn_sent(root, receipt_day, &PROMISES)?;
    let idle_writes = write_count(&ws.base);
    let outcome =
        careerbrew_feed::read_json(root, &pm_learn::outcome_relative(receipt_day), json!({}))?;
    let revision = text(field(&outcome, "revision"));
    let revision_ok =
        revision.len() == 32 && revision.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'));
    pm_learn::save_sent(
        root,
        invalid_day,
        "Invalid receipt text",
        Some(&pm_learn::Receipt {
            channel: "c0lower",
            ts: "1.2",
        }),
        &PROMISES,
    )?;
    let invalid = pm_learn::pm_sent_text(root, invalid_day)?;
    let channel = pm_learn::pm_sent_text(root, channel_day)?;
    let learned_channel = pm_learn::learn_sent(root, channel_day, &PROMISES)?;
    set_setting(root, "pm_slack", "pm-person")?;
    let dm = pm_learn::pm_sent_text(root, dm_day)?;
    let learned_dm = pm_learn::learn_sent(root, dm_day, &PROMISES)?;
    let none = pm_learn::pm_sent_text(root, none_day)?;
    let learned_none = pm_learn::learn_sent(root, none_day, &PROMISES)?;
    careerbrew_feed::write_json(
        root,
        &pm_learn::receipt_relative(manual_day),
        &json!({"id": "slack:bad"}),
    )?;
    let malformed = pm_learn::pm_sent_text(root, manual_day)?;
    pm_learn::save_sent(
        root,
        receipt_day,
        "Billing: manual correction.",
        None,
        &PROMISES,
    )?;
    let after_manual = pm_learn::pm_sent_text(root, receipt_day)?;
    pm_learn::save_sent(root, invalid_day, "   ", None, &PROMISES)?;
    let cleared = pm_learn::pm_sent_text(root, invalid_day)?;
    let mut files = serde_json::Map::new();
    for item in days {
        files.insert(
            item.into(),
            json!([
                root.join(pm_learn::sent_relative(item)).exists(),
                root.join(pm_learn::receipt_relative(item)).exists(),
                root.join(pm_learn::outcome_relative(item)).exists(),
            ]),
        );
    }
    let mut locks = std::fs::read_dir(root.join("day"))?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".pm-sent-") && name.ends_with(".lock"))
        .collect::<Vec<_>>();
    locks.sort();
    Ok(json!({
        "no_target": no_target,
        "no_target_writes": no_target_writes,
        "manual": hide(&manual.unwrap_or_default()),
        "receipt_id": receipt_id,
        "receipt_text": hide(&receipt_text.unwrap_or_default()),
        "learned": [learned_receipt, learned_again, learned_channel, learned_dm, learned_none],
        "idle_writes": idle_writes,
        "revision_ok": revision_ok,
        "invalid": opt(invalid),
        "channel": opt(channel),
        "dm": opt(dm),
        "none": opt(none),
        "malformed": hide(&malformed.unwrap_or_default()),
        "after_manual": opt(after_manual),
        "cleared": opt(cleared),
        "files": files,
        "history": history_view(root)?,
        "locks": locks,
        "sealed": sealed(&root.join(pm_learn::sent_relative(receipt_day)))?,
        "plain_absent": plain_absent(&root.join(pm_learn::HISTORY))?,
    }))
}

const RAW: &str = "Billing: shipped invoices; next: tax export.\nSearch: tuned ranking.\nOps: status check: no action needed.\nLedger: fixed totals; overdue: audit.";
const BODY: &str = "Discovery: tuned ranking.\nBilling: shipped invoices.\nLedger: fixed totals; overdue: audit.\nLed the planning meeting with design.\nPayments: In draft";

fn pm_lessons(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "pm-lessons")?;
    let root = &ws.root;
    at("2026-10-20T18:00:00+00:00");
    pm_learn::remember(root, "2026-10-20", RAW, RAW)?;
    pm_learn::record(root, "2026-10-20", &format!("{BODY}\n{CANARY}"), "web")?;
    let after_one = pm_learn::rules(root, None)?;
    at("2026-10-21T18:00:00+00:00");
    pm_learn::remember(root, "2026-10-21", RAW, RAW)?;
    pm_learn::record(root, "2026-10-21", BODY, "web")?;
    let after_two = pm_learn::rules(root, None)?;
    pm_learn::remember(root, "2026-10-22", RAW, RAW)?;
    reset_write_count(&ws.base)?;
    pm_learn::record(root, "2026-10-22", RAW, "web")?;
    let unchanged_writes = write_count(&ws.base);
    pm_learn::record(root, "2026-10-22", "", "web")?;
    pm_learn::remember(root, "2026-10-23", "", "")?;
    pm_learn::record(root, "2026-10-23", "anything", "web")?;
    pm_learn::remember(root, "2026-10-24", RAW, RAW)?;
    pm_learn::record(root, "2026-10-24", BODY, "sent")?;
    reset_write_count(&ws.base)?;
    pm_learn::record(root, "2026-10-24", "web save after the sent text", "web")?;
    let web_after_sent_writes = write_count(&ws.base);
    let learned = pm_learn::rules(root, None)?;
    let lines = crate::splitlines(RAW);
    let ranked = BTreeSet::from(["Ledger".to_owned()]);
    let applied = pm_learn::apply(&lines, &ranked, &learned);
    let empty = json!({"cut": [], "rename": {}, "parts": [], "order": {}, "added": {}, "days": 0});
    let applied_none = pm_learn::apply(&lines, &BTreeSet::new(), &empty);
    let all_cut = pm_learn::apply(
        &["Ops: status check: no action needed.".to_owned()],
        &BTreeSet::new(),
        &learned,
    );
    let left = pm_learn::left_out(&lines, &learned);
    let rows = careerbrew_feed::read_jsonl(root, pm_learn::HISTORY)?;
    let projection_cut = pm_learn::cut_projection(
        root,
        &lines,
        "Billing: shipped.\nLedger: fixed.",
        &learned,
        &rows,
    )?;
    let projection_present =
        pm_learn::cut_projection(root, &lines, "- Ops: still here.", &learned, &rows)?;
    let projection_unknown =
        pm_learn::cut_projection(root, &lines, "| table | row |", &learned, &rows)?;
    let hints = [
        "We launched billing",
        "Led the sync",
        "Posted an update in #team",
        "Fixed a bug",
    ]
    .iter()
    .map(|line| pm_learn::hint(line))
    .collect::<Vec<_>>();
    at("2026-10-25T09:00:00+00:00");
    let reset_had = pm_learn::reset(root, "**ops**")?;
    let reset_missing = error_of(pm_learn::reset(root, "Nope"));
    let after_reset = pm_learn::rules(root, None)?;
    if plant() == "duplicate" {
        let rows = careerbrew_feed::read_jsonl(root, pm_learn::HISTORY)?;
        careerbrew_feed::write_jsonl(root, pm_learn::HISTORY, &rows)?;
    }
    for n in 0..15u32 {
        let item = day("2026-09-01")
            .checked_add_days(chrono::Days::new(n.into()))
            .expect("fixture day")
            .format("%Y-%m-%d")
            .to_string();
        pm_learn::remember(root, &item, "A: one.", "A: one.")?;
        pm_learn::record(root, &item, "A: two.", "web")?;
    }
    let kept = careerbrew_feed::read_jsonl(root, pm_learn::HISTORY)?
        .iter()
        .map(|row| text(field(row, "day")))
        .collect::<Vec<_>>();
    if plant() == "missing" {
        std::fs::remove_file(root.join(pm_learn::RESETS))?;
    }
    let projection = |value: (Vec<String>, Value)| json!([value.0, value.1]);
    let history = root.join(pm_learn::HISTORY);
    Ok(json!({
        "after_one": hide_value(&after_one),
        "after_two": hide_value(&after_two),
        "unchanged_writes": unchanged_writes,
        "web_after_sent_writes": web_after_sent_writes,
        "learned": hide_value(&learned),
        "applied": applied,
        "applied_none": applied_none,
        "all_cut": all_cut,
        "left_out": left,
        "projection": projection(projection_cut),
        "projection_present": projection(projection_present),
        "projection_unknown": projection(projection_unknown),
        "hints": hints,
        "reset_had": reset_had,
        "reset_missing": reset_missing,
        "after_reset_cut": after_reset["cut"],
        "kept_days": kept,
        "sealed": sealed(&history)?,
        "plain_absent": plain_absent(&history)?,
    }))
}

pub fn run(case: &str, base: &Path) -> Result<Value> {
    match case {
        "difflib-vectors" => difflib_vectors(base),
        "results-records" => results_records(base),
        "handoff-records" => handoff_records(base),
        "pm-sent-receipts" => pm_sent_receipts(base),
        "pm-lessons" => pm_lessons(base),
        _ => Err(crate::value_error("unknown rust-17 case")),
    }
}

pub fn run_all(selected: Option<&str>) -> Result<String> {
    let base = careerbrew_feed::root()?.join("rust17-workload");
    std::fs::create_dir_all(&base)?;
    let mut cases = Vec::new();
    for case in CASES {
        if selected.is_none_or(|selected| selected == *case) {
            let result = match run(case, &base) {
                Ok(result) => result,
                Err(error) => json!({"error": error.class, "message": error.message}),
            };
            cases.push(json!({"id": case, "result": result}));
        }
    }
    Ok(serde_json::to_string(&careerbrew_projects::typed(&json!({"cases": cases})))? + "\n")
}
