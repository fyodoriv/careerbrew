//! The synthetic rust-24 workload, the native mirror of
//! `tests/oracles/rust24.py`. It reads the same fixture and writes the same
//! setup files, then runs the native builders.

use crate::{charts, checklist, field, repo_health, telemetry, time_saved, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const CASES: &[&str] = &[
    "event-clean",
    "telemetry-allowlist",
    "repo-health",
    "time-saved",
    "field-totals",
    "charts",
    "checklist",
];
const FIXTURE: &str = "tests/fixtures/rust24/cases.json";
const MACHINE: &str = "m-synthetic0";

fn fixture() -> Result<Value> {
    let text = std::fs::read_to_string(FIXTURE)
        .map_err(|_| "cannot read the rust-24 fixture".to_owned())?;
    serde_json::from_str(&text).map_err(|_| "invalid rust-24 fixture".to_owned())
}

fn write(path: &Path, text: &str) -> Result<()> {
    std::fs::write(path, text).map_err(|_| format!("cannot write {}", path.display()))
}

fn settings(text: &str) -> Result<()> {
    write(&crate::data_root()?.join("settings.yaml"), text)
}

fn scratch(name: &str) -> Result<PathBuf> {
    let base = PathBuf::from(std::env::var("TMPDIR").map_err(|_| "TMPDIR is required".to_owned())?)
        .join(name);
    std::fs::create_dir_all(&base).map_err(|_| "cannot create scratch folder".to_owned())?;
    Ok(base)
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

fn event_clean(data: &Value) -> Result<Value> {
    let mut cleaned = Vec::new();
    for fields in data["events"].as_array().into_iter().flatten() {
        let out = careerbrew_vault::events::clean(&crate::canon(fields))
            .map_err(|_| "event fields".to_owned())?;
        cleaned.push(crate::from_canon(&out));
    }
    Ok(json!({"cleaned": cleaned}))
}

fn telemetry_allowlist(data: &Value) -> Result<Value> {
    use crate::allowlists::*;
    let t = &data["telemetry"];
    let events = t["records"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|record| telemetry::to_event(record, text(&t["distinct_id"]), text(&t["since"])))
        .collect::<Vec<_>>();
    Ok(json!({
        "events": events,
        "command_names": telemetry::command_names().iter().collect::<Vec<_>>(),
        "errors": telemetry::sorted(ERRORS),
        "sets": {
            "event_names": telemetry::sorted(EVENT_NAMES),
            "statuses": telemetry::sorted(STATUSES),
            "source_kinds": telemetry::sorted(SOURCE_KINDS),
            "tiers": telemetry::sorted(TIERS),
            "view_routes": telemetry::sorted(VIEW_ROUTES),
            "web_acts": telemetry::sorted(WEB_ACTS),
            "actions": telemetry::sorted(ACTIONS),
            "user_commands": telemetry::sorted(USER_COMMANDS),
        },
    }))
}

fn apply(payload: &Value, path: &[Value], op: &str, value: &Value) -> Value {
    let mut copy = payload.clone();
    let mut target = &mut copy;
    for part in &path[..path.len() - 1] {
        target = match part {
            Value::Number(n) => &mut target[n.as_u64().unwrap_or(0) as usize],
            other => &mut target[other.as_str().unwrap_or_default()],
        };
    }
    let last = &path[path.len() - 1];
    match (target, last) {
        (Value::Array(items), Value::Number(n)) => {
            let index = n.as_u64().unwrap_or(0) as usize;
            if op == "del" {
                items.remove(index);
            } else {
                items[index] = value.clone();
            }
        }
        (Value::Object(items), Value::String(key)) => {
            if op == "del" {
                items.remove(key);
            } else {
                items.insert(key.clone(), value.clone());
            }
        }
        _ => {}
    }
    copy
}

fn outcome(result: std::result::Result<(), repo_health::PublicError>) -> Value {
    match result {
        Ok(()) => json!("accepted"),
        Err(error) => json!(error.0),
    }
}

fn repo_health_case(data: &Value) -> Result<Value> {
    let r = &data["repo_health"];
    settings(text(&data["settings"]))?;
    let root = crate::data_root()?;
    write(&root.join("sensitive-terms.txt"), text(&r["terms"]))?;
    let repo = scratch("rust24-repo")?;
    for folder in ["scripts", ".metrics"] {
        std::fs::create_dir_all(repo.join(folder))
            .map_err(|_| "cannot create repo folder".to_owned())?;
    }
    write(&repo.join("scripts/check.sh"), text(&r["check_sh"]))?;
    let lines = r["metrics_events"]
        .as_array()
        .into_iter()
        .flatten()
        .map(text)
        .collect::<Vec<_>>();
    write(
        &repo.join(".metrics/events.jsonl"),
        &(lines.join("\n") + "\n"),
    )?;
    write(
        &repo.join(".metrics/e2e-results.json"),
        text(&r["e2e_results"]),
    )?;
    let reports = root.join("out/reports");
    std::fs::create_dir_all(&reports).map_err(|_| "cannot create reports".to_owned())?;
    for pair in r["summaries"].as_array().into_iter().flatten() {
        write(&reports.join(text(&pair[0])), text(&pair[1]))?;
    }
    std::env::set_var("CAREERBREW_REPO_DIR", &repo);
    let days = r["days"].as_i64().unwrap_or(28);
    let payload = repo_health::collect(days).map_err(|error| error.0)?;
    let out = scratch("rust24-public")?;
    let written = repo_health::write_rows(&payload, &out)
        .map_err(|error| error.0)?
        .iter()
        .map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let bad_days = r["bad_days"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|days| outcome(repo_health::collect(days.as_i64().unwrap_or(0)).map(|_| ())))
        .collect::<Vec<_>>();
    let mutations = r["mutations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            let path = m[0].as_array().cloned().unwrap_or_default();
            outcome(repo_health::validate(&apply(
                &payload,
                &path,
                text(&m[1]),
                &m[2],
            )))
        })
        .collect::<Vec<_>>();
    let inside = outcome(repo_health::write_rows(&payload, &root.join("public")).map(|_| ()));
    Ok(json!({
        "gate_steps": repo_health::gate_steps().iter().collect::<Vec<_>>(),
        "payload": payload,
        "valid": outcome(repo_health::validate(&payload)),
        "bad_days": bad_days,
        "mutations": mutations,
        "written": written,
        "inside": inside,
    }))
}

fn log(data: &Value) -> Result<PathBuf> {
    let path = crate::data_root()?.join("rust24/events.jsonl");
    std::fs::create_dir_all(path.parent().expect("log has a parent"))
        .map_err(|_| "cannot create log folder".to_owned())?;
    let lines = data["time_saved"]["log"]
        .as_array()
        .into_iter()
        .flatten()
        .map(text)
        .collect::<Vec<_>>();
    write(&path, &(lines.join("\n") + "\n"))?;
    Ok(path)
}

fn time_saved_case(data: &Value) -> Result<Value> {
    settings(text(&data["settings"]))?;
    let log = log(data)?;
    let web = data["time_saved"]["web"]
        .as_array()
        .into_iter()
        .flatten()
        .map(
            |row| match time_saved::record_web(&row[0], &row[1], &row[2]) {
                Ok(fields) => json!({"accepted": fields}),
                Err(message) => json!({"refused": message}),
            },
        )
        .collect::<Vec<_>>();
    Ok(json!({
        "computed": time_saved::compute(&log)?,
        "web": web,
        "command_action": time_saved::command_action(),
    }))
}

fn field_totals(data: &Value) -> Result<Value> {
    settings(text(&data["settings"]))?;
    let log = log(data)?;
    let machine = field::Pinned(MACHINE.into());
    Ok(json!({
        "field": field::compute(&log, &machine)?,
        "empty": field::compute(&log.parent().expect("log has a parent").join("missing.jsonl"), &machine)?,
    }))
}

fn rows(value: &Value) -> Vec<(String, f64)> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            (
                text(&row[0]).to_owned(),
                crate::number(&row[1]).unwrap_or(0.0),
            )
        })
        .collect()
}

