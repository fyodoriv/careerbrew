//! Registered tutor item, card, attempt, mastery, and readiness providers.

use crate::fsrs::{self, CardState};
use crate::{
    append_jsonl, chars, emit, field, read_json, read_jsonl, read_yaml, read_yaml_abs, stamp_at,
    strings, text, write_json, write_yaml, Result, TutorError,
};
use careerbrew_vault::canon::Canon;
use chrono::{DateTime, FixedOffset};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub fn now() -> DateTime<FixedOffset> {
    crate::now()
}

pub fn gap_items_path() -> &'static str {
    "tutor/gap-items.yaml"
}

fn cards_path() -> &'static str {
    "tutor/cards.json"
}

fn attempts_path() -> &'static str {
    "tutor/attempts.jsonl"
}

pub type Resources = [(String, PathBuf)];

/// Base files first, then each installed plugin's declared files, matching
/// `plugins.resources(kind)` without reading any owner-global plugin state.
pub fn resources(_root: &Path, repo_root: &Path, kind: &str) -> Result<Vec<(String, PathBuf)>> {
    if !careerbrew_setup::plugins::RESOURCE_KINDS.contains(&kind) {
        return Err(TutorError::new(
            "PluginError",
            format!("unknown resource kind {kind}"),
        ));
    }
    let mut out = Vec::new();
    let base = repo_root.join("knowledge").join(kind);
    if base.is_dir() {
        let mut files = std::fs::read_dir(base)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?
            .into_iter()
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        files.sort();
        out.extend(files.into_iter().map(|path| ("base".to_owned(), path)));
    }
    for plugin in careerbrew_setup::plugins::installed()? {
        let provides = field(&plugin.manifest, "provides");
        let declared = field(provides.unwrap_or(&Value::Null), kind);
        let specs = declared
            .map(|value| {
                if value.is_string() {
                    vec![text(Some(value))]
                } else {
                    strings(Some(value))
                }
            })
            .unwrap_or_default();
        let origin = text(field(&plugin.entry, "origin"));
        for spec in specs {
            let pattern = plugin.root.join(spec).to_string_lossy().into_owned();
            let matches = glob::glob(&pattern)
                .map_err(|_| TutorError::new("PluginError", "invalid plugin resource pattern"))?;
            for found in matches {
                let found = found
                    .map_err(|_| TutorError::new("PluginError", "cannot read plugin resource"))?;
                let files = if found.is_dir() {
                    let recursive = found.join("**/*").to_string_lossy().into_owned();
                    glob::glob(&recursive)
                        .map_err(|_| {
                            TutorError::new("PluginError", "invalid plugin resource pattern")
                        })?
                        .collect::<std::result::Result<Vec<_>, _>>()
                        .map_err(|_| {
                            TutorError::new("PluginError", "cannot read plugin resource")
                        })?
                } else {
                    vec![found]
                };
                let mut files = files
                    .into_iter()
                    .filter(|path| path.is_file())
                    .collect::<Vec<_>>();
                files.sort();
                out.extend(files.into_iter().map(|path| (origin.clone(), path)));
            }
        }
    }
    Ok(out)
}

pub struct Attempt<'a> {
    pub seconds: Option<i64>,
    pub at: DateTime<FixedOffset>,
    pub graded_by: &'a str,
}

pub fn rubrics(
    _root: &Path,
    _repo_root: &Path,
    plugin: &Resources,
) -> Result<BTreeMap<String, Value>> {
    let mut out = BTreeMap::new();
    for (_, path) in plugin {
        if !matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("yaml" | "yml")
        ) {
            continue;
        }
        let data = read_yaml_abs(path, json!({}))?;
        let id = text(field(&data, "id"));
        let id = if id.is_empty() {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            id
        };
        out.insert(id, data);
    }
    Ok(out)
}

pub fn items(root: &Path, repo_root: &Path, plugin: &Resources) -> Result<BTreeMap<String, Value>> {
    let mut out = BTreeMap::new();
    let base = read_yaml_abs(&repo_root.join("knowledge/topics.yaml"), json!({}))?;
    for item in field(&base, "items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let mut item = item;
        item["origin"] = json!("base");
        out.insert(text(field(&item, "id")), item);
    }
    for (origin, path) in plugin {
        if !matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("yaml" | "yml")
        ) {
            continue;
        }
        let data = read_yaml_abs(path, json!({}))?;
        for item in field(&data, "items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let mut item = item;
            item["origin"] = json!(origin);
            out.insert(text(field(&item, "id")), item);
        }
    }
    let generated = read_yaml(root, gap_items_path(), json!({}))?;
    for item in field(&generated, "items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let mut item = item;
        item["origin"] = json!("generated");
        out.insert(text(field(&item, "id")), item);
    }
    Ok(out)
}

