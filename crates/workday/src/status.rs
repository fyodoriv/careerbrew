//! Canonical day status rows and writer receipts.

use crate::{
    emit, field, one_line, read_json, stamp, text, value_error, write_json, write_json_if_changed,
    Result,
};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, Utc};
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const STATUSES: &[&str] = &["waiting", "running", "done", "blocked", "failed"];
pub const ERROR_CLASSES: &[&str] = &["env", "auth", "network", "conflict", "tool"];
pub const ENDS: &[&str] = &["done", "blocked", "failed"];
pub const MAX_ATTEMPTS: i64 = 2;
pub const MAX_TEXT: usize = 200;
const HISTORY: &str = "day/work-history.json";

pub fn path_for(root: &Path, day: NaiveDate) -> PathBuf {
    root.join(format!("day/work-{}.json", day.format("%Y-%m-%d")))
}

fn relative(day: NaiveDate) -> String {
    format!("day/work-{}.json", day.format("%Y-%m-%d"))
}

pub(crate) fn data(root: &Path, day: NaiveDate) -> Result<Value> {
    let data = read_json(root, &relative(day), json!({}))?;
    Ok(if data.is_object() { data } else { json!({}) })
}

pub(crate) fn with_day_lock<T>(
    root: &Path,
    day: NaiveDate,
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let path = path_for(root, day);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    careerbrew_vault::lock::with_lock(&path, f)?
}

pub fn blank(step: &str) -> Value {
    json!({
        "step": step,
        "status": "waiting",
        "summary": "",
        "needs_you": "",
        "link": "",
        "at": "",
        "started": "",
        "attempt": 0,
        "retry": false,
        "timed_out": false,
        "verify": "",
        "error_class": "",
        "next": "",
        "ended": "",
        "seconds": null,
    })
}

fn normalized_row(row: &Value) -> Option<Value> {
    let step = field(row, "step").and_then(Value::as_str)?;
    if step.is_empty() {
        return None;
    }
    let mut out = blank(step);
    if let (Some(target), Some(source)) = (out.as_object_mut(), row.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
    Some(out)
}

pub fn stored(root: &Path, day: NaiveDate) -> Result<IndexMap<String, Value>> {
    let data = data(root, day)?;
    let mut out = IndexMap::new();
    for row in data
        .get("rows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(row) = normalized_row(row) {
            out.insert(text(field(&row, "step")), row);
        }
    }
    Ok(out)
}

pub fn handled(root: &Path, day: NaiveDate) -> Result<BTreeMap<String, String>> {
    let data = data(root, day)?;
    let mut out = BTreeMap::new();
    if let Some(items) = data.get("handled").and_then(Value::as_object) {
        for (key, value) in items {
            if let Some(step) = value.as_str() {
                out.insert(key.clone(), step.to_owned());
            }
        }
    }
    Ok(out)
}

pub fn rows(root: &Path, steps: &[String], day: NaiveDate) -> Result<Vec<Value>> {
    let have = stored(root, day)?;
    Ok(steps
        .iter()
        .map(|step| have.get(step).cloned().unwrap_or_else(|| blank(step)))
        .collect())
}

fn limited(field_name: &str, value: &str) -> Result<String> {
    let value = one_line(value);
    if value.chars().count() > MAX_TEXT {
        return Err(value_error(format!(
            "keep --{} short: {} characters, at most {} (put the detail behind --link)",
            field_name.replace('_', "-"),
            value.chars().count(),
            MAX_TEXT
        )));
    }
    Ok(value)
}

fn write_document(
    root: &Path,
    day: NaiveDate,
    rows: &[Value],
    owners: &BTreeMap<String, String>,
) -> Result<()> {
    let mut document = data(root, day)?;
    document["day"] = json!(day.format("%Y-%m-%d").to_string());
    document["rows"] = Value::Array(rows.to_vec());
    document["handled"] = json!(owners);
    write_json(root, &relative(day), &document)
}

