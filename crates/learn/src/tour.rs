//! Maintainer-reviewed code tours whose mastered steps fade to prediction.

use crate::{error, field, project, read_json, stamp, text, write_json, Result};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const FADED: &str =
    "Predict before you read: what does this code do, and why is it here? ({title})";

fn repos(root: &Path, project_name: &str) -> Result<BTreeMap<String, PathBuf>> {
    let manifest = project::manifest(root, project_name)?;
    Ok(manifest
        .get("sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|source| text(field(source, "kind")) == "repo")
        .map(|source| {
            (
                text(field(source, "label")),
                PathBuf::from(text(field(source, "path"))),
            )
        })
        .collect())
}

fn relative_name(name: &str) -> Result<String> {
    if !Regex::new(r"^[a-z0-9][a-z0-9-]*$")
        .expect("tour name")
        .is_match(name)
    {
        return Err(error(
            "ValueError",
            "a tour name is lowercase letters, digits, and hyphens",
        ));
    }
    Ok(format!("tours/{name}.json"))
}

fn path(root: &Path, project_name: &str, name: &str) -> Result<PathBuf> {
    Ok(project::folder(root, project_name).join(relative_name(name)?))
}

pub fn check(root: &Path, project_name: &str, tour: &Value) -> Result<Vec<String>> {
    let repos = repos(root, project_name)?;
    let known = project::concepts(root, project_name)?
        .iter()
        .map(|concept| text(field(concept, "id")))
        .collect::<BTreeSet<_>>();
    let mut problems = Vec::new();
    let repo_name = text(field(tour, "repo"));
    let Some(repo) = repos.get(&repo_name) else {
        return Ok(vec![format!(
            "repo must be a staged repo: {}",
            if repos.is_empty() {
                "none staged".into()
            } else {
                repos.keys().cloned().collect::<Vec<_>>().join(", ")
            }
        )]);
    };
    if text(field(tour, "title")).trim().is_empty() {
        problems.push("a tour needs a title".into());
    }
    let steps = field(tour, "steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if steps.is_empty() {
        problems.push("a tour needs at least one step".into());
    }
    for (index, step) in steps.iter().enumerate() {
        let number = index + 1;
        let file = text(field(step, "file"));
        let target = repo.join(&file);
        if file.is_empty() || !target.is_file() {
            problems.push(format!(
                "step {number}: no file {file} in {}",
                repo.file_name().unwrap_or_default().to_string_lossy()
            ));
            continue;
        }
        let lines = std::fs::read_to_string(&target)
            .unwrap_or_default()
            .lines()
            .count();
        let valid_line = field(step, "line")
            .and_then(Value::as_u64)
            .is_some_and(|line| line >= 1 && line <= lines as u64);
        if !valid_line {
            problems.push(format!("step {number}: line must be 1 to {lines}"));
        }
        if text(field(step, "description")).trim().is_empty() {
            problems.push(format!("step {number}: needs a description"));
        }
        let concept = text(field(step, "concept"));
        if !concept.is_empty() && !known.contains(&concept) {
            problems.push(format!("step {number}: concept {concept} is not imported"));
        }
    }
    Ok(problems)
}

pub fn add(root: &Path, project_name: &str, name: &str, tour: &Value) -> Result<Value> {
    let problems = check(root, project_name, tour)?;
    if !problems.is_empty() {
        return Err(error(
            "ValueError",
            format!("tour not added:\n  {}", problems.join("\n  ")),
        ));
    }
    let mut record = tour.as_object().cloned().unwrap_or_default();
    record.insert("approved".into(), Value::Null);
    let record = Value::Object(record);
    let target = path(root, project_name, name)?;
    let relative = target
        .strip_prefix(root)
        .map_err(|_| error("ValueError", "tour escaped data root"))?
        .to_string_lossy()
        .into_owned();
    write_json(root, &relative, &record)?;
    Ok(record)
}

pub fn load(root: &Path, project_name: &str, name: &str) -> Result<Value> {
    let target = path(root, project_name, name)?;
    let relative = target
        .strip_prefix(root)
        .map_err(|_| error("ValueError", "tour escaped data root"))?
        .to_string_lossy()
        .into_owned();
    let tour = read_json(root, &relative, Value::Null)?;
    if tour.is_null() {
        return Err(error("KeyError", format!("no tour {name} in {project_name}; add one with `careerbrew learn tour add {project_name} {name} FILE`")));
    }
    Ok(tour)
}

pub fn approve(root: &Path, project_name: &str, name: &str, by: &str) -> Result<Value> {
    let mut tour = load(root, project_name, name)?;
    let repos = repos(root, project_name)?;
    let repo = &repos[&text(field(&tour, "repo"))];
    let sha = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default();
    tour["approved"] = json!({"by": by, "sha": sha, "at": stamp()});
    let target = path(root, project_name, name)?;
    let relative = target
        .strip_prefix(root)
        .map_err(|_| error("ValueError", "tour escaped data root"))?
        .to_string_lossy()
        .into_owned();
    write_json(root, &relative, &tour)?;
    Ok(tour)
}

pub fn review_state(
    root: &Path,
    project_name: &str,
    tour: &Value,
) -> Result<(String, Vec<String>)> {
    if field(tour, "approved").is_none_or(Value::is_null) {
        return Ok(("unreviewed".into(), Vec::new()));
    }
    let repos = repos(root, project_name)?;
    let repo = &repos[&text(field(tour, "repo"))];
    let files = field(tour, "steps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|step| text(field(step, "file")))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let sha = text(field(&tour["approved"], "sha"));
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("diff")
        .arg("--name-only")
        .arg(sha)
        .arg("--")
        .args(&files)
        .output()?;
    let changed = if out.status.success() {
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    } else {
        files
    };
    Ok((
        if changed.is_empty() {
            "approved".into()
        } else {
            "stale".into()
        },
        changed,
    ))
}

pub fn export(root: &Path, project_name: &str, name: &str) -> Result<(PathBuf, Value)> {
    let tour = load(root, project_name, name)?;
    let log = project::by_concept(&project::log(root, project_name)?);
    let titles = project::concepts(root, project_name)?
        .iter()
        .map(|concept| (text(field(concept, "id")), text(field(concept, "title"))))
        .collect::<BTreeMap<_, _>>();
    let (state, changed) = review_state(root, project_name, &tour)?;
    let mut steps = Vec::new();
    let mut faded = 0_u64;
    for step in field(&tour, "steps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let concept = text(field(step, "concept"));
        let mastered =
            !concept.is_empty() && project::mastered(log.get(&concept).map_or(&[], Vec::as_slice));
        faded += u64::from(mastered);
        let description = if mastered {
            FADED.replace("{title}", &titles[&concept])
        } else {
            text(field(step, "description"))
        };
        steps.push(json!({"file": text(field(step, "file")), "line": field(step, "line").cloned().unwrap_or(Value::Null), "description": description}));
    }
    if state != "approved" {
        let note = if state == "unreviewed" {
            "No maintainer has approved this tour yet.".into()
        } else {
            format!(
                "Changed since a maintainer approved it: {}. Read with care.",
                changed.join(", ")
            )
        };
        steps[0]["description"] = json!(format!(
            "**{note}**\n\n{}",
            text(field(&steps[0], "description"))
        ));
    }
    let out = root
        .join("out")
        .join("tours")
        .join(format!("{}-{name}.tour", project::slug(project_name)));
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut object = Map::new();
    object.insert("$schema".into(), json!("https://aka.ms/codetour-schema"));
    object.insert(
        "title".into(),
        field(&tour, "title").cloned().unwrap_or(Value::Null),
    );
    object.insert("steps".into(), Value::Array(steps));
    std::fs::write(&out, serde_json::to_string_pretty(&Value::Object(object))?)?;
    Ok((
        out,
        json!({"state": state, "faded": faded, "steps": tour["steps"].as_array().map_or(0, Vec::len), "changed": changed}),
    ))
}
