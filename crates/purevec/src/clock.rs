//! Set `clock`: careerbrew's pure clock helpers from `calendar.py` and the
//! `timezone` validator `config._tz`, with the zone, the pinned
//! `CAREERBREW_NOW` value and the wall clock all injected by the vector.
//! Nothing here reads the wall clock, the environment or `/etc/localtime`.
//!
//! Python rules kept:
//! - `datetime.fromisoformat` for the forms the vectors use (see [`parse`]);
//! - attaching a zone with `replace` keeps the wall time, and fold=0 takes
//!   the offset before a transition in a gap and in an overlap;
//! - adding a `timedelta` to an aware value is wall-clock arithmetic, so a
//!   day's end is the next local midnight (23 h or 25 h of real time);
//! - `isoformat()`: microseconds only when nonzero, offset as `+HH:MM`.
use chrono::{
    DateTime, Duration, FixedOffset, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset,
    TimeZone, Utc,
};
use chrono_tz::Tz;
use serde_json::{json, Value};

pub fn map(op: &str, input: &Value) -> Result<Value, String> {
    match op {
        "tz-valid" => tz_valid(text(input, "value")?).map(|v| typed("str", &v)),
        "system-tz" => Ok(typed("str", &system_tz(text(input, "tz_env")?)?)),
        "day-bounds" => {
            let (start, end) = day_bounds(date(text(input, "day")?)?, zone(input)?);
            Ok(list(vec![iso(&start), iso(&end)]))
        }
        "day-span" => {
            let (start, end) = day_bounds(date(text(input, "day")?)?, zone(input)?);
            let wall = (end.naive_local() - start.naive_local()).num_seconds();
            let real = (end - start).num_seconds();
            Ok(list(vec![int(wall), int(real)]))
        }
        "aware" => {
            let tz = zone(input)?;
            let value = text(input, "value")?;
            let aware = match text(input, "kind")? {
                "date" => attach(date(value)?.and_time(NaiveTime::MIN), tz),
                "datetime" => match parse(value)? {
                    (naive, Some(offset)) => fixed(naive, offset)?,
                    (naive, None) => attach(naive, tz),
                },
                _ => return Err("unknown kind".into()),
            };
            Ok(iso(&aware))
        }
        "shift" => {
            let (naive, offset) = parse(text(input, "value")?)?;
            let delta = Duration::days(num(input, "days")?)
                + Duration::hours(num(input, "hours")?)
                + Duration::minutes(num(input, "minutes")?)
                + Duration::seconds(num(input, "seconds")?);
            let moved = naive.checked_add_signed(delta).ok_or("out of range")?;
            match (offset, &input["tz"]) {
                (Some(offset), _) => Ok(iso(&fixed(moved, offset)?)),
                (None, Value::Null) => Ok(typed("datetime", &naive_iso(&moved))),
                (None, _) => Ok(iso(&attach(moved, zone(input)?))),
            }
        }
        "now" | "now-utc" => {
            let tz = zone(input)?;
            let now = now(input["pinned"].as_str(), tz, text(input, "clock")?)?;
            if op == "now" {
                Ok(iso(&now))
            } else {
                Ok(iso(&now.with_timezone(&Utc.fix())))
            }
        }
        _ => Err("unknown op".into()),
    }
}

/// `calendar.now()`: a non-empty pinned value is parsed, and a naive one
/// gets the zone with fold=0; otherwise the injected clock in the zone.
fn now(pinned: Option<&str>, tz: Tz, clock: &str) -> Result<DateTime<FixedOffset>, String> {
    match pinned.filter(|p| !p.is_empty()) {
        Some(pinned) => match parse(pinned)? {
            (naive, Some(offset)) => fixed(naive, offset),
            (naive, None) => Ok(attach(naive, tz)),
        },
        None => {
            let (naive, offset) = parse(clock)?;
            let instant = fixed(naive, offset.ok_or("the clock needs an offset")?)?;
            let local = instant.with_timezone(&tz);
            Ok(local.with_timezone(&local.offset().fix()))
        }
    }
}

