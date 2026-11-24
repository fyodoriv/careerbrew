//! Component adapters for the rust-12 learn and project-document CLI handlers.

use crate::{
    error, explain, field, prd, project, read_text, replay, text, tour, write_text, Result,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub exit: u8,
    pub stdout: String,
}

pub fn invocation(exit: u8, stdout: impl Into<String>) -> Invocation {
    Invocation {
        exit,
        stdout: stdout.into(),
    }
}

pub fn caught(result: Result<Invocation>) -> Invocation {
    match result {
        Ok(output) => output,
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

pub fn value(output: Invocation) -> Value {
    json!({"exit": output.exit, "stdout": output.stdout})
}

pub fn learn_list(root: &Path) -> Result<Invocation> {
    let names = project::projects(root)?;
    let mut out = String::new();
    for name in &names {
        let status = project::status(root, name, None)?;
        out.push_str(&format!(
            "{name}: {}/{} mastered, {} due\n",
            status["mastered"].as_u64().unwrap_or(0),
            status["concepts"].as_u64().unwrap_or(0),
            status["due_reviews"].as_u64().unwrap_or(0)
                + status["due_retests"].as_u64().unwrap_or(0)
        ));
    }
    out.push_str(&format!(
        "{} learn project(s).{}",
        names.len(),
        if names.is_empty() {
            " Start one: careerbrew learn start <name> --repo PATH"
        } else {
            ""
        }
    ));
    out.push('\n');
    Ok(invocation(0, out))
}

pub fn learn_start(
    root: &Path,
    name: &str,
    specs: &[(String, String)],
    consume: bool,
) -> Result<Invocation> {
    let manifest = project::start(root, name, specs, consume)?;
    let fresh = &manifest["freshness"];
    let changed = crate::strings(field(fresh, "changed"));
    let unchanged = field(fresh, "unchanged")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    Ok(invocation(0, format!("{name}: {} source(s) staged; changed: {}; unchanged: {unchanged}\nnext: write notes (careerbrew learn note {name} <section>/<note>.md --set FILE), then careerbrew learn concepts {name} FILE\n", manifest["sources"].as_array().map_or(0, Vec::len), if changed.is_empty() { "none".into() } else { changed.join(", ") })))
}

pub fn learn_note(
    root: &Path,
    name: &str,
    relative: &str,
    set: Option<&Path>,
    consume: bool,
) -> Result<Invocation> {
    if let Some(source) = set {
        project::note(
            root,
            name,
            relative,
            Some(&std::fs::read_to_string(source)?),
        )?;
        if consume {
            std::fs::remove_file(source)?;
        }
        Ok(invocation(0, format!("saved note {relative}\n")))
    } else {
        Ok(invocation(
            0,
            project::note(root, name, relative, None)? + "\n",
        ))
    }
}

pub fn learn_concepts(
    root: &Path,
    name: &str,
    source: &Path,
    check: bool,
    consume: bool,
) -> Result<Invocation> {
    let data = careerbrew_vault::json_yaml::yaml_read(&std::fs::read_to_string(source)?)
        .map_err(|_| error("ValueError", "invalid concepts YAML"))?;
    let data = careerbrew_setup::value::from_canon(&data);
    let rows = field(&data, "concepts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if check {
        let problems = project::check_concepts(root, name, &rows)?;
        let mut out = problems
            .iter()
            .map(|problem| format!("  {problem}\n"))
            .collect::<String>();
        out.push_str(&format!(
            "concepts check: {}\n",
            if problems.is_empty() {
                "pass".into()
            } else {
                format!("{} problem(s)", problems.len())
            }
        ));
        return Ok(invocation(if problems.is_empty() { 0 } else { 1 }, out));
    }
    let result = project::import_concepts(root, name, &rows)?;
    if consume {
        std::fs::remove_file(source)?;
    }
    Ok(invocation(
        0,
        format!(
            "imported {} concept(s) in {} section(s); each is a card with spaced reviews\n",
            result["concepts"].as_u64().unwrap_or(0),
            result["sections"].as_u64().unwrap_or(0)
        ),
    ))
}

pub fn learn_next(root: &Path, name: &str) -> Result<Invocation> {
    let step = project::next_step(root, name, None)?;
    let mode = text(field(&step, "mode"));
    let title = text(field(&step, "title"));
    let mut out = format!("{mode}: {title} ({})", text(field(&step, "why"))).replace(":  (", ": (");
    out.push('\n');
    if let Some(question) = field(&step, "question") {
        out.push_str(&format!(
            "ask at {}: {}\n",
            text(field(&step, "level")),
            text(field(question, "prompt"))
        ));
    }
    Ok(invocation(0, out))
}

#[allow(clippy::too_many_arguments)]
pub fn learn_answer(
    root: &Path,
    name: &str,
    concept: &str,
    result: &str,
    confidence: &str,
    level: &str,
    mode: &str,
    kind: &str,
    seconds: Option<i64>,
) -> Result<Invocation> {
    let row = project::answer(
        root, name, concept, result, confidence, level, mode, kind, seconds, None,
    )?;
    let mut out = format!("{concept}: {result}");
    if row["mastered"] == true {
        out.push_str("; mastered (retests at +1 and +7 days)");
    }
    if confidence == "high" && result == "wrong" {
        out.push_str(" misconception: high confidence and wrong; drill it first");
    }
    if row.get("leech") == Some(&Value::Bool(true)) {
        out.push_str("; leech: forgotten many times, rewrite its note and questions");
    }
    out.push('\n');
    Ok(invocation(0, out))
}

pub fn learn_status(root: &Path, name: &str, json_output: bool) -> Result<Invocation> {
    let status = project::status(root, name, None)?;
    if json_output {
        return Ok(invocation(0, serde_json::to_string_pretty(&status)? + "\n"));
    }
    let mut out = String::new();
    for (name, section) in status["sections"].as_object().into_iter().flatten() {
        out.push_str(&format!(
            "  {:3}%  {name}  ({} left)\n",
            section["known_pct"].as_u64().unwrap_or(0),
            section["remaining"].as_array().map_or(0, Vec::len)
        ));
    }
    let optional = |key: &str| {
        field(&status, key)
            .and_then(Value::as_f64)
            .map(careerbrew_projects::python_float)
            .unwrap_or_else(|| "n/a".into())
    };
    out.push_str(&format!("{name}: {}/{} mastered · due: {} review(s), {} retest(s) · 7-day retest kept: {} · calibration (Brier): {}\n", status["mastered"].as_u64().unwrap_or(0), status["concepts"].as_u64().unwrap_or(0), status["due_reviews"].as_u64().unwrap_or(0), status["due_retests"].as_u64().unwrap_or(0), optional("retest_retained_7d"), optional("brier")));
    let stale = crate::strings(field(&status, "stale"));
    if !stale.is_empty() {
        out.push_str(&format!(
            "changed sources to re-teach: {}\n",
            stale.join(", ")
        ));
    }
    Ok(invocation(0, out))
}

pub fn replay_list(
    root: &Path,
    name: &str,
    supplied: Option<&str>,
    limit: usize,
) -> Result<Invocation> {
    let repo = replay::repo(root, name, supplied)?;
    let rows = replay::candidates(&repo, limit)?;
    let mut out = String::new();
    for row in &rows {
        out.push_str(&format!(
            "  #{:<5} {} test file(s)  {}\n",
            row["pr"].as_u64().unwrap_or(0),
            row["tests"].as_array().map_or(0, Vec::len),
            text(field(row, "title"))
        ));
    }
    out.push_str(&format!(
        "{} small PR(s); start one: careerbrew learn replay start {name} <pr>\n",
        rows.len()
    ));
    Ok(invocation(0, out))
}

pub fn replay_start(
    root: &Path,
    name: &str,
    pr: u64,
    supplied: Option<&str>,
    python: Option<&str>,
) -> Result<Invocation> {
    let task = replay::start(root, name, pr, supplied, python)?;
    Ok(invocation(0, format!("PR #{pr}: {}\ncode at {} is from just before the PR; these tests now fail: {}\nmake them pass, then: careerbrew learn replay check {name} {pr}\n", text(field(&task, "title")), text(field(&task, "dir")), crate::strings(field(&task, "tests")).join(", "))))
}

pub fn replay_check(root: &Path, name: &str, pr: u64) -> Result<Invocation> {
    let result = replay::check(root, name, pr)?;
    let passed = result["passed"] == true;
    Ok(invocation(
        if passed { 0 } else { 1 },
        format!(
            "PR #{pr}: {} ({})\n",
            if passed { "pass" } else { "fail" },
            text(field(&result, "last"))
        ),
    ))
}

pub fn replay_measure(repo: &Path, limit: usize, python: Option<&str>) -> Result<Invocation> {
    let result = replay::measure(repo, limit, python)?;
    let mut out = String::new();
    for row in result["rows"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  #{:<5} {}  {}\n",
            row["pr"].as_u64().unwrap_or(0),
            if row["runnable"] == true {
                "yes"
            } else {
                "no "
            },
            text(field(row, "why"))
        ));
    }
    out.push_str(&format!(
        "replay coverage: {}/{} small PRs make a runnable task ({})\n",
        result["runnable"].as_u64().unwrap_or(0),
        result["prs"].as_u64().unwrap_or(0),
        field(&result, "share")
            .and_then(Value::as_f64)
            .map(careerbrew_projects::python_float)
            .unwrap_or_else(|| "None".into())
    ));
    Ok(invocation(0, out))
}

