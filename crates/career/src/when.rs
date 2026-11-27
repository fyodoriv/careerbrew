//! `datetime.fromisoformat`, `isoformat`, `ZoneInfo` and `strftime` spellings.

use crate::{Error, Result};
use chrono::{
    DateTime, Duration, FixedOffset, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset,
    TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;

pub type At = DateTime<FixedOffset>;

/// `ZoneInfo(name)`.
pub fn zone(name: &str) -> Result<Tz> {
    name.parse::<Tz>()
        .map_err(|_| Error::new("ZoneInfoNotFoundError", "unknown time zone"))
}

/// `calendar.system_tz()` with an injected `TZ` (UTC when unset or unknown).
pub fn system_tz() -> Tz {
    let name = std::env::var("TZ").unwrap_or_default();
    name.trim_start_matches(':')
        .parse::<Tz>()
        .unwrap_or(Tz::UTC)
}

/// `calendar.local_tz()`: the `timezone` setting, else the system zone.
pub fn local_tz() -> Result<Tz> {
    let settings = crate::settings()?;
    match settings.get("timezone") {
        Some(serde_json::Value::String(name)) if !name.is_empty() => zone(name),
        _ => Ok(system_tz()),
    }
}

/// Python `replace(tzinfo=tz)` with fold=0.
pub fn attach(naive: NaiveDateTime, tz: Tz) -> At {
    let offset = match tz.offset_from_local_datetime(&naive) {
        LocalResult::Single(o) => o.fix(),
        LocalResult::Ambiguous(a, b) => {
            let (a, b) = (a.fix(), b.fix());
            if a.local_minus_utc() >= b.local_minus_utc() {
                a
            } else {
                b
            }
        }
        LocalResult::None => {
            let around = |d: Duration| tz.offset_from_utc_datetime(&(naive + d)).fix();
            let (a, b) = (around(Duration::days(-1)), around(Duration::days(1)));
            if a.local_minus_utc() <= b.local_minus_utc() {
                a
            } else {
                b
            }
        }
    };
    offset.from_local_datetime(&naive).unwrap()
}

pub fn fixed(naive: NaiveDateTime, offset: FixedOffset) -> At {
    offset.from_local_datetime(&naive).unwrap()
}

/// `dt.astimezone(tz)`.
pub fn into(at: &At, tz: Tz) -> At {
    at.with_timezone(&tz).fixed_offset()
}

/// `datetime.fromisoformat` for `YYYY-MM-DD`, then optionally `T` or a space,
/// `HH[:MM[:SS]]` with an optional `.f{1,6}` or `,f{1,6}` fraction, and an
/// offset `Z`, `+HH`, `+HH:MM` or `+HHMM`. Other forms Python accepts (week
/// dates, compact forms, offsets with seconds) are refused: a known gap.
pub fn parse(text: &str) -> std::result::Result<(NaiveDateTime, Option<FixedOffset>), String> {
    let bad = || format!("Invalid isoformat string: {text:?}");
    if !text.is_ascii() || text.len() < 10 {
        return Err(bad());
    }
    let day = date(&text[..10]).map_err(|_| bad())?;
    if text.len() == 10 {
        return Ok((day.and_time(NaiveTime::MIN), None));
    }
    if !matches!(&text[10..11], "T" | " ") {
        return Err(bad());
    }
    let rest = &text[11..];
    let split = rest.find(['Z', '+', '-']).unwrap_or(rest.len());
    let (clock, zone) = rest.split_at(split);
    let (hms, fraction) = match clock.split_once(['.', ',']) {
        Some((hms, fraction)) => (hms, Some(fraction)),
        None => (clock, None),
    };
    let parts: Vec<&str> = hms.split(':').collect();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| !two_digits(p)) {
        return Err(bad());
    }
    let field = |i: usize| parts.get(i).map_or(0, |p| p.parse::<u32>().unwrap());
    let micros = match fraction {
        None => 0,
        Some(f) if (1..=6).contains(&f.len()) && f.bytes().all(|b| b.is_ascii_digit()) => {
            format!("{f:0<6}").parse::<u32>().unwrap()
        }
        Some(_) => return Err(bad()),
    };
    let time =
        NaiveTime::from_hms_micro_opt(field(0), field(1), field(2), micros).ok_or_else(bad)?;
    let offset = match zone {
        "" => None,
        "Z" => Some(Utc.fix()),
        _ => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let digits = zone[1..].replace(':', "");
            let digits = if zone.len() == 3 {
                format!("{digits}00")
            } else {
                digits
            };
            let shape = matches!(zone.len(), 3 | 5 | 6)
                && (zone.len() != 6 || &zone[3..4] == ":")
                && digits.len() == 4
                && digits.bytes().all(|b| b.is_ascii_digit());
            if !shape {
                return Err(bad());
            }
            let (h, m): (i32, i32) = (digits[..2].parse().unwrap(), digits[2..].parse().unwrap());
            if h > 23 || m > 59 {
                return Err(bad());
            }
            Some(FixedOffset::east_opt(sign * (h * 3600 + m * 60)).ok_or_else(bad)?)
        }
    };
    Ok((day.and_time(time), offset))
}

fn two_digits(part: &str) -> bool {
    part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit())
}

/// `date.fromisoformat` for `YYYY-MM-DD`.
pub fn date(text: &str) -> std::result::Result<NaiveDate, String> {
    let b = text.as_bytes();
    let shape = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|&i| b[i].is_ascii_digit());
    if !shape {
        return Err("not an ISO date".into());
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| "not a calendar date".into())
}

fn offset_text(offset: &FixedOffset) -> String {
    let seconds = offset.local_minus_utc();
    let sign = if seconds < 0 { '-' } else { '+' };
    let seconds = seconds.abs();
    format!("{sign}{:02}:{:02}", seconds / 3600, (seconds % 3600) / 60)
}

/// `dt.isoformat()`.
pub fn iso(at: &At) -> String {
    let micros = at.nanosecond() / 1000;
    let clock = if micros > 0 {
        at.format("%Y-%m-%dT%H:%M:%S").to_string() + &format!(".{micros:06}")
    } else {
        at.format("%Y-%m-%dT%H:%M:%S").to_string()
    };
    clock + &offset_text(at.offset())
}

/// `dt.isoformat(timespec="seconds")`.
pub fn iso_seconds(at: &At) -> String {
    at.format("%Y-%m-%dT%H:%M:%S").to_string() + &offset_text(at.offset())
}

/// `strftime(format)` for the ASCII directives the reports use.
pub fn strftime(at: &At, format: &str) -> String {
    at.format(format).to_string()
}

/// The typed datetime the oracle prints.
pub fn typed(at: &At) -> serde_json::Value {
    serde_json::json!({ "datetime": iso(at) })
}
