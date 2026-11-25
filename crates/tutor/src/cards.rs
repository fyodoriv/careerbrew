//! Interview flashcards, FSRS reviews, leeches, and local-day statistics.

use crate::fsrs::{self, CardState, Phase};
use crate::{
    append_jsonl, casefold, digest, emit, field, local_zone, read_json, read_jsonl, read_yaml,
    stamp_at, strings, text, truthy, write_json, write_yaml, Result, TutorError,
};
use careerbrew_vault::canon::Canon;
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, TimeZone};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const LEECH_LAPSES: u64 = 8;
pub const DEFAULT_NEW_PER_DAY: i64 = 20;

fn slug(value: &str) -> String {
    let folded = casefold(value);
    let slug = Regex::new(r"[^a-z0-9]+")
        .expect("card slug regex")
        .replace_all(&folded, "-")
        .trim_matches('-')
        .to_owned();
    if slug.is_empty() {
        "card".into()
    } else {
        slug
    }
}

fn state_path() -> &'static str {
    "tutor/card-state.json"
}

fn reviews_path() -> &'static str {
    "tutor/card-reviews.jsonl"
}

fn meta_path() -> &'static str {
    "tutor/card-meta.json"
}

fn deck_path(name: &str) -> String {
    format!("tutor/decks/{}.yaml", slug(name))
}

pub fn write_deck(root: &Path, name: &str, deck: &[Value]) -> Result<usize> {
    let mut clean = Vec::new();
    for card in deck {
        let question = text(field(card, "question")).trim().to_owned();
        let answer = text(field(card, "answer")).trim().to_owned();
        if question.is_empty() || answer.is_empty() {
            continue;
        }
        let tags = strings(field(card, "tags"));
        let topic = text(field(card, "topic"));
        let topic = if topic.is_empty() {
            tags.first().cloned().unwrap_or_else(|| "general".into())
        } else {
            topic
        };
        let mut row = json!({
            "id": text(field(card, "id")),
            "question": question,
            "answer": answer,
            "topic": casefold(&topic),
            "tags": tags,
        });
        if truthy(field(card, "source")) {
            row["source"] = field(card, "source").cloned().unwrap_or(Value::Null);
        }
        clean.push(row);
    }
    let count = clean.len();
    write_yaml(root, &deck_path(name), &json!({"cards": clean}))?;
    Ok(count)
}

pub fn add(
    root: &Path,
    question: &str,
    answer: &str,
    tags: &[String],
    deck: &str,
) -> Result<Value> {
    if question.trim().is_empty() {
        return Err(TutorError::value("a card needs a question"));
    }
    if answer.trim().is_empty() {
        return Err(TutorError::value("a card needs an answer"));
    }
    let relative = deck_path(deck);
    let mut data = read_yaml(root, &relative, json!({"cards": []}))?;
    let id = format!("{}-{}", slug(deck), &digest(question)[..10]);
    let card = json!({
        "id": id,
        "question": question.trim(),
        "answer": answer.trim(),
        "topic": casefold(tags.first().map(String::as_str).unwrap_or("general")),
        "tags": tags,
    });
    let mut rows = field(&data, "cards")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    rows.retain(|old| text(field(old, "id")) != id);
    rows.push(card.clone());
    data["cards"] = Value::Array(rows);
    write_yaml(root, &relative, &data)?;
    emit(root, "cards.add", vec![("status", Canon::Str("ok".into()))])?;
    let mut card = card;
    card["deck"] = json!(slug(deck));
    Ok(card)
}

pub fn all_cards(root: &Path, public_dir: &Path) -> Result<BTreeMap<String, Value>> {
    let folder = root.join("tutor/decks");
    let mut files = std::fs::read_dir(&folder)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "yaml")
        })
        .collect::<Vec<_>>();
    files.sort();
    let mut out = BTreeMap::new();
    for path in files {
        let data = crate::read_yaml_abs(&path, json!({}))?;
        for mut card in field(&data, "cards")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let id = text(field(&card, "id"));
            card["deck"] = json!(path.file_stem().unwrap_or_default().to_string_lossy());
            out.insert(id, card);
        }
    }
    let mine = out
        .values()
        .map(|card| crate::public::qhash(&text(field(card, "question"))))
        .collect::<BTreeSet<_>>();
    for card in crate::public::load(public_dir)? {
        let mut matches = strings(field(&card, "match"));
        matches.push(crate::public::qhash(&text(field(&card, "question"))));
        if matches.iter().all(|hash| !mine.contains(hash)) {
            let id = text(field(&card, "id"));
            out.insert(
                id,
                json!({
                    "id": text(field(&card, "id")),
                    "question": text(field(&card, "question")),
                    "answer": text(field(&card, "answer")),
                    "topic": text(field(&card, "topic")),
                    "tags": strings(field(&card, "tags")),
                    "deck": "public",
                }),
            );
        }
    }
    Ok(out)
}

