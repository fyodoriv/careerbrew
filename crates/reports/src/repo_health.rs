//! `repo_health`: the allowlisted, content-free export of how the careerbrew
//! repo itself is doing, with `report.all_summaries`/`latest` (the run-page
//! summary readers it needs). `validate` is the allowlist; a sensitive term
//! fails it through the shared native `privacy.find_leaks` port.

use crate::{float, int, is_int, number, py_list, py_round, Result};
use chrono::{Duration, NaiveDate, Utc};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const SCHEMA: i64 = 1;
const KEYS: &[&str] = &[
    "daily",
    "generated_at",
    "schema",
    "steps",
    "summary",
    "version",
    "window_days",
];
const SUMMARY_KEYS: &[&str] = &[
    "checkup_at",
    "checkup_blockers",
    "checkup_next",
    "checkup_status",
    "e2e_passed",
    "e2e_total",
    "failed_steps",
    "failure_rate",
    "gate_runs",
    "gate_seconds_avg",
    "open_prs",
    "step_runs",
];
const STEP_FIELDS: &[(&str, &str)] = &[
    ("step", "step"),
    ("runs", "count"),
    ("failed", "count"),
    ("failure_rate", "number"),
    ("seconds_avg", "number"),
];
const DAY_FIELDS: &[(&str, &str)] = &[
    ("day", "day"),
    ("gate_runs", "count"),
    ("step_runs", "count"),
    ("failed_steps", "count"),
];
const STATUSES: &[&str] = &["ok", "blocked", "none"];

#[derive(Debug)]
pub struct PublicError(pub String);

fn fail<T>(message: impl Into<String>) -> std::result::Result<T, PublicError> {
    Err(PublicError(message.into()))
}

fn regex(cell: &'static OnceLock<Regex>, source: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(source).expect("static pattern"))
}

fn gate_step_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(
        &CELL,
        r"(?m)(?:^|;\s*then|&&)\s*step ([a-z][a-z0-9-]{0,30}) ",
    )
}

fn py_fullmatch(pattern: &Regex, value: &str) -> bool {
    pattern.is_match(value)
}

fn day_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(&CELL, r"^\d{4}-\d{2}-\d{2}\z")
}

fn time_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(
        &CELL,
        r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:[+-]\d{2}:\d{2}|Z)?\z",
    )
}

fn summary_id_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(&CELL, r"^[a-z][a-z0-9-]{0,30}-\d{8}-\d{6}(?:-\d{1,9})?\z")
}

fn pr_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(&CELL, r"^pr-\d+\z")
}

/// Python `re.fullmatch`/`match(...$)` allow one final newline for `$`; the
/// export regexes use `fullmatch` for TIME/DAY, so no newline is allowed.
fn py_dollar(pattern: &Regex, value: &str) -> bool {
    pattern.is_match(value)
        || value
            .strip_suffix('\n')
            .is_some_and(|v| pattern.is_match(v))
}