fn next_action(step: &str, status: &str, error_class: &str) -> String {
    if status == "blocked" {
        return format!("answer the ask in the end-of-steps questions, then run {step} again");
    }
    if status != "failed" {
        return String::new();
    }
    match error_class {
        "env" => format!("install or fix the tool or setting {step} needs, then run {step} again"),
        "auth" => format!("sign in again or renew the token {step} uses (careerbrew secret set <name>), then run {step} again"),
        "network" => format!("run {step} again when the network or the service is back"),
        "conflict" => format!("resolve the conflict by hand, then run {step} again"),
        _ => format!("read the log of {step}, fix the tool or the step, then run {step} again"),
    }
}

fn parse_datetime(value: &str) -> Option<DateTime<FixedOffset>> {
    if let Ok(value) = DateTime::parse_from_rfc3339(value) {
        return Some(value);
    }
    let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S").ok()?;
    naive.and_local_timezone(*crate::now().offset()).single()
}

fn seconds(started: &str, ended: &str) -> Option<i64> {
    let start = parse_datetime(started)?;
    let end = parse_datetime(ended)?;
    Some((end - start).num_seconds().max(0))
}

fn close(row: &mut Value, previous: &str, stamp: &str) {
    let status = text(field(row, "status"));
    if status == previous {
        return;
    }
    if status == "running" {
        row["ended"] = json!("");
        row["seconds"] = Value::Null;
    } else if ENDS.contains(&status.as_str()) {
        row["ended"] = json!(stamp);
        row["seconds"] =
            seconds(&text(field(row, "started")), stamp).map_or(Value::Null, |value| json!(value));
    }
}

fn unreadable_aside(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let stamp = crate::now()
        .with_timezone(&Utc)
        .format("%Y%m%dT%H%M%S")
        .to_string();
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let base = path.with_file_name(format!("{name}.unreadable-{stamp}"));
    let mut destination = base.clone();
    let mut n = 1;
    while destination.exists() {
        n += 1;
        destination = base.with_file_name(format!(
            "{}-{n}",
            base.file_name().unwrap_or_default().to_string_lossy()
        ));
    }
    std::fs::rename(path, destination)?;
    Ok(())
}

fn record_history(root: &Path, day: NaiveDate, row: &Value) -> Result<()> {
    let status = text(field(row, "status"));
    if !ENDS.contains(&status.as_str()) {
        return Ok(());
    }
    let path = root.join(HISTORY);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    careerbrew_vault::lock::with_lock(&path, || -> Result<()> {
        let mut history = match read_json(root, HISTORY, json!({})) {
            Ok(value) => value,
            Err(_) => {
                unreadable_aside(&path)?;
                json!({})
            }
        };
        if !history.is_object() {
            history = json!({});
        }
        let mut days = history
            .get("days")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        days.retain(|_, value| value.is_object());
        days.entry(day.format("%Y-%m-%d").to_string())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .expect("day history")
            .insert(
                text(field(row, "step")),
                json!({"seconds": row.get("seconds").cloned().unwrap_or(Value::Null), "status": status}),
            );
        let keep = days.keys().cloned().collect::<Vec<_>>();
        for old in keep.iter().take(keep.len().saturating_sub(120)) {
            days.remove(old);
        }
        history["days"] = Value::Object(days);
        write_json_if_changed(root, HISTORY, &history)?;
        Ok(())
    })?
}

#[derive(Clone, Debug, Default)]
pub struct Update {
    pub status: Option<String>,
    pub summary: Option<String>,
    pub needs_you: Option<String>,
    pub link: Option<String>,
    pub handled_items: Vec<String>,
    pub error_class: Option<String>,
    pub verify: Option<String>,
}

