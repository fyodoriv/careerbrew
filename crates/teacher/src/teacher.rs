//! `teacher.py`: one step at a time toward the owner's upcoming goal.
//!
//! The order, the teaching rules, the local-day counting and the predicted
//! feedback follow the Python module line by line. Step timers use the wall
//! clock, as in Python; every other stamp uses the pinned clock.

use crate::seams;
use crate::{err, field, goals, now_local, now_utc, or, stamp, text, truthy, Ctx, Result};
use careerbrew_policy::vectors::Out;
use careerbrew_vault::canon::Canon;
use chrono::{Duration, SecondsFormat, Utc};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const HINTS_BEFORE_SOLUTION: usize = 3;
pub const RETRY_DAYS: i64 = 3;
pub const LOW_RATING: i64 = 2;
pub const LOW_TIMES: usize = 2;
pub const LOW_DAYS: i64 = 30;

const LOG: &str = "teach/steps.jsonl";
const STATE: &str = "teach/state.json";
const DRAFTS: &str = "teach/drafts.json";
const FEEDBACK: &str = "teach/feedback.jsonl";
const APPROVALS: &str = "approvals.json";
const APPLICATIONS: &str = "jobs/applications.jsonl";

fn auto(signal: &str) -> &'static str {
    match signal {
        "repeat-mistake" => "applied: exercises now open with a 'watch out' tip for the mistake kinds you make most",
        "stuck-exercise" => "applied: a worked example after two failures, the reference solution after three, and the exercise comes back later",
        "low-confirm" => "applied: the lesson comes back with a worked example",
        "card-leech" => "applied: the card is suspended so it stops costing time",
        _ => "",
    }
}

/// `rules_version`: the first 8 characters of the teaching rules digest.
pub fn rules_version(ctx: &Ctx) -> Result<String> {
    let rules = std::fs::read_to_string(ctx.repo.join("knowledge/teaching.yaml"))?;
    Ok(careerbrew_tutor::digest(&rules)[..8].to_owned())
}

/// `save_draft`.
pub fn save_draft(ctx: &Ctx, exercise_id: &str, code: &str) -> Result<()> {
    if exercise_id.is_empty() || code.chars().count() > 100_000 {
        return Err(err(
            "ValueError",
            "send an exercise id and at most 100000 characters of code",
        ));
    }
    let mut all = drafts(ctx)?;
    all.insert(exercise_id.to_owned(), json!(code));
    careerbrew_tutor::write_json(&ctx.root, DRAFTS, &Value::Object(all))
}

/// `drafts`.
pub fn drafts(ctx: &Ctx) -> Result<Map<String, Value>> {
    Ok(crate::read_json(&ctx.root, DRAFTS, json!({}))?
        .as_object()
        .cloned()
        .unwrap_or_default())
}

pub fn log(ctx: &Ctx) -> Result<Vec<Value>> {
    crate::read_jsonl(&ctx.root, LOG)
}

fn zone(ctx: &Ctx) -> chrono_tz::Tz {
    // Planted defect for the negative control: count days in UTC.
    if std::env::var("CAREERBREW_RUST21_PLANT_UTC").as_deref() == Ok("1") {
        return chrono_tz::UTC;
    }
    careerbrew_tutor::local_zone(&ctx.root)
}

/// `_today_rows`: rows stamped in UTC, counted by their day in the settings zone.
pub fn today_rows(ctx: &Ctx) -> Result<Vec<Value>> {
    let tz = zone(ctx);
    let today = now_local(&ctx.root).with_timezone(&tz).date_naive();
    Ok(log(ctx)?
        .into_iter()
        .filter(|row| {
            careerbrew_tutor::parse_at(&text(field(row, "at")))
                .is_ok_and(|at| at.with_timezone(&tz).date_naive() == today)
        })
        .collect())
}

pub fn state(ctx: &Ctx) -> Result<Value> {
    let value = crate::read_json(&ctx.root, STATE, json!({}))?;
    Ok(if truthy(Some(&value)) {
        value
    } else {
        json!({})
    })
}

pub fn save_state(ctx: &Ctx, value: &Value) -> Result<()> {
    careerbrew_tutor::write_json(&ctx.root, STATE, value)
}

fn seconds_of(row: &Value) -> i64 {
    match field(row, "seconds") {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|f| f.trunc() as i64))
            .unwrap_or(0),
        Some(Value::String(text)) => text.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn today_minutes(ctx: &Ctx) -> Result<i64> {
    Ok(today_rows(ctx)?
        .iter()
        .map(seconds_of)
        .sum::<i64>()
        .div_euclid(60))
}

