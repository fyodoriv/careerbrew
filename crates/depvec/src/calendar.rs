//! Vector set `calendar`: what `calendar.py` does with ICS text and time
//! zones.
//!
//! Ops (each vector names one):
//! - `day`: input `{ics, day, tz}`; the events of one local day, as
//!   `calendar.parse_ics` gives them (RRULE expansion with `until`, `count`
//!   and EXDATE, all-day events, TRANSP and CLASS). Each event is `{start,
//!   end, title, busy, personal, recurring}`, times in `tz` with their
//!   offset, sorted by start, end and title;
//! - `local-day`: input `{at, tz}`; the local date of an instant in `tz`,
//!   the day bucket `calendar.now()` and `_day_bounds` use.
//!
//! The parser is `icalendar`, the expansion `rrule`, the zones `chrono-tz`.
//! This file only reads properties and joins the three. Not covered yet:
//! RECURRENCE-ID overrides, DURATION, and floating (zone-less) times.
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use icalendar::parser::{read_calendar, unfold, Component};
use rrule::{RRule, Tz, Unvalidated};
use serde_json::{json, Value};

pub fn run(op: &str, input: &Value) -> Result<Value, String> {
    match op {
        "day" => day(
            field(input, "ics")?,
            field(input, "day")?,
            field(input, "tz")?,
        ),
        "local-day" => {
            let tz = zone(field(input, "tz")?)?;
            let at =
                DateTime::parse_from_rfc3339(field(input, "at")?).map_err(|_| "bad instant")?;
            Ok(Value::String(
                at.with_timezone(&tz)
                    .date_naive()
                    .format("%Y-%m-%d")
                    .to_string(),
            ))
        }
        _ => Err("unknown op".into()),
    }
}

fn field<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input[key]
        .as_str()
        .ok_or_else(|| format!("input has no {key}"))
}

fn zone(name: &str) -> Result<Tz, String> {
    name.parse::<chrono_tz::Tz>()
        .map(Tz::Tz)
        .map_err(|_| "unknown time zone".to_owned())
}

struct Event {
    start: DateTime<Tz>,
    end: DateTime<Tz>,
    title: String,
    busy: bool,
    personal: bool,
    recurring: bool,
}

fn day(ics: &str, day: &str, tz: &str) -> Result<Value, String> {
    let tz = zone(tz)?;
    let day = NaiveDate::parse_from_str(day, "%Y-%m-%d").map_err(|_| "bad day")?;
    let from = at_midnight(day, tz)?;
    let to = at_midnight(day + Duration::days(1), tz)?;
    let unfolded = unfold(ics);
    let cal = read_calendar(&unfolded).map_err(|_| "bad calendar")?;
    let vevents: Vec<&Component> = cal
        .components
        .iter()
        .filter(|c| c.name.as_str() == "VEVENT")
        .collect();
    let mut out = Vec::new();
    for ev in vevents {
        out.extend(occurrences(ev, tz, from, to)?);
    }
    out.sort_by(|a, b| (a.start, a.end, &a.title).cmp(&(b.start, b.end, &b.title)));
    Ok(Value::Array(
        out.iter()
            .map(|e| {
                json!({
                    "start": stamp(e.start, tz),
                    "end": stamp(e.end, tz),
                    "title": e.title,
                    "busy": e.busy,
                    "personal": e.personal,
                    "recurring": e.recurring,
                })
            })
            .collect(),
    ))
}

fn stamp(at: DateTime<Tz>, tz: Tz) -> String {
    at.with_timezone(&tz)
        .format("%Y-%m-%dT%H:%M:%S%:z")
        .to_string()
}

fn at_midnight(day: NaiveDate, tz: Tz) -> Result<DateTime<Tz>, String> {
    tz.from_local_datetime(&day.and_time(NaiveTime::MIN))
        .earliest()
        .ok_or_else(|| "no local midnight".to_owned())
}

fn prop<'a>(ev: &'a Component, name: &str) -> Option<&'a icalendar::parser::Property<'a>> {
    ev.properties.iter().find(|p| p.name.as_str() == name)
}

fn text(ev: &Component, name: &str) -> Option<String> {
    prop(ev, name).map(|p| p.val.as_str().to_owned())
}

/// A DTSTART, DTEND or EXDATE value: `(time, all_day)`. A date is midnight
/// in the day's zone, a `Z` time is UTC, and a TZID time is in its zone.
fn when(p: &icalendar::parser::Property, tz: Tz) -> Result<Vec<(DateTime<Tz>, bool)>, String> {
    let tzid = p
        .params
        .iter()
        .find(|q| q.key.as_str() == "TZID")
        .and_then(|q| q.val.as_ref().map(|v| v.as_str().to_owned()));
    p.val
        .as_str()
        .split(',')
        .map(|raw| {
            if raw.len() == 8 {
                let d = NaiveDate::parse_from_str(raw, "%Y%m%d").map_err(|_| "bad date")?;
                return Ok((at_midnight(d, tz)?, true));
            }
            if let Some(utc) = raw.strip_suffix('Z') {
                let n =
                    NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").map_err(|_| "bad time")?;
                return Ok((Tz::UTC.from_utc_datetime(&n), false));
            }
            let n = NaiveDateTime::parse_from_str(raw, "%Y%m%dT%H%M%S").map_err(|_| "bad time")?;
            let zone_of = match &tzid {
                Some(name) => zone(name)?,
                None => return Err("floating time".to_owned()),
            };
            zone_of
                .from_local_datetime(&n)
                .earliest()
                .map(|t| (t, false))
                .ok_or_else(|| "no such local time".to_owned())
        })
        .collect()
}

fn occurrences(
    ev: &Component,
    tz: Tz,
    from: DateTime<Tz>,
    to: DateTime<Tz>,
) -> Result<Vec<Event>, String> {
    let start_prop = prop(ev, "DTSTART").ok_or("event has no start")?;
    let (start, all_day) = *when(start_prop, tz)?.first().ok_or("event has no start")?;
    let end = match prop(ev, "DTEND") {
        Some(p) => when(p, tz)?.first().ok_or("bad end")?.0,
        None if all_day => start + Duration::days(1),
        None => start,
    };
    let length = end - start;
    let rule = text(ev, "RRULE");
    let recurring = rule.is_some() || prop(ev, "RECURRENCE-ID").is_some();
    let starts = match &rule {
        Some(rule) => {
            let mut set = rule
                .parse::<RRule<Unvalidated>>()
                .map_err(|_| "bad rule")?
                .build(start)
                .map_err(|_| "bad rule")?;
            for p in ev.properties.iter().filter(|p| p.name.as_str() == "EXDATE") {
                for (ex, _) in when(p, tz)? {
                    set = set.exdate(ex);
                }
            }
            // Every start before the day ends; the overlap test is below.
            set.before(to).all(u16::MAX).dates
        }
        None => vec![start],
    };
    let transparent = text(ev, "TRANSP").is_some_and(|t| t.eq_ignore_ascii_case("TRANSPARENT"));
    let personal = text(ev, "CLASS").is_some_and(|c| c.eq_ignore_ascii_case("PRIVATE"));
    let title = text(ev, "SUMMARY").unwrap_or_default();
    Ok(starts
        .into_iter()
        .map(|s| (s, s + length))
        // An event overlaps the day; one with no length counts at its start.
        .filter(|(s, e)| *s < to && (*e > from || (s == e && *s >= from)))
        .map(|(s, e)| Event {
            start: s,
            end: e,
            title: title.clone(),
            busy: !all_day && !transparent,
            personal,
            recurring,
        })
        .collect())
}
