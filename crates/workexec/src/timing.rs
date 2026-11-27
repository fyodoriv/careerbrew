//! Per-step timing (`workday/timing.py`): the lenient history read, medians,
//! slow flags, the history summary, durations in words and agent labels.
//! The history writer itself is the rust-15 `status` writer.

use crate::{field, iso_day, read_json, round_half_even, setting_int, text, Result};
use chrono::{Duration, NaiveDate};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const ENDS: &[&str] = &["done", "blocked", "failed"];
pub const WINDOW: usize = 10;
pub const MIN_SAMPLES: usize = 3;
const HISTORY: &str = "day/work-history.json";

/// The history, every day a mapping; a file nobody can read reads as empty.
pub fn history(root: &Path) -> Map<String, Value> {
    let data = read_json(root, HISTORY, json!({})).unwrap_or_else(|_| json!({}));
    field(&data, "days")
        .and_then(Value::as_object)
        .map(|days| {
            days.iter()
                .filter(|(_, v)| v.is_object())
                .map(|(d, v)| (d.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn int_of(value: Option<&Value>) -> Option<i64> {
    value
        .filter(|v| v.is_i64() || v.is_u64())
        .and_then(Value::as_i64)
}

/// `int(statistics.median(values))` for nonnegative integers.
pub fn int_median(values: &[i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    Some(if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]).div_euclid(2)
    })
}

/// The median of the step's done runs on up to `WINDOW` days before `before`.
pub fn median(step: &str, before: NaiveDate, days: &Map<String, Value>) -> Option<i64> {
    let before = iso_day(before);
    let mut keys = days.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    let got = keys
        .iter()
        .filter(|d| **d < before)
        .filter_map(|d| days[d].get(step))
        .filter(|e| e.is_object() && text(field(e, "status")) == "done")
        .filter_map(|e| int_of(field(e, "seconds")))
        .collect::<Vec<_>>();
    let got = &got[got.len().saturating_sub(WINDOW)..];
    if got.len() >= MIN_SAMPLES {
        int_median(got)
    } else {
        None
    }
}

/// `{step: {median, slow}}` for the day's rows.
pub fn flags(root: &Path, rows: &[Value], day: NaiveDate) -> Result<Value> {
    let percent = setting_int(root, "work_slow_percent", 200)?;
    let days = history(root);
    let mut out = Map::new();
    for row in rows {
        let step = text(field(row, "step"));
        let mid = median(&step, day, &days);
        let took = int_of(field(row, "seconds"));
        let ended = ENDS.contains(&text(field(row, "status")).as_str());
        let slow =
            matches!((mid, took), (Some(mid), Some(took)) if ended && took * 100 > mid * percent);
        out.insert(step, json!({"median": mid, "slow": slow}));
    }
    Ok(Value::Object(out))
}

/// A duration in words: `45 s`, `18 min`, `1 h 5 min`; empty when there is none.
pub fn shown(seconds: Option<i64>) -> String {
    let Some(seconds) = seconds else {
        return String::new();
    };
    if seconds < 60 {
        return format!("{seconds} s");
    }
    let minutes = round_half_even(seconds, 60);
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{} h {} min", minutes / 60, minutes % 60)
    }
}

/// The last `n_days` days up to `today`: per step and per day totals.
pub fn summary(root: &Path, today: NaiveDate, n_days: i64) -> Value {
    let since = iso_day(today - Duration::days(n_days - 1));
    let until = iso_day(today);
    let days = history(root)
        .into_iter()
        .filter(|(d, _)| since <= *d && *d <= until)
        .collect::<BTreeMap<_, _>>();
    let mut per: BTreeMap<String, (Map<String, Value>, Vec<i64>)> = BTreeMap::new();
    let mut out_days = Vec::new();
    for (d, value) in days.iter().rev() {
        let entries = value
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, e)| e.is_object())
            .collect::<Vec<_>>();
        let took = entries
            .iter()
            .filter_map(|(_, e)| int_of(field(e, "seconds")))
            .sum::<i64>();
        let mut day = json!({"day": d, "steps": entries.len(), "seconds": took});
        for end in ENDS {
            day[*end] = json!(entries
                .iter()
                .filter(|(_, e)| text(field(e, "status")) == *end)
                .count());
        }
        out_days.push(day);
        for (step, e) in entries {
            let (p, times) = per.entry(step.clone()).or_insert_with(|| {
                let mut p = Map::new();
                p.insert("step".into(), json!(step));
                p.insert("runs".into(), json!(0));
                for end in ENDS {
                    p.insert((*end).into(), json!(0));
                }
                p.insert("seconds".into(), json!(0));
                (p, Vec::new())
            });
            let bump = |p: &mut Map<String, Value>, key: &str, by: i64| {
                let have = p.get(key).and_then(Value::as_i64).unwrap_or(0);
                p.insert(key.into(), json!(have + by));
            };
            bump(p, "runs", 1);
            let state = text(field(e, "status"));
            if ENDS.contains(&state.as_str()) {
                bump(p, &state, 1);
            }
            if let Some(seconds) = int_of(field(e, "seconds")) {
                bump(p, "seconds", seconds);
                if state == "done" {
                    times.push(seconds);
                }
            }
        }
    }
    let mut steps = per
        .into_values()
        .map(|(mut p, times)| {
            p.insert("median".into(), json!(int_median(&times)));
            Value::Object(p)
        })
        .collect::<Vec<_>>();
    steps.sort_by(|a, b| {
        let fa = a["failed"].as_i64().unwrap_or(0);
        let fb = b["failed"].as_i64().unwrap_or(0);
        fb.cmp(&fa)
            .then_with(|| text(field(a, "step")).cmp(&text(field(b, "step"))))
    });
    json!({"since": since, "until": until, "days": out_days, "steps": steps})
}

/// `{step: what an agent may see}`: plugin steps never show their own names.
pub fn labels(declared: &[Value], names: &[String]) -> Map<String, Value> {
    let mut out = Map::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for s in declared {
        let step = text(field(s, "step"));
        let origin = text(field(s, "from"));
        if !origin.starts_with("plugin:") {
            out.insert(step.clone(), json!(step));
        } else {
            let n = seen.entry(origin.clone()).or_insert(0);
            *n += 1;
            out.insert(step, json!(format!("{origin} step {n}")));
        }
    }
    let mut n = 0;
    for name in names {
        if !out.contains_key(name) {
            n += 1;
            out.insert(name.clone(), json!(format!("other step {n}")));
        }
    }
    out
}
