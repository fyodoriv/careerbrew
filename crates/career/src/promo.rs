//! `work/promo.py`: ladders, evidence matching, the assessment, gaps and the
//! promotion document.
//!
//! The model summary is the fallback text (models are a separate exit, owned
//! by the model route; a configured route stays Python: rust-followup).

use crate::py::{self, Re};
use crate::when::{self, At};
use crate::{evidence, float, py_str, truthy, Error, Result};
use careerbrew_vault::canon::Canon;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub const BROAD_SHARE: f64 = 0.3;
pub const GOAL_BOOST: f64 = 1.5;
const GENERIC: &[&str] = &[
    "technical",
    "code",
    "work",
    "with",
    "skills",
    "quality",
    "impact",
    "team",
    "teams",
    "ownership",
];

fn ladder_error(message: impl Into<String>) -> Error {
    Error::new("LadderError", message)
}

fn key_text(value: Option<&Value>) -> String {
    py_str(value)
}

/// `validate_ladder(data)`.
pub fn validate_ladder(data: &Value) -> Result<Value> {
    let ok = data.is_object()
        && truthy(data.get("id"))
        && truthy(data.get("levels"))
        && truthy(data.get("competencies"));
    if !ok {
        return Err(ladder_error("a ladder needs id, levels, and competencies"));
    }
    let levels = data["levels"].as_array().cloned().unwrap_or_default();
    for c in data["competencies"].as_array().into_iter().flatten() {
        for key in ["id", "name", "weight", "signals", "expect"] {
            if c.get(key).is_none() {
                let id = c.get("id").map_or("?".to_owned(), |v| key_text(Some(v)));
                return Err(ladder_error(format!("competency {id} lacks `{key}`")));
            }
        }
        let expect = &c["expect"];
        let missing = levels
            .iter()
            .filter(|level| expect.get(key_text(Some(level))).is_none())
            .map(|level| key_text(Some(level)))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(ladder_error(format!(
                "competency {} has no expectation for levels {}",
                key_text(c.get("id")),
                py::list_repr(&missing)
            )));
        }
        for key in ["descriptions", "examples"] {
            if c.get(key).is_some_and(|v| !v.is_object()) {
                return Err(ladder_error(format!(
                    "competency {}: `{key}` must map a level to its text",
                    key_text(c.get("id"))
                )));
            }
        }
    }
    Ok(data.clone())
}

fn base_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../knowledge/ladders")
}

fn sorted_files(dir: &std::path::Path, yaml_only: bool) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let keep = if yaml_only {
            path.extension().is_some_and(|e| e == "yaml")
        } else {
            path.is_file()
        };
        if keep {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// `ladders()`: the base ladders, then the user's (installed plugins are a
/// rust-followup: none are installed in the synthetic workload).
pub fn ladders() -> Result<BTreeMap<String, Value>> {
    let mut found = sorted_files(&base_dir(), false)?
        .into_iter()
        .map(|p| ("base", p))
        .collect::<Vec<_>>();
    let user = crate::root()?.join("ladders");
    found.extend(sorted_files(&user, true)?.into_iter().map(|p| ("user", p)));
    let mut out = BTreeMap::new();
    for (origin, path) in found {
        let data = validate_ladder(&crate::read_yaml(&path, json!({}))?)?;
        let mut data = data;
        data["origin"] = Value::String(origin.into());
        out.insert(key_text(data.get("id")), data);
    }
    Ok(out)
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().map(|v| key_text(Some(v))).collect())
        .unwrap_or_default()
}

/// `_keyword_pattern(keyword)`.
pub fn keyword_pattern(keyword: &str) -> Re {
    let k = py::lower(keyword);
    match k.strip_suffix('*') {
        Some(stem) => Re::new(&format!(r"(?<!\w){}", py::escape(stem)), false),
        None => Re::new(&format!(r"(?<!\w){}(?!\w)", py::escape(&k)), false),
    }
}

