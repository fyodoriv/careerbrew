//! Read-only ICS and Microsoft Graph calendar facts. Recurrence uses the
//! maintained `icalendar` and `rrule` crates behind this small adapter.

use crate::{field, resolve, text, Result, SourceError};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use icalendar::parser::{read_calendar, unfold, Component, Property};
use rrule::{RRule, Tz, Unvalidated};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub start: DateTime<Tz>,
    pub end: DateTime<Tz>,
    pub title: String,
    pub busy: bool,
    pub personal: bool,
    pub uid: String,
    pub led: bool,
    pub recurring: bool,
}

impl Event {
    pub fn to_json(&self) -> Value {
        json!({
            "start": stamp(self.start),
            "end": stamp(self.end),
            "title": self.title,
            "busy": self.busy,
            "personal": self.personal,
            "uid": self.uid,
            "led": self.led,
            "recurring": self.recurring,
        })
    }
}

fn stamp(at: DateTime<Tz>) -> String {
    at.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

pub fn zone(name: &str) -> Result<Tz> {
    name.parse::<chrono_tz::Tz>()
        .map(Tz::Tz)
        .map_err(|_| SourceError::value("unknown time zone"))
}

fn midnight(day: NaiveDate, tz: Tz) -> Result<DateTime<Tz>> {
    tz.from_local_datetime(&day.and_time(NaiveTime::MIN))
        .earliest()
        .ok_or_else(|| SourceError::value("no local midnight"))
}

fn prop<'a>(event: &'a Component, name: &str) -> Option<&'a Property<'a>> {
    event
        .properties
        .iter()
        .find(|property| property.name.as_str() == name)
}

fn property_text(event: &Component, name: &str) -> Option<String> {
    prop(event, name).map(|property| property.val.as_str().to_owned())
}

