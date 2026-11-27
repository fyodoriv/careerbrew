//! Lossless py-fsrs state codec and the FSRS-6 scheduling adapter.
//!
//! Old values stay as f64 so a read/write cycle never loses precision. The
//! maintained crate computes new memory state as f32, matching the depvec gate.

use crate::{stamp_state, Result, TutorError};
use chrono::{DateTime, Duration, FixedOffset, Utc};
use fsrs::{MemoryState, FSRS, FSRS6_DEFAULT_PARAMETERS};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const DESIRED_RETENTION: f32 = 0.9;
pub const LEARNING_STEPS: [i64; 2] = [60, 600];
pub const RELEARNING_STEPS: [i64; 1] = [600];
pub const MAXIMUM_INTERVAL: i64 = 36_500;
pub const STABILITY_MIN: f32 = 0.001;
pub const DAY: i64 = 86_400;
const RETRIEVABILITY_FACTOR: f64 = 0.9803464944134797;
const RETRIEVABILITY_DECAY: f64 = -0.1542;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Learning,
    Review,
    Relearning,
}

impl Phase {
    pub fn code(self) -> u64 {
        match self {
            Phase::Learning => 1,
            Phase::Review => 2,
            Phase::Relearning => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Phase::Learning => "Learning",
            Phase::Review => "Review",
            Phase::Relearning => "Relearning",
        }
    }