fn with_explanation(root: &Path, public_dir: &Path, mut card: Value) -> Result<Value> {
    let explanation =
        crate::public::explanation_for(root, public_dir, &text(field(&card, "question")))?;
    card["explanation"] = json!(explanation
        .as_ref()
        .map(|row| text(field(row, "explanation")))
        .unwrap_or_default());
    card["explanation_id"] = json!(explanation
        .as_ref()
        .map(|row| text(field(row, "id")))
        .unwrap_or_default());
    Ok(card)
}

pub fn states(root: &Path) -> Result<BTreeMap<String, CardState>> {
    let raw = read_json(root, state_path(), json!({}))?;
    let mut out = BTreeMap::new();
    for (id, value) in raw.as_object().into_iter().flatten() {
        out.insert(id.clone(), CardState::from_json(value)?);
    }
    Ok(out)
}

pub fn scheduled(root: &Path) -> Result<BTreeSet<String>> {
    Ok(states(root)?.into_keys().collect())
}

fn suspended(root: &Path) -> Result<BTreeSet<String>> {
    let meta = read_json(root, meta_path(), json!({}))?;
    Ok(strings(field(&meta, "suspended")).into_iter().collect())
}

pub fn reviews(root: &Path) -> Result<Vec<Value>> {
    read_jsonl(root, reviews_path())
}

fn new_per_day(root: &Path) -> Result<i64> {
    let settings = read_yaml(root, "settings.yaml", json!({}))?;
    let value = field(&settings, "cards_new_per_day");
    let parsed = value
        .and_then(Value::as_i64)
        .or_else(|| {
            value
                .and_then(Value::as_str)
                .and_then(|text| text.parse().ok())
        })
        .unwrap_or(DEFAULT_NEW_PER_DAY);
    Ok(parsed.max(0))
}

fn interleave(rows: &[Value]) -> Vec<Value> {
    let mut by_topic = BTreeMap::<String, Vec<Value>>::new();
    for row in rows {
        by_topic
            .entry(text(field(row, "topic")))
            .or_default()
            .push(row.clone());
    }
    let mut indices = BTreeMap::<String, usize>::new();
    let mut out = Vec::new();
    loop {
        let mut added = false;
        for (topic, values) in &by_topic {
            let index = indices.entry(topic.clone()).or_default();
            if let Some(value) = values.get(*index) {
                out.push(value.clone());
                *index += 1;
                added = true;
            }
        }
        if !added {
            return out;
        }
    }
}

fn day(root: &Path, at: DateTime<FixedOffset>) -> NaiveDate {
    at.with_timezone(&local_zone(root)).date_naive()
}

fn review_day(root: &Path, value: &Value) -> Option<NaiveDate> {
    field(value, "at")
        .and_then(Value::as_str)
        .and_then(|stamp| DateTime::parse_from_rfc3339(stamp).ok())
        .map(|at| day(root, at))
}

pub fn queue(
    root: &Path,
    public_dir: &Path,
    at: DateTime<FixedOffset>,
    cap: Option<i64>,
    limit: usize,
    topics: &[String],
) -> Result<Vec<Value>> {
    let cap = match cap {
        Some(cap) => cap,
        None => new_per_day(root)?,
    };
    let cards = all_cards(root, public_dir)?;
    let states = states(root)?;
    let suspended = suspended(root)?;
    let mut due = cards
        .iter()
        .filter(|(id, _)| {
            !suspended.contains(*id) && states.get(*id).is_some_and(|state| state.due <= at)
        })
        .map(|(id, card)| {
            let mut card = card.clone();
            card["mode"] = json!("review");
            (states[id].due, id.clone(), card)
        })
        .collect::<Vec<_>>();
    due.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    let due_rows = interleave(&due.into_iter().map(|(_, _, card)| card).collect::<Vec<_>>());
    let today = day(root, at);
    let introduced = reviews(root)?
        .iter()
        .filter(|row| {
            field(row, "first") == Some(&Value::Bool(true)) && review_day(root, row) == Some(today)
        })
        .map(|row| text(field(row, "card")))
        .collect::<BTreeSet<_>>();
    let slots = (cap - introduced.len() as i64).max(0) as usize;
    let fresh = cards
        .iter()
        .filter(|(id, _)| !states.contains_key(*id) && !suspended.contains(*id))
        .map(|(_, card)| {
            let mut card = card.clone();
            card["mode"] = json!("new");
            card
        })
        .collect::<Vec<_>>();
    let wanted = topics
        .iter()
        .map(|topic| casefold(topic))
        .collect::<BTreeSet<_>>();
    let on_goal = fresh
        .iter()
        .filter(|card| {
            wanted.contains(&casefold(&text(field(card, "topic"))))
                || strings(field(card, "tags"))
                    .iter()
                    .any(|tag| wanted.contains(&casefold(tag)))
        })
        .cloned()
        .collect::<Vec<_>>();
    let goal_ids = on_goal
        .iter()
        .map(|card| text(field(card, "id")))
        .collect::<BTreeSet<_>>();
    let rest = fresh
        .iter()
        .filter(|card| !goal_ids.contains(&text(field(card, "id"))))
        .cloned()
        .collect::<Vec<_>>();
    let new_rows = interleave(&on_goal)
        .into_iter()
        .chain(interleave(&rest))
        .take(slots)
        .collect::<Vec<_>>();
    due_rows
        .into_iter()
        .chain(new_rows)
        .take(limit)
        .map(|card| with_explanation(root, public_dir, card))
        .collect()
}

