//! `goals.py`: long-term goals and the queue of upcoming goals.
//!
//! The current goal is the nearest dated goal still ahead (by the pinned
//! clock's own date), else the first undated one.

use crate::seams::{self, PATTERNS};
use crate::{err, field, or, py_str, round_even, text, today, truthy, Ctx, Result};
use careerbrew_vault::canon::Canon;
use chrono::NaiveDate;
use serde_json::{json, Value};

pub const KINDS: &[&str] = &["interview", "learn", "job-search", "promotion", "custom"];
pub const JOB_HUNT_KINDS: &[&str] = &["interview", "job-search"];
pub const DEFAULT_MINUTES: i64 = 90;
const CODING_STEPS: &[&str] = &[
    "technical-phone-screen",
    "coding-onsite",
    "online-assessment",
    "pair-programming",
    "take-home",
];
const PATH: &str = "goals.yaml";

fn goal_error(message: impl Into<String>) -> crate::TutorError {
    err("GoalError", message)
}

fn date_of(goal: &Value) -> Option<String> {
    or(field(goal, "date")).map(|value| text(Some(value)))
}

/// `_current(queue, today)`.
pub fn current(queue: &[Value], today: NaiveDate) -> Option<Value> {
    let t = today.format("%Y-%m-%d").to_string();
    let mut ahead = queue
        .iter()
        .filter(|goal| date_of(goal).is_some_and(|date| date >= t))
        .collect::<Vec<_>>();
    ahead.sort_by_key(|goal| date_of(goal));
    ahead
        .first()
        .map(|goal| (*goal).clone())
        .or_else(|| queue.iter().find(|goal| date_of(goal).is_none()).cloned())
}

fn list(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

/// `goals.load`.
pub fn load(ctx: &Ctx) -> Result<Value> {
    let data = careerbrew_tutor::read_yaml(&ctx.root, PATH, json!({}))?;
    let queue = match field(&data, "queue") {
        Some(Value::Null) | None => match or(field(&data, "upcoming")) {
            Some(upcoming) => vec![upcoming.clone()],
            None => Vec::new(),
        },
        Some(queue) => list(Some(queue)),
    };
    let upcoming = current(&queue, today(&ctx.root)).unwrap_or(Value::Null);
    Ok(json!({
        "long_term": field(&data, "long_term").cloned().unwrap_or(json!([])),
        "queue": queue,
        "upcoming": upcoming,
        "history": field(&data, "history").cloned().unwrap_or(json!([])),
    }))
}

/// `goals.job_hunt_goals`: the rust-07 port.
pub fn job_hunt_goals(data: &Value) -> usize {
    careerbrew_setup::machine::job_hunt_goals(data)
}

fn save(ctx: &Ctx, data: &Value) -> Result<()> {
    let history = list(field(data, "history"));
    let keep = history.len().saturating_sub(20);
    careerbrew_tutor::write_yaml(
        &ctx.root,
        PATH,
        &json!({
            "long_term": data["long_term"],
            "queue": data["queue"],
            "history": history[keep..],
        }),
    )
}

/// `_slug`.
pub fn slug(value: &str) -> String {
    let folded = careerbrew_tutor::casefold(value);
    let mut out = String::new();
    let mut gap = false;
    for ch in folded.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            if gap {
                out.push('-');
            }
            gap = false;
            out.push(ch);
        } else {
            gap = true;
        }
    }
    if gap {
        out.push('-');
    }
    let trimmed = out.trim_matches('-');
    let cut = trimmed.chars().take(40).collect::<String>();
    if cut.is_empty() {
        "goal".into()
    } else {
        cut
    }
}

/// `date.fromisoformat` for the date forms the owner writes.
fn parse_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .filter(|_| value.len() == 10)
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y%m%d")
                .ok()
                .filter(|_| value.len() == 8)
        })
}

/// `_date`.
fn date(value: Option<&Value>, name: &str) -> Result<Value> {
    let Some(value) = value.filter(|value| !value.is_null() && value.as_str() != Some("")) else {
        return Ok(Value::Null);
    };
    parse_date(&py_str(Some(value)))
        .map(|date| json!(date.format("%Y-%m-%d").to_string()))
        .ok_or_else(|| goal_error(format!("{name} must be a date like 2026-11-05")))
}