    fn from(value: u64) -> Result<Self> {
        match value {
            1 => Ok(Phase::Learning),
            2 => Ok(Phase::Review),
            3 => Ok(Phase::Relearning),
            _ => Err(TutorError::value("unknown FSRS state")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CardState {
    pub card_id: u64,
    pub phase: Phase,
    pub step: Option<usize>,
    pub stability: Option<f64>,
    pub difficulty: Option<f64>,
    pub due: DateTime<FixedOffset>,
    pub last_review: Option<DateTime<FixedOffset>>,
}

impl CardState {
    pub fn new(card_id: u64, at: DateTime<FixedOffset>) -> Self {
        Self {
            card_id,
            phase: Phase::Learning,
            step: Some(0),
            stability: None,
            difficulty: None,
            due: at,
            last_review: None,
        }
    }

    /// Match py-fsrs: allocate from wall-clock milliseconds and wait 1 ms so
    /// two cards created in one review batch cannot share an id. The supplied
    /// review clock still controls due and last_review.
    pub fn generated(at: DateTime<FixedOffset>) -> Self {
        let id = Utc::now().timestamp_millis() as u64;
        std::thread::sleep(std::time::Duration::from_millis(1));
        Self::new(id, at)
    }

    pub fn from_json(value: &Value) -> Result<Self> {
        let card_id = value
            .get("card_id")
            .and_then(Value::as_u64)
            .ok_or_else(|| TutorError::value("FSRS state lacks card_id"))?;
        let phase = Phase::from(
            value
                .get("state")
                .and_then(Value::as_u64)
                .ok_or_else(|| TutorError::value("FSRS state lacks state"))?,
        )?;
        let step = match value.get("step") {
            Some(Value::Null) => None,
            Some(Value::Number(number)) => Some(
                number
                    .as_u64()
                    .map(|step| step as usize)
                    .ok_or_else(|| TutorError::value("FSRS state has invalid step"))?,
            ),
            _ => return Err(TutorError::value("FSRS state has invalid step")),
        };
        let number = |key: &str| -> Result<Option<f64>> {
            let raw = value
                .get(key)
                .ok_or_else(|| TutorError::value(format!("FSRS state lacks {key}")))?;
            let falsy = match raw {
                Value::Null => true,
                Value::Bool(value) => !value,
                Value::Number(number) => number.as_f64() == Some(0.0),
                Value::String(text) => text.is_empty(),
                _ => false,
            };
            if falsy {
                return Ok(None);
            }
            let parsed = match raw {
                Value::Number(number) => number.as_f64(),
                Value::String(text) => text.parse::<f64>().ok(),
                Value::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
                _ => None,
            }
            .filter(|number| number.is_finite());
            parsed
                .map(Some)
                .ok_or_else(|| TutorError::value(format!("FSRS state has invalid {key}")))
        };
        let due = value
            .get("due")
            .and_then(Value::as_str)
            .and_then(|stamp| DateTime::parse_from_rfc3339(stamp).ok())
            .ok_or_else(|| TutorError::value("FSRS state lacks a valid due time"))?;
        let last_review = match value.get("last_review") {
            Some(Value::Null) => None,
            Some(Value::String(stamp)) => Some(
                DateTime::parse_from_rfc3339(stamp)
                    .map_err(|_| TutorError::value("FSRS state has invalid last_review"))?,
            ),
            _ => return Err(TutorError::value("FSRS state has invalid last_review")),
        };
        Ok(Self {
            card_id,
            phase,
            step,
            stability: number("stability")?,
            difficulty: number("difficulty")?,
            due,
            last_review,
        })
    }

    pub fn to_json(&self) -> Value {
        json!({
            "card_id": self.card_id,
            "state": self.phase.code(),
            "step": self.step,
            "stability": self.stability,
            "difficulty": self.difficulty,
            "due": stamp_state(self.due),
            "last_review": self.last_review.map(stamp_state),
        })
    }

    pub fn memory(&self) -> Option<MemoryState> {
        match (self.stability, self.difficulty) {
            (Some(stability), Some(difficulty)) => Some(MemoryState {
                stability: stability as f32,
                difficulty: difficulty as f32,
                stability_fast: stability as f32,
            }),
            _ => None,
        }
    }
}

pub fn states_json(states: &BTreeMap<String, CardState>) -> Value {
    Value::Object(
        states
            .iter()
            .map(|(id, state)| (id.clone(), state.to_json()))
            .collect(),
    )
}

pub fn rating_of(value: &str) -> Result<u32> {
    match value {
        "again" => Ok(1),
        "hard" => Ok(2),
        "good" => Ok(3),
        "easy" => Ok(4),
        _ => Err(TutorError::value(format!(
            "rating must be one of ['again', 'hard', 'good', 'easy'], not {value:?}"
        ))),
    }
}

pub fn rating_name(rating: u32) -> &'static str {
    match rating {
        1 => "again",
        2 => "hard",
        3 => "good",
        _ => "easy",
    }
}

pub fn score_to_rating(score: f64) -> u32 {
    if score < 0.5 {
        1
    } else if score < 0.7 {
        2
    } else if score < 0.9 {
        3
    } else {
        4
    }
}

fn remember(
    model: &FSRS,
    memory: Option<MemoryState>,
    days: i64,
    rating: u32,
) -> Result<MemoryState> {
    let days =
        u32::try_from(days.max(0)).map_err(|_| TutorError::value("elapsed days out of range"))?;
    let next = model
        .next_states(memory, DESIRED_RETENTION, days)
        .map_err(|_| TutorError::value("no next FSRS state"))?;
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

pub fn next_state(
    old: Option<&CardState>,
    rating: u32,
    at: DateTime<FixedOffset>,
) -> Result<CardState> {
    let model =
        FSRS::new(&FSRS6_DEFAULT_PARAMETERS).map_err(|_| TutorError::value("no FSRS model"))?;
    let mut phase = old.map_or(Phase::Learning, |state| state.phase);
    let mut step = old.map_or(Some(0), |state| state.step);
    let days = old.and_then(|state| state.last_review).map_or(0, |last| {
        at.signed_duration_since(last).num_seconds().div_euclid(DAY)
    });
    let next = remember(&model, old.and_then(CardState::memory), days, rating)?;
    let review_wait = interval_days(&model, next) * DAY;
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
        card_id: old.map_or(0, |state| state.card_id),
        phase,
        step,
        stability: Some(f64::from(next.stability)),
        difficulty: Some(f64::from(next.difficulty)),
        due: at + Duration::seconds(wait),
        last_review: Some(at),
    })
}

pub fn retrievability(state: &CardState, at: DateTime<FixedOffset>) -> f64 {
    let Some(last) = state.last_review else {
        return 0.0;
    };
    let Some(stability) = state.stability else {
        return 0.0;
    };
    let elapsed = at
        .signed_duration_since(last)
        .num_seconds()
        .div_euclid(DAY)
        .max(0) as f64;
    (1.0 + RETRIEVABILITY_FACTOR * elapsed / stability).powf(RETRIEVABILITY_DECAY)
}