/// The checkout the export reads: `CAREERBREW_REPO_DIR`, else this checkout.
pub fn repo() -> PathBuf {
    std::env::var_os("CAREERBREW_REPO_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

/// The step names `scripts/check.sh` defines.
pub fn gate_steps() -> BTreeSet<String> {
    std::fs::read_to_string(repo().join("scripts/check.sh"))
        .map(|text| {
            gate_step_re()
                .captures_iter(&text)
                .map(|caps| caps[1].to_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn metrics(name: &str) -> PathBuf {
    repo().join(".metrics").join(name)
}

/// A stamp's day in the owner's zone, or "".
fn day_of(ts: &Value, tz: &chrono_tz::Tz) -> String {
    let text = match ts {
        Value::String(text) => text.clone(),
        Value::Null => "None".into(),
        other => other.to_string(),
    };
    match crate::from_iso(&text.replace('Z', "+00:00")) {
        Some((naive, Some(offset))) => {
            let aware = naive - Duration::seconds(i64::from(offset.local_minus_utc()));
            aware.and_utc().with_timezone(tz).date_naive().to_string()
        }
        Some((naive, None)) => naive.date().to_string(),
        None => String::new(),
    }
}

fn rate(failed: i64, runs: i64) -> f64 {
    if runs == 0 {
        0.0
    } else {
        py_round(failed as f64 / runs as f64, 4)
    }
}

fn e2e() -> (i64, i64) {
    let Ok(text) = std::fs::read_to_string(metrics("e2e-results.json")) else {
        return (0, 0);
    };
    let Ok(Value::Object(top)) = serde_json::from_str::<Value>(&text) else {
        return (0, 0);
    };
    match top.get("results") {
        Some(Value::Object(results)) => (
            results
                .values()
                .filter(|v| v.as_str() == Some("ok"))
                .count() as i64,
            results.len() as i64,
        ),
        Some(value) if crate::truthy(Some(value)) => (0, 0),
        _ => (0, 0),
    }
}

/// `report.folder()`: `<data root>/out/reports`, created when missing.
pub fn reports_folder() -> Result<PathBuf> {
    let folder = crate::data_root()?.join("out/reports");
    std::fs::create_dir_all(&folder).map_err(|_| "cannot create the reports folder".to_owned())?;
    Ok(folder)
}

fn seq(value: Option<&Value>) -> i64 {
    match value {
        None => 0,
        Some(Value::String(text)) => text.trim().parse().unwrap_or(0),
        Some(other) => number(other).map_or(0, |x| x.trunc() as i64),
    }
}

/// `report.all_summaries()`: every kept summary, newest first.
pub fn all_summaries() -> Result<Vec<Map<String, Value>>> {
    let folder = reports_folder()?;
    let mut rows = Vec::new();
    for entry in std::fs::read_dir(&folder).map_err(|_| "cannot list reports".to_owned())? {
        let path = entry.map_err(|_| "cannot list reports".to_owned())?.path();
        let (Some(stem), Some("json")) = (
            path.file_stem().and_then(|s| s.to_str()),
            path.extension().and_then(|s| s.to_str()),
        ) else {
            continue;
        };
        if !py_dollar(summary_id_re(), stem) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(Value::Object(mut row)) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        row.insert("id".into(), json!(stem));
        rows.push(row);
    }
    rows.sort_by(|a, b| {
        let key = |row: &Map<String, Value>| {
            let at = match row.get("at") {
                Some(Value::String(text)) => text.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            (seq(row.get("seq")), at)
        };
        key(b).cmp(&key(a))
    });
    Ok(rows)
}

pub fn latest(run: Option<&str>) -> Result<Option<Map<String, Value>>> {
    Ok(all_summaries()?
        .into_iter()
        .find(|row| run.is_none() || row.get("run").and_then(Value::as_str) == run))
}

fn is_time(value: &Value) -> bool {
    let Some(text) = value.as_str() else {
        return false;
    };
    py_fullmatch(time_re(), text) && crate::from_iso(&text.replace('Z', "+00:00")).is_some()
}

fn is_day(value: &Value) -> bool {
    let Some(text) = value.as_str() else {
        return false;
    };
    py_fullmatch(day_re(), text) && NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()
}

fn count(value: &Value) -> bool {
    is_int(value) && value.as_f64().is_some_and(|x| x >= 0.0)
}

fn non_negative(value: &Value) -> bool {
    matches!(value, Value::Number(_)) && value.as_f64().is_some_and(|x| x.is_finite() && x >= 0.0)
}

fn checkup() -> Result<Map<String, Value>> {
    let summary = latest(Some("repo-checkup"))?.unwrap_or_default();
    let at = match summary.get("at") {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    };
    let next = match summary.get("next") {
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    };
    let status = summary
        .get("status")
        .and_then(Value::as_str)
        .filter(|s| STATUSES.contains(s));
    let open_prs = next
        .iter()
        .filter(|item| {
            item.as_object().is_some_and(|item| {
                let id = match item.get("id") {
                    Some(Value::String(text)) => text.clone(),
                    Some(Value::Null) | None => "None".into(),
                    Some(other) => other.to_string(),
                };
                pr_re().is_match(&id)
            })
        })
        .count() as i64;
    let mut out = Map::new();
    out.insert("checkup_status".into(), json!(status.unwrap_or("none")));
    out.insert(
        "checkup_at".into(),
        json!(if is_time(&json!(at)) {
            at
        } else {
            String::new()
        }),
    );
    out.insert(
        "checkup_blockers".into(),
        int(summary
            .get("blockers")
            .and_then(Value::as_array)
            .map_or(0, Vec::len) as i64),
    );
    out.insert("checkup_next".into(), int(next.len() as i64));
    out.insert("open_prs".into(), int(open_prs));
    Ok(out)
}

fn metric_events() -> Vec<Map<String, Value>> {
    let Ok(text) = std::fs::read_to_string(metrics("events.jsonl")) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(row)) => Some(row),
            _ => None,
        })
        .collect()
}

/// `repo_health.collect(days)`.
pub fn collect(days: i64) -> std::result::Result<Value, PublicError> {
    if !(1..=366).contains(&days) {
        return fail("--days must be 1 to 366");
    }
    let wrap = |error: String| PublicError(format!("internal: {error}"));
    let now = crate::now().map_err(wrap)?;
    let tz = crate::local_tz().map_err(wrap)?;
    let start = (now.date_naive() - Duration::days(days - 1)).to_string();
    let mut per_step: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut fails: BTreeMap<String, i64> = BTreeMap::new();
    let mut daily: BTreeMap<String, (i64, i64, i64)> = BTreeMap::new();
    let known = gate_steps();
    // Python sums the per-step totals in first-seen (dict) order.
    let mut first_seen: Vec<String> = Vec::new();
    for event in metric_events() {
        let name = event.get("name").and_then(Value::as_str);
        let day = day_of(event.get("ts").unwrap_or(&Value::Null), &tz);
        let skip = event.get("event").and_then(Value::as_str) != Some("check.step")
            || crate::truthy(event.get("cached"))
            || !name.is_some_and(|n| known.contains(n))
            || day < start;
        if skip {
            continue;
        }
        let name = name.unwrap_or_default().to_owned();
        let seconds = match event.get("seconds") {
            Some(Value::Number(n)) => n.as_f64().filter(|x| x.is_finite()),
            _ => None,
        };
        let Some(seconds) = seconds else {
            continue;
        };
        if !per_step.contains_key(&name) {
            first_seen.push(name.clone());
        }
        per_step.entry(name.clone()).or_default().push(seconds);
        let row = daily.entry(day).or_insert((0, 0, 0));
        row.1 += 1;
        row.0 += i64::from(name == "docs");
        if event.get("ok") == Some(&Value::Bool(false)) {
            *fails.entry(name.clone()).or_insert(0) += 1;
            row.2 += 1;
        }
    }
    let failed_of = |name: &str| fails.get(name).copied().unwrap_or(0);
    let mut ordered = per_step.iter().collect::<Vec<_>>();
    ordered.sort_by(|(a, va), (b, vb)| {
        let ra = rate(failed_of(a), va.len() as i64);
        let rb = rate(failed_of(b), vb.len() as i64);
        rb.partial_cmp(&ra)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(b))
    });
    let steps = ordered
        .iter()
        .map(|(name, values)| {
            let runs = values.len() as i64;
            let total: f64 = values.iter().sum();
            json!({
                "step": name, "runs": int(runs), "failed": int(failed_of(name)),
                "failure_rate": float(rate(failed_of(name), runs)),
                "seconds_avg": float(py_round(total / runs as f64, 1)),
            })
        })
        .collect::<Vec<_>>();
    let gate_runs = per_step.get("docs").map_or(0, Vec::len) as i64;
    let step_runs = per_step.values().map(Vec::len).sum::<usize>() as i64;
    let failed: i64 = fails.values().sum();
    let all_seconds: f64 = first_seen
        .iter()
        .map(|name| per_step[name].iter().sum::<f64>())
        .sum();
    let (e2e_ok, e2e_total) = e2e();
    let mut summary = Map::new();
    summary.insert("gate_runs".into(), int(gate_runs));
    summary.insert("step_runs".into(), int(step_runs));
    summary.insert("failed_steps".into(), int(failed));
    summary.insert("failure_rate".into(), float(rate(failed, step_runs)));
    summary.insert(
        "gate_seconds_avg".into(),
        float(if gate_runs > 0 {
            py_round(all_seconds / gate_runs as f64, 1)
        } else {
            0.0
        }),
    );
    summary.insert("e2e_passed".into(), int(e2e_ok));
    summary.insert("e2e_total".into(), int(e2e_total));
    summary.extend(checkup().map_err(wrap)?);
    let daily = daily
        .iter()
        .map(|(day, (gate, steps, failed))| {
            json!({"day": day, "gate_runs": int(*gate), "step_runs": int(*steps), "failed_steps": int(*failed)})
        })
        .collect::<Vec<_>>();
    let payload = json!({
        "schema": int(SCHEMA),
        "version": crate::version(),
        "generated_at": now.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%S+00:00").to_string(),
        "window_days": int(days),
        "summary": summary,
        "steps": steps,
        "daily": daily,
    });
    validate(&payload)?;
    emit_ok(
        payload["steps"].as_array().map_or(0, Vec::len),
        payload["daily"].as_array().map_or(0, Vec::len),
    );
    Ok(payload)
}

fn kind_label(kind: &str) -> String {
    kind.replace('_', " ")
}

fn check(
    place: &str,
    kind: &str,
    value: &Value,
    steps: &BTreeSet<String>,
) -> std::result::Result<(), PublicError> {
    let ok = match kind {
        "step" => value.as_str().is_some_and(|s| steps.contains(s)),
        // Python `v == 1` also holds for `True` and `1.0`.
        "schema" => number(value) == Some(SCHEMA as f64),
        "version" => value.as_str() == Some(crate::version().as_str()),
        "generated_at" => is_time(value),
        "window_days" => count(value) && value.as_f64().is_some_and(|x| (1.0..=366.0).contains(&x)),
        "status" => value.as_str().is_some_and(|s| STATUSES.contains(&s)),
        "time_or_empty" => value.as_str() == Some("") || is_time(value),
        "day" => is_day(value),
        "count" => count(value),
        "number" => non_negative(value),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        fail(format!(
            "{place} is not an allowed {}; only numbers and allowlisted words may be published",
            kind_label(kind)
        ))
    }
}

fn summary_kind(key: &str) -> &'static str {
    match key {
        "failure_rate" | "gate_seconds_avg" => "number",
        "checkup_status" => "status",
        "checkup_at" => "time_or_empty",
        _ => "count",
    }
}

fn key_set(map: &Map<String, Value>) -> BTreeSet<&str> {
    map.keys().map(String::as_str).collect()
}

fn check_rows(
    name: &str,
    rows: &Value,
    fields: &[(&str, &str)],
    steps: &BTreeSet<String>,
) -> std::result::Result<(), PublicError> {
    let Value::Array(rows) = rows else {
        return fail(format!("{name} must be a list of rows"));
    };
    let mut wanted = fields.iter().map(|(k, _)| *k).collect::<Vec<_>>();
    wanted.sort_unstable();
    for (n, row) in rows.iter().enumerate() {
        let Value::Object(row) = row else {
            return fail(format!(
                "{name}[{n}] keys must be exactly {}",
                py_list(&wanted)
            ));
        };
        if key_set(row) != wanted.iter().copied().collect() {
            return fail(format!(
                "{name}[{n}] keys must be exactly {}",
                py_list(&wanted)
            ));
        }
        for (key, kind) in fields {
            check(&format!("{name}[{n}].{key}"), kind, &row[*key], steps)?;
        }
    }
    Ok(())
}

/// Sensitive terms: `<data root>/sensitive-terms.txt`, read through the
/// native store. rust-followup: profile and plugin terms join this list when
/// `privacy.sensitive_terms` ports with its owner step.
pub fn sensitive_terms() -> Result<Vec<String>> {
    let context = crate::context()?;
    let path = context.root.join("sensitive-terms.txt");
    let text = context
        .store()
        .read_text(&path, None, true)
        .map_err(|_| "cannot read sensitive terms".to_owned())?
        .unwrap_or_default();
    let mut terms = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && line.chars().count() >= 3)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    terms.sort_by_key(|term| std::cmp::Reverse(term.chars().count()));
    Ok(terms)
}

/// `repo_health.validate`: the allowlist.
pub fn validate(payload: &Value) -> std::result::Result<(), PublicError> {
    let keys = || format!("export keys must be exactly {}", py_list(KEYS));
    let Value::Object(top) = payload else {
        return fail(keys());
    };
    if key_set(top) != KEYS.iter().copied().collect() {
        return fail(keys());
    }
    let steps = gate_steps();
    for key in ["schema", "version", "generated_at", "window_days"] {
        check(key, key, &top[key], &steps)?;
    }
    let summary_keys = || format!("summary keys must be exactly {}", py_list(SUMMARY_KEYS));
    let Value::Object(summary) = &top["summary"] else {
        return fail(summary_keys());
    };
    if key_set(summary) != SUMMARY_KEYS.iter().copied().collect() {
        return fail(summary_keys());
    }
    for (key, value) in summary {
        check(&format!("summary.{key}"), summary_kind(key), value, &steps)?;
    }
    check_rows("steps", &top["steps"], STEP_FIELDS, &steps)?;
    check_rows("daily", &top["daily"], DAY_FIELDS, &steps)?;
    let terms = sensitive_terms().map_err(PublicError)?;
    let text = serde_json::to_string(payload).unwrap_or_default();
    if !careerbrew_policy::find_leaks(&text, &terms).is_empty() {
        return fail("the export holds one of your sensitive terms; nothing was written");
    }
    Ok(())
}

/// `repo_health.write_rows`: summary.json, steps.json and daily.json.
pub fn write_rows(payload: &Value, out: &Path) -> std::result::Result<Vec<PathBuf>, PublicError> {
    validate(payload)?;
    let target = crate::resolve(out);
    let root = crate::data_root().map_err(PublicError)?;
    let here = std::env::current_dir().unwrap_or_default();
    for private in [
        crate::resolve(&root),
        crate::resolve(&here.join("private")),
        crate::resolve(&repo().join("private")),
    ] {
        if target.starts_with(&private) {
            return fail(format!(
                "--out must be outside {}: these files are meant to be published",
                private.display()
            ));
        }
    }
    if std::fs::create_dir_all(&target).is_err() {
        return fail(format!(
            "--out {} is not a folder careerbrew can write (OSError)",
            out.display()
        ));
    }
    let mut summary = payload["summary"].as_object().cloned().unwrap_or_default();
    summary.insert("generated_at".into(), payload["generated_at"].clone());
    summary.insert("window_days".into(), payload["window_days"].clone());
    let files = [
        ("summary", Value::Array(vec![Value::Object(summary)])),
        ("steps", payload["steps"].clone()),
        ("daily", payload["daily"].clone()),
    ];
    let plant = std::env::var("CAREERBREW_RUST24_PLANT").unwrap_or_default();
    let mut written = Vec::new();
    for (name, rows) in files {
        let path = target.join(format!("{name}.json"));
        if plant == "missing" && name == "daily" {
            written.push(path);
            continue;
        }
        let _ = std::fs::remove_file(&path);
        let text = crate::pretty(&json!({"rows": rows})) + "\n";
        if std::fs::write(&path, text).is_err() {
            return fail(format!("could not write {name}.json in --out (OSError)"));
        }
        written.push(path);
    }
    if plant == "extra" {
        let _ = std::fs::write(target.join("extra.json"), "{}\n");
    }
    Ok(written)
}

pub fn emit_ok(steps: usize, days: usize) {
    use careerbrew_vault::canon::Canon;
    struct Clock;
    impl careerbrew_vault::events::Clock for Clock {
        fn stamp(&self) -> String {
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
        }
        fn version(&self) -> String {
            crate::version()
        }
    }
    let Ok(root) = crate::data_root() else {
        return;
    };
    let fields = Canon::Map(vec![
        (Canon::Str("status".into()), Canon::Str("ok".into())),
        (
            Canon::Str("steps".into()),
            Canon::Int((steps as i64).into()),
        ),
        (Canon::Str("days".into()), Canon::Int((days as i64).into())),
    ]);
    let events = std::env::var("CAREERBREW_EVENTS").ok();
    let _ =
        careerbrew_vault::events::emit(&root, events.as_deref(), "report.public", &fields, &Clock);
}