/// `calendar._day_bounds`: local midnight of `day` and of the next day.
fn day_bounds(day: NaiveDate, tz: Tz) -> (DateTime<FixedOffset>, DateTime<FixedOffset>) {
    let start = day.and_time(NaiveTime::MIN);
    (attach(start, tz), attach(start + Duration::days(1), tz))
}

/// Python `replace(tzinfo=tz)` with fold=0: the wall time is kept; in an
/// overlap the earlier (larger) offset applies, and in a gap the offset
/// before the gap (the smaller of the offsets a day either side).
fn attach(naive: NaiveDateTime, tz: Tz) -> DateTime<FixedOffset> {
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
    // A FixedOffset maps every wall time to exactly one instant.
    offset.from_local_datetime(&naive).unwrap()
}

fn fixed(naive: NaiveDateTime, offset: FixedOffset) -> Result<DateTime<FixedOffset>, String> {
    offset
        .from_local_datetime(&naive)
        .single()
        .ok_or_else(|| "out of range".into())
}

/// `config._tz`: strip; a non-empty value must name an IANA zone.
fn tz_valid(value: &str) -> Result<String, String> {
    let value = value.trim();
    if !value.is_empty() {
        value.parse::<Tz>().map_err(|_| "unknown time zone")?;
    }
    Ok(value.to_owned())
}

/// `calendar.system_tz` with an injected, non-empty `TZ`: a leading `:` is
/// stripped and an unknown zone falls back to `UTC`. An empty name (the
/// `/etc/localtime` lookup) is not ported and is refused: a gap owned by
/// rust-05.
fn system_tz(tz_env: &str) -> Result<String, String> {
    let name = tz_env.trim_start_matches(':');
    if name.is_empty() {
        return Err("empty TZ: /etc/localtime lookup is owned by rust-05".into());
    }
    Ok(match name.parse::<Tz>() {
        Ok(_) => name.to_owned(),
        Err(_) => "UTC".to_owned(),
    })
}

/// `datetime.fromisoformat` for `YYYY-MM-DD`, then optionally `T` or a space,
/// `HH[:MM[:SS]]` with an optional `.f{1,6}` or `,f{1,6}` seconds fraction
/// after any of them, and an offset `Z`, `+HH`, `+HH:MM` or `+HHMM`. Other
/// forms Python accepts (week dates, compact dates and times, offsets with
/// seconds) are refused here: a known gap.
fn parse(text: &str) -> Result<(NaiveDateTime, Option<FixedOffset>), String> {
    let bad = || "not an ISO date and time".to_string();
    if !text.is_ascii() || text.len() < 10 {
        return Err(bad());
    }
    let day = date(&text[..10])?;
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
fn date(text: &str) -> Result<NaiveDate, String> {
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

fn zone(input: &Value) -> Result<Tz, String> {
    text(input, "tz")?
        .parse::<Tz>()
        .map_err(|_| "unknown time zone".into())
}

fn text<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input[key].as_str().ok_or_else(|| format!("no {key}"))
}

fn num(input: &Value, key: &str) -> Result<i64, String> {
    input[key].as_i64().ok_or_else(|| format!("no {key}"))
}

/// Python `isoformat()` of an aware value.
fn iso(value: &DateTime<FixedOffset>) -> Value {
    let seconds = value.offset().local_minus_utc();
    let sign = if seconds < 0 { '-' } else { '+' };
    let (h, rest) = (seconds.abs() / 3600, seconds.abs() % 3600);
    let mut offset = format!("{sign}{h:02}:{:02}", rest / 60);
    if rest % 60 != 0 {
        offset.push_str(&format!(":{:02}", rest % 60));
    }
    typed("datetime", &(naive_iso(&value.naive_local()) + &offset))
}

/// Python `isoformat()` of a naive value.
fn naive_iso(value: &NaiveDateTime) -> String {
    let base = value.format("%Y-%m-%dT%H:%M:%S").to_string();
    match value.and_utc().timestamp_subsec_micros() {
        0 => base,
        micros => format!("{base}.{micros:06}"),
    }
}

fn typed(kind: &str, value: &str) -> Value {
    json!({"t": kind, "v": value})
}

fn int(value: i64) -> Value {
    typed("int", &value.to_string())
}

fn list(items: Vec<Value>) -> Value {
    json!({"t": "list", "v": items})
}
