//! The end of the work day by clock (`workday/end.py`): the wrap-up window,
//! its owner move or hold, the phase, the plan's `wrap_up` view, the clock
//! check each pass makes, and the end-of-day step counts.
//!
//! Starting wrap-up writes tomorrow's handoff and the day log; those records
//! belong to rust-17, so this component refuses that transition with
//! `NotPorted` before it writes anything.

use crate::{
    day_data, day_relative, emit, error, field, iso_auto, iso_day, iso_minutes, now, setting_int,
    setting_text, text, value_error, with_day_lock, write_json, Result,
};
use chrono::{DateTime, Duration, FixedOffset, NaiveTime};
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;

pub const COMMAND: &str = "careerbrew day end";
pub const STATUSES: &[&str] = careerbrew_workday::status::STATUSES;

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
    }
}

fn control_of(data: &Value) -> Value {
    field(data, "wrap_control")
        .filter(|v| truthy(Some(v)))
        .cloned()
        .unwrap_or_else(|| json!({}))
}

fn clock_time(value: &str) -> Result<NaiveTime> {
    match crate::parse_iso(&format!("2000-01-01T{value}")) {
        Some(crate::Stamp::Naive(at)) => Ok(at.time()),
        _ => Err(value_error(format!("Invalid isoformat string: {value:?}"))),
    }
}

fn combine(at: DateTime<FixedOffset>, clock: NaiveTime) -> DateTime<FixedOffset> {
    at.date_naive()
        .and_time(clock)
        .and_local_timezone(*at.offset())
        .single()
        .expect("fixed offset")
}

fn window(
    root: &Path,
    at: DateTime<FixedOffset>,
    control: &Value,
) -> Result<(DateTime<FixedOffset>, DateTime<FixedOffset>)> {
    let wrap = setting_int(root, "work_wrap_minutes", 30)?;
    let chosen = text(field(control, "at"));
    if !chosen.is_empty() {
        let wrap_at = combine(at, clock_time(&chosen)?);
        return Ok((wrap_at, wrap_at + Duration::minutes(wrap)));
    }
    let end_at = combine(at, clock_time(&setting_text(root, "work_end", "17:30")?)?);
    Ok((end_at - Duration::minutes(wrap), end_at))
}

/// (wrap-up start, work end) for the day of `at`, in `at`'s time zone.
pub fn times(
    root: &Path,
    at: DateTime<FixedOffset>,
) -> Result<(DateTime<FixedOffset>, DateTime<FixedOffset>)> {
    let data = day_data(root, at.date_naive())?;
    window(root, at, &control_of(&data))
}

/// `work`, `wrap-up`, `held` or `ended` for the day of `at`.
pub fn phase(root: &Path, at: DateTime<FixedOffset>) -> Result<&'static str> {
    let data = day_data(root, at.date_naive())?;
    if truthy(field(&data, "end")) {
        return Ok("ended");
    }
    if truthy(field(&control_of(&data), "hold")) {
        return Ok("held");
    }
    Ok(
        if truthy(field(&data, "wrapup")) || at >= times(root, at)?.0 {
            "wrap-up"
        } else {
            "work"
        },
    )
}

/// The plan's `wrap_up`: when wrap-up starts, the phase now and the end-of-day command.
pub fn view(root: &Path, at: DateTime<FixedOffset>) -> Result<Value> {
    let (wrap_at, end_at) = times(root, at)?;
    let data = day_data(root, at.date_naive())?;
    Ok(json!({
        "at": iso_minutes(&wrap_at), "phase": phase(root, at)?, "command": COMMAND,
        "hold": truthy(field(&control_of(&data), "hold")), "end_at": iso_minutes(&end_at),
    }))
}