/// `_matches(item, comp)`.
pub fn matches(item: &Value, comp: &Value) -> bool {
    let sig = &comp["signals"];
    let tags = strings(item.get("tags"));
    let dim = format!("dim:{}", key_text(comp.get("id")));
    if tags.contains(&dim) {
        return true;
    }
    let wanted = strings(sig.get("tags"));
    if tags.iter().any(|t| wanted.contains(t)) {
        return true;
    }
    let title = py::lower(&crate::text_of(item.get("title")));
    strings(sig.get("keywords"))
        .iter()
        .any(|k| keyword_pattern(k).search(&title))
}

fn number(value: Option<&Value>) -> f64 {
    value
        .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(0.0)
}

fn words(text: &str) -> BTreeSet<String> {
    Re::new(r"[a-z0-9]{4,}", false)
        .find_iter(text)
        .into_iter()
        .map(|(word, _)| word)
        .collect()
}

/// `_long_term_words()`: words of the owner's long-term goals.
pub fn long_term_words() -> Result<BTreeSet<String>> {
    let data = crate::read_yaml(&crate::sub(&["goals.yaml"])?, json!({}))?;
    let mut out = BTreeSet::new();
    for goal in data
        .get("long_term")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let text = goal.get("text").map_or(String::new(), |t| py_str(Some(t)));
        out.extend(words(&py::lower(&text)));
    }
    Ok(out)
}

/// `_named_by_goal(comp, words)`.
pub fn named_by_goal(comp: &Value, goal_words: &BTreeSet<String>) -> bool {
    let text = py::lower(&format!(
        "{} {}",
        key_text(comp.get("id")),
        key_text(comp.get("name"))
    ))
    .replace('-', " ");
    let own: BTreeSet<String> = words(&text)
        .into_iter()
        .filter(|w| !GENERIC.contains(&w.as_str()))
        .collect();
    let stems: BTreeSet<String> = own.iter().map(|w| py::take(w, 5)).collect();
    goal_words
        .iter()
        .filter(|w| !GENERIC.contains(&w.as_str()))
        .any(|w| stems.contains(&py::take(w, 5)) || own.iter().any(|s| s.starts_with(w.as_str())))
}

/// One competency row of an assessment.
#[derive(Clone)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub weight: Value,
    pub weight_f: f64,
    pub need: f64,
    pub credit: f64,
    pub coverage: f64,
    pub priority: f64,
    pub goal: bool,
    pub how_to_close: String,
    pub description: String,
    pub examples: Value,
    pub evidence: Vec<Value>,
}

pub struct Assessment {
    pub ladder: String,
    pub origin: String,
    pub level: String,
    pub overall: f64,
    pub rows: Vec<Row>,
    pub warnings: Vec<String>,
    pub at: String,
}

fn when_of(item: &Value, at: &At) -> At {
    let Some(Value::String(text)) = item.get("at") else {
        return *at;
    };
    match when::parse(&text.replace('Z', "+00:00")) {
        Ok((naive, Some(offset))) => when::fixed(naive, offset),
        // A naive stamp would raise TypeError in Python; the stored rows carry offsets.
        Ok((naive, None)) => when::fixed(naive, *at.offset()),
        Err(_) => *at,
    }
}