pub fn add_gap_items(root: &Path, skills: &[String]) -> Result<Vec<String>> {
    let mut data = read_yaml(root, gap_items_path(), json!({"items": []}))?;
    let mut rows = field(&data, "items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let have = rows
        .iter()
        .map(|item| text(field(item, "id")))
        .collect::<BTreeSet<_>>();
    let mut added = Vec::new();
    for skill in skills {
        let slug = skill
            .to_lowercase()
            .chars()
            .map(|character| {
                if character.is_alphanumeric() {
                    character
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .trim_matches('-')
            .to_owned();
        let id = format!("gap-{slug}-1");
        if have.contains(&id) {
            continue;
        }
        rows.push(json!({
            "id": id,
            "topic": skill,
            "round": "skill",
            "rubric": "skill",
            "teach": format!("Study {skill}: core concept, when to use it, one real example, one pitfall. Use the official docs first."),
            "prompt": format!("Explain {skill} as you would to an interviewer: the core idea, when you would and would not use it, a concrete example, and a common pitfall."),
        }));
        added.push(id);
    }
    data["items"] = Value::Array(rows);
    write_yaml(root, gap_items_path(), &data)?;
    Ok(added)
}

pub fn cards(root: &Path) -> Result<BTreeMap<String, CardState>> {
    let raw = read_json(root, cards_path(), json!({}))?;
    let mut out = BTreeMap::new();
    for (id, value) in raw.as_object().into_iter().flatten() {
        out.insert(id.clone(), CardState::from_json(value)?);
    }
    Ok(out)
}

pub fn save_cards(root: &Path, cards: &BTreeMap<String, CardState>) -> Result<()> {
    write_json(root, cards_path(), &fsrs::states_json(cards))
}

pub fn attempts(root: &Path) -> Result<Vec<Value>> {
    read_jsonl(root, attempts_path())
}

pub fn next_item(
    root: &Path,
    repo_root: &Path,
    plugin: &Resources,
    focus_topics: &[String],
    at: DateTime<FixedOffset>,
) -> Result<Option<Value>> {
    let items = items(root, repo_root, plugin)?;
    let cards = cards(root)?;
    let mut due = cards
        .iter()
        .filter(|(id, card)| items.contains_key(*id) && card.due <= at)
        .map(|(id, card)| (card.due, id.clone()))
        .collect::<Vec<_>>();
    due.sort();
    if let Some((_, id)) = due.first() {
        let mut item = items[id].clone();
        item["mode"] = json!("review");
        return Ok(Some(item));
    }
    let mut new = items
        .values()
        .filter(|item| !cards.contains_key(&text(field(item, "id"))))
        .cloned()
        .collect::<Vec<_>>();
    if !focus_topics.is_empty() {
        let focus = new
            .iter()
            .filter(|item| focus_topics.contains(&text(field(item, "topic"))))
            .cloned()
            .collect::<Vec<_>>();
        if !focus.is_empty() {
            new = focus;
        }
    }
    if new.is_empty() {
        return Ok(None);
    }
    let mastery = mastery(root, repo_root, plugin, plugin, at)?;
    new.sort_by(|left, right| {
        let left_topic = text(field(left, "topic"));
        let right_topic = text(field(right, "topic"));
        let left_score = mastery
            .get(&left_topic)
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let right_score = mastery
            .get(&right_topic)
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        left_score
            .total_cmp(&right_score)
            .then(text(field(left, "id")).cmp(&text(field(right, "id"))))
    });
    let mut item = new.remove(0);
    item["mode"] = json!("learn");
    Ok(Some(item))
}

pub fn record(
    root: &Path,
    repo_root: &Path,
    plugin_knowledge: &Resources,
    plugin_rubrics: &Resources,
    item_id: &str,
    met: &[String],
    attempt: Attempt<'_>,
) -> Result<Value> {
    let Attempt {
        seconds,
        at,
        graded_by,
    } = attempt;
    let items = items(root, repo_root, plugin_knowledge)?;
    let item = items
        .get(item_id)
        .ok_or_else(|| TutorError::new("KeyError", format!("unknown item {item_id}")))?;
    let rubrics = rubrics(root, repo_root, plugin_rubrics)?;
    let rubric_id = text(field(item, "rubric"));
    let rubric = rubrics
        .get(&rubric_id)
        .ok_or_else(|| TutorError::new("KeyError", format!("rubric {rubric_id} missing")))?;
    let ids = field(rubric, "criteria")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|criterion| text(field(criterion, "id")))
        .collect::<Vec<_>>();
    let met_set = met.iter().cloned().collect::<BTreeSet<_>>();
    let bad = met_set
        .difference(&ids.iter().cloned().collect())
        .cloned()
        .collect::<Vec<_>>();
    if !bad.is_empty() {
        return Err(TutorError::value(format!(
            "unknown criteria {bad:?}; this rubric has {ids:?}"
        )));
    }
    let score = careerbrew_projects::python_round(met_set.len() as f64 / ids.len() as f64, 3);
    let rating = fsrs::score_to_rating(score);
    let mut cards = cards(root)?;
    let initial;
    let old = match cards.get(item_id) {
        Some(card) => Some(card),
        None => {
            initial = CardState::generated(at);
            Some(&initial)
        }
    };
    let next = fsrs::next_state(old, rating, at)?;
    cards.insert(item_id.to_owned(), next);
    save_cards(root, &cards)?;
    let row = json!({
        "item": item_id,
        "topic": text(field(item, "topic")),
        "round": text(field(item, "round")),
        "met": met_set.iter().cloned().collect::<Vec<_>>(),
        "missed": ids
            .iter()
            .filter(|id| !met_set.contains(*id))
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>(),
        "score": score,
        "rating": fsrs::rating_name(rating),
        "seconds": seconds,
        "graded_by": graded_by,
        "at": stamp_at(at),
        "due": stamp_at(cards[item_id].due),
        "origin": field(item, "origin").cloned().unwrap_or_else(|| json!("base")),
    });
    append_jsonl(root, attempts_path(), std::slice::from_ref(&row))?;
    let readiness = readiness(root, repo_root, plugin_knowledge, plugin_rubrics, &[], at)?;
    append_jsonl(
        root,
        "tutor/readiness.jsonl",
        &[json!({"at": stamp_at(at), "readiness": readiness})],
    )?;
    emit(
        root,
        "tutor.attempt",
        vec![
            ("status", Canon::Str(fsrs::rating_name(rating).into())),
            ("score", Canon::Float(score)),
        ],
    )?;
    Ok(row)
}

pub fn mastery(
    root: &Path,
    repo_root: &Path,
    plugin_knowledge: &Resources,
    plugin_rubrics: &Resources,
    at: DateTime<FixedOffset>,
) -> Result<BTreeMap<String, Value>> {
    let items = items(root, repo_root, plugin_knowledge)?;
    let cards = cards(root)?;
    let _ = plugin_rubrics;
    let mut last_score = BTreeMap::new();
    for attempt in attempts(root)? {
        last_score.insert(
            text(field(&attempt, "item")),
            field(&attempt, "score")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        );
    }
    let mut per_topic = BTreeMap::<String, Vec<f64>>::new();
    for (id, item) in items {
        let value = match (cards.get(&id), last_score.get(&id)) {
            (Some(card), Some(score)) => fsrs::retrievability(card, at) * score,
            _ => 0.0,
        };
        per_topic
            .entry(text(field(&item, "topic")))
            .or_default()
            .push(value);
    }
    Ok(per_topic
        .into_iter()
        .map(|(topic, values)| {
            let score = careerbrew_projects::python_round(
                values.iter().sum::<f64>() / values.len() as f64,
                3,
            );
            (topic, json!(score))
        })
        .collect())
}

pub fn readiness(
    root: &Path,
    repo_root: &Path,
    plugin_knowledge: &Resources,
    plugin_rubrics: &Resources,
    topics: &[String],
    at: DateTime<FixedOffset>,
) -> Result<f64> {
    let mastery = mastery(root, repo_root, plugin_knowledge, plugin_rubrics, at)?;
    let keys = if topics.is_empty() {
        mastery.keys().cloned().collect::<Vec<_>>()
    } else {
        topics
            .iter()
            .filter(|topic| mastery.contains_key(*topic))
            .cloned()
            .collect::<Vec<_>>()
    };
    if keys.is_empty() {
        return Ok(0.0);
    }
    let sum = keys
        .iter()
        .filter_map(|topic| mastery.get(topic).and_then(Value::as_f64))
        .sum::<f64>();
    Ok(careerbrew_projects::python_round(
        sum / keys.len() as f64,
        3,
    ))
}

pub fn feedback(
    root: &Path,
    repo_root: &Path,
    plugin_knowledge: &Resources,
    plugin_rubrics: &Resources,
    row: &Value,
) -> Result<Vec<String>> {
    let items = items(root, repo_root, plugin_knowledge)?;
    let item = &items[&text(field(row, "item"))];
    let rubrics = rubrics(root, repo_root, plugin_rubrics)?;
    let criteria = field(&rubrics[&text(field(item, "rubric"))], "criteria")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let score = field(row, "score").and_then(Value::as_f64).unwrap_or(0.0);
    let mut lines = vec![format!(
        "Score {:.0}% -> {}; next review {}.",
        score * 100.0,
        text(field(row, "rating")),
        chars(&text(field(row, "due")), 10)
    )];
    for id in crate::strings(field(row, "missed")) {
        let criterion = criteria
            .iter()
            .find(|criterion| text(field(criterion, "id")) == id);
        if let Some(criterion) = criterion {
            lines.push(format!("Practice: {}", text(field(criterion, "text"))));
        }
    }
    Ok(lines)
}
