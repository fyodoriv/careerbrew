//! The FSRS vector set: card schedules from the maintained `fsrs` crate
//! (FSRS-6 memory model), checked against `tutor/cards.py`, which reviews
//! with py-fsrs 6.3.2 `Scheduler()`.
//!
//! The crate gives stability, difficulty and the raw interval. The small
//! adapter below adds what py-fsrs keeps outside the memory model: the
//! learning and relearning steps, the card state, the stability floor, the
//! whole-day interval and the due time. Fuzzing is off on both sides: py-fsrs
//! draws it from Python's `random()`, so it cannot be a fixed vector.
use fsrs::{MemoryState, FSRS, FSRS6_DEFAULT_PARAMETERS};
use serde_json::{json, Value};

/// py-fsrs 6.3.2 `Scheduler()` defaults, the ones `cards.review` uses.
const DESIRED_RETENTION: f32 = 0.9;
const LEARNING_STEPS: [i64; 2] = [60, 600];
const RELEARNING_STEPS: [i64; 1] = [600];
const MAXIMUM_INTERVAL: i64 = 36_500;
/// py-fsrs clamps stability at 0.001; the crate's own floor is 0.0001.
const STABILITY_MIN: f32 = 0.001;
const DAY: i64 = 86_400;

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Learning,
    Review,
    Relearning,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Phase::Learning => "learning",
            Phase::Review => "review",
            Phase::Relearning => "relearning",
        }
    }
}

pub fn run(op: &str, input: &Value) -> Result<Value, String> {
    match op {
        "parameters" => parameters(input),
        "schedule" => schedule(input),
        _ => Err("unknown op".into()),
    }
}

fn parameters(input: &Value) -> Result<Value, String> {
    if !input.is_null() {
        return Err("parameters takes no input".into());
    }
    let weights: Vec<String> = FSRS6_DEFAULT_PARAMETERS
        .iter()
        .map(|w| format!("{w:.4}"))
        .collect();
    Ok(json!({
        "parameters": weights,
        "desired_retention": format!("{DESIRED_RETENTION:.2}"),
        "learning_steps_seconds": LEARNING_STEPS,
        "relearning_steps_seconds": RELEARNING_STEPS,
        "maximum_interval_days": MAXIMUM_INTERVAL,
    }))
}

fn rating_of(value: &Value) -> Result<u32, String> {
    match value.as_str() {
        Some("again") => Ok(1),
        Some("hard") => Ok(2),
        Some("good") => Ok(3),
        Some("easy") => Ok(4),
        _ => Err("unknown rating".into()),
    }
}

/// The next memory state from the crate, with py-fsrs's stability floor.
fn remember(
    model: &FSRS,
    memory: Option<MemoryState>,
    days: i64,
    rating: u32,
) -> Result<MemoryState, String> {
    let days = u32::try_from(days.max(0)).map_err(|_| "elapsed days out of range")?;
    let next = model
        .next_states(memory, DESIRED_RETENTION, days)
        .map_err(|_| "no next state")?;
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

/// Whole days, rounded half to even like Python's `round`, from 1 to the
/// maximum interval.
fn interval_days(model: &FSRS, memory: MemoryState) -> i64 {
    let days = f64::from(model.next_interval_for_state(memory, DESIRED_RETENTION));
    (days.round_ties_even() as i64).clamp(1, MAXIMUM_INTERVAL)
}

fn schedule(input: &Value) -> Result<Value, String> {
    let model = FSRS::new(&FSRS6_DEFAULT_PARAMETERS).map_err(|_| "no model")?;
    let reviews = input["reviews"].as_array().ok_or("no reviews")?;
    let mut phase = Phase::Learning;
    let mut step: Option<usize> = Some(0);
    let mut memory: Option<MemoryState> = None;
    let mut last: Option<i64> = None;
    let mut out = Vec::with_capacity(reviews.len());
    for review in reviews {
        let at = review["at_minutes"].as_i64().ok_or("no review time")? * 60;
        let rating = rating_of(&review["rating"])?;
        // py-fsrs: `(review - last_review).days`, a floor.
        let days = last.map_or(0, |l| (at - l).div_euclid(DAY));
        let next = remember(&model, memory, days, rating)?;
        memory = Some(next);
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
        last = Some(at);
        out.push(json!({
            "state": phase.name(),
            "step": step,
            "due_seconds": at + wait,
            "stability": format!("{:.2}", next.stability),
            "difficulty": format!("{:.2}", next.difficulty),
        }));
    }
    Ok(Value::Array(out))
}