/// Move or hold only today's wrap-up. An explicit owner `day end` overrides it.
pub fn control(root: &Path, at_text: &str, hold: bool) -> Result<Value> {
    if !at_text.is_empty() {
        let pattern = Regex::new(r"^(?:[01][0-9]|2[0-3]):[0-5][0-9]$").expect("hh:mm");
        if !pattern.is_match(at_text) {
            return Err(value_error(
                "--at must be a local time HH:MM, from 00:00 to 23:59",
            ));
        }
        let chosen = clock_time(at_text)?;
        use chrono::Timelike;
        let wrap = setting_int(root, "work_wrap_minutes", 30)?;
        if i64::from(chosen.hour() * 60 + chosen.minute()) + wrap >= 24 * 60 {
            return Err(value_error("wrap-up and clock ending belong to today's record: choose a same-day wrap time or hold, then use explicit careerbrew day end"));
        }
    }
    if !at_text.is_empty() || hold {
        let day = now().date_naive();
        with_day_lock(root, day, || {
            let mut data = day_data(root, day)?;
            if truthy(field(&data, "end")) {
                return Err(value_error(
                    "the day already ended; start the next work day instead",
                ));
            }
            let prior = control_of(&data);
            let chosen = if at_text.is_empty() {
                text(field(&prior, "at"))
            } else {
                at_text.to_owned()
            };
            let mut document = json!({"rows": [], "handled": {}});
            let object = data.as_object_mut().expect("day data");
            object.insert(
                "wrap_control".into(),
                json!({"at": chosen, "hold": hold, "changed_at": iso_minutes(&now())}),
            );
            object.remove("wrapup");
            for (key, value) in object.iter() {
                document[key] = value.clone();
            }
            document["day"] = json!(iso_day(day));
            write_json(root, &day_relative(day), &document)
        })?;
        emit(
            root,
            "workday.end",
            json!({"status": if hold { "held" } else { "moved" }, "action": "control", "by_clock": false}),
        )?;
    }
    view(root, now())
}

/// The clock check each watch pass makes. Before wrap-up, held or ended, it changes nothing.
pub fn tick(root: &Path, at: DateTime<FixedOffset>, auto_end: bool) -> Result<Value> {
    let day = at.date_naive();
    let (wrap_at, end_at) = times(root, at)?;
    let mut out = json!({
        "phase": "work", "wrap_at": iso_minutes(&wrap_at), "end_at": iso_minutes(&end_at),
        "started": false, "reminded": "", "ended": false,
    });
    let data = day_data(root, day)?;
    if truthy(field(&data, "end")) {
        out["phase"] = json!("ended");
        return Ok(out);
    }
    if truthy(field(&control_of(&data), "hold")) {
        out["phase"] = json!("held");
        return Ok(out);
    }
    if at < wrap_at {
        return Ok(out);
    }
    // Python's `_put(only_if_missing=True, respect_hold=True, clock_at=at)`, decided under the day lock.
    let would_start = with_day_lock(root, day, || {
        let data = day_data(root, day)?;
        if truthy(field(&control_of(&data), "hold")) {
            return Ok(false);
        }
        let (wrap_at, _) = window(root, at, &control_of(&data))?;
        Ok(at >= wrap_at && !truthy(field(&data, "wrapup")))
    })?;
    let (_, end_at) = times(root, at)?;
    if would_start || (auto_end && at >= end_at) {
        return Err(error(
            "NotPorted",
            "starting wrap-up or ending the day writes rust-17 handoff and day-log records",
        ));
    }
    let (wrap_at, end_at) = times(root, at)?;
    out["phase"] = json!(phase(root, at)?);
    out["wrap_at"] = json!(iso_minutes(&wrap_at));
    out["end_at"] = json!(iso_minutes(&end_at));
    Ok(out)
}

/// How the day's steps ended, as counts for `workday.end`.
pub fn step_outcomes(rows: &[Value]) -> Value {
    let mut out = serde_json::Map::new();
    for state in STATUSES {
        out.insert(
            format!("steps_{state}"),
            json!(rows
                .iter()
                .filter(|r| text(field(r, "status")) == *state)
                .count()),
        );
    }
    out.insert(
        "needs_you".into(),
        json!(rows
            .iter()
            .filter(|r| truthy(field(r, "needs_you")))
            .count()),
    );
    Value::Object(out)
}

/// The window as Python's default `isoformat` text.
pub fn times_text(root: &Path, at: DateTime<FixedOffset>) -> Result<Vec<String>> {
    let (wrap_at, end_at) = times(root, at)?;
    Ok(vec![iso_auto(&wrap_at), iso_auto(&end_at)])
}