pub fn tour_add(
    root: &Path,
    name: &str,
    tour_name: &str,
    source: &Path,
    consume: bool,
) -> Result<Invocation> {
    let data: Value = serde_json::from_str(&std::fs::read_to_string(source)?)?;
    let tour = tour::add(root, name, tour_name, &data)?;
    if consume {
        std::fs::remove_file(source)?;
    }
    Ok(invocation(0, format!("tour {tour_name}: {} step(s), not yet approved; a maintainer runs: careerbrew learn tour approve {name} {tour_name} --by NAME\n", tour["steps"].as_array().map_or(0, Vec::len))))
}

pub fn tour_approve(root: &Path, name: &str, tour_name: &str, by: &str) -> Result<Invocation> {
    let tour = tour::approve(root, name, tour_name, by)?;
    let sha = text(field(&tour["approved"], "sha"));
    Ok(invocation(
        0,
        format!(
            "tour {tour_name} approved at {}\n",
            &sha[..sha.len().min(12)]
        ),
    ))
}

pub fn tour_export(root: &Path, name: &str, tour_name: &str) -> Result<Invocation> {
    let (out, info) = tour::export(root, name, tour_name)?;
    Ok(invocation(0, format!("tour {tour_name}: {}, {}/{} step(s) faded to a prediction; open {} with CodeTour: Open Tour File\n", text(field(&info, "state")), info["faded"].as_u64().unwrap_or(0), info["steps"].as_u64().unwrap_or(0), out.display())))
}

