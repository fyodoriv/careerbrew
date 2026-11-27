//! `careerbrew.jobs.want`: the 0-100 Want score, rating tags and the weight refit.
//!
//! Arithmetic mirrors the Python order of operations exactly (`**` is `powf`).

use crate::sources::{ascii_word, bounded, Posting};
use crate::{float, int, sub, truthy, JobsError, Result};
use careerbrew_projects::python_round;
use careerbrew_tutor::{append_jsonl, emit, now, read_jsonl_abs, read_yaml, stamp_at, write_yaml};
use careerbrew_vault::canon::Canon;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const KEYS: [&str; 6] = [
    "comp",
    "tech_match",
    "growth",
    "learning",
    "remote",
    "company",
];
pub const DEFAULT_WEIGHTS: [f64; 6] = [3.0, 2.0, 2.0, 1.0, 1.0, 2.0];
const TAG_TIMES: i64 = 2;
const TAG_LOW: i64 = 2;
const TAG_HIGH: i64 = 4;
const TAG_DOWN: f64 = 0.85;
const TAG_UP: f64 = 1.1;

pub type Weights = [f64; 6];

pub fn weights_json(w: &Weights) -> Value {
    Value::Object(
        KEYS.iter()
            .zip(w)
            .map(|(key, value)| ((*key).to_owned(), json!(value)))
            .collect::<Map<_, _>>(),
    )
}

pub fn profile(root: &Path) -> Result<Value> {
    read_yaml(root, "profile.yaml", json!({}))
}

pub fn weights(root: &Path) -> Result<Weights> {
    let saved = read_yaml(root, "jobs/weights.yaml", json!({}))?;
    let mut out = DEFAULT_WEIGHTS;
    for (index, key) in KEYS.iter().enumerate() {
        if let Some(value) = saved.get(*key) {
            out[index] = float(value)?;
        }
    }
    Ok(out)
}

fn lower_keys(value: Option<&Value>) -> BTreeSet<String> {
    match value.filter(|v| truthy(Some(v))) {
        Some(Value::Object(map)) => map.keys().map(|k| k.to_lowercase()).collect(),
        Some(other) => careerbrew_tutor::strings(Some(other))
            .iter()
            .map(|k| k.to_lowercase())
            .collect(),
        None => BTreeSet::new(),
    }
}

