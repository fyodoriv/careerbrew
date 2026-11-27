//! `work/ladder_grade.py`: grade update lines against the next ladder level,
//! and keep each tagged line as evidence.

use crate::py::{self, list_repr};
use crate::{evidence, promo, py_str, truthy, when, Error, Result};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const STRONG: usize = 2;

/// `target()`: the `ladder` and `target_level` settings.
pub fn target() -> Result<(String, String)> {
    let settings = crate::settings()?;
    let get = |key: &str, default: &str| match settings.get(key) {
        Some(value) => py_str(Some(value)),
        None => default.to_owned(),
    };
    Ok((get("ladder", "generic-swe"), get("target_level", "senior")))
}

fn strings(value: Option<&Value>) -> Vec<Value> {
    value
        .filter(|v| truthy(Some(v)))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// `levels(ladder_id, level)`.
pub fn levels(ladder_id: &str, level: &str) -> Result<Vec<Value>> {
    let lads = promo::ladders()?;
    let Some(lad) = lads.get(ladder_id) else {
        let have = lads.keys().cloned().collect::<Vec<_>>();
        return Err(Error::new(
            "LadderError",
            format!("unknown ladder {ladder_id}; have {}", list_repr(&have)),
        ));
    };
    let names = lad["levels"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| py_str(Some(v)))
        .collect::<Vec<_>>();
    if !names.iter().any(|n| n == level) {
        return Err(Error::new(
            "LadderError",
            format!("level {level} not in {}", list_repr(&names)),
        ));
    }
    let mut out = Vec::new();
    for c in lad["competencies"].as_array().into_iter().flatten() {
        let ex = c
            .get("examples")
            .filter(|v| truthy(Some(v)))
            .and_then(|e| e.get(level))
            .filter(|v| truthy(Some(v)))
            .cloned()
            .unwrap_or_else(|| json!({}));
        out.push(json!({
            "id": c["id"],
            "name": c["name"],
            "description": c.get("descriptions").filter(|v| truthy(Some(v)))
                .and_then(|d| d.get(level)).cloned().unwrap_or_else(|| json!("")),
            "looks_like": strings(ex.get("looks_like")),
            "doesnt_look_like": strings(ex.get("doesnt_look_like")),
            "how_to_close": c.get("how_to_close").cloned().unwrap_or_else(|| json!("")),
        }));
    }
    Ok(out)
}

fn titled(title: &str) -> Value {
    json!({ "title": title })
}

/// `dims_of(title, ladder_id)`.
pub fn dims_of(title: &str, ladder_id: Option<&str>) -> Result<Vec<Value>> {
    let id = match ladder_id {
        Some(id) if !id.is_empty() => id.to_owned(),
        _ => target()?.0,
    };
    let lads = promo::ladders()?;
    let Some(lad) = lads.get(&id) else {
        return Ok(Vec::new());
    };
    Ok(lad["competencies"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| promo::matches(&titled(title), c))
        .map(|c| c["id"].clone())
        .collect())
}

/// `grade(lines, ladder_id, level)`.
pub fn grade(lines: &[&str], ladder_id: &str, level: &str) -> Result<Value> {
    let lads = promo::ladders()?;
    let comps = lads
        .get(ladder_id)
        .and_then(|lad| lad["competencies"].as_array().cloned())
        .unwrap_or_default();
    let comp = |id: &Value| comps.iter().find(|c| &c["id"] == id).cloned();
    let dims = levels(ladder_id, level)?;
    let mut tagged = Vec::new();
    for line in lines.iter().filter(|l| !py::strip(l).is_empty()) {
        let shown = dims
            .iter()
            .filter(|d| comp(&d["id"]).is_some_and(|c| promo::matches(&titled(line), &c)))
            .map(|d| d["id"].clone())
            .collect::<Vec<_>>();
        tagged.push(json!({ "line": line, "dims": shown }));
    }
    let mut table = Vec::new();
    let mut hints = Vec::new();
    for d in &dims {
        let shown = tagged
            .iter()
            .filter(|t| t["dims"].as_array().is_some_and(|ds| ds.contains(&d["id"])))
            .map(|t| t["line"].clone())
            .collect::<Vec<_>>();
        let coverage = if shown.len() >= STRONG {
            "strong"
        } else if !shown.is_empty() {
            "some"
        } else {
            "none"
        };
        let name = py_str(d.get("name"));
        if coverage == "some" {
            let bar = if truthy(d.get("description")) {
                py_str(d.get("description"))
            } else {
                py::lower(&name)
            };
            hints.push(json!({
                "id": d["id"],
                "line": shown[0],
                "hint": format!("say the {level}-level part of this line with true facts only (the bar: {bar})"),
            }));
        } else if coverage == "none" {
            let close = if truthy(d.get("how_to_close")) {
                py_str(d.get("how_to_close"))
            } else {
                name.clone()
            };
            hints.push(json!({
                "id": d["id"],
                "line": "",
                "hint": format!("no line shows it, and no true fact fits; work that would create one: {close}"),
            }));
        }
        table.push(
            json!({ "id": d["id"], "name": d["name"], "lines": shown, "coverage": coverage }),
        );
    }
    Ok(json!({
        "ladder": ladder_id,
        "level": level,
        "lines": tagged,
        "coverage": table,
        "suggestions": hints,
    }))
}

/// `record(graded, day)`: each tagged line once, at 18:00 local on `day`.
pub fn record(graded: &Value, day: &str) -> Result<usize> {
    let have: BTreeSet<String> = evidence::load()?
        .iter()
        .map(|it| crate::text_of(it.get("id")))
        .collect();
    let mut added = 0;
    for t in graded["lines"].as_array().into_iter().flatten() {
        let dims = t["dims"].as_array().cloned().unwrap_or_default();
        if dims.is_empty() {
            continue;
        }
        let (naive, _) = when::parse(&format!("{day}T18:00:00")).map_err(Error::value)?;
        let at = when::iso(&when::attach(naive, when::local_tz()?));
        let line = py_str(t.get("line"));
        if have.contains(&format!("note:{}", evidence::digest(&[&line, &at])?)) {
            continue;
        }
        let tags = dims
            .iter()
            .map(|d| format!("dim:{}", py_str(Some(d))))
            .collect::<Vec<_>>();
        evidence::note(&line, &tags, "", Some(&at), &[])?;
        added += 1;
    }
    Ok(added)
}