fn list_of(value: &Value, key: &str) -> Vec<String> {
    careerbrew_tutor::strings(field(value, key))
}

fn sent_jobs(ctx: &Ctx) -> Result<BTreeSet<String>> {
    let mut out = crate::read_jsonl(&ctx.root, APPLICATIONS)?
        .iter()
        .map(|row| match or(field(row, "posting_id")) {
            Some(id) => text(Some(id)),
            None => careerbrew_tutor::casefold(&text(field(row, "company"))),
        })
        .collect::<BTreeSet<_>>();
    out.extend(list_of(&state(ctx)?, "skipped_jobs"));
    Ok(out)
}

fn step(kind: &str, title: impl Into<String>, why: impl Into<String>, data: Value) -> Value {
    // The step timer runs on the wall clock: it measures real seconds.
    let mut out = json!({
        "kind": kind,
        "title": title.into(),
        "why": why.into(),
        "started": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, false),
    });
    for (key, value) in data.as_object().into_iter().flatten() {
        out[key] = value.clone();
    }
    out
}

fn to_value(out: &Out) -> Result<Value> {
    Ok(serde_json::from_str(&out.dumps(false))?)
}

fn py_error(error: careerbrew_policy::allow::PyError) -> crate::TutorError {
    err(error.class, error.message)
}

/// `approvals.pending` over the rust-04 decision and rust-05 storage.
pub fn pending(ctx: &Ctx) -> Result<Vec<Value>> {
    let records = crate::read_json(&ctx.root, APPROVALS, json!([]))?;
    careerbrew_policy::approvals::pending(
        &careerbrew_policy::Rules::python(),
        &Out::from_value(&records),
    )
    .map_err(py_error)?
    .iter()
    .map(to_value)
    .collect()
}

/// `approvals.decide` under `.approvals.json.lock`.
pub fn decide(ctx: &Ctx, id: &str, decision: &str, note: &str) -> Result<Value> {
    careerbrew_policy::approvals::validate_decision(decision).map_err(py_error)?;
    let path = ctx.root.join(APPROVALS);
    let decided = careerbrew_vault::lock::with_lock(&path, || -> Result<Value> {
        let records = crate::read_json(&ctx.root, APPROVALS, json!([]))?;
        let (decided, items) = careerbrew_policy::approvals::decide_items_with(
            &careerbrew_policy::Rules::python(),
            &Out::from_value(&records),
            id,
            decision,
            note,
            || stamp(&ctx.root),
        )
        .map_err(py_error)?;
        let items = items.iter().map(to_value).collect::<Result<Vec<_>>>()?;
        careerbrew_tutor::write_json(&ctx.root, APPROVALS, &json!(items))?;
        to_value(&decided)
    })??;
    careerbrew_tutor::emit(
        &ctx.root,
        "approvals.decide",
        vec![("status", Canon::Str(decision.into()))],
    )?;
    Ok(decided)
}

/// Cards in Python's `all_cards()` order: deck files by name, cards in deck
/// order, first position kept; public cards after them.
pub fn ordered_cards(ctx: &Ctx) -> Result<Vec<Value>> {
    let all = careerbrew_tutor::cards::all_cards(&ctx.root, &ctx.public)?;
    let decks = ctx.root.join("tutor/decks");
    let mut files = if decks.is_dir() {
        std::fs::read_dir(&decks)?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "yaml"))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    files.sort();
    let mut order = Vec::<String>::new();
    for file in files {
        let deck = careerbrew_tutor::read_yaml_abs(&file, json!({}))?;
        for card in field(&deck, "cards")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let id = text(field(card, "id"));
            if !order.contains(&id) {
                order.push(id);
            }
        }
    }
    for id in all.keys() {
        if !order.contains(id) {
            order.push(id.clone());
        }
    }
    Ok(order
        .into_iter()
        .filter_map(|id| all.get(&id).cloned())
        .collect())
}

fn public_exercise(ctx: &Ctx, ex: &Value) -> Result<Value> {
    let mut out = Map::new();
    for key in [
        "id",
        "order",
        "pattern",
        "difficulty",
        "title",
        "statement",
        "signature",
        "lesson",
    ] {
        let value = field(ex, key)
            .cloned()
            .ok_or_else(|| err("KeyError", format!("'{key}'")))?;
        out.insert(key.into(), value);
    }
    let id = text(field(ex, "id"));
    let starter = drafts(ctx)?
        .get(&id)
        .filter(|code| truthy(Some(code)))
        .cloned()
        .or_else(|| or(field(ex, "starter")).cloned())
        .unwrap_or_else(|| json!(format!("{}\n    pass\n", text(field(ex, "signature")))));
    out.insert("starter".into(), starter);
    Ok(Value::Object(out))
}