fn one_space(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Python `int(value)` for the minute field.
fn int(value: &Value) -> Option<i64> {
    match value {
        Value::Bool(flag) => Some(i64::from(*flag)),
        Value::Number(number) => number.as_i64().or_else(|| {
            number
                .as_f64()
                .filter(|f| f.is_finite())
                .map(|f| f.trunc() as i64)
        }),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// `set_long_term`.
pub fn set_long_term(ctx: &Ctx, items: &[Value]) -> Result<Value> {
    let mut clean = Vec::new();
    for item in items {
        let line = match field(item, "text") {
            None => String::new(),
            Some(value) => one_space(&py_str(Some(value))),
        };
        if line.is_empty() {
            continue;
        }
        let id = or(field(item, "id"))
            .cloned()
            .unwrap_or_else(|| json!(slug(&line)));
        clean.push(json!({
            "id": id,
            "text": line.chars().take(300).collect::<String>(),
            "by": date(field(item, "by"), "by")?,
        }));
    }
    let mut data = load(ctx)?;
    data["long_term"] = json!(clean);
    save(ctx, &data)?;
    careerbrew_tutor::emit(
        &ctx.root,
        "goals.update",
        vec![
            ("status", Canon::Str("long_term".into())),
            ("items", Canon::Int((clean.len() as i64).into())),
        ],
    )?;
    Ok(data)
}

/// `_clean`.
pub fn clean(ctx: &Ctx, goal: &Value) -> Result<Value> {
    let kind = or(field(goal, "kind"))
        .map(|value| py_str(Some(value)))
        .unwrap_or_else(|| "custom".into());
    if !KINDS.contains(&kind.as_str()) {
        return Err(goal_error(format!(
            "kind must be one of {}",
            KINDS.join(", ")
        )));
    }
    let title = match field(goal, "title") {
        None => String::new(),
        Some(value) => one_space(&py_str(Some(value))),
    };
    if title.is_empty() {
        return Err(goal_error("give the goal a title"));
    }
    let minutes = match field(goal, "minutes_per_day") {
        None => Some(DEFAULT_MINUTES),
        Some(value) => int(value),
    }
    .ok_or_else(|| goal_error("minutes_per_day must be a whole number"))?;
    if !(10..=600).contains(&minutes) {
        return Err(goal_error("minutes_per_day must be between 10 and 600"));
    }
    let id = or(field(goal, "id"))
        .map(|value| py_str(Some(value)))
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| slug(&title));
    let company = or(field(goal, "company"))
        .map(|value| py_str(Some(value)).trim().to_owned())
        .filter(|company| !company.is_empty());
    let topics = field(goal, "topics")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|topic| py_str(Some(topic)))
        .filter(|topic| !topic.trim().is_empty())
        .map(|topic| careerbrew_tutor::casefold(&topic).trim().to_owned())
        .collect::<Vec<_>>();
    let set_at = or(field(goal, "set_at"))
        .cloned()
        .unwrap_or_else(|| json!(crate::stamp(&ctx.root)));
    Ok(json!({
        "id": id,
        "kind": kind,
        "title": title.chars().take(200).collect::<String>(),
        "date": date(field(goal, "date"), "date")?,
        "company": company,
        "topics": topics,
        "minutes_per_day": minutes,
        "set_at": set_at,
    }))
}

/// `set_queue`: replace every upcoming goal at once and re-plan.
pub fn set_queue(ctx: &Ctx, queue: &[Value]) -> Result<Value> {
    let clean = queue
        .iter()
        .map(|goal| clean(ctx, goal))
        .collect::<Result<Vec<_>>>()?;
    let ids = clean
        .iter()
        .map(|goal| text(field(goal, "id")))
        .collect::<Vec<_>>();
    let unique = ids.iter().collect::<std::collections::BTreeSet<_>>();
    if unique.len() != ids.len() {
        return Err(goal_error(
            "two upcoming goals have the same title; make them different",
        ));
    }
    if careerbrew_setup::machine::role()? == "corporate"
        && clean
            .iter()
            .any(|goal| JOB_HUNT_KINDS.contains(&text(field(goal, "kind")).as_str()))
    {
        return Err(goal_error("interview and job-search goals are personal-only and this machine's role is corporate. Set them on your personal machine (job-hunt data never goes on employer hardware)."));
    }
    let mut data = load(ctx)?;
    let before = data["upcoming"].clone();
    data["queue"] = json!(clean);
    let after = current(&clean, today(&ctx.root));
    if truthy(Some(&before))
        && after
            .as_ref()
            .is_none_or(|after| field(after, "id") != field(&before, "id"))
    {
        let mut replaced = before.clone();
        replaced["replaced_at"] = json!(crate::stamp(&ctx.root));
        let mut history = list(field(&data, "history"));
        history.push(replaced);
        data["history"] = json!(history);
    }
    save(ctx, &data)?;
    careerbrew_tutor::emit(
        &ctx.root,
        "goals.update",
        vec![
            ("status", Canon::Str("upcoming".into())),
            ("items", Canon::Int((clean.len() as i64).into())),
        ],
    )?;
    let mut out = load(ctx)?;
    out["plan"] = ctx.seams.planner.replan()?;
    Ok(out)
}

/// `set_upcoming`: replace the current goal (None removes it) and re-plan.
pub fn set_upcoming(ctx: &Ctx, goal: Option<&Value>) -> Result<Value> {
    let data = load(ctx)?;
    let current = &data["upcoming"];
    let mut queue = list(field(&data, "queue"))
        .into_iter()
        .filter(|row| !truthy(Some(current)) || field(row, "id") != field(current, "id"))
        .collect::<Vec<_>>();
    if let Some(goal) = goal {
        let mut goal = goal.clone();
        goal["id"] = Value::Null;
        queue.insert(0, goal);
    }
    set_queue(ctx, &queue)
}

/// `remove`.
pub fn remove(ctx: &Ctx, id: &str) -> Result<Value> {
    let queue = list(field(&load(ctx)?, "queue"))
        .into_iter()
        .filter(|goal| text(field(goal, "id")) != id)
        .collect::<Vec<_>>();
    set_queue(ctx, &queue)
}

/// `days_left`.
pub fn days_left(ctx: &Ctx, goal: Option<&Value>, on: Option<NaiveDate>) -> Result<Option<i64>> {
    let goal = match goal {
        Some(goal) => goal.clone(),
        None => load(ctx)?["upcoming"].clone(),
    };
    let Some(date) = (if truthy(Some(&goal)) {
        date_of(&goal)
    } else {
        None
    }) else {
        return Ok(None);
    };
    let date = parse_date(&date).ok_or_else(|| err("ValueError", "invalid goal date"))?;
    let on = on.unwrap_or_else(|| today(&ctx.root));
    Ok(Some((date - on).num_days().max(0)))
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// `focus`: what the goal means for study.
pub fn focus(ctx: &Ctx, goal: Option<&Value>) -> Result<Value> {
    let goal = match goal {
        Some(goal) => goal.clone(),
        None => load(ctx)?["upcoming"].clone(),
    };
    let all = strings(PATTERNS);
    if !truthy(Some(&goal)) {
        return Ok(
            json!({"patterns": all, "card_topics": [], "activities": ["exercise", "cards", "lesson"], "why": "the default order (no upcoming goal yet)"}),
        );
    }
    let topics = or(field(&goal, "topics"))
        .map(|value| careerbrew_tutor::strings(Some(value)))
        .unwrap_or_default();
    let kind = text(field(&goal, "kind"));
    let in_topics = all
        .iter()
        .filter(|pattern| topics.contains(pattern))
        .cloned()
        .collect::<Vec<_>>();
    match kind.as_str() {
        "interview" => {
            let company = or(field(&goal, "company")).map(|value| text(Some(value)));
            let steps = match &company {
                Some(company) => ctx.seams.loops.loop_steps(company)?,
                None => Vec::new(),
            };
            let has = |id: &str| steps.iter().any(|step| step == id);
            let mut activities = Vec::new();
            if CODING_STEPS.iter().any(|step| has(step)) || steps.is_empty() {
                activities.push("exercise");
            }
            if has("system-design") {
                activities.push("system-design");
            }
            if ["behavioral", "hiring-manager", "bar-raiser"]
                .iter()
                .any(|step| has(step))
                || steps.is_empty()
            {
                activities.push("behavioural");
            }
            activities.extend(["cards", "mistakes"]);
            let mut card_topics = Vec::new();
            if has("system-design") {
                card_topics.push("system design".to_owned());
            }
            card_topics.extend(strings(&["behavioural", "leadership"]));
            card_topics.extend(topics);
            let patterns = if in_topics.is_empty() { all } else { in_topics };
            Ok(json!({
                "patterns": patterns,
                "card_topics": card_topics,
                "activities": activities,
                "loop": steps,
                "why": format!("{} interview loop", company.unwrap_or_else(|| "the".into())),
            }))
        }
        "learn" => {
            let card_topics = topics
                .iter()
                .filter(|topic| !in_topics.contains(topic))
                .cloned()
                .collect::<Vec<_>>();
            let mut activities = if in_topics.is_empty() {
                Vec::new()
            } else {
                vec!["exercise"]
            };
            activities.extend(["lesson", "cards", "mistakes"]);
            let patterns = if in_topics.is_empty() { all } else { in_topics };
            Ok(
                json!({"patterns": patterns, "card_topics": card_topics, "activities": activities, "why": "your learning topics"}),
            )
        }
        "job-search" => Ok(
            json!({"patterns": all, "card_topics": topics, "activities": ["applications", "exercise", "cards"], "why": "the jobs you chose"}),
        ),
        "promotion" => Ok(
            json!({"patterns": [], "card_topics": topics, "activities": ["promotion", "cards"], "why": "your promotion case"}),
        ),
        _ => Ok(
            json!({"patterns": all, "card_topics": topics, "activities": ["exercise", "cards", "lesson"], "why": "your goal"}),
        ),
    }
}

fn minutes_of(goal: &Value) -> i64 {
    or(field(goal, "minutes_per_day"))
        .and_then(int)
        .unwrap_or(DEFAULT_MINUTES)
}

/// `study_blocks`: today's study blocks for the upcoming goal.
pub fn study_blocks(ctx: &Ctx, on: Option<NaiveDate>) -> Result<Vec<Value>> {
    let queue = list(field(&load(ctx)?, "queue"));
    let Some(goal) = current(&queue, on.unwrap_or_else(|| today(&ctx.root))) else {
        return Ok(Vec::new());
    };
    let kind = text(field(&goal, "kind"));
    if careerbrew_setup::machine::role()? == "corporate"
        && (kind == "interview" || kind == "job-search")
    {
        return Ok(Vec::new());
    }
    let f = focus(ctx, Some(&goal))?;
    let minutes = minutes_of(&goal);
    let left = days_left(ctx, Some(&goal), on)?;
    let why = text(field(&f, "why"));
    if left == Some(0) {
        return Ok(vec![
            json!({"id": "goal:review", "title": format!("Goal day: {}. Light review only, then rest.", text(field(&goal, "title"))), "minutes": minutes.min(30), "goal": true, "reason": "the goal is today"}),
        ]);
    }
    let acts = careerbrew_tutor::strings(field(&f, "activities"));
    let share = |act: &str| if act == "exercise" { 2 } else { 1 };
    let total = acts.iter().map(|act| share(act)).sum::<i64>();
    let patterns = careerbrew_tutor::strings(field(&f, "patterns"));
    let card_topics = careerbrew_tutor::strings(field(&f, "card_topics"));
    let mut blocks = Vec::new();
    for act in &acts {
        let m = 10.max(round_even((minutes * share(act)) as f64 / total as f64 / 5.0) * 5);
        match act.as_str() {
            "exercise" => {
                let st = seams::status(ctx)?;
                let todo = seams::load(ctx)?
                    .into_iter()
                    .filter(|exercise| {
                        patterns.contains(&text(field(exercise, "pattern")))
                            && !seams::solved(&st, &text(field(exercise, "id")))
                    })
                    .collect::<Vec<_>>();
                let per_day = match left {
                    Some(left) if left != 0 => 1.max((todo.len() as i64 + left - 1) / left),
                    _ => 2,
                };
                let title = match todo.first() {
                    Some(first) => format!(
                        "Coding: {} exercise(s) in {}",
                        per_day.min(1.max(m / 20)),
                        text(field(first, "pattern"))
                    ),
                    None => "Coding: review solved exercises".into(),
                };
                blocks.push(json!({"id": "goal:exercise", "title": title, "minutes": m, "goal": true, "reason": why}));
            }
            "cards" => {
                let suffix = if card_topics.is_empty() {
                    String::new()
                } else {
                    format!(
                        " ({})",
                        card_topics.iter().take(2).cloned().collect::<Vec<_>>().join(", ")
                    )
                };
                blocks.push(json!({"id": "goal:cards", "title": format!("Flashcards: due reviews, then new cards{suffix}"), "minutes": m, "goal": true, "reason": why}));
            }
            "mistakes" => blocks.push(json!({"id": "goal:mistakes", "title": "Mistake review: your prevention cards", "minutes": 10.max(m / 2), "goal": true, "reason": "so mistakes do not repeat"})),
            other => {
                let label = match other {
                    "system-design" => "System design: one practice design",
                    "behavioural" => "Behavioural: one STAR story out loud",
                    "lesson" => "Lesson: learn and confirm one topic",
                    "applications" => "Applications: prepare one chosen job",
                    "promotion" => "Promotion: close one gap",
                    other => other,
                };
                blocks.push(json!({"id": format!("goal:{other}"), "title": label, "minutes": m, "goal": true, "reason": why}));
            }
        }
    }
    Ok(blocks)
}