pub fn update(
    root: &Path,
    step: &str,
    plan_steps: &[String],
    update: &Update,
    day: NaiveDate,
) -> Result<Value> {
    let step = crate::steps::normalize(step)?;
    if !plan_steps.contains(&step) {
        return Err(value_error(format!(
            "step {step} is not in today's plan (steps: {}; see `careerbrew day run`)",
            plan_steps.join(", ")
        )));
    }
    if let Some(status) = &update.status {
        if !STATUSES.contains(&status.as_str()) {
            return Err(value_error(format!(
                "unknown status {status:?}; use one of: {}",
                STATUSES.join(", ")
            )));
        }
    }
    if let Some(class) = &update.error_class {
        if !ERROR_CLASSES.contains(&class.as_str()) {
            return Err(value_error(format!(
                "unknown error class {class:?}; use one of: {}",
                ERROR_CLASSES.join(", ")
            )));
        }
    }
    if let Some(link) = &update.link {
        if !link.is_empty() && !(link.starts_with("https://") || link.starts_with("http://")) {
            return Err(value_error(
                "--link must be a full http(s):// URL, or empty",
            ));
        }
    }
    for item in &update.handled_items {
        let valid = (item.starts_with("checkout:") || item.starts_with("pr:"))
            && item.split_once(':').is_some_and(|(_, rest)| {
                (1..=200).contains(&rest.chars().count()) && !rest.chars().any(char::is_whitespace)
            });
        if !valid {
            return Err(value_error(format!(
                "--handled {:?}: use checkout:<path> or pr:<repo>#<number> (no spaces)",
                crate::chars(item, 40)
            )));
        }
    }
    let step_for_event = step.clone();
    let mut before = String::new();
    let row = with_day_lock(root, day, || {
        let mut have = stored(root, day)?.into_values().collect::<Vec<_>>();
        let mut owners = handled(root, day)?;
        for item in &update.handled_items {
            if owners.get(item).is_some_and(|owner| owner != &step) {
                return Err(value_error(format!(
                    "{item} was already handled today by {}: skip it (one step owns a checkout or PR per day)",
                    owners[item]
                )));
            }
            owners.insert(item.clone(), step.clone());
        }
        let position = have.iter().position(|row| text(field(row, "step")) == step);
        let mut row = position
            .map(|index| have[index].clone())
            .unwrap_or_else(|| blank(&step));
        let previous = text(field(&row, "status"));
        before = previous.clone();
        for (key, value) in [
            ("status", update.status.clone()),
            ("summary", update.summary.clone()),
            ("needs_you", update.needs_you.clone()),
            ("link", update.link.clone()),
        ] {
            if let Some(value) = value {
                row[key] = if key == "status" || key == "link" {
                    json!(value)
                } else {
                    json!(limited(key, &value)?)
                };
            }
        }
        let at = stamp();
        if update.status.as_deref() == Some("running") && previous != "running" {
            let attempt = if row.get("retry").and_then(Value::as_bool).unwrap_or(false) {
                row.get("attempt").and_then(Value::as_i64).unwrap_or(0) + 1
            } else {
                1
            };
            row["attempt"] = json!(attempt);
            row["started"] = json!(at);
            row["retry"] = json!(false);
            row["timed_out"] = json!(false);
            row["verify"] = json!("");
        }
        close(&mut row, &previous, &at);
        if let Some(class) = &update.error_class {
            if text(field(&row, "status")) != "failed" {
                return Err(value_error("--error-class goes only with --status failed: a failed row says what broke (blocked rows wait on you instead)"));
            }
            row["error_class"] = json!(class);
        }
        if text(field(&row, "status")) != "failed" {
            row["error_class"] = json!("");
        }
        if let Some(verify) = &update.verify {
            row["verify"] = json!(verify);
        }
        if update.status.is_some() || update.needs_you.is_some() || update.error_class.is_some() {
            if text(field(&row, "status")) == "blocked" && text(field(&row, "needs_you")).is_empty()
            {
                return Err(value_error("blocked needs a question record: pass --needs-you \"<the ask>\" (blocked waits on you; when something broke, use --status failed --error-class <class>)"));
            }
            if text(field(&row, "status")) == "failed"
                && text(field(&row, "error_class")).is_empty()
            {
                return Err(value_error(format!(
                    "failed needs an error class: pass --error-class {} (failed means something broke; when the step waits on you, use --status blocked --needs-you)",
                    ERROR_CLASSES.join("|")
                )));
            }
        }
        if !(text(field(&row, "status")) == "waiting"
            && row.get("retry").and_then(Value::as_bool).unwrap_or(false))
        {
            row["next"] = json!(next_action(
                &step,
                &text(field(&row, "status")),
                &text(field(&row, "error_class"))
            ));
        }
        row["at"] = json!(at);
        if let Some(index) = position {
            have[index] = row.clone();
        } else {
            have.push(row.clone());
        }
        record_history(root, day, &row)?;
        write_document(root, day, &have, &owners)?;
        Ok(row)
    })?;
    let changed = before != text(field(&row, "status"));
    let seconds = if before == "running" && ENDS.contains(&text(field(&row, "status")).as_str()) {
        row.get("seconds").cloned().unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    emit(
        root,
        "workday.update",
        json!({
            "status": row["status"],
            "changed": changed,
            "needs_you": !text(field(&row, "needs_you")).is_empty(),
            "error_class": row["error_class"],
            "step_kind": if step_for_event == "/careerbrew" { "builtin" } else { "workflow" },
            "seconds": seconds,
        }),
    )?;
    Ok(row)
}

pub fn timed_out(
    root: &Path,
    step: &str,
    plan_steps: &[String],
    started: &str,
    day: NaiveDate,
) -> Result<Option<Value>> {
    if !plan_steps.contains(&step.to_owned()) {
        return Err(value_error(format!("step {step} is not in today's plan")));
    }
    let row = with_day_lock(root, day, || {
        let mut have = stored(root, day)?.into_values().collect::<Vec<_>>();
        let owners = handled(root, day)?;
        let Some(position) = have.iter().position(|row| text(field(row, "step")) == step) else {
            return Ok(None);
        };
        let mut row = have[position].clone();
        if text(field(&row, "status")) != "running" || text(field(&row, "started")) != started {
            return Ok(None);
        }
        let attempt = row.get("attempt").and_then(Value::as_i64).unwrap_or(1);
        if attempt < MAX_ATTEMPTS {
            row["status"] = json!("waiting");
            row["retry"] = json!(true);
            row["next"] = json!(format!(
                "run {step} again: attempt {} of {MAX_ATTEMPTS}, after a timeout",
                attempt + 1
            ));
        } else {
            row["status"] = json!("failed");
            row["error_class"] = json!("tool");
            row["timed_out"] = json!(true);
            row["retry"] = json!(false);
            row["next"] = json!(next_action(step, "failed", "tool"));
        }
        let at = stamp();
        row["at"] = json!(at);
        close(&mut row, "running", &at);
        have[position] = row.clone();
        record_history(root, day, &row)?;
        write_document(root, day, &have, &owners)?;
        Ok(Some(row))
    })?;
    if let Some(row) = &row {
        emit(
            root,
            "workday.timeout",
            json!({"retry": row["status"] == "waiting", "status": row["status"]}),
        )?;
        emit(
            root,
            "workday.update",
            json!({
                "status": row["status"], "changed": true,
                "needs_you": !text(field(row, "needs_you")).is_empty(),
                "error_class": row.get("error_class").cloned().unwrap_or_else(|| json!("")),
                "step_kind": if step == "/careerbrew" { "builtin" } else { "workflow" },
                "seconds": row.get("seconds").cloned().unwrap_or(Value::Null),
            }),
        )?;
    }
    Ok(row)
}

pub fn start_record(root: &Path, day: NaiveDate) -> Result<Value> {
    let data = data(root, day)?;
    Ok(data
        .get("start")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| json!({})))
}

pub fn save_start(root: &Path, record: &Value, day: NaiveDate) -> Result<()> {
    with_day_lock(root, day, || {
        let mut document = json!({"rows": [], "handled": {}});
        if let (Some(target), Some(source)) =
            (document.as_object_mut(), data(root, day)?.as_object())
        {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        }
        document["day"] = json!(day.format("%Y-%m-%d").to_string());
        document["start"] = record.clone();
        write_json(root, &relative(day), &document)
    })
}

pub fn counts(rows: &[Value]) -> BTreeMap<String, i64> {
    let mut out = STATUSES
        .iter()
        .map(|status| ((*status).to_owned(), 0))
        .collect::<BTreeMap<_, _>>();
    for row in rows {
        let status = text(field(row, "status"));
        if let Some(count) = out.get_mut(&status) {
            *count += 1;
        }
    }
    out.insert(
        "needs_you".into(),
        rows.iter()
            .filter(|row| !text(field(row, "needs_you")).is_empty())
            .count() as i64,
    );
    out
}