fn charts_case(data: &Value) -> Result<Value> {
    let c = &data["charts"];
    let mut rep = charts::Report::new(text(&c["title"]), text(&c["subtitle"]));
    let kpis = c["kpis"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| (text(&item[0]).to_owned(), text(&item[1]).to_owned()))
        .collect::<Vec<_>>();
    rep.kpis(&kpis);
    let lines = c["text"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|line| text(line).to_owned())
        .collect::<Vec<_>>();
    rep.text(&lines);
    for bar in c["bars"].as_array().into_iter().flatten() {
        let fmt = charts::Fmt::parse(text(&bar["fmt"])).ok_or("unsupported chart format")?;
        rep.bars(
            text(&bar["title"]),
            &rows(&bar["rows"]),
            crate::number(&bar["max_value"]).unwrap_or(1.0),
            fmt,
        );
    }
    for line in c["lines"].as_array().into_iter().flatten() {
        let fmt = charts::Fmt::parse(text(&line["fmt"])).ok_or("unsupported chart format")?;
        rep.line(
            text(&line["title"]),
            &rows(&line["points"]),
            crate::number(&line["y_max"]).unwrap_or(1.0),
            fmt,
        );
    }
    Ok(json!({"html": rep.html(), "empty": charts::Report::new("Empty", "").html()}))
}