fn shown_tests(ex: &Value) -> Value {
    json!(field(ex, "tests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(2)
        .map(|test| {
            let args = match test.get("args") {
                Some(args) => args.clone(),
                None => test.get("ops").cloned().unwrap_or(Value::Null),
            };
            json!({"args": args, "expected": test.get("expected").cloned().unwrap_or(Value::Null), "note": test.get("note").cloned().unwrap_or(json!(""))})
        })
        .collect::<Vec<_>>())
}

fn mini(ex: &Value) -> Value {
    json!({"title": field(ex, "title"), "statement": field(ex, "statement"), "solution": field(ex, "solution")})
}

/// `next_step`: the single most useful thing to do now.
pub fn next_step(ctx: &Ctx) -> Result<Value> {
    let goal = goals::load(ctx)?["upcoming"].clone();
    let has_goal = truthy(Some(&goal));
    let focus = goals::focus(ctx, if has_goal { Some(&goal) } else { None })?;
    let budget = or(field(&goal, "minutes_per_day"))
        .and_then(Value::as_i64)
        .unwrap_or(goals::DEFAULT_MINUTES);
    if let Some(approval) = pending(ctx)?.into_iter().next() {
        return Ok(step(
            "approve",
            format!("Approve or reject: {}", text(field(&approval, "title"))),
            "An agent is waiting for your decision.",
            json!({"approval": approval}),
        ));
    }
    if !has_goal {
        return Ok(step(
            "goal",
            "Set a goal first",
            "Lessons and exercises follow your upcoming goal, and none is set yet.",
            json!({"setup": "careerbrew goals set \"<your goal>\""}),
        ));
    }
    let s = state(ctx)?;
    let all_mistakes = crate::load_mistakes(&ctx.root)?;
    let recent = &all_mistakes[all_mistakes.len().saturating_sub(5)..];
    let last_alarm = text(field(&s, "last_alarm"));
    if let Some(rep) = recent
        .iter()
        .rev()
        .find(|m| truthy(field(m, "repeat")) && text(field(m, "at")) > last_alarm)
    {
        return Ok(step(
            "mistake",
            format!(
                "This mistake repeated: {} in {}",
                text(field(rep, "kind")).replace('-', " "),
                text(field(rep, "pattern"))
            ),
            "Repeated mistakes get fixed first, so they stop.",
            json!({"mistake": rep}),
        ));
    }
    if today_minutes(ctx)? >= budget {
        return Ok(step(
            "done",
            "That is today's study time. Well done.",
            format!(
                "You used your {budget} minutes for {}.",
                text(field(&goal, "title"))
            ),
            json!({"summary": summary(ctx)?}),
        ));
    }
    let topics = list_of(&focus, "card_topics");
    let due = careerbrew_tutor::cards::queue(
        &ctx.root,
        &ctx.public,
        careerbrew_tutor::now(),
        None,
        1,
        &topics,
    )?;
    let low = low_rated_kinds(ctx)?;
    if let Some(back) = retry_due(ctx)? {
        return Ok(back);
    }
    let review_first = due
        .first()
        .is_some_and(|card| text(field(card, "mode")) == "review");
    if review_first && !(low.contains("card") && !low.contains("exercise")) {
        let card = &due[0];
        return Ok(step(
            "card",
            text(field(card, "question")),
            "Due cards come before new material, so you keep what you learned.",
            json!({"card": card}),
        ));
    }
    let activities = list_of(&focus, "activities");
    let retry_later = list_of(&s, "retry_later");
    let ex = if activities.iter().any(|a| a == "exercise") || !has_goal {
        seams::next_exercise(ctx, &list_of(&focus, "patterns"), &retry_later)?
    } else {
        None
    };
    let confirmed = list_of(&s, "confirmed_patterns");
    if let Some(ex) = ex.as_ref() {
        let pattern = text(field(ex, "pattern"));
        if !confirmed.contains(&pattern) {
            let mut data = json!({
                "pattern": pattern,
                "lesson": field(ex, "lesson"),
                "prompt": format!("In one sentence, when would you use {pattern}?"),
            });
            let not_yet = log(ctx)?.iter().any(|row| {
                text(field(row, "kind")) == "lesson"
                    && text(field(row, "outcome")) == "not-yet"
                    && text(field(row, "ref")) == pattern
            });
            if not_yet {
                let first = seams::load(ctx)?
                    .into_iter()
                    .find(|e| text(field(e, "pattern")) == pattern)
                    .ok_or_else(|| err("StopIteration", ""))?;
                data["example"] = mini(&first);
            }
            return Ok(step(
                "lesson",
                format!("Lesson: {pattern}"),
                format!("You are about to start {pattern} exercises; learn the idea first."),
                data,
            ));
        }
        let id = text(field(ex, "id"));
        let tries = seams::attempts(ctx, Some(&id))?
            .into_iter()
            .filter(|a| !truthy(field(a, "correct")))
            .count();
        let mut public = public_exercise(ctx, ex)?;
        let hints = field(ex, "hints")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        public["hints"] = json!(hints[..tries.min(hints.len())]);
        if tries >= 2 {
            let done = seams::status(ctx)?;
            let order = field(ex, "order").and_then(Value::as_f64).unwrap_or(0.0);
            let prior = seams::load(ctx)?
                .into_iter()
                .filter(|e| {
                    text(field(e, "pattern")) == pattern
                        && field(e, "order").and_then(Value::as_f64).unwrap_or(0.0) < order
                        && seams::solved(&done, &text(field(e, "id")))
                })
                .collect::<Vec<_>>();
            if let Some(last) = prior.last() {
                public["worked_example"] = mini(last);
            }
        }
        public["watch_out"] = json!(watch_out(ctx, ex)?);
        public["tests"] = shown_tests(ex);
        return Ok(step(
            "exercise",
            text(field(ex, "title")),
            format!(
                "Next in the curriculum for {}: {pattern}, {}.",
                text(field(&focus, "why")),
                text(field(ex, "difficulty"))
            ),
            json!({"exercise": public, "attempts": tries}),
        ));
    }
    if let Some(card) = due.first() {
        let why = if text(field(card, "mode")) == "review" {
            "A due card."
        } else {
            "A new card for your goal."
        };
        return Ok(step(
            "card",
            text(field(card, "question")),
            why,
            json!({"card": card}),
        ));
    }
    let validated = list_of(&s, "validated_cards");
    let mine = ordered_cards(ctx)?.into_iter().find(|card| {
        matches!(text(field(card, "deck")).as_str(), "mine" | "notion")
            && !validated.contains(&text(field(card, "id")))
    });
    if let Some(card) = mine {
        let card = careerbrew_tutor::cards::with_explanation(&ctx.root, &ctx.public, card)?;
        return Ok(step(
            "validate",
            format!("Check your card: {}", text(field(&card, "question"))),
            "Cards you wrote are checked once against the explanation, so you never memorise a wrong answer.",
            json!({"card": card}),
        ));
    }
    let plan = crate::read_json(&ctx.root, "jobs/strategy.json", json!({}))?;
    let sent = sent_jobs(ctx)?;
    for tier in ["practice", "target", "dream"] {
        let rows = or(field(&plan, "tiers"))
            .and_then(|tiers| tiers.get(tier))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for row in rows {
            let company = text(field(&row, "company"));
            if !sent.contains(&text(field(&row, "posting_id")))
                && !sent.contains(&careerbrew_tutor::casefold(&company))
            {
                let skills = careerbrew_tutor::strings(field(&row, "skills"))
                    .into_iter()
                    .take(5)
                    .collect::<Vec<_>>()
                    .join(", ");
                return Ok(step(
                    "resume",
                    format!("Send your resume to {company}"),
                    format!("You chose this {tier} job. careerbrew never sends anything: open the posting, send it yourself, then confirm."),
                    json!({
                        "job": row,
                        "checklist": [
                            "Open the posting and re-read the requirements.",
                            format!("Tailor the top three resume bullets to the role's skills: {skills}"),
                            "Send it from your own email or the company's form.",
                            "Come back and press 'I sent it'.",
                        ],
                    }),
                ));
            }
        }
    }
    Ok(step(
        "done",
        "Nothing is waiting. You are ahead of your plan.",
        "Set a new upcoming goal, choose more jobs, or add cards.",
        json!({"summary": summary(ctx)?}),
    ))
}

/// `_retry_due`: the first retry exercise whose last try is RETRY_DAYS old.
fn retry_due(ctx: &Ctx) -> Result<Option<Value>> {
    let st = seams::status(ctx)?;
    let keep = list_of(&state(ctx)?, "retry_later")
        .into_iter()
        .filter(|id| !seams::solved(&st, id))
        .collect::<Vec<_>>();
    let cut = careerbrew_tutor::stamp_state(now_utc(&ctx.root) - Duration::days(RETRY_DAYS));
    for id in keep {
        let tries = seams::attempts(ctx, Some(&id))?;
        let Some(last) = tries.last() else { continue };
        if crate::py_str(field(last, "at").or(Some(&json!("")))) <= cut {
            let ex = seams::get(ctx, &id)?;
            let mut public = public_exercise(ctx, &ex)?;
            public["hints"] = json!([]);
            public["watch_out"] = json!(watch_out(ctx, &ex)?);
            public["tests"] = shown_tests(&ex);
            return Ok(Some(step(
                "exercise",
                text(field(&ex, "title")),
                format!("Back after {RETRY_DAYS} days, as promised: try it again from memory."),
                json!({"exercise": public, "attempts": 0, "retry": true}),
            )));
        }
    }
    Ok(None)
}

/// `_low_rated_kinds`.
pub fn low_rated_kinds(ctx: &Ctx) -> Result<BTreeSet<String>> {
    let cut = stamp_from(now_utc(&ctx.root) - Duration::days(LOW_DAYS));
    let mut counts = BTreeMap::<String, usize>::new();
    for row in crate::read_jsonl(&ctx.root, FEEDBACK)? {
        let rating = field(&row, "rating").filter(|value| value.is_i64() || value.is_u64());
        let at = match field(&row, "at") {
            None => String::new(),
            other => crate::py_str(other),
        };
        if truthy(field(&row, "step"))
            && rating
                .and_then(Value::as_i64)
                .is_some_and(|rating| rating <= LOW_RATING)
            && at >= cut
        {
            *counts.entry(text(field(&row, "step"))).or_default() += 1;
        }
    }
    Ok(counts
        .into_iter()
        .filter(|(_, n)| *n >= LOW_TIMES)
        .map(|(kind, _)| kind)
        .collect())
}

fn stamp_from(at: chrono::DateTime<chrono::FixedOffset>) -> String {
    careerbrew_tutor::stamp_at(at)
}

/// `watch_out`: this exercise's pitfall tips for the mistake kinds you make most.
pub fn watch_out(ctx: &Ctx, ex: &Value) -> Result<Vec<Value>> {
    let mut order = Vec::<String>::new();
    let mut counts = BTreeMap::<String, usize>::new();
    for m in crate::load_mistakes(&ctx.root)? {
        if text(field(&m, "source")).starts_with("lesson:") {
            continue;
        }
        let kind = text(field(&m, "kind"));
        let kind = if kind == "complexity" {
            "timeout".to_owned()
        } else {
            kind
        };
        if !counts.contains_key(&kind) {
            order.push(kind.clone());
        }
        *counts.entry(kind).or_default() += 1;
    }
    // Counter.most_common: count descending, ties in first-seen order.
    order.sort_by(|left, right| counts[right].cmp(&counts[left]));
    let pitfalls = or(field(ex, "pitfalls")).cloned().unwrap_or(json!({}));
    Ok(order
        .iter()
        .filter_map(|kind| or(pitfalls.get(kind)).cloned())
        .take(2)
        .collect())
}

fn started_seconds(step: &Value) -> i64 {
    let started = or(field(step, "started"))
        .and_then(|value| careerbrew_tutor::parse_at(&text(Some(value))).ok())
        .map(|at| at.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    (Utc::now() - started).num_seconds().max(0)
}

fn set_union(value: &mut Value, key: &str, item: String) {
    let mut set = list_of(value, key).into_iter().collect::<BTreeSet<_>>();
    set.insert(item);
    value[key] = json!(set.into_iter().collect::<Vec<_>>());
}

/// `complete`: record a finished step and return the message and the next step.
pub fn complete(ctx: &Ctx, step: &Value, result: &Value) -> Result<Value> {
    let kind = text(field(step, "kind"));
    let seconds = started_seconds(step);
    let mut s = state(ctx)?;
    let (mut message, mut outcome) = (String::new(), "done".to_owned());
    match kind.as_str() {
        "approve" => {
            let id = text(field(&step["approval"], "id"));
            let note = text(field(result, "note"));
            let decision = field(result, "decision")
                .map(|v| text(Some(v)))
                .ok_or_else(|| err("KeyError", "'decision'"))?;
            let a = decide(ctx, &id, &decision, &note)?;
            let status = text(field(&a, "status"));
            message = format!("Recorded: {status}. The agent will see it.");
            outcome = status;
        }
        "mistake" => {
            s["last_alarm"] = step["mistake"]["at"].clone();
            message = format!("Good. Remember: {}", text(field(&step["mistake"], "tip")));
        }
        "card" => {
            let rating = text(field(result, "rating"));
            let r = careerbrew_tutor::cards::review(
                &ctx.root,
                &ctx.public,
                &text(field(&step["card"], "id")),
                &rating,
                careerbrew_tutor::now(),
                Some(seconds),
            )?;
            message = if truthy(field(&r, "leech")) {
                "Leech: rewrite this card smaller.".into()
            } else if rating == "good" || rating == "easy" {
                "Keep going.".into()
            } else {
                "It will come back soon. Read the explanation once more.".into()
            };
            outcome = rating;
        }
        "lesson" => {
            let pattern = text(field(step, "pattern"));
            if truthy(field(result, "confirmed")) {
                set_union(&mut s, "confirmed_patterns", pattern);
                message = "Confirmed. Now try it on a real problem.".into();
                outcome = "confirmed".into();
            } else {
                careerbrew_tutor::mistakes::record(
                    &ctx.root,
                    &ctx.public,
                    &format!("lesson:{pattern}"),
                    &pattern,
                    "concept",
                    "not yet confident after the lesson",
                    None,
                )?;
                message = "That is fine. Read it once more, then explain it in your own words; it will come back tomorrow.".into();
                outcome = "not-yet".into();
            }
        }
        "exercise" => return complete_exercise(ctx, step, result, s, seconds),
        "validate" => {
            let decision = or(field(result, "decision"))
                .map(|v| text(Some(v)))
                .unwrap_or_else(|| "keep".into());
            let card = &step["card"];
            let id = text(field(card, "id"));
            if decision == "fix" && truthy(field(result, "answer")) {
                careerbrew_tutor::cards::add(
                    &ctx.root,
                    &text(field(card, "question")),
                    &text(field(result, "answer")),
                    &careerbrew_tutor::strings(field(card, "tags")),
                    &text(field(card, "deck")),
                )?;
            }
            set_union(&mut s, "validated_cards", id.clone());
            message = match decision.as_str() {
                "fix" => "Fixed and validated.",
                "drop" => "Dropped from your review.",
                _ => "Validated.",
            }
            .into();
            outcome = decision.clone();
            if decision == "drop" {
                let mut dropped = field(&s, "dropped_cards")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                dropped.push(json!(id));
                s["dropped_cards"] = json!(dropped);
                match careerbrew_tutor::cards::suspend(&ctx.root, &ctx.public, &id) {
                    Err(error) if error.class == "KeyError" => {}
                    other => other?,
                }
            }
        }
        "resume" => {
            let job = &step["job"];
            let company = text(field(job, "company"));
            if truthy(field(result, "sent")) {
                log_application(ctx, &company, "applied", "applied")?;
                message = format!("Logged: applied to {company}. Good luck.");
                outcome = "sent".into();
            } else {
                set_union(&mut s, "skipped_jobs", text(field(job, "posting_id")));
                message = "Skipped for now.".into();
                outcome = "skipped".into();
            }
        }
        _ => {}
    }
    save_state(ctx, &s)?;
    record(ctx, &kind, &outcome, seconds, step)?;
    Ok(json!({"message": message, "next": next_step(ctx)?}))
}

/// `strategy.log_application` (rust-19 owns it; this is its record side).
fn log_application(ctx: &Ctx, company: &str, tier: &str, stage: &str) -> Result<()> {
    let row = json!({"company": company, "tier": tier, "stage": stage, "at": stamp(&ctx.root), "origin": "user"});
    careerbrew_tutor::append_jsonl(&ctx.root, APPLICATIONS, &[row])?;
    careerbrew_tutor::emit(
        &ctx.root,
        "jobs.application",
        vec![
            ("tier", Canon::Str(tier.into())),
            ("status", Canon::Str(stage.into())),
        ],
    )
}

fn complete_exercise(
    ctx: &Ctx,
    step: &Value,
    result: &Value,
    mut s: Value,
    seconds: i64,
) -> Result<Value> {
    let exercise = &step["exercise"];
    let id = text(field(exercise, "id"));
    let retry_later = list_of(&s, "retry_later");
    if truthy(field(step, "retry")) && retry_later.contains(&id) {
        if !s.get("retry_since").is_some_and(Value::is_object) {
            s["retry_since"] = json!({});
        }
        s["retry_since"][&id] = json!(stamp(&ctx.root));
        s["retry_later"] = json!(retry_later.iter().filter(|i| **i != id).collect::<Vec<_>>());
    }
    let hints = field(exercise, "hints")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let code = text(field(result, "code"));
    let mut r = seams::submit(ctx, &id, &code, Some(seconds), hints)?;
    let g = r["grade"].clone();
    let correct = truthy(field(&g, "correct"));
    let outcome = if correct { "pass" } else { "fail" };
    // Python reads the saved state here, before this step's changes are saved.
    let saved = state(ctx)?;
    let since = match saved.get("retry_since").and_then(|v| v.get(&id)) {
        None => String::new(),
        other => crate::py_str(other),
    };
    let fails = seams::attempts(ctx, Some(&id))?
        .iter()
        .filter(|a| {
            !truthy(field(a, "correct"))
                && crate::py_str(field(a, "at").or(Some(&json!("")))) > since
        })
        .count();
    let message = if correct {
        let cx = field(&g, "complexity").filter(|v| truthy(Some(v)));
        let expected = cx.map(|cx| text(field(cx, "expected"))).unwrap_or_default();
        let complexity = match cx {
            Some(cx) if truthy(field(cx, "ok")) => format!(" Complexity {expected}: ok."),
            Some(_) => format!(" It works but grows too fast: aim for {expected}."),
            None => String::new(),
        };
        let style = careerbrew_tutor::strings(field(&g, "style"));
        let tidy = if style.is_empty() {
            " Clean code.".to_owned()
        } else {
            format!(" Tidy: {}.", style.join(", "))
        };
        format!("All tests pass. Now say in one sentence why it is correct and what its complexity is.{complexity}{tidy}")
    } else if fails >= HINTS_BEFORE_SOLUTION {
        r["solution"] = field(&seams::get(ctx, &id)?, "solution")
            .cloned()
            .unwrap_or(Value::Null);
        let mut later = field(&s, "retry_later")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if !later.contains(&json!(id)) {
            later.push(json!(id));
        }
        s["retry_later"] = json!(later);
        "Three tries: here is the reference solution. Read it line by line; this exercise comes back in a few days.".into()
    } else {
        let first = r["mistakes"].as_array().and_then(|m| m.first()).cloned();
        let lead = match first {
            Some(m) => format!(
                "{}{}",
                if truthy(field(&m, "repeat")) {
                    "Again: "
                } else {
                    ""
                },
                text(field(&m, "tip"))
            ),
            None => "Not yet.".into(),
        };
        format!("{lead} A new hint is unlocked.")
    };
    save_state(ctx, &s)?;
    record(ctx, "exercise", outcome, seconds, step)?;
    Ok(json!({"message": message, "result": r, "next": next_step(ctx)?}))
}

fn record(ctx: &Ctx, kind: &str, outcome: &str, seconds: i64, step: &Value) -> Result<()> {
    let reference = [
        or(field(step, "exercise")).and_then(|e| or(e.get("id"))),
        or(field(step, "card")).and_then(|c| or(c.get("id"))),
        or(field(step, "pattern")),
    ]
    .into_iter()
    .flatten()
    .next()
    .map(|value| text(Some(value)))
    .unwrap_or_default();
    let row = json!({"at": stamp(&ctx.root), "kind": kind, "outcome": outcome, "seconds": seconds, "ref": reference, "rules": rules_version(ctx)?});
    careerbrew_tutor::append_jsonl(&ctx.root, LOG, &[row])?;
    careerbrew_tutor::emit(
        &ctx.root,
        "teach.step",
        vec![
            ("status", Canon::Str(outcome.into())),
            ("seconds", Canon::Int(seconds.into())),
        ],
    )
}

/// `summary`: today's steps by the settings zone's local day.
pub fn summary(ctx: &Ctx) -> Result<Value> {
    let rows = today_rows(ctx)?;
    let mut by_kind = Map::new();
    for row in &rows {
        let kind = text(field(row, "kind"));
        let count = by_kind.get(&kind).and_then(Value::as_i64).unwrap_or(0);
        by_kind.insert(kind, json!(count + 1));
    }
    Ok(json!({
        "steps": rows.len(),
        "minutes": rows.iter().map(seconds_of).sum::<i64>().div_euclid(60),
        "by_kind": by_kind,
        "mistake_repeat_rate": careerbrew_tutor::mistakes::repeat_rate(&ctx.root)?,
    }))
}

/// `feedback`.
pub fn feedback(ctx: &Ctx, words: &str, step_kind: &str, rating: Option<i64>) -> Result<Value> {
    let line = words.split_whitespace().collect::<Vec<_>>().join(" ");
    let row = json!({
        "at": stamp(&ctx.root),
        "text": line.chars().take(2000).collect::<String>(),
        "step": step_kind.chars().take(30).collect::<String>(),
        "rating": rating,
    });
    careerbrew_tutor::append_jsonl(&ctx.root, FEEDBACK, std::slice::from_ref(&row))?;
    careerbrew_tutor::emit(
        &ctx.root,
        "teach.feedback",
        vec![("status", Canon::Str("written".into()))],
    )?;
    Ok(row)
}

fn median(values: &[i64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2] as f64
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) as f64 / 2.0
    }
}

