//! `time_saved`: manual minutes an action replaces, minus careerbrew's own
//! time, per local ISO week. Reads only action ids, steps, seconds and stamps.

use crate::{float, int, number, py_round, truthy, Result};
use careerbrew_vault::canon::Canon;
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDateTime, TimeZone, Utc};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const ESTIMATE_SOURCE: &str = "careerbrew default, no published source";

/// action id, the CLI commands that do it, estimated manual minutes.
pub const ACTIONS: &[(&str, &[&str], i64)] = &[
    ("pm_update", &["day.send"], 15),
    ("standup", &["work.standup"], 10),
    ("day_log", &["day.log"], 10),
    ("day_plan", &["day.plan"], 10),
    ("evidence", &["work.collect"], 20),
    ("promo_doc", &["work.promo"], 120),
    ("job_scan", &["jobs.scan"], 30),
    ("inbox", &["day.accept"], 3),
];
pub const WEB_ACTIONS: &[&str] = &["pm_update", "standup", "inbox"];
pub const WEB_STEPS: &[&str] = &["open", "copy", "send", "accept"];
pub const DONE_STEPS: &[&str] = &["copy", "send", "accept"];
pub const MAX_WEB_SECONDS: f64 = 86400.0;

pub fn command_action() -> Value {
    let mut out = Map::new();
    for (action, commands, _) in ACTIONS {
        for command in *commands {
            out.insert((*command).to_owned(), json!(action));
        }
    }
    Value::Object(out)
}

fn known(action: &str) -> bool {
    ACTIONS.iter().any(|(id, _, _)| *id == action)
}

/// `time_saved.record_web`: the content-free `web.timing` fields, or the
/// refusal text. The row goes through the shared native event cleaner.
pub fn record_web(
    saves: &Value,
    step: &Value,
    seconds: &Value,
) -> std::result::Result<Value, String> {
    let saves_ok = saves.as_str().is_some_and(|s| WEB_ACTIONS.contains(&s));
    let step_ok = step.as_str().is_some_and(|s| WEB_STEPS.contains(&s));
    if !saves_ok || !step_ok {
        return Err(format!(
            "send {{saves: {}, step: {}, seconds}}",
            WEB_ACTIONS.join("|"),
            WEB_STEPS.join("|")
        ));
    }
    let value = match seconds {
        Value::Number(n) => n.as_f64().filter(|x| (0.0..=MAX_WEB_SECONDS).contains(x)),
        _ => None,
    };
    let Some(value) = value else {
        return Err(format!(
            "seconds must be a number from 0 to {}",
            MAX_WEB_SECONDS as i64
        ));
    };
    let fields = Canon::Map(vec![
        (Canon::Str("event".into()), Canon::Str("web.timing".into())),
        (Canon::Str("saves".into()), crate::canon(saves)),
        (Canon::Str("step".into()), crate::canon(step)),
        (
            Canon::Str("seconds".into()),
            Canon::Float(py_round(value, 1)),
        ),
    ]);
    let cleaned =
        careerbrew_vault::events::clean(&fields).map_err(|_| "event fields".to_owned())?;
    Ok(crate::from_canon(&cleaned))
}

/// `time_saved.baseline`: manual minutes and the kind (`measured` or an estimate).
pub fn baseline(settings: &Value, action: &str) -> (f64, String) {
    let measured = settings.get(format!("time_baseline_{action}").as_str());
    if truthy(measured) {
        let minutes = match measured {
            Some(Value::String(text)) => text.trim().parse().unwrap_or(0.0),
            other => other.and_then(number).unwrap_or(0.0),
        };
        return (minutes, "measured".into());
    }
    let minutes = ACTIONS
        .iter()
        .find(|(id, _, _)| *id == action)
        .map_or(0, |(_, _, minutes)| *minutes);
    (minutes as f64, format!("estimate, {ESTIMATE_SOURCE}"))
}

struct Run {
    ts: String,
    saves: String,
    step: Option<String>,
    web: bool,
    minutes: f64,
}

fn runs(text: &str) -> Vec<Run> {
    let mut rows = Vec::new();
    for line in text.lines() {
        if !line.contains("\"cli.run\"") && !line.contains("\"web.timing\"") {
            continue;
        }
        let Ok(Value::Object(row)) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let get = |key: &str| row.get(key).and_then(Value::as_str).map(str::to_owned);
        let event = get("event");
        let saves = get("saves").unwrap_or_default();
        let ts = get("ts").unwrap_or_default();
        if event.as_deref() == Some("cli.run")
            && get("status").as_deref() == Some("ok")
            && known(&saves)
        {
            if let Some(ms) = row.get("ms").and_then(number) {
                rows.push(Run {
                    ts,
                    saves,
                    step: get("step"),
                    web: false,
                    minutes: ms / 60000.0,
                });
            }
        } else if event.as_deref() == Some("web.timing")
            && WEB_ACTIONS.contains(&saves.as_str())
            && get("step").is_some_and(|s| WEB_STEPS.contains(&s.as_str()))
        {
            if let Some(seconds) = row.get("seconds").and_then(number) {
                rows.push(Run {
                    ts,
                    saves,
                    step: get("step"),
                    web: true,
                    minutes: seconds / 60.0,
                });
            }
        }
    }
    rows
}

