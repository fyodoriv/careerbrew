//! Task estimates, ranking, impact records and completion.

use crate::{
    field, float, now, project_priority, projects, read_yaml, stamp, text, write_yaml,
    ProjectError, Result,
};
use chrono::NaiveDate;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

pub const MAX_HOURS: f64 = 200.0;
pub const MAX_RATIO: f64 = 10.0;

pub fn hours(value: &Value) -> Result<f64> {
    let hours = float(value).ok_or_else(|| ProjectError::value("hours must be a number"))?;
    if !hours.is_finite() || !(0.05..=MAX_HOURS).contains(&hours) {
        return Err(ProjectError::value(
            "hours must be a number from 0.05 to 200",
        ));
    }
    Ok(hours)
}

pub fn hours_text(value: &str) -> Result<f64> {
    hours(&json!(value))
}

pub fn estimate(task: &Value) -> f64 {
    let estimate = field(task, "estimate_h").and_then(float).unwrap_or(1.0);
    if estimate.is_finite() && estimate > 0.0 && estimate <= MAX_HOURS {
        estimate
    } else {
        1.0
    }
}

pub fn is_web(url: &str) -> bool {
    let url = url.trim();
    url.strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .and_then(|rest| rest.split('/').next())
        .is_some_and(|host| !host.is_empty())
}

pub fn parse_link(value: &str) -> Value {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if is_web(&value) {
        return json!({"label": "", "url": crate::chars(&value, 300)});
    }
    if let Some((label, rest)) = value.split_once('=') {
        if !label.trim().is_empty() && is_web(rest) {
            return json!({"label": crate::chars(label.trim(), 80), "url": crate::chars(rest.trim(), 300)});
        }
    }
    json!({"label": "", "url": crate::chars(&value, 300)})
}

fn short(url: &str) -> String {
    if !is_web(url) {
        return url.into();
    }
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let mut parts = rest.split('/');
    let host = parts.next().unwrap_or_default();
    let path = parts.filter(|part| !part.is_empty()).collect::<Vec<_>>();
    match path.as_slice() {
        [] => host.into(),
        [one] => format!("{host}/{one}"),
        many => format!("{host}/…/{}", many[many.len() - 1]),
    }
}

pub fn links(task: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for raw in task["links"].as_array().into_iter().flatten() {
        let link = if raw.is_string() {
            parse_link(&text(Some(raw)))
        } else {
            raw.clone()
        };
        let url = text(field(&link, "url"));
        if !url.trim().is_empty() {
            let label = text(field(&link, "label"));
            out.push(
                json!({"label": if label.is_empty() { short(&url) } else { label }, "url": url}),
            );
        }
    }
    out
}

pub fn load(root: &Path) -> Result<Vec<Value>> {
    let data = read_yaml(root, "work/tasks.yaml", json!({"tasks": []}))?;
    Ok(data["tasks"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|mut task| {
            if let Some(old) = task
                .as_object_mut()
                .and_then(|object| object.remove("link"))
            {
                let old = text(Some(&old));
                let mut links = task["links"].as_array().cloned().unwrap_or_default();
                if !old.is_empty()
                    && !links
                        .iter()
                        .any(|link| text(field(link, "url")) == old || text(Some(link)) == old)
                {
                    links.insert(0, json!({"label": "", "url": old}));
                }
                task["links"] = Value::Array(links);
            }
            task
        })
        .collect())
}

pub fn save(root: &Path, tasks: &[Value]) -> Result<()> {
    write_yaml(root, "work/tasks.yaml", &json!({"tasks": tasks}))
}

pub fn is_open(task: &Value) -> bool {
    field(task, "status").is_none_or(|status| text(Some(status)) == "open")
}

pub fn add(
    root: &Path,
    title: &str,
    impact: i64,
    urgency: i64,
    estimate_h: Value,
    due: Option<&str>,
    links_in: &[String],
) -> Result<Value> {
    let estimate_h = hours(&estimate_h)?;
    if let Some(due) = due {
        crate::date(due)?;
    }
    let mut tasks = load(root)?;
    let id = format!("t{:03}", tasks.len() + 1);
    let links = links_in
        .iter()
        .map(|value| parse_link(value))
        .filter(|link| !text(field(link, "url")).is_empty())
        .take(8)
        .collect::<Vec<_>>();
    let task = json!({
        "id": id,
        "title": title,
        "impact": impact,
        "urgency": urgency,
        "estimate_h": estimate_h,
        "competencies": [],
        "due": due,
        "status": "open",
        "source": "user",
        "added_at": now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        "why": "",
        "links": links,
    });
    tasks.push(task.clone());
    save(root, &tasks)?;
    Ok(task)
}

