//! Project-scoped FSRS decks. No call reads or writes the interview deck.

use crate::{
    append_jsonl, error, field, now, read_json, read_jsonl, read_yaml, stamp_at, text, write_json,
    write_yaml, Result,
};
use careerbrew_projects::ProjectError;
use chrono::{DateTime, Duration, FixedOffset};
use fsrs::{MemoryState, FSRS, FSRS6_DEFAULT_PARAMETERS};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const DESIRED_RETENTION: f32 = 0.9;
const LEARNING_STEPS: [i64; 2] = [60, 600];
const RELEARNING_STEPS: [i64; 1] = [600];
const MAXIMUM_INTERVAL: i64 = 36_500;
const STABILITY_MIN: f32 = 0.001;
const DAY: i64 = 86_400;
pub const LEECH_LAPSES: u64 = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Learning,
    Review,
    Relearning,
}

impl Phase {
    fn code(self) -> u64 {
        match self {
            Phase::Learning => 1,
            Phase::Review => 2,
            Phase::Relearning => 3,
        }
    }

    fn from(value: u64) -> Self {
        match value {
            2 => Phase::Review,
            3 => Phase::Relearning,
            _ => Phase::Learning,
        }
    }
}

#[derive(Clone)]
struct CardState {
    phase: Phase,
    step: Option<usize>,
    memory: Option<MemoryState>,
    due: DateTime<FixedOffset>,
    last: Option<DateTime<FixedOffset>>,
}

fn relative(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)
        .map_err(|_| error("ValueError", "learn card path escaped its data root"))?
        .to_string_lossy()
        .into_owned())
}

fn path(folder: &Path, name: &str) -> PathBuf {
    folder.join("srs").join(name)
}