/// `assess(ladder_id, level, at)`.
pub fn assess(ladder_id: &str, level: &str, at: &At) -> Result<Assessment> {
    let lads = ladders()?;
    let Some(lad) = lads.get(ladder_id) else {
        let have = lads.keys().cloned().collect::<Vec<_>>();
        return Err(ladder_error(format!(
            "unknown ladder {ladder_id}; have {}",
            py::list_repr(&have)
        )));
    };
    let levels = strings(lad.get("levels"));
    if !levels.iter().any(|l| l == level) {
        return Err(ladder_error(format!(
            "level {level} not in {}",
            py::list_repr(&levels)
        )));
    }
    let months = lad.get("window_months").map_or(12.0, |v| number(Some(v))) as i64;
    let window = chrono::Duration::days(30 * months);
    let items = evidence::load()?;
    let goal_words = long_term_words()?;
    let mut rows = Vec::new();
    let mut warnings = Vec::new();
    for comp in lad["competencies"].as_array().into_iter().flatten() {
        let matched = items
            .iter()
            .filter(|it| matches(it, comp))
            .cloned()
            .collect::<Vec<_>>();
        let mut credit = 0.0;
        for it in &matched {
            let seen = when_of(it, at);
            credit += if *at - seen <= window { 1.0 } else { 0.5 };
        }
        let need = number(comp["expect"].get(level));
        let coverage = if need == 0.0 {
            1.0
        } else {
            (credit / need).min(1.0)
        };
        let effort = comp.get("effort").map_or(2.0, |v| number(Some(v)));
        if !items.is_empty() && matched.len() as f64 / items.len() as f64 > BROAD_SHARE {
            warnings.push(format!(
                "competency {} matches {} of {} evidence items (over {}); narrow its signals",
                key_text(comp.get("id")),
                matched.len(),
                items.len(),
                py::percent(BROAD_SHARE)
            ));
        }
        let goal = named_by_goal(comp, &goal_words);
        let weight_f = number(comp.get("weight"));
        let boost = if goal { GOAL_BOOST } else { 1.0 };
        let mut sorted = matched.clone();
        sorted.sort_by_key(|it| std::cmp::Reverse(crate::text_of(it.get("at"))));
        sorted.truncate(8);
        rows.push(Row {
            id: key_text(comp.get("id")),
            name: key_text(comp.get("name")),
            weight: comp["weight"].clone(),
            weight_f,
            need,
            credit: py::round(credit, 1),
            coverage: py::round(coverage, 3),
            priority: py::round(weight_f * (1.0 - coverage) / effort * boost, 3),
            goal,
            how_to_close: crate::text_of(comp.get("how_to_close")),
            description: comp
                .get("descriptions")
                .and_then(|d| d.get(level))
                .map(|v| key_text(Some(v)))
                .unwrap_or_default(),
            examples: comp
                .get("examples")
                .and_then(|e| e.get(level))
                .filter(|v| truthy(Some(v)))
                .cloned()
                .unwrap_or_else(|| json!({})),
            evidence: sorted,
        });
    }
    let total: f64 = rows.iter().map(|r| r.weight_f).sum();
    let total = if total == 0.0 { 1.0 } else { total };
    let overall = py::round(
        rows.iter().map(|r| r.weight_f * r.coverage).sum::<f64>() / total,
        3,
    );
    Ok(Assessment {
        ladder: ladder_id.to_owned(),
        origin: key_text(lad.get("origin")),
        level: level.to_owned(),
        overall,
        rows,
        warnings,
        at: when::iso_seconds(at),
    })
}

/// `gaps(assessment)`: below the bar, highest priority first.
pub fn gaps(a: &Assessment) -> Vec<Row> {
    let mut out = a
        .rows
        .iter()
        .filter(|r| r.coverage < 1.0)
        .cloned()
        .collect::<Vec<_>>();
    out.sort_by(|x, y| {
        y.priority
            .partial_cmp(&x.priority)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| x.id.cmp(&y.id))
    });
    out
}

impl Row {
    /// The row as the oracle prints it (evidence by id).
    pub fn view(&self) -> Value {
        let mut map = Map::new();
        map.insert("id".into(), json!(self.id));
        map.insert("name".into(), json!(self.name));
        map.insert("weight".into(), self.weight.clone());
        map.insert("need".into(), float(self.need));
        map.insert("credit".into(), float(self.credit));
        map.insert("coverage".into(), float(self.coverage));
        map.insert("priority".into(), float(self.priority));
        map.insert("goal".into(), json!(self.goal));
        map.insert("how_to_close".into(), json!(self.how_to_close));
        map.insert("description".into(), json!(self.description));
        map.insert("examples".into(), self.examples.clone());
        map.insert(
            "evidence".into(),
            Value::Array(self.evidence.iter().map(|e| e["id"].clone()).collect()),
        );
        Value::Object(map)
    }
}

impl Assessment {
    pub fn view(&self) -> Value {
        json!({
            "ladder": self.ladder,
            "ladder_origin": self.origin,
            "level": self.level,
            "overall": float(self.overall),
            "competencies": self.rows.iter().map(Row::view).collect::<Vec<_>>(),
            "warnings": self.warnings,
            "at": self.at,
        })
    }
}