pub fn rank(root: &Path, day: NaiveDate) -> Result<Vec<Value>> {
    let entries = project_priority::entries(root)?;
    let linked = projects::task_projects(root)?;
    let mut out = Vec::new();
    for task in load(root)? {
        if !is_open(&task) {
            continue;
        }
        let mut urgency = field(&task, "urgency").and_then(float).unwrap_or(3.0);
        if !urgency.is_finite() {
            urgency = 3.0;
        }
        let mut due_days = Value::Null;
        if let Some(due) = field(&task, "due")
            .and_then(Value::as_str)
            .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
        {
            let days = (due - day).num_days();
            due_days = json!(days);
            if days <= 2 {
                urgency += 3.0;
            }
        }
        let mut impact = field(&task, "impact").and_then(float).unwrap_or(3.0);
        if !impact.is_finite() {
            impact = 3.0;
        }
        let size = estimate(&task).max(0.25);
        let project = project_priority::of_task(&task, &linked, &entries);
        let project_bonus = project
            .as_ref()
            .map(|entry| {
                project_priority::bonus(
                    field(entry, "rank").and_then(Value::as_u64).unwrap_or(0) as usize,
                    entries.len(),
                )
            })
            .unwrap_or(0.0);
        let score = crate::python_round((impact + urgency + project_bonus) / size.sqrt(), 2);
        let mut row = task.clone();
        row["score"] = json!(score);
        row["promo_bonus"] = json!(0.0);
        row["parts"] = json!({
            "impact": impact,
            "urgency": urgency,
            "due_days": due_days,
            "bonus": 0.0,
            "size_h": size,
            "project": project.as_ref().map(|entry| text(field(entry, "name"))),
            "project_rank": project.as_ref().and_then(|entry| field(entry, "rank")).cloned().unwrap_or(Value::Null),
            "projects": entries.len(),
            "project_bonus": project_bonus,
        });
        out.push(row);
    }
    out.sort_by(|a, b| {
        b["score"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&a["score"].as_f64().unwrap_or(0.0))
            .then_with(|| text(field(a, "id")).cmp(&text(field(b, "id"))))
    });
    Ok(out)
}

pub fn regrade(
    root: &Path,
    id: &str,
    impact: Option<i64>,
    urgency: Option<i64>,
    estimate_h: Option<Value>,
) -> Result<Value> {
    if impact.is_some_and(|value| !(1..=5).contains(&value))
        || urgency.is_some_and(|value| !(1..=5).contains(&value))
    {
        return Err(ProjectError::value("impact and urgency are 1 to 5"));
    }
    let checked = estimate_h.as_ref().map(hours).transpose()?;
    let mut tasks = load(root)?;
    for task in &mut tasks {
        if text(field(task, "id")) != id {
            continue;
        }
        if let Some(value) = impact {
            task["impact"] = json!(value);
        }
        if let Some(value) = urgency {
            task["urgency"] = json!(value);
        }
        if let Some(value) = checked {
            task["estimate_h"] = json!(value);
        }
        let answer = task.clone();
        save(root, &tasks)?;
        return Ok(answer);
    }
    Err(ProjectError::new("KeyError", "unknown task"))
}

fn evidence_tags(title: &str, tags: &[String]) -> Vec<String> {
    let low = title.to_lowercase();
    let head = low.split_once(':').map(|(head, _)| head).unwrap_or(&low);
    let head = head
        .split_once('(')
        .map(|(head, _)| head)
        .unwrap_or(head)
        .trim();
    let mut out = BTreeSet::new();
    let kind = match head {
        "fix" => &["quality"][..],
        "feat" => &["delivery"][..],
        "perf" => &["quality", "impact"][..],
        "refactor" => &["quality"][..],
        "docs" => &["communication"][..],
        "test" => &["quality"][..],
        "ci" => &["tooling"][..],
        _ => &[][..],
    };
    out.extend(kind.iter().map(|tag| (*tag).to_owned()));
    for (word, tag) in [
        ("incident", "ownership"),
        ("mentor", "leadership"),
        ("design", "design"),
        ("rfc", "design"),
        ("migrat", "impact"),
        ("latency", "impact"),
        ("review", "collaboration"),
    ] {
        if low.contains(word) {
            out.insert(tag.to_owned());
        }
    }
    out.extend(tags.iter().cloned());
    out.into_iter().collect()
}