fn read_deck_at(root: &Path, folder: &Path) -> Result<Vec<Value>> {
    let deck = path(folder, "decks/concepts.yaml");
    let data = read_yaml(root, &relative(root, &deck)?, json!({}))?;
    Ok(field(&data, "cards")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

pub fn all_cards(root: &Path, folder: &Path) -> Result<BTreeMap<String, Value>> {
    let mut out = BTreeMap::new();
    for mut card in read_deck_at(root, folder)? {
        let id = text(field(&card, "id"));
        card["deck"] = json!("concepts");
        out.insert(id, card);
    }
    Ok(out)
}

pub fn topic_name(value: &str) -> String {
    crate::casefold(value)
}

pub fn write_deck(root: &Path, folder: &Path, rows: &[Value]) -> Result<usize> {
    let mut clean = Vec::new();
    for card in rows {
        let question = text(field(card, "question")).trim().to_owned();
        let answer = text(field(card, "answer")).trim().to_owned();
        if question.is_empty() || answer.is_empty() {
            continue;
        }
        let tags = crate::strings(field(card, "tags"));
        let topic = text(field(card, "topic"));
        let topic = if topic.is_empty() {
            tags.first().cloned().unwrap_or_else(|| "general".into())
        } else {
            topic
        };
        let mut row = json!({
            "id": text(field(card, "id")), "question": question, "answer": answer,
            "topic": topic_name(&topic), "tags": tags,
        });
        let source = text(field(card, "source"));
        if !source.is_empty() {
            row["source"] = json!(source);
        }
        clean.push(row);
    }
    let deck = path(folder, "decks/concepts.yaml");
    write_yaml(root, &relative(root, &deck)?, &json!({"cards": clean}))?;
    Ok(read_deck_at(root, folder)?.len())
}

fn states(root: &Path, folder: &Path) -> Result<BTreeMap<String, CardState>> {
    let state_path = path(folder, "card-state.json");
    let raw = read_json(root, &relative(root, &state_path)?, json!({}))?;
    let mut out = BTreeMap::new();
    for (id, value) in raw.as_object().into_iter().flatten() {
        let stability = field(value, "stability").and_then(Value::as_f64);
        let difficulty = field(value, "difficulty").and_then(Value::as_f64);
        let memory = match (stability, difficulty) {
            (Some(stability), Some(difficulty)) => Some(MemoryState {
                stability: stability as f32,
                difficulty: difficulty as f32,
                stability_fast: stability as f32,
            }),
            _ => None,
        };
        let last = field(value, "last_review")
            .and_then(Value::as_str)
            .and_then(|stamp| DateTime::parse_from_rfc3339(stamp).ok());
        let due = field(value, "due")
            .and_then(Value::as_str)
            .and_then(|stamp| DateTime::parse_from_rfc3339(stamp).ok())
            .unwrap_or_else(now);
        out.insert(
            id.clone(),
            CardState {
                phase: Phase::from(field(value, "state").and_then(Value::as_u64).unwrap_or(1)),
                step: field(value, "step")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize),
                memory,
                due,
                last,
            },
        );
    }
    Ok(out)
}

pub fn scheduled(root: &Path, folder: &Path) -> Result<BTreeSet<String>> {
    let state_path = path(folder, "card-state.json");
    let raw = read_json(root, &relative(root, &state_path)?, json!({}))?;
    Ok(raw
        .as_object()
        .into_iter()
        .flat_map(|rows| rows.keys())
        .cloned()
        .collect())
}

fn state_json(state: &CardState) -> Value {
    json!({
        "card_id": 0,
        "state": state.phase.code(),
        "step": state.step,
        "stability": state.memory.map(|memory| memory.stability),
        "difficulty": state.memory.map(|memory| memory.difficulty),
        "due": stamp_at(state.due),
        "last_review": state.last.map(stamp_at),
    })
}

fn write_states(root: &Path, folder: &Path, states: &BTreeMap<String, CardState>) -> Result<()> {
    let raw = states
        .iter()
        .map(|(id, state)| (id.clone(), state_json(state)))
        .collect::<Map<_, _>>();
    let state_path = path(folder, "card-state.json");
    write_json(root, &relative(root, &state_path)?, &Value::Object(raw))
}

fn suspended(root: &Path, folder: &Path) -> Result<BTreeSet<String>> {
    let meta_path = path(folder, "card-meta.json");
    let meta = read_json(root, &relative(root, &meta_path)?, json!({}))?;
    Ok(crate::strings(field(&meta, "suspended"))
        .into_iter()
        .collect())
}

pub fn is_suspended(root: &Path, folder: &Path, id: &str) -> Result<bool> {
    Ok(suspended(root, folder)?.contains(id))
}

pub fn unsuspend(root: &Path, folder: &Path, id: &str) -> Result<()> {
    let meta_path = path(folder, "card-meta.json");
    let mut meta = read_json(root, &relative(root, &meta_path)?, json!({}))?;
    let remaining = suspended(root, folder)?
        .into_iter()
        .filter(|known| known != id)
        .collect::<Vec<_>>();
    meta["suspended"] = json!(remaining);
    let lapses = reviews(root, folder)?
        .iter()
        .filter(|row| {
            text(field(row, "card")) == id && field(row, "lapse") == Some(&Value::Bool(true))
        })
        .count() as u64;
    meta["lapse_base"][id] = json!(lapses);
    write_json(root, &relative(root, &meta_path)?, &meta)
}

fn reviews(root: &Path, folder: &Path) -> Result<Vec<Value>> {
    let review_path = path(folder, "card-reviews.jsonl");
    read_jsonl(root, &relative(root, &review_path)?)
}

fn rating_of(value: &str) -> Result<u32> {
    match value {
        "again" => Ok(1),
        "hard" => Ok(2),
        "good" => Ok(3),
        "easy" => Ok(4),
        _ => Err(ProjectError::value("unknown FSRS rating")),
    }
}

fn remember(
    model: &FSRS,
    memory: Option<MemoryState>,
    days: i64,
    rating: u32,
) -> Result<MemoryState> {
    let days =
        u32::try_from(days.max(0)).map_err(|_| ProjectError::value("elapsed days out of range"))?;
    let next = model
        .next_states(memory, DESIRED_RETENTION, days)
        .map_err(|_| ProjectError::value("no next FSRS state"))?;
    let mut state = match rating {
        1 => next.again.memory,
        2 => next.hard.memory,
        3 => next.good.memory,
        _ => next.easy.memory,
    };
    state.stability = state.stability.max(STABILITY_MIN);
    state.stability_fast = state.stability;
    Ok(state)
}

fn interval_days(model: &FSRS, memory: MemoryState) -> i64 {
    let days = f64::from(model.next_interval_for_state(memory, DESIRED_RETENTION));
    (days.round_ties_even() as i64).clamp(1, MAXIMUM_INTERVAL)
}

fn next_state(
    old: Option<&CardState>,
    rating: u32,
    at: DateTime<FixedOffset>,
) -> Result<CardState> {
    let model =
        FSRS::new(&FSRS6_DEFAULT_PARAMETERS).map_err(|_| ProjectError::value("no FSRS model"))?;
    let mut phase = old.map_or(Phase::Learning, |state| state.phase);
    let mut step = old.map_or(Some(0), |state| state.step);
    let days = old.and_then(|state| state.last).map_or(0, |last| {
        (at.timestamp() - last.timestamp()).div_euclid(DAY)
    });
    let memory = remember(&model, old.and_then(|state| state.memory), days, rating)?;
    let review_wait = interval_days(&model, memory) * DAY;
    let wait = match phase {
        Phase::Learning | Phase::Relearning => {
            let steps: &[i64] = if phase == Phase::Learning {
                &LEARNING_STEPS
            } else {
                &RELEARNING_STEPS
            };
            let current = step.unwrap_or(0);
            let graduates = (current >= steps.len() && rating >= 2)
                || rating == 4
                || (rating == 3 && current + 1 == steps.len());
            if steps.is_empty() || graduates {
                phase = Phase::Review;
                step = None;
                review_wait
            } else {
                match rating {
                    1 => {
                        step = Some(0);
                        steps[0]
                    }
                    2 if current == 0 && steps.len() == 1 => steps[0] * 3 / 2,
                    2 if current == 0 => (steps[0] + steps[1]) / 2,
                    2 => steps[current],
                    _ => {
                        step = Some(current + 1);
                        steps[current + 1]
                    }
                }
            }
        }
        Phase::Review if rating == 1 => {
            phase = Phase::Relearning;
            step = Some(0);
            RELEARNING_STEPS[0]
        }
        Phase::Review => review_wait,
    };
    Ok(CardState {
        phase,
        step,
        memory: Some(memory),
        due: at + Duration::seconds(wait),
        last: Some(at),
    })
}

pub fn review(
    root: &Path,
    folder: &Path,
    id: &str,
    rating: &str,
    at: DateTime<FixedOffset>,
    seconds: Option<i64>,
) -> Result<Value> {
    let cards = all_cards(root, folder)?;
    let card = cards
        .get(id)
        .ok_or_else(|| error("KeyError", format!("unknown card {id}")))?;
    if is_suspended(root, folder, id)? {
        return Err(error(
            "ValueError",
            format!("card {id} is suspended as a leech"),
        ));
    }
    let rating_number = rating_of(rating)?;
    let mut all_states = states(root, folder)?;
    let first = !all_states.contains_key(id);
    let lapse = rating == "again"
        && !first
        && all_states
            .get(id)
            .is_some_and(|state| state.phase == Phase::Review);
    let next = next_state(all_states.get(id), rating_number, at)?;
    all_states.insert(id.to_owned(), next);
    write_states(root, folder, &all_states)?;
    let old_reviews = reviews(root, folder)?;
    let lapses = old_reviews
        .iter()
        .filter(|row| {
            text(field(row, "card")) == id && field(row, "lapse") == Some(&Value::Bool(true))
        })
        .count() as u64
        + u64::from(lapse);
    let meta_path = path(folder, "card-meta.json");
    let meta = read_json(root, &relative(root, &meta_path)?, json!({}))?;
    let base = field(&meta, "lapse_base")
        .and_then(|rows| rows.get(id))
        .and_then(Value::as_u64);
    let threshold = base.map_or(LEECH_LAPSES, |base| base + LEECH_LAPSES / 2);
    let leech = lapse && lapses >= threshold;
    if leech {
        let mut meta = meta;
        let ids = suspended(root, folder)?
            .into_iter()
            .chain([id.to_owned()])
            .collect::<BTreeSet<_>>();
        meta["suspended"] = json!(ids.into_iter().collect::<Vec<_>>());
        write_json(root, &relative(root, &meta_path)?, &meta)?;
    }
    let row = json!({
        "card": id, "topic": text(field(card, "topic")), "rating": rating, "first": first,
        "at": stamp_at(at), "due": stamp_at(all_states[id].due), "seconds": seconds,
        "lapse": lapse, "lapses": lapses, "leech": leech,
    });
    append_jsonl(
        root,
        &relative(root, &path(folder, "card-reviews.jsonl"))?,
        std::slice::from_ref(&row),
    )?;
    Ok(row)
}

fn interleave(rows: &[Value]) -> Vec<Value> {
    let mut by_topic: BTreeMap<String, Vec<Value>> = BTreeMap::new();
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

pub fn review_queue(root: &Path, folder: &Path, at: DateTime<FixedOffset>) -> Result<Vec<Value>> {
    let cards = all_cards(root, folder)?;
    let all_states = states(root, folder)?;
    let suspended = suspended(root, folder)?;
    let mut due = cards
        .iter()
        .filter(|(id, _)| {
            !suspended.contains(*id) && all_states.get(*id).is_some_and(|state| state.due <= at)
        })
        .map(|(id, card)| {
            let mut card = card.clone();
            card["mode"] = json!("review");
            (all_states[id].due, id.clone(), card)
        })
        .collect::<Vec<_>>();
    due.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    Ok(interleave(
        &due.into_iter().map(|(_, _, card)| card).collect::<Vec<_>>(),
    ))
}

pub fn stats(root: &Path, folder: &Path, at: DateTime<FixedOffset>) -> Result<Value> {
    let cards = all_cards(root, folder)?;
    let all_states = states(root, folder)?;
    let suspended = suspended(root, folder)?;
    let due_now = cards
        .keys()
        .filter(|id| {
            !suspended.contains(*id) && all_states.get(*id).is_some_and(|state| state.due <= at)
        })
        .count() as u64;
    let since = stamp_at(at - Duration::days(7));
    let week = reviews(root, folder)?
        .into_iter()
        .filter(|row| {
            cards.contains_key(&text(field(row, "card")))
                && text(field(row, "at")) >= since
                && field(row, "first") != Some(&Value::Bool(true))
        })
        .collect::<Vec<_>>();
    let retention = if week.is_empty() {
        Value::Null
    } else {
        let kept = week
            .iter()
            .filter(|row| text(field(row, "rating")) != "again")
            .count() as f64;
        json!(careerbrew_projects::python_round(
            kept / week.len() as f64,
            3
        ))
    };
    Ok(json!({"due_now": due_now, "retention_7d": retention}))
}