fn when(property: &Property, tz: Tz) -> Result<Vec<(DateTime<Tz>, bool)>> {
    let tzid = property
        .params
        .iter()
        .find(|param| param.key.as_str() == "TZID")
        .and_then(|param| param.val.as_ref().map(|value| value.as_str().to_owned()));
    property
        .val
        .as_str()
        .split(',')
        .map(|raw| {
            if raw.len() == 8 {
                let date = NaiveDate::parse_from_str(raw, "%Y%m%d")
                    .map_err(|_| SourceError::value("bad calendar date"))?;
                return Ok((midnight(date, tz)?, true));
            }
            if let Some(utc) = raw.strip_suffix('Z') {
                let naive = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S")
                    .map_err(|_| SourceError::value("bad calendar time"))?;
                return Ok((Tz::UTC.from_utc_datetime(&naive), false));
            }
            let naive = NaiveDateTime::parse_from_str(raw, "%Y%m%dT%H%M%S")
                .map_err(|_| SourceError::value("bad calendar time"))?;
            let zone = match tzid.as_deref() {
                Some(name) => zone(name)?,
                None => return Err(SourceError::value("floating calendar time")),
            };
            zone.from_local_datetime(&naive)
                .earliest()
                .map(|at| (at, false))
                .ok_or_else(|| SourceError::value("no such local calendar time"))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn occurrence(
    start: DateTime<Tz>,
    end: DateTime<Tz>,
    title: &str,
    busy: bool,
    personal: bool,
    led: bool,
    recurring: bool,
) -> Event {
    Event {
        start,
        end,
        title: title.to_owned(),
        busy,
        personal,
        uid: String::new(),
        led,
        recurring,
    }
}

fn occurrences(
    event: &Component,
    tz: Tz,
    from: DateTime<Tz>,
    to: DateTime<Tz>,
    me: &[String],
    exceptions: &BTreeMap<DateTime<Tz>, &Component>,
) -> Result<Vec<Event>> {
    let start_property =
        prop(event, "DTSTART").ok_or_else(|| SourceError::value("event has no start"))?;
    let (start, all_day) = *when(start_property, tz)?
        .first()
        .ok_or_else(|| SourceError::value("event has no start"))?;
    let end = match prop(event, "DTEND") {
        Some(property) => {
            when(property, tz)?
                .first()
                .ok_or_else(|| SourceError::value("bad event end"))?
                .0
        }
        None if all_day => start + Duration::days(1),
        None => start,
    };
    let length = end - start;
    let rule = property_text(event, "RRULE");
    let recurring = rule.is_some() || prop(event, "RECURRENCE-ID").is_some();
    let starts = match &rule {
        Some(rule) => {
            let mut set = rule
                .parse::<RRule<Unvalidated>>()
                .map_err(|_| SourceError::value("bad recurrence rule"))?
                .build(start)
                .map_err(|_| SourceError::value("bad recurrence rule"))?;
            for property in event
                .properties
                .iter()
                .filter(|property| property.name.as_str() == "EXDATE")
            {
                for (excluded, _) in when(property, tz)? {
                    set = set.exdate(excluded);
                }
            }
            set.before(to).all(u16::MAX).dates
        }
        None => vec![start],
    };
    let transparent = property_text(event, "TRANSP")
        .is_some_and(|value| value.eq_ignore_ascii_case("TRANSPARENT"));
    let personal =
        property_text(event, "CLASS").is_some_and(|value| value.eq_ignore_ascii_case("PRIVATE"));
    let organizer = property_text(event, "ORGANIZER")
        .unwrap_or_default()
        .to_lowercase();
    let organizer = organizer.strip_prefix("mailto:").unwrap_or(&organizer);
    let led = !me.is_empty() && me.iter().any(|address| address == organizer);
    let title = property_text(event, "SUMMARY").unwrap_or_default();
    Ok(starts
        .into_iter()
        // RECURRENCE-ID replaces this exact instant, including across a DST
        // transition. The exception component renders itself once separately.
        .filter(|start| !exceptions.contains_key(start))
        .map(|start| (start, start + length))
        .filter(|(start, end)| *start < to && (*end > from || (start == end && *start >= from)))
        .map(|(start, end)| {
            occurrence(
                start,
                end,
                &title,
                !all_day && !transparent,
                personal,
                led,
                recurring,
            )
        })
        .collect())
}

pub fn ics_text(
    cfg: &Value,
    fixture_root: &std::path::Path,
    allowed: Option<&[String]>,
) -> Result<String> {
    let path = text(field(cfg, "path"));
    if !path.is_empty() {
        return Ok(std::fs::read_to_string(path)?);
    }
    let url = resolve(&text(field(cfg, "url")))?;
    if let Some(path) = url.strip_prefix("file://") {
        return Ok(std::fs::read_to_string(path)?);
    }
    let url = url.replacen("webcal://", "https://", 1);
    let bytes = crate::http::get_bytes(fixture_root, &url, allowed)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn parse_ics(source: &str, day: NaiveDate, tz: Tz, me: &[String]) -> Result<Vec<Event>> {
    let from = midnight(day, tz)?;
    let to = midnight(day + Duration::days(1), tz)?;
    let unfolded = unfold(source);
    let calendar = read_calendar(&unfolded).map_err(|_| SourceError::value("bad calendar"))?;
    let events = calendar
        .components
        .iter()
        .filter(|component| component.name.as_str() == "VEVENT")
        .collect::<Vec<_>>();
    let mut exceptions = BTreeMap::<String, BTreeMap<DateTime<Tz>, &Component>>::new();
    for event in events
        .iter()
        .copied()
        .filter(|event| prop(event, "RECURRENCE-ID").is_some())
    {
        let uid = property_text(event, "UID").unwrap_or_default();
        let original = prop(event, "RECURRENCE-ID").expect("filtered recurrence id");
        for (instant, _) in when(original, tz)? {
            exceptions
                .entry(uid.clone())
                .or_default()
                .insert(instant, event);
        }
    }
    let empty = BTreeMap::new();
    let mut out = Vec::new();
    for event in events {
        let uid = property_text(event, "UID").unwrap_or_default();
        let replacements = if prop(event, "RECURRENCE-ID").is_some() {
            &empty
        } else {
            exceptions.get(&uid).unwrap_or(&empty)
        };
        out.extend(occurrences(event, tz, from, to, me, replacements)?);
    }
    for event in &mut out {
        event.start = event.start.with_timezone(&tz);
        event.end = event.end.with_timezone(&tz);
    }
    Ok(out)
}

fn graph_time(value: &Value, tz: Tz) -> Result<DateTime<Tz>> {
    let raw = text(field(value, "dateTime"));
    let naive = NaiveDateTime::parse_from_str(raw.trim_end_matches('Z'), "%Y-%m-%dT%H:%M:%S%.f")
        .map_err(|_| SourceError::value("bad Graph calendar time"))?;
    Ok(Utc.from_utc_datetime(&naive).with_timezone(&tz))
}

pub fn graph_events(
    cfg: &Value,
    day: NaiveDate,
    tz: Tz,
    fetch: crate::connectors::Fetch,
) -> Result<Vec<Event>> {
    let _token = resolve(&text(field(cfg, "token")))?;
    let start = midnight(day, tz)?
        .with_timezone(&Utc)
        .format("%Y-%m-%dT%H:%M:%SZ");
    let end = midnight(day + Duration::days(1), tz)?
        .with_timezone(&Utc)
        .format("%Y-%m-%dT%H:%M:%SZ");
    let api = if text(field(cfg, "api")).is_empty() {
        crate::connectors::GRAPH_API.to_owned()
    } else {
        text(field(cfg, "api")).trim_end_matches('/').to_owned()
    };
    let data = fetch(&format!("{api}/me/calendarView?startDateTime={start}&endDateTime={end}&$select=id,subject,start,end,showAs,isAllDay,sensitivity,isOrganizer,type&$top=100"))?;
    let mut out = Vec::new();
    for event in field(&data, "value")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        let show = text(field(event, "showAs"));
        let busy = !matches!(show.as_str(), "free" | "workingElsewhere")
            && field(event, "isAllDay") != Some(&Value::Bool(true));
        let id = text(field(event, "id"));
        out.push(Event {
            start: graph_time(object_field(event, "start"), tz)?,
            end: graph_time(object_field(event, "end"), tz)?,
            title: text(field(event, "subject")),
            busy,
            personal: matches!(
                text(field(event, "sensitivity")).as_str(),
                "personal" | "private"
            ),
            uid: if id.is_empty() {
                String::new()
            } else {
                format!("outlook:{id}")
            },
            led: field(event, "isOrganizer") == Some(&Value::Bool(true)),
            recurring: matches!(
                text(field(event, "type")).as_str(),
                "occurrence" | "exception"
            ),
        });
    }
    Ok(out)
}

fn object_field<'a>(value: &'a Value, key: &str) -> &'a Value {
    field(value, key).unwrap_or(&Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recurrence_exdate_and_private_organizer_survive() {
        let source = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:s\r\nDTSTART;TZID=America/New_York:20261019T090000\r\nRRULE:FREQ=DAILY;COUNT=3\r\nEXDATE;TZID=America/New_York:20261020T090000\r\nSUMMARY:Review\r\nCLASS:PRIVATE\r\nORGANIZER:mailto:me@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let day = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        let rows = parse_ics(
            source,
            day,
            zone("America/New_York").unwrap(),
            &["me@example.com".into()],
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].personal && rows[0].led && rows[0].recurring);
        assert_eq!(rows[0].start, rows[0].end);
    }

    #[test]
    fn recurrence_exceptions_replace_master_instants_once_across_dst() {
        let source = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:s\r\nDTSTART;TZID=America/New_York:20261030T090000\r\nDTEND;TZID=America/New_York:20261030T100000\r\nRRULE:FREQ=DAILY;COUNT=4\r\nSUMMARY:Original\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:s\r\nRECURRENCE-ID;TZID=America/New_York:20261101T090000\r\nDTSTART;TZID=America/New_York:20261101T103000\r\nDTEND;TZID=America/New_York:20261101T113000\r\nSUMMARY:Moved\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:s\r\nRECURRENCE-ID;TZID=America/New_York:20261102T090000\r\nDTSTART;TZID=America/New_York:20261102T090000\r\nSUMMARY:Cancelled\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let moved = parse_ics(
            source,
            NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            zone("America/New_York").unwrap(),
            &[],
        )
        .unwrap();
        let cancelled = parse_ics(
            source,
            NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
            zone("America/New_York").unwrap(),
            &[],
        )
        .unwrap();
        assert_eq!(
            moved
                .iter()
                .map(|event| event.title.as_str())
                .collect::<Vec<_>>(),
            ["Moved"]
        );
        assert_eq!(
            moved[0].start.format("%Y-%m-%dT%H:%M:%S%:z").to_string(),
            "2026-11-01T10:30:00-05:00"
        );
        assert_eq!(
            cancelled
                .iter()
                .map(|event| event.title.as_str())
                .collect::<Vec<_>>(),
            ["Cancelled"]
        );
    }
}