fn py_min(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

fn py_max(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

pub fn features(p: &Posting, prof: &Value, crit: &Value) -> Result<Weights> {
    let skills = lower_keys(prof.get("skills"));
    let learn = lower_keys(prof.get("want_to_learn"));
    let target = crit
        .get("min_salary")
        .filter(|v| truthy(Some(v)))
        .or_else(|| prof.get("target_salary"))
        .filter(|v| truthy(Some(v)));
    let comp = match (p.salary_max.filter(|v| *v != 0), target) {
        (Some(max), Some(target)) => {
            let target = float(target)?;
            py_max(
                0.0,
                py_min(1.0, 0.5 + (max as f64 - target) / (2.0 * target) * 2.0),
            )
        }
        _ => 0.5,
    };
    let ps = p
        .skills
        .iter()
        .map(|s| s.to_lowercase())
        .collect::<BTreeSet<_>>();
    let tech = if ps.is_empty() {
        0.5
    } else {
        ps.intersection(&skills).count() as f64 / ps.len() as f64
    };
    let level = match prof.get("current_level") {
        Some(value) => int(value)?,
        None => 3,
    };
    let growth = if p.seniority > level {
        1.0
    } else if p.seniority == level {
        0.6
    } else {
        0.2
    };
    let learning = if learn.is_empty() {
        0.5
    } else {
        py_min(1.0, ps.intersection(&learn).count() as f64 / 2.0)
    };
    let preference = prof
        .get("remote_preference")
        .and_then(Value::as_str)
        .unwrap_or("any");
    let remote = if (p.remote && preference != "office") || preference == "any" {
        1.0
    } else {
        0.3
    };
    let company = match crit
        .get("company_scores")
        .filter(|v| truthy(Some(v)))
        .and_then(|scores| scores.get(&p.company))
    {
        Some(value) => float(value)?,
        None => 0.5,
    };
    Ok([comp, tech, growth, learning, remote, company])
}

pub fn features_json(f: &Weights) -> Value {
    weights_json(f)
}

pub fn score(
    p: &Posting,
    w: &Weights,
    prof: &Value,
    crit: &Value,
    tags: &Tags,
) -> Result<(f64, Weights)> {
    let f = features(p, prof, crit)?;
    let mut total = w.iter().fold(0.0, |sum, v| sum + v);
    if total == 0.0 {
        total = 1.0;
    }
    let weighted = w.iter().zip(&f).fold(0.0, |sum, (wi, fi)| sum + wi * fi);
    Ok((
        python_round(100.0 * weighted / total * tag_factor(p, tags), 1),
        f,
    ))
}

/// `(disliked, liked)` rating tags.
pub type Tags = (BTreeSet<String>, BTreeSet<String>);

fn ratings_path(root: &Path) -> Result<std::path::PathBuf> {
    sub(root, "jobs/ratings.jsonl")
}

/// The latest row per posting id, in first-seen order.
fn latest_rows(root: &Path) -> Result<Vec<(String, Value)>> {
    let mut latest: Vec<(String, Value)> = Vec::new();
    for row in read_jsonl_abs(&ratings_path(root)?)? {
        let id = crate::py_str(row.get("id"));
        match latest.iter_mut().find(|(key, _)| *key == id) {
            Some(slot) => slot.1 = row,
            None => latest.push((id, row)),
        }
    }
    Ok(latest)
}

pub fn tag_words(root: &Path) -> Result<Tags> {
    let mut low: BTreeMap<String, i64> = BTreeMap::new();
    let mut high: BTreeMap<String, i64> = BTreeMap::new();
    for (_, row) in latest_rows(root)? {
        let rating = int(row.get("rating").unwrap_or(&Value::Null))?;
        let bucket = if rating <= TAG_LOW {
            Some(&mut low)
        } else if rating >= TAG_HIGH {
            Some(&mut high)
        } else {
            None
        };
        if let Some(bucket) = bucket {
            let tags = row.get("tags").filter(|v| truthy(Some(v)));
            for tag in tags.and_then(Value::as_array).into_iter().flatten() {
                let tag = crate::py_str(Some(tag)).trim().to_lowercase();
                if !tag.is_empty() {
                    *bucket.entry(tag).or_insert(0) += 1;
                }
            }
        }
    }
    let disliked = low
        .into_iter()
        .filter(|(_, n)| *n >= TAG_TIMES)
        .map(|(t, _)| t)
        .collect::<BTreeSet<_>>();
    let liked = high
        .into_iter()
        .filter(|(t, n)| *n >= TAG_TIMES && !disliked.contains(t))
        .map(|(t, _)| t)
        .collect();
    Ok((disliked, liked))
}

pub fn tag_factor(p: &Posting, tags: &Tags) -> f64 {
    let (disliked, liked) = tags;
    if disliked.is_empty() && liked.is_empty() {
        return 1.0;
    }
    let mut parts = vec![p.title.clone(), p.company.clone(), p.location.clone()];
    parts.extend(p.skills.iter().cloned());
    let text = parts.join(" ").to_lowercase();
    let count =
        |set: &BTreeSet<String>| set.iter().filter(|t| bounded(&text, t, ascii_word)).count();
    python_round(
        TAG_DOWN.powf(count(disliked) as f64) * TAG_UP.powf(count(liked) as f64),
        4,
    )
}

pub fn rate(root: &Path, posting_id: &str, rating: i64, tags: &[&str]) -> Result<()> {
    if !(1..=5).contains(&rating) {
        return Err(JobsError::value("rating must be 1..5"));
    }
    ratings_path(root)?;
    append_jsonl(
        root,
        "jobs/ratings.jsonl",
        &[
            json!({"id": posting_id, "rating": rating, "tags": tags, "at": stamp_at(now()), "origin": "user"}),
        ],
    )?;
    emit(root, "jobs.rate", vec![("status", Canon::Str("ok".into()))])
}

pub fn ratings(root: &Path) -> Result<BTreeMap<String, i64>> {
    let mut out = BTreeMap::new();
    for row in read_jsonl_abs(&ratings_path(root)?)? {
        out.insert(
            crate::py_str(row.get("id")),
            int(row.get("rating").unwrap_or(&Value::Null))?,
        );
    }
    Ok(out)
}

pub fn rank(xs: &[f64]) -> Vec<f64> {
    let mut order = (0..xs.len()).collect::<Vec<_>>();
    order.sort_by(|a, b| {
        xs[*a]
            .partial_cmp(&xs[*b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut ranks = vec![0.0; xs.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && xs[order[j + 1]] == xs[order[i]] {
            j += 1;
        }
        for k in i..=j {
            ranks[order[k]] = (i + j) as f64 / 2.0;
        }
        i = j + 1;
    }
    ranks
}

pub fn spearman(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.len() < 3 {
        return None;
    }
    let (ra, rb) = (rank(a), rank(b));
    let mean = |r: &[f64]| r.iter().fold(0.0, |s, x| s + x) / r.len() as f64;
    let (ma, mb) = (mean(&ra), mean(&rb));
    let num = ra
        .iter()
        .zip(&rb)
        .fold(0.0, |s, (x, y)| s + (x - ma) * (y - mb));
    let sa = ra.iter().fold(0.0, |s, x| s + (x - ma).powf(2.0));
    let sb = rb.iter().fold(0.0, |s, y| s + (y - mb).powf(2.0));
    let den = (sa * sb).powf(0.5);
    (den != 0.0).then(|| python_round(num / den, 3))
}

pub fn agreement(
    root: &Path,
    postings: &[Posting],
    w: &Weights,
    prof: &Value,
    crit: &Value,
) -> Result<Option<f64>> {
    let r = ratings(root)?;
    let tags = tag_words(root)?;
    let mut scores = Vec::new();
    let mut given = Vec::new();
    for posting in postings.iter().filter(|p| r.contains_key(&p.id)) {
        scores.push(score(posting, w, prof, crit, &tags)?.0);
        given.push(r[&posting.id] as f64);
    }
    Ok(spearman(&scores, &given))
}

fn option(value: Option<f64>) -> Value {
    value.map_or(Value::Null, |v| json!(v))
}

pub fn learn_weights(
    root: &Path,
    postings: &[Posting],
    prof: &Value,
    crit: &Value,
    steps: usize,
    lr: f64,
) -> Result<Value> {
    let r = ratings(root)?;
    let rated = postings
        .iter()
        .filter(|p| r.contains_key(&p.id))
        .collect::<Vec<_>>();
    if rated.len() < 5 {
        return Ok(json!({"status": "need-more-ratings", "rated": rated.len()}));
    }
    let before = agreement(root, postings, &weights(root)?, prof, crit)?;
    let current = weights(root)?;
    let sum = current.iter().fold(0.0, |s, v| s + v);
    let mut w = current.map(|v| v / sum);
    let xs = rated
        .iter()
        .map(|p| features(p, prof, crit))
        .collect::<Result<Vec<_>>>()?;
    let ys = rated
        .iter()
        .map(|p| (r[&p.id] - 1) as f64 / 4.0)
        .collect::<Vec<_>>();
    let n = xs.len() as f64;
    for _ in 0..steps {
        let mut grad = [0.0; 6];
        for (x, y) in xs.iter().zip(&ys) {
            let err = w.iter().zip(x).fold(0.0, |s, (wi, xi)| s + wi * xi) - y;
            for i in 0..KEYS.len() {
                grad[i] += 2.0 * err * x[i] / n;
            }
        }
        for i in 0..KEYS.len() {
            w[i] = py_max(0.0, w[i] - lr * grad[i]);
        }
    }
    let mut total = w.iter().fold(0.0, |s, v| s + v);
    if total == 0.0 {
        total = 1.0;
    }
    let new = w.map(|wi| python_round(10.0 * wi / total, 3));
    let after = agreement(root, postings, &new, prof, crit)?;
    let kept = after.is_some() && (before.is_none() || after >= before);
    if kept {
        write_yaml(root, "jobs/weights.yaml", &weights_json(&new))?;
    }
    append_jsonl(
        root,
        "metrics.jsonl",
        &[
            json!({"metric": "want_agreement", "value": option(if kept { after } else { before }), "at": stamp_at(now())}),
        ],
    )?;
    let status = if kept { "kept" } else { "reverted" };
    emit(
        root,
        "jobs.learn",
        vec![("status", Canon::Str(status.into()))],
    )?;
    let shown = if kept { new } else { weights(root)? };
    Ok(
        json!({"status": status, "before": option(before), "after": option(after), "weights": weights_json(&shown)}),
    )
}