fn evidence_note(root: &Path, task: &Value, impact: &str, links: &[String]) -> Result<Value> {
    let when = stamp();
    let title = text(field(task, "title"));
    let id = "note:".to_owned()
        + &careerbrew_vault::helpers::digest(&[
            careerbrew_setup::value::to_canon(&json!(title)),
            careerbrew_setup::value::to_canon(&json!(when)),
        ])?;
    let mut item = json!({
        "id": id,
        "kind": "note",
        "title": title,
        "impact": impact,
        "at": when,
        "tags": evidence_tags(&title, &crate::strings(field(task, "competencies"))),
        "origin": "user",
    });
    if !links.is_empty() {
        item["links"] = json!(links);
    }
    let context = crate::context()?;
    let store = context.store();
    store.append_jsonl(
        &root.join("work/evidence.jsonl"),
        &[careerbrew_setup::value::to_canon(&item)],
    )?;
    Ok(item)
}

#[derive(Default)]
pub struct ImpactInput<'a> {
    pub expected: &'a str,
    pub who: &'a str,
    pub metric: &'a str,
    pub result: &'a str,
    pub evidence: &'a [String],
    pub unsure: bool,
}

pub fn record_impact(root: &Path, id: &str, input: ImpactInput<'_>) -> Result<Value> {
    let ImpactInput {
        expected,
        who,
        metric,
        result,
        evidence,
        unsure,
    } = input;
    let links_in = evidence
        .iter()
        .map(|link| link.trim().to_owned())
        .filter(|link| !link.is_empty())
        .collect::<Vec<_>>();
    if [expected, who, metric, result]
        .iter()
        .all(|value| value.is_empty())
        && links_in.is_empty()
    {
        return Err(ProjectError::value(
            "nothing to record: pass --expect, --who, --metric, --result, or --evidence",
        ));
    }
    let mut tasks = load(root)?;
    for task in &mut tasks {
        if text(field(task, "id")) != id {
            continue;
        }
        if !expected.is_empty() || !who.is_empty() || !metric.is_empty() {
            let mut plan = task["expected_impact"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            for (key, value) in [("hypothesis", expected), ("who", who), ("metric", metric)] {
                if !value.is_empty() {
                    plan.insert(
                        key.into(),
                        json!(crate::chars(
                            &value.split_whitespace().collect::<Vec<_>>().join(" "),
                            300
                        )),
                    );
                }
            }
            task["expected_impact"] = Value::Object(plan);
        }
        if !result.is_empty() || !links_in.is_empty() {
            let mut measured = task["measured_impact"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            if !result.is_empty() {
                let normalized = result.split_whitespace().collect::<Vec<_>>().join(" ");
                let normalized = crate::chars(&normalized, 300);
                let normalized = if unsure && !result.starts_with("Not sure:") {
                    format!("Not sure: {normalized}")
                } else {
                    normalized
                };
                measured.insert("result".into(), json!(normalized));
            }
            let existing = measured
                .get("evidence")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut merged = Vec::new();
            for link in existing.iter().map(|link| text(Some(link))).chain(links_in) {
                if !merged.contains(&link) {
                    merged.push(link);
                }
            }
            merged.truncate(8);
            measured.insert("evidence".into(), json!(merged));
            measured.insert("at".into(), json!(stamp()));
            task["measured_impact"] = Value::Object(measured);
        }
        let answer = task.clone();
        save(root, &tasks)?;
        if !result.is_empty() {
            let measured = &answer["measured_impact"];
            evidence_note(
                root,
                &answer,
                &text(field(measured, "result")),
                &crate::strings(field(measured, "evidence")),
            )?;
        }
        return Ok(answer);
    }
    Err(ProjectError::new("KeyError", "unknown task"))
}

pub fn done(root: &Path, id: &str, actual_h: Value) -> Result<Value> {
    let actual_h = hours(&actual_h)?;
    let mut tasks = load(root)?;
    for task in &mut tasks {
        if text(field(task, "id")) != id {
            continue;
        }
        if text(field(task, "status")) == "done" {
            return Err(ProjectError::value("task is already done"));
        }
        task["status"] = json!("done");
        task["actual_h"] = json!(actual_h);
        task["done_at"] = json!(stamp());
        let answer = task.clone();
        save(root, &tasks)?;
        let ratio = crate::python_round(MAX_RATIO.min(actual_h / estimate(&answer).max(0.25)), 3);
        let row = json!({"metric": "estimate_ratio", "value": ratio, "at": answer["done_at"], "task": id, "source": text(field(&answer, "source"))});
        let context = crate::context()?;
        let store = context.store();
        store.append_jsonl(
            &root.join("metrics.jsonl"),
            &[careerbrew_setup::value::to_canon(&row)],
        )?;
        return Ok(answer);
    }
    Err(ProjectError::new("KeyError", "unknown task"))
}