fn document(root: &Path, path: &Path) -> Result<String> {
    let contained = path
        .canonicalize()
        .ok()
        .zip(root.canonicalize().ok())
        .is_some_and(|(path, root)| path.starts_with(root));
    if contained {
        Ok(read_text(path)?.unwrap_or_default())
    } else {
        Ok(std::fs::read_to_string(path)?)
    }
}

pub fn project_explain(
    root: &Path,
    name: &str,
    check: Option<&Path>,
    short: bool,
    deep: bool,
) -> Result<Invocation> {
    if let Some(path) = check {
        let problems = explain::check(&document(root, path)?, short, deep);
        let mut out = problems
            .iter()
            .map(|problem| format!("problem: {problem}\n"))
            .collect::<String>();
        out.push_str(&format!(
            "explain check: {}\n",
            if problems.is_empty() {
                "pass".into()
            } else {
                format!("{} problem(s)", problems.len())
            }
        ));
        return Ok(invocation(if problems.is_empty() { 0 } else { 1 }, out));
    }
    let out = root.join("out").join(format!("explain-{name}.md"));
    write_text(&out, &explain::scaffold(root, name, short, deep)?)?;
    Ok(invocation(0, format!("wrote {}: fill it from the sources, then: careerbrew work project explain {name} --check {}{}\n", out.display(), out.display(), if short { " --short" } else if deep { " --deep" } else { "" })))
}

pub fn project_prd(
    root: &Path,
    name: &str,
    check: Option<&Path>,
    slot: Option<f64>,
) -> Result<Invocation> {
    if let Some(path) = check {
        let problems = prd::check(&document(root, path)?);
        let mut out = problems
            .iter()
            .map(|problem| format!("problem: {problem}\n"))
            .collect::<String>();
        out.push_str(&format!(
            "prd check: {}\n",
            if problems.is_empty() {
                "pass".into()
            } else {
                format!("{} problem(s)", problems.len())
            }
        ));
        return Ok(invocation(if problems.is_empty() { 0 } else { 1 }, out));
    }
    if slot.is_some_and(|slot| !(0.0 < slot && slot <= 520.0)) {
        return Err(error("ValueError", "invalid PRD plan slot"));
    }
    let out = root.join("out").join(format!("prd-{name}.md"));
    write_text(&out, &prd::scaffold(root, name, slot)?)?;
    Ok(invocation(0, format!("wrote {}: fill it from the sources, then: careerbrew work project prd {name} --check {}\n", out.display(), out.display())))
}

pub fn absolute(path: &str) -> PathBuf {
    PathBuf::from(path)
}