fn list_or_one(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(items)) => items.iter().map(|v| py_str(Some(v))).collect(),
        _ => Vec::new(),
    }
}

/// The promotion document's text (`write_doc` before it writes).
pub fn render_doc(a: &Assessment) -> String {
    let mut strongest = a.rows.clone();
    strongest.sort_by(|x, y| {
        y.coverage
            .partial_cmp(&x.coverage)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    strongest.truncate(3);
    let names = strongest
        .iter()
        .map(|r| py::lower(&r.name))
        .collect::<Vec<_>>()
        .join(", ");
    let summary = format!(
        "Case for {}: overall ladder coverage {}. Strongest areas: {names}.",
        a.level,
        py::percent(a.overall)
    );
    let mut lines = vec![
        format!("# Promotion case: {}", a.level),
        String::new(),
        format!(
            "_Ladder `{}` ({}) · generated {} · summary by fallback_",
            a.ladder, a.origin, a.at
        ),
        String::new(),
        "## Summary".into(),
        String::new(),
        py::strip(&summary).to_owned(),
        String::new(),
        "## Evidence by competency".into(),
    ];
    for r in &a.rows {
        lines.push(String::new());
        lines.push(format!(
            "### {} — {} of the bar ({}/{})",
            r.name,
            py::percent(r.coverage),
            py::float_repr(r.credit),
            py::general(r.need)
        ));
        lines.push(String::new());
        if !r.description.is_empty() {
            lines.push(format!("**Bar at {}:** {}", a.level, r.description));
            lines.push(String::new());
        }
        for (key, label) in [
            ("looks_like", "Looks like"),
            ("doesnt_look_like", "Does not look like"),
        ] {
            let shown = r.examples.get(key).filter(|v| truthy(Some(v)));
            let shown = list_or_one(shown);
            if !shown.is_empty() {
                lines.push(format!("**{label}:**"));
                lines.extend(shown.iter().map(|x| format!("- {x}")));
                lines.push(String::new());
            }
        }
        if r.evidence.is_empty() {
            lines.push("- No evidence yet.".into());
        }
        for e in &r.evidence {
            let link = if truthy(e.get("url")) {
                format!(" ({})", py_str(e.get("url")))
            } else {
                String::new()
            };
            let impact = if truthy(e.get("impact")) {
                format!(" — {}", py_str(e.get("impact")))
            } else {
                String::new()
            };
            let at = py::take(&crate::text_of(e.get("at")), 10);
            lines.push(format!("- {at} {}{impact}{link}", py_str(e.get("title"))));
        }
    }
    lines.push(String::new());
    lines.push("## What is missing (highest priority first)".into());
    lines.push(String::new());
    let gaps = gaps(a);
    for (i, g) in gaps.iter().enumerate() {
        lines.push(format!(
            "{}. **{}** — {} covered, priority {}. {}",
            i + 1,
            g.name,
            py::percent(g.coverage),
            py::float_repr(g.priority),
            g.how_to_close
        ));
    }
    if gaps.is_empty() {
        lines.push(
            "Nothing: every competency meets the bar. Ask your manager to start the promotion process."
                .into(),
        );
    }
    lines.join("\n") + "\n"
}

/// `write_doc(assessment)`: the document in `out/`, then one metrics row.
pub fn write_doc(a: &Assessment) -> Result<PathBuf> {
    let text = render_doc(a);
    let out = crate::sub(&["out", &format!("promo-{}-{}.md", a.ladder, a.level)])?;
    std::fs::write(&out, text)?;
    let row = Canon::Map(vec![
        (
            Canon::Str("metric".into()),
            Canon::Str("promo_coverage".into()),
        ),
        (Canon::Str("value".into()), Canon::Float(a.overall)),
        (Canon::Str("at".into()), Canon::Str(a.at.clone())),
        (Canon::Str("ladder".into()), Canon::Str(a.ladder.clone())),
        (Canon::Str("level".into()), Canon::Str(a.level.clone())),
    ]);
    crate::append_jsonl(&crate::sub(&["metrics.jsonl"])?, &[row])?;
    Ok(out)
}