struct FixtureFacts<'a>(&'a Value);

impl checklist::Facts for FixtureFacts<'_> {
    fn sources(&self) -> Result<Vec<Value>> {
        Ok(self.0["sources"].as_array().cloned().unwrap_or_default())
    }
    fn watch_state(&self) -> Result<Value> {
        Ok(self.0["watch"].clone())
    }
    fn tasks(&self) -> Result<Vec<Value>> {
        Ok(self.0["tasks"].as_array().cloned().unwrap_or_default())
    }
    fn goals(&self) -> Result<Value> {
        Ok(self.0["goals"].clone())
    }
    fn inbox(&self) -> Result<Vec<Value>> {
        Ok(self.0["inbox"].as_array().cloned().unwrap_or_default())
    }
}

fn checklist_case(data: &Value) -> Result<Value> {
    let mut results = Vec::new();
    for facts in data["checklist"].as_array().into_iter().flatten() {
        settings(text(&facts["settings"]))?;
        results.push(checklist::build(&FixtureFacts(facts))?);
    }
    Ok(json!({"results": results}))
}

fn run(case: &str, data: &Value) -> Result<Value> {
    match case {
        "event-clean" => event_clean(data),
        "telemetry-allowlist" => telemetry_allowlist(data),
        "repo-health" => repo_health_case(data),
        "time-saved" => time_saved_case(data),
        "field-totals" => field_totals(data),
        "charts" => charts_case(data),
        "checklist" => checklist_case(data),
        _ => Err("unknown case".into()),
    }
}

pub fn run_all(selected: Option<&str>) -> Result<String> {
    if std::env::var_os("CAREERBREW_HOME").is_none_or(|v| v.is_empty())
        || std::env::var_os("TMPDIR").is_none()
    {
        return Err("CAREERBREW_HOME and TMPDIR are required".into());
    }
    std::env::set_var("CAREERBREW_EVENTS", "off");
    let data = fixture()?;
    let mut cases = Vec::new();
    for case in CASES
        .iter()
        .filter(|case| selected.is_none_or(|s| s == **case))
    {
        cases.push(json!({"id": case, "result": run(case, &data)?}));
    }
    Ok(crate::compact(&json!({"cases": cases})))
}