fn local(ts: &str, tz: &chrono_tz::Tz) -> Result<DateTime<FixedOffset>> {
    let naive = NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%SZ")
        .map_err(|_| format!("time data {ts:?} does not match format"))?;
    Ok(Utc
        .from_utc_datetime(&naive)
        .with_timezone(tz)
        .fixed_offset())
}

pub fn week_of(day: &DateTime<FixedOffset>) -> String {
    let week = day.iso_week();
    format!("{}-W{:02}", week.year(), week.week())
}

#[derive(Default)]
struct Cell {
    runs: i64,
    runs_web: i64,
    runs_cli: i64,
    opened: i64,
    minutes_saved: f64,
    minutes_spent: f64,
}

/// `time_saved.compute`: numbers and ids only.
pub fn compute(log: &Path) -> Result<Value> {
    let settings = crate::settings()?;
    let tz = crate::local_tz()?;
    let text = if log.is_file() {
        std::fs::read_to_string(log).map_err(|_| "cannot read the event log".to_owned())?
    } else {
        String::new()
    };
    let mut weeks: BTreeMap<String, BTreeMap<String, Cell>> = BTreeMap::new();
    let mut starts: BTreeMap<String, String> = BTreeMap::new();
    for row in runs(&text) {
        let local = local(&row.ts, &tz)?;
        let week = week_of(&local);
        starts.entry(week.clone()).or_insert_with(|| {
            let monday = local - Duration::days(i64::from(local.weekday().num_days_from_monday()));
            monday.format("%Y-%m-%d").to_string()
        });
        let cell = weeks
            .entry(week)
            .or_default()
            .entry(row.saves.clone())
            .or_default();
        if row.web && !row.step.as_deref().is_some_and(|s| DONE_STEPS.contains(&s)) {
            cell.opened += 1;
            continue;
        }
        let (minutes, _) = baseline(&settings, &row.saves);
        cell.runs += 1;
        if row.web {
            cell.runs_web += 1;
        } else {
            cell.runs_cli += 1;
        }
        cell.minutes_spent += row.minutes;
        cell.minutes_saved += minutes - row.minutes;
    }
    let mut history = Vec::new();
    let mut saved_total: Option<f64> = None;
    let mut runs_total = 0;
    for (week, cells) in &weeks {
        let mut actions = Map::new();
        let mut week_runs = 0;
        let mut week_saved: Option<f64> = None;
        for (action, cell) in cells {
            if cell.runs == 0 && cell.opened == 0 {
                continue;
            }
            let time = [("web", cell.runs_web), ("cli", cell.runs_cli)]
                .iter()
                .filter(|(_, n)| *n > 0)
                .map(|(v, _)| *v)
                .collect::<Vec<_>>()
                .join("+");
            let saved = py_round(cell.minutes_saved, 1);
            actions.insert(
                action.clone(),
                json!({
                    "runs": int(cell.runs), "runs_web": int(cell.runs_web), "runs_cli": int(cell.runs_cli),
                    "opened": int(cell.opened), "minutes_saved": float(saved),
                    "minutes_spent": float(py_round(cell.minutes_spent, 1)),
                    "time": if time.is_empty() { "none".to_owned() } else { time },
                }),
            );
            week_runs += cell.runs;
            week_saved = Some(week_saved.map_or(saved, |sum| sum + saved));
        }
        let week_saved_value = week_saved.map_or(int(0), |sum| float(py_round(sum, 1)));
        if let Some(sum) = week_saved {
            let rounded = py_round(sum, 1);
            saved_total = Some(saved_total.map_or(rounded, |total| total + rounded));
        }
        runs_total += week_runs;
        history.push(json!({
            "week": week, "start": starts[week], "runs": int(week_runs),
            "minutes_saved": week_saved_value, "actions": actions,
        }));
    }
    let mut baselines = Map::new();
    let mut measured = 0;
    for (action, _, _) in ACTIONS {
        let (minutes, kind) = baseline(&settings, action);
        measured += i64::from(kind == "measured");
        baselines.insert(
            (*action).to_owned(),
            json!({"minutes": float(minutes), "kind": kind}),
        );
    }
    let now = crate::now()?;
    Ok(json!({
        "computed": now.format("%Y-%m-%d").to_string(),
        "this_week": week_of(&now),
        "measured_baselines": int(measured),
        "baselines": baselines,
        "weeks": history,
        "minutes_saved_total": saved_total.map_or(int(0), |sum| float(py_round(sum, 1))),
        "runs_total": int(runs_total),
    }))
}
