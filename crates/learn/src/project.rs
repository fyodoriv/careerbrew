//! Staged sources, cited concepts, mastery and the next learning step.

use crate::{
    append_jsonl, cards, error, field, now, read_json, read_jsonl, read_text, read_yaml, stamp,
    stamp_at, strings, text, write_json, write_text, write_yaml, Result,
};
use chrono::{DateTime, Duration, FixedOffset};
use regex::Regex;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const KINDS: &[&str] = &["repo", "doc", "web", "context", "file"];
pub const BLOOM: &[&str] = &[
    "remember",
    "understand",
    "apply",
    "analyze",
    "evaluate",
    "create",
];
pub const RESULTS: &[&str] = &["correct", "partial", "wrong", "dontknow"];
pub const MODES: &[&str] = &[
    "pretest", "teach", "quiz", "recall", "review", "retest", "task",
];
pub const RECALL_CAP: f64 = 0.3;
pub const REPO_DOCS: &[&str] = &[
    "README*",
    "AGENTS.md",
    "ARCHITECTURE*",
    "CONTRIBUTING*",
    "docs/**/*.md",
];

pub fn slug(value: &str) -> String {
    let found = Regex::new(r"[^a-z0-9]+")
        .expect("slug regex")
        .replace_all(&value.to_lowercase(), "-")
        .trim_matches('-')
        .to_owned();
    if found.is_empty() {
        "project".into()
    } else {
        found
    }
}

pub fn folder(root: &Path, project: &str) -> PathBuf {
    root.join("learn").join(slug(project))
}

fn ensure_folder(root: &Path, project: &str) -> Result<()> {
    std::fs::create_dir_all(folder(root, project))?;
    Ok(())
}

pub fn manifest(root: &Path, project: &str) -> Result<Value> {
    ensure_folder(root, project)?;
    read_json(
        root,
        &format!("learn/{}/manifest.json", slug(project)),
        json!({}),
    )
}