fn push(out: &mut Vec<Value>, signal: &str, reference: String, proposal: String) {
    out.push(json!({"signal": signal, "ref": reference, "proposal": proposal}));
}

/// `improvements`: predicted feedback with a proposed fix.
pub fn improvements(ctx: &Ctx) -> Result<Vec<Value>> {
    let rows = log(ctx)?;
    let mut out = Vec::new();
    let mut by_kind = BTreeMap::<String, Vec<i64>>::new();
    for row in &rows {
        by_kind
            .entry(text(field(row, "kind")))
            .or_default()
            .push(seconds_of(row));
    }
    for row in &rows[rows.len().saturating_sub(50)..] {
        let kind = text(field(row, "kind"));
        let values = &by_kind[&kind];
        let med = (values.len() >= 5).then(|| median(values));
        let seconds = seconds_of(row);
        if let Some(med) = med.filter(|med| *med != 0.0) {
            if seconds as f64 > 3.0 * med && seconds > 120 {
                let reference = text(field(row, "ref"));
                let item = if reference.is_empty() {
                    "this item".to_owned()
                } else {
                    reference.clone()
                };
                push(
                    &mut out,
                    "slow-step",
                    reference,
                    format!(
                        "{kind} step on {item} took {} min (median {} min): split it or add a lesson before it.",
                        seconds.div_euclid(60),
                        (med.trunc() as i64).div_euclid(60)
                    ),
                );
            }
        }
    }
    let mut stuck_order = Vec::<String>::new();
    let mut stuck = BTreeMap::<String, i64>::new();
    for a in seams::attempts(ctx, None)? {
        let id = text(field(&a, "exercise"));
        if !stuck.contains_key(&id) {
            stuck_order.push(id.clone());
        }
        let n = if truthy(field(&a, "correct")) {
            0
        } else {
            stuck.get(&id).copied().unwrap_or(0) + 1
        };
        stuck.insert(id, n);
    }
    for id in stuck_order {
        let n = stuck[&id];
        if n >= 3 {
            push(&mut out, "stuck-exercise", id.clone(), format!("{id}: {n} failed attempts. Add an easier warm-up exercise in its pattern, or a worked example in its lesson."));
        }
    }
    let mut rep_order = Vec::<String>::new();
    let mut reps = BTreeMap::<String, i64>::new();
    for m in crate::load_mistakes(&ctx.root)? {
        if truthy(field(&m, "repeat")) {
            let key = format!(
                "{}: {}",
                text(field(&m, "pattern")),
                text(field(&m, "kind"))
            );
            if !reps.contains_key(&key) {
                rep_order.push(key.clone());
            }
            *reps.entry(key).or_default() += 1;
        }
    }
    for key in rep_order {
        let n = reps[&key];
        if n >= 2 {
            push(&mut out, "repeat-mistake", key.clone(), format!("{key} repeated {n} times. Add a pitfall tip for this kind to the pattern's exercises and a focused card."));
        }
    }
    let kinds = rows[rows.len().saturating_sub(6)..]
        .iter()
        .filter(|row| text(field(row, "outcome")) == "skipped")
        .map(|row| text(field(row, "kind")))
        .collect::<Vec<_>>();
    if kinds.len() >= 2 && kinds.iter().all(|kind| *kind == kinds[0]) {
        push(
            &mut out,
            "skipped",
            kinds[0].clone(),
            format!(
                "{} steps were skipped twice in a row: move them later in the order, or ask why.",
                kinds[0]
            ),
        );
    }
    let nots = rows
        .iter()
        .filter(|row| {
            text(field(row, "kind")) == "lesson" && text(field(row, "outcome")) == "not-yet"
        })
        .collect::<Vec<_>>();
    for row in &nots[nots.len().saturating_sub(3)..] {
        let reference = text(field(row, "ref"));
        push(&mut out, "low-confirm", reference.clone(), format!("The lesson for {reference} did not land: rewrite it with an analogy and a tiny example."));
    }
    let stats = careerbrew_tutor::cards::stats(&ctx.root, &ctx.public, careerbrew_tutor::now())?;
    for card in careerbrew_tutor::strings(field(&stats, "suspended")) {
        push(&mut out, "card-leech", card.clone(), format!("Card {card} is a leech: rewrite it smaller, or report it for a better explanation."));
    }
    let written = crate::read_jsonl(&ctx.root, FEEDBACK)?;
    for row in &written[written.len().saturating_sub(10)..] {
        let reference = or(field(row, "step"))
            .map(|v| text(Some(v)))
            .unwrap_or_default();
        push(
            &mut out,
            "written",
            reference,
            format!("You wrote: {}", text(field(row, "text"))),
        );
    }
    for row in &mut out {
        let signal = text(field(row, "signal"));
        row["auto"] = json!(auto(&signal));
    }
    Ok(out)
}
