//! `careerbrew.jobs.criteria`: hard filters from `jobs/criteria.yaml`.

use crate::sources::{CustomRegions, Posting, REGIONS};
use crate::{int, truthy, JobsError, Result};
use careerbrew_tutor::{read_yaml, strings};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub fn defaults() -> Value {
    json!({
        "titles_any": ["engineer", "developer"],
        "titles_none": ["intern", "manager", "director", "recruiter"],
        "min_seniority": 0,
        "remote": "any",
        "locations_any": [],
        "min_salary": null,
        "exclude_companies": [],
        "regions_ok": null,
        "region_words": {},
    })
}

/// `{**DEFAULTS, **user}`.
pub fn load(root: &Path) -> Result<Value> {
    let user = read_yaml(root, "jobs/criteria.yaml", json!({}))?;
    let mut merged = defaults();
    if let (Some(base), Some(user)) = (merged.as_object_mut(), user.as_object()) {
        for (key, value) in user {
            base.insert(key.clone(), value.clone());
        }
    }
    Ok(merged)
}

const LABELS_ERROR: &str = "criteria regions_ok must be a list of labels in quotes or plain words, for example [global, examplia]";

fn region_labels(c: &Value) -> Result<Vec<String>> {
    let raw = c.get("regions_ok").filter(|v| truthy(Some(v)));
    let labels = match raw {
        None => Vec::new(),
        Some(Value::String(one)) => vec![Value::String(one.clone())],
        Some(Value::Array(items)) => items.clone(),
        Some(_) => return Err(JobsError::value(LABELS_ERROR)),
    };
    labels
        .iter()
        .map(|label| match label.as_str() {
            Some(text) if !text.trim().is_empty() => Ok(text.trim().to_lowercase()),
            _ => Err(JobsError::value(LABELS_ERROR)),
        })
        .collect()
}

fn normal_words(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// rust-followup: several `region_words` keys keep YAML insertion order in
/// Python; this reader sees them in sorted order.
pub fn custom_regions(c: &Value) -> Result<CustomRegions> {
    let built_in = REGIONS.iter().map(|(label, _)| *label).collect::<Vec<_>>();
    let extra = match c.get("region_words").filter(|v| truthy(Some(v))) {
        None => serde_json::Map::new(),
        Some(Value::Object(map)) => map.clone(),
        Some(_) => return Err(JobsError::value(
            "criteria region_words must be a mapping of label to a list of words, for example {examplia: [examplian]}",
        )),
    };
    let mut words: Vec<(String, Vec<String>)> = Vec::new();
    let entry = |words: &mut Vec<(String, Vec<String>)>, name: &str| -> usize {
        if let Some(index) = words.iter().position(|(label, _)| label == name) {
            return index;
        }
        words.push((name.to_owned(), vec![name.to_owned()]));
        words.len() - 1
    };
    for name in region_labels(c)? {
        if !built_in.contains(&name.as_str()) {
            entry(&mut words, &name);
        }
    }
    for (label, more) in extra {
        let name = label.trim().to_lowercase();
        if name.is_empty() {
            return Err(JobsError::value(
                "criteria region_words: each label must be a non-empty text",
            ));
        }
        if built_in.contains(&name.as_str()) {
            return Err(JobsError::value(format!(
                "criteria region_words: {name} is a built-in label; add words only for a label of your own"
            )));
        }
        let Some(items) = more.as_array() else {
            return Err(JobsError::value(format!(
                "criteria region_words: the words for {name} must be a list, for example [examplian]"
            )));
        };
        if !items
            .iter()
            .all(|w| w.as_str().is_some_and(|w| !w.trim().is_empty()))
        {
            return Err(JobsError::value(format!(
                "criteria region_words: each word for {name} must be a non-empty text"
            )));
        }
        let index = entry(&mut words, &name);
        for item in items {
            words[index]
                .1
                .push(normal_words(item.as_str().unwrap_or_default()));
        }
    }
    Ok(words
        .into_iter()
        .map(|(name, list)| {
            let mut unique: Vec<String> = Vec::new();
            for word in list {
                if !unique.contains(&word) {
                    unique.push(word);
                }
            }
            (name, unique)
        })
        .collect())
}

fn lower_list(c: &Value, key: &str) -> Vec<String> {
    strings(c.get(key))
        .iter()
        .map(|item| item.to_lowercase())
        .collect()
}

pub fn reject_reason(p: &Posting, c: &Value) -> Result<Option<&'static str>> {
    let title = p.title.to_lowercase();
    let any = lower_list(c, "titles_any");
    if !any.is_empty() && !any.iter().any(|t| title.contains(t.as_str())) {
        return Ok(Some("title"));
    }
    if lower_list(c, "titles_none")
        .iter()
        .any(|t| title.contains(t.as_str()))
    {
        return Ok(Some("title-excluded"));
    }
    let min_seniority = match c.get("min_seniority").filter(|v| truthy(Some(v))) {
        Some(value) => int(value)?,
        None => 0,
    };
    if p.seniority < min_seniority {
        return Ok(Some("seniority"));
    }
    if lower_list(c, "exclude_companies").contains(&p.company.to_lowercase()) {
        return Ok(Some("company-excluded"));
    }
    if truthy(c.get("regions_ok"))
        && !p.region.is_empty()
        && !region_labels(c)?.contains(&p.region.to_lowercase())
    {
        return Ok(Some("region"));
    }
    if c.get("remote").and_then(Value::as_str) == Some("required") && !p.remote {
        return Ok(Some("not-remote"));
    }
    let locations = lower_list(c, "locations_any");
    if !locations.is_empty()
        && !p.location.is_empty()
        && !p.remote
        && !locations
            .iter()
            .any(|loc| p.location.to_lowercase().contains(loc.as_str()))
    {
        return Ok(Some("location"));
    }
    if let Some(min) = c.get("min_salary").filter(|v| truthy(Some(v))) {
        let min = int(min)?;
        if p.salary_max.is_some_and(|max| max < min) {
            return Ok(Some("salary"));
        }
    }
    Ok(None)
}

pub fn apply(postings: &[Posting], c: &Value) -> Result<(Vec<Posting>, BTreeMap<String, i64>)> {
    let mut kept = Vec::new();
    let mut rejected = BTreeMap::new();
    for posting in postings {
        match reject_reason(posting, c)? {
            Some(reason) => *rejected.entry(reason.to_owned()).or_insert(0) += 1,
            None => kept.push(posting.clone()),
        }
    }
    Ok((kept, rejected))
}