pub fn review(
    root: &Path,
    public_dir: &Path,
    id: &str,
    rating: &str,
    at: DateTime<FixedOffset>,
    seconds: Option<i64>,
) -> Result<Value> {
    let rating_number = fsrs::rating_of(rating)?;
    let cards = all_cards(root, public_dir)?;
    let card = cards
        .get(id)
        .ok_or_else(|| TutorError::new("KeyError", format!("unknown card {id}")))?;
    if suspended(root)?.contains(id) {
        return Err(TutorError::value(format!(
            "card {id} is suspended as a leech"
        )));
    }
    let mut states = states(root)?;
    let first = !states.contains_key(id);
    let lapse = rating == "again"
        && !first
        && states
            .get(id)
            .is_some_and(|state| state.phase == Phase::Review);
    let initial;
    let old = match states.get(id) {
        Some(state) => Some(state),
        None => {
            initial = CardState::new(at.timestamp_millis() as u64, at);
            Some(&initial)
        }
    };
    let next = fsrs::next_state(old, rating_number, at)?;
    states.insert(id.to_owned(), next);
    write_json(root, state_path(), &fsrs::states_json(&states))?;
    let old_reviews = reviews(root)?;
    let lapses = old_reviews
        .iter()
        .filter(|row| {
            text(field(row, "card")) == id && field(row, "lapse") == Some(&Value::Bool(true))
        })
        .count() as u64
        + u64::from(lapse);
    let meta = read_json(root, meta_path(), json!({}))?;
    let base = field(&meta, "lapse_base")
        .and_then(|rows| rows.get(id))
        .and_then(Value::as_u64);
    let threshold = base.map_or(LEECH_LAPSES, |base| base + LEECH_LAPSES / 2);
    let leech = lapse && lapses >= threshold;
    if leech {
        let mut meta = meta;
        let ids = suspended(root)?
            .into_iter()
            .chain([id.to_owned()])
            .collect::<BTreeSet<_>>();
        meta["suspended"] = json!(ids.into_iter().collect::<Vec<_>>());
        write_json(root, meta_path(), &meta)?;
    }
    let row = json!({
        "card": id,
        "topic": text(field(card, "topic")),
        "rating": rating,
        "first": first,
        "at": stamp_at(at),
        "due": stamp_at(states[id].due),
        "seconds": seconds,
        "lapse": lapse,
        "lapses": lapses,
        "leech": leech,
    });
    append_jsonl(root, reviews_path(), std::slice::from_ref(&row))?;
    emit(
        root,
        "cards.review",
        vec![("status", Canon::Str(rating.into()))],
    )?;
    Ok(row)
}

pub fn suspend(root: &Path, public_dir: &Path, id: &str) -> Result<()> {
    if !all_cards(root, public_dir)?.contains_key(id) {
        return Err(TutorError::new("KeyError", format!("unknown card {id}")));
    }
    let mut meta = read_json(root, meta_path(), json!({}))?;
    let ids = suspended(root)?
        .into_iter()
        .chain([id.to_owned()])
        .collect::<BTreeSet<_>>();
    meta["suspended"] = json!(ids.into_iter().collect::<Vec<_>>());
    write_json(root, meta_path(), &meta)
}