pub fn projects(root: &Path) -> Result<Vec<String>> {
    let base = root.join("learn");
    let mut out = Vec::new();
    if base.is_dir() {
        for entry in std::fs::read_dir(base)? {
            let path = entry?.path();
            if path.join("manifest.json").exists() {
                out.push(
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    out.sort();
    Ok(out)
}

fn git(repo: &Path, args: &[&str]) -> String {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default()
}

fn repo_docs(repo: &Path) -> Result<Vec<PathBuf>> {
    let mut found = BTreeSet::new();
    for pattern in REPO_DOCS {
        let full = repo.join(pattern).to_string_lossy().into_owned();
        for path in glob::glob(&full)
            .map_err(|_| error("ValueError", "invalid repository document pattern"))?
        {
            let path = path.map_err(|_| error("OSError", "cannot read repository document"))?;
            if path.is_file() && !path.components().any(|part| part.as_os_str() == ".git") {
                found.insert(path);
            }
        }
    }
    Ok(found.into_iter().collect())
}

struct Source {
    record: Value,
    origin: Option<PathBuf>,
    data: Option<Vec<u8>>,
}

fn source(kind: &str, spec: &str) -> Result<Source> {
    if kind == "repo" {
        let expanded = if let Some(rest) = spec.strip_prefix("~/") {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(rest)
        } else {
            PathBuf::from(spec)
        };
        let repo = expanded.canonicalize().map_err(|_| {
            error(
                "FileNotFoundError",
                format!(
                    "no folder {}: pass a local checkout with --repo",
                    expanded.display()
                ),
            )
        })?;
        if !repo.is_dir() {
            return Err(error(
                "FileNotFoundError",
                format!(
                    "no folder {}: pass a local checkout with --repo",
                    repo.display()
                ),
            ));
        }
        let docs = repo_docs(&repo)?;
        let mut hash = Sha256::new();
        for path in &docs {
            hash.update(
                path.strip_prefix(&repo)
                    .map_err(|_| error("ValueError", "repository document escaped"))?
                    .to_string_lossy()
                    .as_bytes(),
            );
            hash.update([0]);
            hash.update(std::fs::read(path)?);
        }
        let name = repo
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        return Ok(Source {
            record: json!({"key": format!("repo:{name}"), "kind": "repo", "label": name, "path": repo, "sha": git(&repo, &["rev-parse", "HEAD"]), "hash": format!("{:x}", hash.finalize()), "docs": docs.len()}),
            origin: None,
            data: None,
        });
    }
    let pair = if kind == "web" {
        spec.rsplit_once(':')
    } else {
        spec.split_once(':')
    };
    let (label, path) = pair.ok_or_else(|| {
        error(
            "ValueError",
            format!("--{kind} takes LABEL:PATH (the file you fetched), not {spec:?}"),
        )
    })?;
    if label.is_empty() || path.is_empty() {
        return Err(error(
            "ValueError",
            format!("--{kind} takes LABEL:PATH (the file you fetched), not {spec:?}"),
        ));
    }
    let src = if let Some(rest) = path.strip_prefix("~/") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(rest)
    } else {
        PathBuf::from(path)
    };
    if !src.is_file() {
        return Err(error(
            "FileNotFoundError",
            format!("no file {} for --{kind} {label}", src.display()),
        ));
    }
    let data = std::fs::read(&src)?;
    let suffix = src
        .extension()
        .map(|value| format!(".{}", value.to_string_lossy()))
        .unwrap_or_else(|| ".md".into());
    Ok(Source {
        record: json!({"key": format!("{kind}:{}", slug(label)), "kind": kind, "label": label, "file": format!("inputs/{kind}/{}{suffix}", slug(label)), "hash": format!("{:x}", Sha256::digest(&data))}),
        origin: Some(src),
        data: Some(data),
    })
}

pub fn start(
    root: &Path,
    project: &str,
    specs: &[(String, String)],
    consume: bool,
) -> Result<Value> {
    let old_manifest = manifest(root, project)?;
    if specs.is_empty() && old_manifest.as_object().is_none_or(|rows| rows.is_empty()) {
        return Err(error("ValueError", "name at least one source: --repo PATH, --doc LABEL:FILE, --web URL:FILE, --context LABEL:FILE, or --file LABEL:FILE"));
    }
    let base = folder(root, project);
    let mut new = old_manifest
        .get("sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|source| (text(field(source, "key")), source.clone()))
        .collect::<BTreeMap<_, _>>();
    let old = new.clone();
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();
    for (kind, spec) in specs {
        if !KINDS.contains(&kind.as_str()) {
            return Err(error("ValueError", format!("unknown source kind {kind:?}")));
        }
        let mut source = source(kind, spec)?;
        let key = text(field(&source.record, "key"));
        let same =
            old.get(&key).and_then(|row| field(row, "hash")) == field(&source.record, "hash");
        if same {
            unchanged.push(key.clone())
        } else {
            changed.push(key.clone())
        }
        if let (Some(origin), Some(data)) = (source.origin.take(), source.data.take()) {
            if !same {
                let content = String::from_utf8_lossy(&data).into_owned();
                let relative = text(field(&source.record, "file"));
                write_text(&base.join(relative), &content)?;
            }
            if consume {
                std::fs::remove_file(origin)?;
            }
        }
        source.record["staged_at"] = json!(stamp());
        if same {
            let mut retained = old[&key].clone();
            retained["staged_at"] = source.record["staged_at"].clone();
            new.insert(key, retained);
        } else {
            new.insert(key, source.record);
        }
    }
    let stale = strings(field(&old_manifest, "stale"))
        .into_iter()
        .chain(changed.iter().filter(|key| old.contains_key(*key)).cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let sources = new.values().cloned().collect::<Vec<_>>();
    let answer = json!({"project": project, "sources": sources, "freshness": {"changed": changed, "unchanged": unchanged}, "stale": stale});
    write_json(
        root,
        &format!("learn/{}/manifest.json", slug(project)),
        &answer,
    )?;
    Ok(answer)
}

pub fn note(root: &Path, project: &str, relative: &str, value: Option<&str>) -> Result<String> {
    if manifest(root, project)?
        .as_object()
        .is_none_or(|rows| rows.is_empty())
    {
        return Err(error("KeyError", format!("no learn project {project:?}; start with `careerbrew learn start {project} --repo PATH`")));
    }
    let relative = relative.trim_matches('/');
    let path = Path::new(relative);
    if path.components().any(|part| part.as_os_str() == "..") || !relative.ends_with(".md") {
        return Err(error(
            "ValueError",
            "a note path is relative and ends in .md, e.g. 01-overview/data-flow.md",
        ));
    }
    let target = folder(root, project).join("notes").join(path);
    if let Some(value) = value {
        write_text(&target, value)?;
        return Ok(value.to_owned());
    }
    read_text(&target)?.ok_or_else(|| {
        error(
            "FileNotFoundError",
            format!("no note {relative} in {project}"),
        )
    })
}

fn notes(root: &Path, project: &str) -> Result<BTreeSet<String>> {
    let base = folder(root, project).join("notes");
    let mut out = BTreeSet::new();
    if base.is_dir() {
        for entry in glob::glob(&base.join("**/*.md").to_string_lossy())
            .map_err(|_| error("ValueError", "invalid note pattern"))?
        {
            let path = entry.map_err(|_| error("OSError", "cannot read notes"))?;
            out.insert(
                path.strip_prefix(&base)
                    .map_err(|_| error("ValueError", "note escaped"))?
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    Ok(out)
}

fn cite_ok(cite: &str, manifest: &Value, notes: &BTreeSet<String>) -> bool {
    if notes.contains(cite.strip_prefix("note:").unwrap_or(cite)) {
        return true;
    }
    for source in manifest
        .get("sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let key = text(field(source, "key"));
        if cite == key || Some(cite) == field(source, "file").and_then(Value::as_str) {
            return true;
        }
        if text(field(source, "kind")) == "repo" && cite.starts_with(&(key.clone() + "/")) {
            let Ok(repo) = PathBuf::from(text(field(source, "path"))).canonicalize() else {
                return false;
            };
            let leaf = cite[key.len() + 1..]
                .split('#')
                .next()
                .unwrap_or("")
                .split(':')
                .next()
                .unwrap_or("");
            let Ok(target) = repo.join(leaf).canonicalize() else {
                return false;
            };
            return target.starts_with(repo) && target.exists();
        }
    }
    false
}

fn order(rows: &[Value]) -> Option<Vec<String>> {
    let ids = rows
        .iter()
        .map(|row| text(field(row, "id")))
        .collect::<Vec<_>>();
    let mut done = Vec::new();
    let mut seen = BTreeSet::new();
    while done.len() < rows.len() {
        let ready = ids
            .iter()
            .find(|id| {
                !seen.contains(*id)
                    && rows
                        .iter()
                        .find(|row| text(field(row, "id")) == ***id)
                        .is_some_and(|row| {
                            strings(field(row, "prereqs"))
                                .iter()
                                .all(|parent| seen.contains(parent) || !ids.contains(parent))
                        })
            })
            .cloned();
        let ready = ready?;
        seen.insert(ready.clone());
        done.push(ready);
    }
    Some(done)
}

pub fn check_concepts(root: &Path, project: &str, concepts: &[Value]) -> Result<Vec<String>> {
    let manifest = manifest(root, project)?;
    let notes = notes(root, project)?;
    let mut problems = Vec::new();
    let ids = concepts
        .iter()
        .map(|row| text(field(row, "id")))
        .collect::<Vec<_>>();
    let mut duplicates = BTreeSet::new();
    for id in &ids {
        if id.is_empty() || ids.iter().filter(|known| *known == id).count() > 1 {
            duplicates.insert(id.clone());
        }
    }
    for id in duplicates {
        problems.push(format!(
            "concept id {} is missing or repeated",
            if id.is_empty() { "(empty)" } else { &id }
        ));
    }
    let known = ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut questions = 0_u64;
    let mut recall = 0_u64;
    for concept in concepts {
        let id = text(field(concept, "id"));
        let id = if id.is_empty() { "?" } else { &id };
        for key in ["section", "title"] {
            if text(field(concept, key)).trim().is_empty() {
                problems.push(format!("{id}: needs a {key}"));
            }
        }
        for parent in strings(field(concept, "prereqs")) {
            if !known.contains(&parent) {
                problems.push(format!("{id}: prerequisite {parent} is not a concept"));
            }
        }
        let cites = strings(field(concept, "cites"));
        if cites.is_empty() {
            problems.push(format!(
                "{id}: cite a note or a staged source (cite or refuse)"
            ));
        }
        for cite in cites {
            if !cite_ok(&cite, &manifest, &notes) {
                problems.push(format!(
                    "{id}: citation {cite} is not a note or a staged source"
                ));
            }
        }
        let questions_rows = field(concept, "questions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if questions_rows.is_empty() {
            problems.push(format!("{id}: needs at least one question"));
        }
        for question in questions_rows {
            questions += 1;
            let level = text(field(&question, "bloom"));
            recall += u64::from(level == "remember");
            if !BLOOM.contains(&level.as_str()) {
                problems.push(format!(
                    "{id}: question bloom must be one of {}",
                    BLOOM.join(", ")
                ));
            }
            if text(field(&question, "prompt")).trim().is_empty()
                || text(field(&question, "answer")).trim().is_empty()
            {
                problems.push(format!("{id}: each question needs a prompt and an answer"));
            }
            let options = field(&question, "options").and_then(Value::as_array);
            if options.is_some_and(|rows| !rows.is_empty())
                && field(&question, "misconceptions")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len)
                    != options.unwrap().len() - 1
            {
                problems.push(format!(
                    "{id}: tag each wrong option with the misconception it targets"
                ));
            }
        }
    }
    if questions > 0 && recall as f64 / questions as f64 > RECALL_CAP {
        problems.push(format!(
            "{recall} of {questions} questions are pure recall (remember); keep it at most 30%"
        ));
    }
    if order(concepts).is_none() {
        problems
            .push("prerequisites form a cycle; the study order needs a line, not a loop".into());
    }
    Ok(problems)
}

pub fn concepts(root: &Path, project: &str) -> Result<Vec<Value>> {
    ensure_folder(root, project)?;
    let data = read_yaml(
        root,
        &format!("learn/{}/concepts.yaml", slug(project)),
        json!({}),
    )?;
    Ok(field(&data, "concepts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

pub fn model(root: &Path, project: &str) -> Result<Value> {
    ensure_folder(root, project)?;
    read_json(
        root,
        &format!("learn/{}/model.json", slug(project)),
        json!({}),
    )
}

pub fn log(root: &Path, project: &str) -> Result<Vec<Value>> {
    ensure_folder(root, project)?;
    read_jsonl(root, &format!("learn/{}/log.jsonl", slug(project)))
}

pub fn import_concepts(root: &Path, project: &str, rows: &[Value]) -> Result<Value> {
    let problems = check_concepts(root, project, rows)?;
    if !problems.is_empty() {
        return Err(error(
            "ValueError",
            format!("concepts not imported:\n  {}", problems.join("\n  ")),
        ));
    }
    write_yaml(
        root,
        &format!("learn/{}/concepts.yaml", slug(project)),
        &json!({"concepts": rows}),
    )?;
    let manifest_value = manifest(root, project)?;
    if !strings(field(&manifest_value, "stale")).is_empty() {
        let mut updated = manifest_value;
        updated["stale"] = json!([]);
        write_json(
            root,
            &format!("learn/{}/manifest.json", slug(project)),
            &updated,
        )?;
    }
    let mut model = model(root, project)?;
    let ids = rows
        .iter()
        .map(|row| text(field(row, "id")))
        .collect::<BTreeSet<_>>();
    let retests = model
        .get("retests")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if retests.keys().any(|id| !ids.contains(id)) {
        model["retests"] = Value::Object(
            retests
                .into_iter()
                .filter(|(id, _)| ids.contains(id))
                .collect(),
        );
        write_json(root, &format!("learn/{}/model.json", slug(project)), &model)?;
    }
    let mut deck = Vec::new();
    for concept in rows {
        let questions = field(concept, "questions")
            .and_then(Value::as_array)
            .expect("validated questions");
        let question = questions
            .iter()
            .find(|question| {
                text(field(question, "kind")) == "open" || field(question, "kind").is_none()
            })
            .unwrap_or(&questions[0]);
        deck.push(json!({"id": text(field(concept, "id")), "question": text(field(question, "prompt")), "answer": text(field(question, "answer")), "tags": [text(field(concept, "section"))], "source": strings(field(concept, "cites")).join(", ")}));
    }
    cards::write_deck(root, &folder(root, project), &deck)?;
    Ok(
        json!({"concepts": rows.len(), "sections": rows.iter().map(|row| text(field(row, "section"))).collect::<BTreeSet<_>>().len()}),
    )
}

fn counts(rows: &[Value]) -> Vec<Value> {
    let misses = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            ["retest", "review"].contains(&text(field(row, "mode")).as_str())
                && ["wrong", "dontknow"].contains(&text(field(row, "result")).as_str())
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let start = misses.last().map_or(0, |index| index + 1);
    rows[start..]
        .iter()
        .filter(|row| {
            text(field(row, "mode")) != "pretest"
                && text(field(row, "result")) == "correct"
                && text(field(row, "confidence")) != "low"
        })
        .cloned()
        .collect()
}

pub fn mastered(rows: &[Value]) -> bool {
    let good = counts(rows);
    good.len() >= 2
        && good
            .iter()
            .any(|row| bloom_index(&text(field(row, "level"))) >= 2)
}

fn bloom_index(level: &str) -> usize {
    BLOOM.iter().position(|known| *known == level).unwrap_or(0)
}

fn best(rows: &[Value]) -> Option<String> {
    counts(rows)
        .iter()
        .map(|row| text(field(row, "level")))
        .max_by_key(|level| bloom_index(level))
}

pub fn by_concept(log: &[Value]) -> BTreeMap<String, Vec<Value>> {
    let mut out = BTreeMap::<String, Vec<Value>>::new();
    for row in log {
        out.entry(text(field(row, "concept")))
            .or_default()
            .push(row.clone());
    }
    out
}

fn confidence(value: &str) -> f64 {
    match value {
        "low" => 0.33,
        "medium" => 0.66,
        _ => 0.9,
    }
}

fn brier(rows: &[Value]) -> Value {
    let scored = rows
        .iter()
        .filter(|row| text(field(row, "result")) != "partial")
        .collect::<Vec<_>>();
    if scored.is_empty() {
        return Value::Null;
    }
    let total = scored
        .iter()
        .map(|row| {
            let correct = if text(field(row, "result")) == "correct" {
                1.0
            } else {
                0.0
            };
            (confidence(&text(field(row, "confidence"))) - correct).powi(2)
        })
        .sum::<f64>();
    json!(careerbrew_projects::python_round(
        total / scored.len() as f64,
        3
    ))
}

// Keep the Python answer API's explicit learning dimensions in one call.
#[allow(clippy::too_many_arguments)]
pub fn answer(
    root: &Path,
    project: &str,
    concept: &str,
    result: &str,
    confidence: &str,
    level: &str,
    mode: &str,
    kind: &str,
    seconds: Option<i64>,
    at: Option<DateTime<FixedOffset>>,
) -> Result<Value> {
    let at = at.unwrap_or_else(now);
    if !RESULTS.contains(&result)
        || !["low", "medium", "high"].contains(&confidence)
        || !BLOOM.contains(&level)
        || !MODES.contains(&mode)
    {
        return Err(error(
            "ValueError",
            "invalid result, confidence, level or mode",
        ));
    }
    let concepts = concepts(root, project)?;
    let by_id = concepts
        .iter()
        .map(|row| (text(field(row, "id")), row))
        .collect::<BTreeMap<_, _>>();
    let found = by_id.get(concept).ok_or_else(|| {
        error(
            "KeyError",
            format!(
                "unknown concept {concept} in {project}; see `careerbrew learn status {project}`"
            ),
        )
    })?;
    let mut model = model(root, project)?;
    let history = log(root, project)?
        .into_iter()
        .filter(|row| text(field(row, "concept")) == concept)
        .collect::<Vec<_>>();
    let mut row = json!({"concept": concept, "section": text(field(found, "section")), "result": result, "confidence": confidence, "level": level, "mode": mode, "kind": kind, "at": stamp_at(at), "seconds": seconds});
    let mastered_now = mastered(
        &history
            .iter()
            .cloned()
            .chain([row.clone()])
            .collect::<Vec<_>>(),
    );
    if model.get("retests").is_none() {
        model["retests"] = json!({});
    }
    if mode == "retest" {
        if let Some(pending) = model["retests"][concept].as_array_mut().and_then(|rows| {
            rows.iter_mut().find(|retest| {
                field(retest, "result").is_none() && text(field(retest, "due")) <= stamp_at(at)
            })
        }) {
            pending["result"] = json!(result);
        }
    }
    if !mastered_now
        && !model["retests"][concept].is_null()
        && ["retest", "review"].contains(&mode)
        && ["wrong", "dontknow"].contains(&result)
    {
        let old = model["retests"]
            .as_object_mut()
            .expect("retests")
            .remove(concept)
            .unwrap_or_else(|| json!([]));
        let completed = old
            .as_array()
            .into_iter()
            .flatten()
            .filter(|retest| field(retest, "result").is_some())
            .cloned()
            .collect::<Vec<_>>();
        if !model["retest_log"].is_array() {
            model["retest_log"] = json!([]);
        }
        model["retest_log"]
            .as_array_mut()
            .expect("retest log")
            .extend(completed);
    }
    if mastered_now && model["retests"][concept].is_null() {
        model["retests"][concept] = json!([
            {"kind": "+1d", "due": stamp_at(at + Duration::days(1))},
            {"kind": "+7d", "due": stamp_at(at + Duration::days(7))}
        ]);
    }
    if mode != "pretest" {
        let rating = if ["wrong", "dontknow"].contains(&result) {
            "again"
        } else if result == "partial" || confidence == "low" {
            "hard"
        } else if confidence == "high" && bloom_index(level) >= 2 {
            "easy"
        } else {
            "good"
        };
        let cards_root = folder(root, project);
        if cards::is_suspended(root, &cards_root, concept)? {
            cards::unsuspend(root, &cards_root, concept)?;
        }
        row["leech"] =
            json!(cards::review(root, &cards_root, concept, rating, at, seconds)?["leech"]);
    }
    write_json(root, &format!("learn/{}/model.json", slug(project)), &model)?;
    row["mastered"] = json!(mastered_now);
    append_jsonl(
        root,
        &format!("learn/{}/log.jsonl", slug(project)),
        &[row.clone()],
    )?;
    Ok(row)
}

pub fn status(root: &Path, project: &str, at: Option<DateTime<FixedOffset>>) -> Result<Value> {
    let at = at.unwrap_or_else(now);
    let concepts = concepts(root, project)?;
    let model = model(root, project)?;
    let log = log(root, project)?;
    let manifest = manifest(root, project)?;
    if manifest.as_object().is_none_or(|rows| rows.is_empty()) {
        return Err(error("KeyError", format!("no learn project {project:?}; start with `careerbrew learn start {project} --repo PATH`")));
    }
    let rows = by_concept(&log);
    let mut sections = Map::new();
    for concept in &concepts {
        let section = text(field(concept, "section"));
        let mut row = sections
            .get(&section)
            .cloned()
            .unwrap_or_else(|| json!({"concepts": 0, "mastered": 0, "remaining": []}));
        row["concepts"] = json!(row["concepts"].as_u64().unwrap_or(0) + 1);
        let id = text(field(concept, "id"));
        if mastered(rows.get(&id).map_or(&[], Vec::as_slice)) {
            row["mastered"] = json!(row["mastered"].as_u64().unwrap_or(0) + 1);
        } else {
            row["remaining"]
                .as_array_mut()
                .expect("remaining")
                .push(json!(id));
        }
        sections.insert(section, row);
    }
    for (_, row) in &mut sections {
        row["known_pct"] = json!((100.0 * row["mastered"].as_f64().unwrap_or(0.0)
            / row["concepts"].as_f64().unwrap_or(1.0))
        .round_ties_even() as u64);
    }
    let cards_root = folder(root, project);
    let srs = cards::stats(root, &cards_root, at)?;
    let scheduled = cards::scheduled(root, &cards_root)?;
    let taught = log
        .iter()
        .filter(|row| text(field(row, "mode")) != "pretest")
        .map(|row| text(field(row, "concept")))
        .collect::<BTreeSet<_>>();
    let ids = concepts
        .iter()
        .map(|row| text(field(row, "id")))
        .collect::<BTreeSet<_>>();
    let retests = model
        .get("retests")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(id, _)| ids.contains(*id))
        .flat_map(|(_, rows)| rows.as_array().into_iter().flatten())
        .cloned()
        .collect::<Vec<_>>();
    let week = retests
        .iter()
        .chain(
            model
                .get("retest_log")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
        .filter(|row| text(field(row, "kind")) == "+7d" && field(row, "result").is_some())
        .collect::<Vec<_>>();
    let retained = if week.is_empty() {
        Value::Null
    } else {
        json!(careerbrew_projects::python_round(
            week.iter()
                .filter(|row| text(field(row, "result")) == "correct")
                .count() as f64
                / week.len() as f64,
            3
        ))
    };
    let scheduled_share = if taught.is_empty() {
        Value::Null
    } else {
        json!(careerbrew_projects::python_round(
            taught.iter().filter(|id| scheduled.contains(*id)).count() as f64 / taught.len() as f64,
            3
        ))
    };
    Ok(json!({
        "project": project, "concepts": concepts.len(), "mastered": concepts.iter().filter(|concept| mastered(rows.get(&text(field(concept, "id"))).map_or(&[], Vec::as_slice))).count(),
        "sections": sections, "taught": taught.len(), "scheduled_share": scheduled_share,
        "due_reviews": srs["due_now"], "due_retests": retests.iter().filter(|row| field(row, "result").is_none() && text(field(row, "due")) <= stamp_at(at)).count(),
        "retention_7d": srs["retention_7d"], "retest_retained_7d": retained,
        "brier": brier(&log.iter().filter(|row| text(field(row, "mode")) != "pretest").cloned().collect::<Vec<_>>()),
        "answers": log.len(), "sources": manifest.get("sources").and_then(Value::as_array).map_or(0, Vec::len), "stale": field(&manifest, "stale").cloned().unwrap_or_else(|| json!([])),
    }))
}

fn step(concept: &Value, mode: &str, why: &str, rows: &BTreeMap<String, Vec<Value>>) -> Value {
    let id = text(field(concept, "id"));
    let best = best(rows.get(&id).map_or(&[], Vec::as_slice));
    let mut target = best.as_deref().map_or("understand", |best| {
        BLOOM[(bloom_index(best) + 1).min(BLOOM.len() - 1)]
    });
    if ["retest", "review"].contains(&mode) {
        target = best.as_deref().unwrap_or("understand");
    }
    let mut questions = field(concept, "questions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    questions.sort_by_key(|question| {
        bloom_index(&text(field(question, "bloom"))).abs_diff(bloom_index(target))
    });
    json!({"mode": mode, "why": why, "concept": id, "section": text(field(concept, "section")), "title": text(field(concept, "title")), "level": target, "question": questions[0], "cites": field(concept, "cites").cloned().unwrap_or_else(|| json!([]))})
}

pub fn next_step(root: &Path, project: &str, at: Option<DateTime<FixedOffset>>) -> Result<Value> {
    let at = at.unwrap_or_else(now);
    let concepts = concepts(root, project)?;
    let model = model(root, project)?;
    let log = log(root, project)?;
    if concepts.is_empty() {
        return Ok(
            json!({"mode": "build", "why": "no concepts yet: write notes, then `careerbrew learn concepts` (skills/careerbrew-learn-project)"}),
        );
    }
    let by_id = concepts
        .iter()
        .map(|row| (text(field(row, "id")), row.clone()))
        .collect::<BTreeMap<_, _>>();
    let rows = by_concept(&log);
    let mut retests = model
        .get("retests")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    retests.sort_by(|left, right| left.0.cmp(right.0));
    for (id, schedule) in retests {
        let Some(concept) = by_id.get(id) else {
            continue;
        };
        for retest in schedule.as_array().into_iter().flatten() {
            if field(retest, "result").is_none() && text(field(retest, "due")) <= stamp_at(at) {
                return Ok(step(
                    concept,
                    "retest",
                    &format!(
                        "{} retest of a mastered concept, in new words",
                        text(field(retest, "kind"))
                    ),
                    &rows,
                ));
            }
        }
    }
    let due = cards::review_queue(root, &folder(root, project), at)?;
    if let Some(card) = due.first() {
        return Ok(step(
            &by_id[&text(field(card, "id"))],
            "review",
            "a spaced review is due",
            &rows,
        ));
    }
    let touched = log
        .iter()
        .map(|row| text(field(row, "section")))
        .collect::<BTreeSet<_>>();
    let order = order(&concepts)
        .unwrap_or_else(|| concepts.iter().map(|row| text(field(row, "id"))).collect());
    for id in &order {
        let concept = &by_id[id];
        if !touched.contains(&text(field(concept, "section"))) {
            return Ok(step(
                concept,
                "pretest",
                "a new section starts with an unscored pretest: predict before you read",
                &rows,
            ));
        }
    }
    let open = order
        .iter()
        .filter(|id| !mastered(rows.get(*id).map_or(&[], Vec::as_slice)))
        .cloned()
        .collect::<Vec<_>>();
    if open.is_empty() {
        return Ok(
            json!({"mode": "done", "why": "every concept is mastered; reviews and retests come back on their own"}),
        );
    }
    let mut weak = BTreeMap::<String, Vec<f64>>::new();
    for concept in &concepts {
        weak.entry(text(field(concept, "section")))
            .or_default()
            .push(
                if mastered(
                    rows.get(&text(field(concept, "id")))
                        .map_or(&[], Vec::as_slice),
                ) {
                    1.0
                } else {
                    0.0
                },
            );
    }
    let mut sections = weak.keys().cloned().collect::<Vec<_>>();
    sections.sort_by(|left, right| {
        let average = |id: &String| weak[id].iter().sum::<f64>() / weak[id].len() as f64;
        average(left)
            .partial_cmp(&average(right))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for section in sections {
        for id in &open {
            let concept = &by_id[id];
            let blocked = strings(field(concept, "prereqs"))
                .iter()
                .any(|parent| !mastered(rows.get(parent).map_or(&[], Vec::as_slice)));
            if text(field(concept, "section")) == section && !blocked {
                let seen = log.iter().any(|row| {
                    text(field(row, "concept")) == *id && text(field(row, "mode")) != "pretest"
                });
                return Ok(step(
                    concept,
                    if seen { "quiz" } else { "teach" },
                    "weakest section first, prerequisites done",
                    &rows,
                ));
            }
        }
    }
    Ok(step(
        &by_id[&open[0]],
        "teach",
        "its prerequisites come first",
        &rows,
    ))
}