pub fn unsuspend(root: &Path, public_dir: &Path, id: &str) -> Result<()> {
    if !all_cards(root, public_dir)?.contains_key(id) {
        return Err(TutorError::new("KeyError", format!("unknown card {id}")));
    }
    let mut meta = read_json(root, meta_path(), json!({}))?;
    let remaining = suspended(root)?
        .into_iter()
        .filter(|known| known != id)
        .collect::<Vec<_>>();
    meta["suspended"] = json!(remaining);
    let lapses = reviews(root)?
        .iter()
        .filter(|row| {
            text(field(row, "card")) == id && field(row, "lapse") == Some(&Value::Bool(true))
        })
        .count() as u64;
    meta["lapse_base"][id] = json!(lapses);
    write_json(root, meta_path(), &meta)
}

fn retention(rows: &[Value]) -> Value {
    if rows.is_empty() {
        return Value::Null;
    }
    let kept = rows
        .iter()
        .filter(|row| text(field(row, "rating")) != "again")
        .count() as f64;
    json!(careerbrew_projects::python_round(
        kept / rows.len() as f64,
        3
    ))
}

pub fn stats(root: &Path, public_dir: &Path, at: DateTime<FixedOffset>) -> Result<Value> {
    let cards = all_cards(root, public_dir)?;
    let states = states(root)?;
    let suspended = suspended(root)?;
    let reviews = reviews(root)?;
    let since = stamp_at(at - Duration::days(30));
    let recent = reviews
        .iter()
        .filter(|row| {
            text(field(row, "at")) >= since && cards.contains_key(&text(field(row, "card")))
        })
        .cloned()
        .collect::<Vec<_>>();
    let week_since = stamp_at(at - Duration::days(7));
    let week = recent
        .iter()
        .filter(|row| {
            text(field(row, "at")) >= week_since && field(row, "first") != Some(&Value::Bool(true))
        })
        .cloned()
        .collect::<Vec<_>>();
    let today = day(root, at);
    let zone = local_zone(root);
    let local = at.with_timezone(&zone);
    let end_naive = today
        .and_hms_micro_opt(23, 59, 59, 999_999)
        .expect("valid local day end");
    let end_of_day = zone
        .from_local_datetime(&end_naive)
        .earliest()
        .unwrap_or(local)
        .fixed_offset();
    let mut by_topic = BTreeMap::<String, Value>::new();
    for (id, card) in &cards {
        let topic = text(field(card, "topic"));
        let mut row = by_topic
            .remove(&topic)
            .unwrap_or_else(|| json!({"total": 0, "due": 0, "new": 0}));
        row["total"] = json!(
            field(&row, "total")
                .and_then(Value::as_u64)
                .unwrap_or_default()
                + 1
        );
        row["new"] = json!(
            field(&row, "new")
                .and_then(Value::as_u64)
                .unwrap_or_default()
                + u64::from(!states.contains_key(id))
        );
        row["due"] = json!(
            field(&row, "due")
                .and_then(Value::as_u64)
                .unwrap_or_default()
                + u64::from(
                    states
                        .get(id)
                        .is_some_and(|state| !suspended.contains(id) && state.due <= at)
                )
        );
        by_topic.insert(topic, row);
    }
    for (topic, row) in by_topic.iter_mut() {
        let rows = recent
            .iter()
            .filter(|review| text(field(review, "topic")) == *topic)
            .cloned()
            .collect::<Vec<_>>();
        row["retention"] = retention(&rows);
    }
    let days = reviews
        .iter()
        .filter_map(|row| review_day(root, row))
        .collect::<BTreeSet<_>>();
    let mut streak = 0;
    let mut current = if days.contains(&today) {
        today
    } else {
        today - Duration::days(1)
    };
    while days.contains(&current) {
        streak += 1;
        current -= Duration::days(1);
    }
    let new = cards.keys().filter(|id| !states.contains_key(*id)).count() as u64;
    let due_now = cards
        .keys()
        .filter(|id| {
            !suspended.contains(*id) && states.get(*id).is_some_and(|state| state.due <= at)
        })
        .count() as u64;
    let due_today = cards
        .keys()
        .filter(|id| {
            !suspended.contains(*id) && states.get(*id).is_some_and(|state| state.due <= end_of_day)
        })
        .count() as u64;
    let reviewed_today = reviews
        .iter()
        .filter(|row| review_day(root, row) == Some(today))
        .count() as u64;
    let suspended = suspended
        .into_iter()
        .filter(|id| cards.contains_key(id))
        .collect::<Vec<_>>();
    Ok(json!({
        "total": cards.len(),
        "new": new,
        "due_now": due_now,
        "due_today": due_today,
        "reviewed_today": reviewed_today,
        "retention_30d": retention(&recent),
        "retention_7d": retention(&week),
        "suspended": suspended,
        "streak_days": streak,
        "new_per_day": new_per_day(root)?,
        "by_topic": by_topic,
    }))
}
