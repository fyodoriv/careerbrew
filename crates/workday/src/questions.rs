//! Question records, rechecks and restricted projections.

use crate::{digest, emit, field, one_line, stamp, text, value_error, write_json, Result};
use careerbrew_vault::cli::editor_argv;
use chrono::{DateTime, Duration, NaiveDate};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration as StdDuration;
use wait_timeout::ChildExt;

pub const SIGNIN: &str = "careerbrew:signin";
pub const REVIEW_PR: &str = "careerbrew:review-pr";
pub const CARRY_DAYS: i64 = 14;
pub const MAX_TEXT: usize = 200;
pub const MAX_OPTION: usize = 80;
pub const MAX_OPTIONS: usize = 6;
pub const MAX_RECHECK: usize = 500;
pub const VERIFY_SECONDS: u64 = 120;
const PUBLIC: &[&str] = &[
    "id",
    "step",
    "steps",
    "source",
    "createdAt",
    "day",
    "closed",
    "closedAt",
];

pub trait RecheckProvider {
    fn signin(&self, root: &Path, command: &str) -> Result<(bool, String)>;
    fn review_pr(&self, root: &Path, url: &str, deadline: Option<f64>) -> Result<(String, String)>;
    fn may_run(&self, root: &Path, source: &str) -> Result<bool>;
}

pub struct LocalRechecks;

impl RecheckProvider for LocalRechecks {
    fn signin(&self, _root: &Path, command: &str) -> Result<(bool, String)> {
        run_verify(command)
    }

    fn review_pr(
        &self,
        _root: &Path,
        _url: &str,
        _deadline: Option<f64>,
    ) -> Result<(String, String)> {
        Ok((
            "unknown".into(),
            "review recheck: could not verify PR state".into(),
        ))
    }

    fn may_run(&self, _root: &Path, source: &str) -> Result<bool> {
        if !source.starts_with("plugin:") {
            return Ok(true);
        }
        let name = source.split_once(':').map(|(_, name)| name).unwrap_or("");
        let installed = careerbrew_setup::plugins::installed()
            .map_err(|error| crate::error(error.class, error.message))?;
        Ok(installed.into_iter().any(|plugin| {
            plugin.name == name
                && field(&plugin.entry, "exec_allowed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        }))
    }
}

pub fn kind(recheck: &str) -> String {
    if recheck.is_empty() {
        "none".into()
    } else if recheck.starts_with("https://") || recheck.starts_with("http://") {
        "url".into()
    } else if recheck.starts_with(REVIEW_PR) {
        "review-pr".into()
    } else if recheck == SIGNIN {
        "signin".into()
    } else {
        "command".into()
    }
}

pub fn qid(step: &str, text: &str) -> String {
    format!("q-{}", &digest(&format!("{step}\n{text}"))[..8])
}

pub fn row_id(step: &str) -> String {
    format!("b-{}", &digest(step)[..8])
}

pub fn age(created: &str, at: DateTime<chrono::FixedOffset>) -> String {
    let Some(then) = DateTime::parse_from_rfc3339(created).ok().or_else(|| {
        chrono::NaiveDateTime::parse_from_str(created, "%Y-%m-%dT%H:%M:%S")
            .ok()
            .and_then(|value| value.and_local_timezone(*at.offset()).single())
    }) else {
        return "unknown".into();
    };
    let minutes = ((at - then).num_seconds().max(0) as f64 / 60.0).floor() as i64;
    if minutes < 60 {
        format!("{minutes} min")
    } else if minutes < 48 * 60 {
        format!("{} h", minutes / 60)
    } else {
        format!("{} d", minutes / (24 * 60))
    }
}

fn limited(field_name: &str, value: &str, limit: usize) -> Result<String> {
    let value = one_line(value);
    if value.chars().count() > limit {
        return Err(value_error(format!(
            "keep {field_name} short: {} characters, at most {limit}",
            value.chars().count()
        )));
    }
    Ok(value)
}

fn review_pr_url(value: &str) -> Result<String> {
    let (prefix, url) = value.split_once(' ').unwrap_or((value, ""));
    if prefix != REVIEW_PR || url.is_empty() {
        return Err(value_error(
            "--recheck: use careerbrew:review-pr followed by one HTTPS PR link",
        ));
    }
    let pattern = Regex::new(r"(?i)^https://(?P<host>[a-z0-9][a-z0-9.-]*)/(?P<owner>[a-z0-9][a-z0-9-]*)/(?P<repo>[a-z0-9_.-]+)/pull/(?P<number>[1-9][0-9]{0,15})$").expect("PR link");
    let valid_repo = pattern
        .captures(url)
        .is_some_and(|parts| !matches!(&parts["repo"], "." | ".."));
    if !valid_repo {
        return Err(value_error("--recheck: use one HTTPS pull request link"));
    }
    Ok(url.to_owned())
}

pub fn check_recheck(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || matches!(kind(value).as_str(), "url" | "signin") {
        if value
            .chars()
            .any(|ch| matches!(ch, '\n' | '\r' | '\0' | ' '))
        {
            return Err(value_error(
                "--recheck: a link has no spaces or line breaks",
            ));
        }
        return Ok(value.to_owned());
    }
    if value.chars().count() > MAX_RECHECK
        || value.chars().any(|ch| matches!(ch, '\n' | '\r' | '\0'))
    {
        return Err(value_error(format!(
            "--recheck: use one line of at most {MAX_RECHECK} characters"
        )));
    }
    if kind(value) == "review-pr" {
        review_pr_url(value)?;
        return Ok(value.to_owned());
    }
    editor_argv(value).map_err(|_| value_error("--recheck: the command has an unclosed quote"))?;
    Ok(value.to_owned())
}

#[allow(clippy::too_many_arguments)]
pub fn record(
    step: &str,
    text: &str,
    options: Option<&[String]>,
    pick: &str,
    recheck: &str,
    source: &str,
    ident: &str,
    steps: Option<&[String]>,
) -> Result<Value> {
    let text = limited("the question", text, MAX_TEXT)?;
    if text.is_empty() {
        return Err(value_error("the question text is empty"));
    }
    let mut opts = Vec::new();
    for option in options.unwrap_or(&[]) {
        let option = limited("an --option", option, MAX_OPTION)?;
        if !option.is_empty() && !opts.contains(&option) {
            opts.push(option);
        }
    }
    if opts.len() > MAX_OPTIONS {
        return Err(value_error(format!(
            "at most {MAX_OPTIONS} --option values"
        )));
    }
    let pick = limited("--pick", pick, MAX_OPTION)?;
    if !pick.is_empty() && !opts.is_empty() && !opts.contains(&pick) {
        return Err(value_error("--pick must be one of the --option values"));
    }
    if !["agent", "owner", "row", "careerbrew"].contains(&source)
        && !(source.starts_with("plugin:") && source.len() > 7)
    {
        return Err(value_error(format!("unknown source {source:?}")));
    }
    let mut out = json!({
        "id": if ident.is_empty() { qid(step, &text) } else { ident.to_owned() },
        "step": step,
        "text": text,
        "options": opts,
        "pick": pick,
        "createdAt": stamp(),
        "recheck": check_recheck(recheck)?,
        "source": source,
        "closed": "",
        "closedAt": "",
    });
    if let Some(steps) = steps.filter(|steps| !steps.is_empty()) {
        out["steps"] = json!(steps);
    }
    Ok(out)
}

pub fn records(root: &Path, day: NaiveDate) -> Result<Vec<Value>> {
    let data = crate::status::data(root, day)?;
    Ok(data
        .get("questions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|question| {
            field(question, "id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty())
        })
        .cloned()
        .collect())
}

fn change<T>(
    root: &Path,
    day: NaiveDate,
    f: impl FnOnce(&mut Vec<Value>) -> Result<T>,
) -> Result<T> {
    crate::status::with_day_lock(root, day, || {
        let mut have = records(root, day)?;
        let before = have.clone();
        let out = f(&mut have)?;
        if have != before {
            let mut document = json!({"rows": [], "handled": {}});
            if let (Some(target), Some(source)) = (
                document.as_object_mut(),
                crate::status::data(root, day)?.as_object(),
            ) {
                for (key, value) in source {
                    target.insert(key.clone(), value.clone());
                }
            }
            document["day"] = json!(day.format("%Y-%m-%d").to_string());
            document["questions"] = Value::Array(have);
            write_json(
                root,
                &format!("day/work-{}.json", day.format("%Y-%m-%d")),
                &document,
            )?;
        }
        Ok(out)
    })
}

fn days(day: NaiveDate) -> Vec<NaiveDate> {
    (0..=CARRY_DAYS).map(|n| day - Duration::days(n)).collect()
}

fn find(root: &Path, ident: &str, day: NaiveDate) -> Result<Option<(NaiveDate, Value)>> {
    for day in days(day) {
        for question in records(root, day)?.into_iter().rev() {
            if text(field(&question, "id")) == ident {
                return Ok(Some((day, question)));
            }
        }
    }
    Ok(None)
}

pub fn save(root: &Path, record: &Value, day: NaiveDate, reopen: bool) -> Result<Value> {
    let ident = text(field(record, "id"));
    let found = find(root, &ident, day)?;
    if let Some((found_day, old)) = &found {
        if *found_day == day
            && !text(field(old, "closed")).is_empty()
            && field(old, "text") == field(record, "text")
            && !reopen
        {
            return Ok(old.clone());
        }
    }
    let home = found
        .as_ref()
        .filter(|(_, old)| text(field(old, "closed")).is_empty())
        .map(|(day, _)| *day)
        .unwrap_or(day);
    let mut added = true;
    let got = change(root, home, |have| {
        for question in have.iter_mut() {
            if text(field(question, "id")) == ident && text(field(question, "closed")).is_empty() {
                let created = field(question, "createdAt")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| text(field(record, "createdAt")));
                let mut replacement = record.clone();
                replacement["createdAt"] = json!(created);
                *question = replacement;
                added = false;
                return Ok(question.clone());
            }
        }
        have.retain(|question| text(field(question, "id")) != ident);
        have.push(record.clone());
        Ok(record.clone())
    })?;
    if added {
        emit(
            root,
            "workday.question",
            json!({
                "action": "add",
                "source": text(field(&got, "source")).split(':').next().unwrap_or(""),
                "recheck": kind(&text(field(&got, "recheck"))),
                "options": got.get("options").and_then(Value::as_array).map_or(0, Vec::len),
            }),
        )?;
    }
    Ok(got)
}

#[allow(clippy::too_many_arguments)]
pub fn add(
    root: &Path,
    step: &str,
    text: &str,
    options: Option<&[String]>,
    pick: &str,
    recheck: &str,
    source: &str,
    day: NaiveDate,
) -> Result<Value> {
    let record = record(step, text, options, pick, recheck, source, "", None)?;
    save(root, &record, day, false)
}

pub fn open_records(root: &Path, day: NaiveDate) -> Result<Vec<Value>> {
    let at = crate::now();
    let mut out: Vec<Value> = Vec::new();
    for current in days(day).into_iter().rev() {
        for question in records(root, current)? {
            let ident = text(field(&question, "id"));
            out.retain(|old| text(field(old, "id")) != ident);
            if text(field(&question, "closed")).is_empty() {
                let mut question = question;
                question["day"] = json!(current.format("%Y-%m-%d").to_string());
                question["age"] = json!(age(&text(field(&question, "createdAt")), at));
                out.push(question);
            }
        }
    }
    Ok(out)
}

pub fn close(
    root: &Path,
    ident: &str,
    why: &str,
    day: NaiveDate,
    expected: Option<&Value>,
) -> Result<bool> {
    let at = stamp();
    let targets = expected.map(|_| vec![day]).unwrap_or_else(|| days(day));
    for current in targets {
        let closed = change(root, current, |have| {
            for question in have.iter_mut() {
                if text(field(question, "id")) == ident
                    && text(field(question, "closed")).is_empty()
                {
                    if let Some(expected) = expected {
                        let filtered = expected
                            .as_object()
                            .into_iter()
                            .flatten()
                            .filter(|(key, _)| !["day", "age"].contains(&key.as_str()))
                            .map(|(key, value)| (key.clone(), value.clone()))
                            .collect::<Map<_, _>>();
                        if question.as_object() != Some(&filtered) {
                            return Ok(false);
                        }
                    }
                    question["closed"] = json!(why);
                    question["closedAt"] = json!(at);
                    return Ok(true);
                }
            }
            Ok(false)
        })?;
        if closed {
            emit(root, "workday.question", json!({"action": "close"}))?;
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn from_row(root: &Path, row: &Value, day: NaiveDate, reopen: bool) -> Result<()> {
    let step = text(field(row, "step"));
    let ident = row_id(&step);
    let needs_you = text(field(row, "needs_you"));
    if !needs_you.is_empty() {
        let record = record(&step, &needs_you, None, "", "", "row", &ident, None)?;
        save(root, &record, day, reopen)?;
    } else {
        close(root, &ident, "the row no longer waits on you", day, None)?;
    }
    Ok(())
}

pub fn run_verify(command: &str) -> Result<(bool, String)> {
    let argv = editor_argv(command).map_err(|_| value_error("could not split command"))?;
    let child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            let class = if error.kind() == std::io::ErrorKind::NotFound {
                "FileNotFoundError"
            } else {
                "OSError"
            };
            return Ok((false, format!("could not start ({class})")));
        }
    };
    let timed_out = child
        .wait_timeout(StdDuration::from_secs(VERIFY_SECONDS))?
        .is_none();
    if timed_out {
        child.kill()?;
    }
    let status = child.wait()?;
    if timed_out {
        return Ok((false, format!("timed out after {VERIFY_SECONDS} s")));
    }
    Ok((
        status.success(),
        if status.success() {
            String::new()
        } else {
            format!("exit {}", status.code().unwrap_or(1))
        },
    ))
}

pub fn check_one(
    root: &Path,
    question: &Value,
    provider: &dyn RecheckProvider,
    deadline: Option<f64>,
) -> Result<(String, String)> {
    let recheck = text(field(question, "recheck"));
    match kind(&recheck).as_str() {
        "none" => Ok(("open".into(), "no recheck: close it by hand".into())),
        "url" => Ok((
            "agent".into(),
            "an agent checks the link, then closes the item when it is done".into(),
        )),
        "signin" => {
            let settings = crate::read_yaml(root, "settings.yaml", json!({}))?;
            let command = text(field(&settings, "work_signin_probe"));
            if command.is_empty() {
                return Ok(("unknown".into(), "no work_signin_probe set".into()));
            }
            let (ok, why) = provider.signin(root, &command)?;
            Ok(if ok {
                ("closed".into(), "signed in".into())
            } else {
                ("open".into(), why)
            })
        }
        _ => {
            let source = text(field(question, "source"));
            if source.starts_with("plugin:") && !provider.may_run(root, &source)? {
                return Ok(("unknown".into(), "the recheck comes from a plugin that may not run commands: careerbrew plugin add <src> --allow-exec".into()));
            }
            if kind(&recheck) == "review-pr" {
                let url = match review_pr_url(&recheck) {
                    Ok(url) => url,
                    Err(_) => {
                        return Ok(("unknown".into(), "review recheck: invalid PR link".into()))
                    }
                };
                return provider.review_pr(root, &url, deadline);
            }
            let (ok, why) = run_verify(&recheck)?;
            if ok {
                Ok(("open".into(), "recheck: still open".into()))
            } else if why.starts_with("exit ") {
                Ok(("closed".into(), format!("recheck: closed ({why})")))
            } else {
                Ok(("unknown".into(), format!("recheck {why}")))
            }
        }
    }
}

pub fn public(question: &Value) -> Value {
    let mut out = Map::new();
    for key in PUBLIC.iter().chain(["age", "state", "why"].iter()) {
        if let Some(value) = question.get(*key) {
            out.insert((*key).to_owned(), value.clone());
        }
    }
    if text(out.get("source")).starts_with("plugin:") {
        out.insert("source".into(), json!("plugin"));
    }
    out.insert(
        "recheck".into(),
        json!(kind(&text(field(question, "recheck")))),
    );
    out.insert(
        "options".into(),
        json!(question
            .get("options")
            .and_then(Value::as_array)
            .map_or(0, Vec::len)),
    );
    Value::Object(out)
}

pub fn quiet_question(root: &Path, step: &str, day: &str, today: &str) -> Result<bool> {
    let Some(ident) = step.strip_prefix("session:") else {
        return Ok(false);
    };
    let state = careerbrew_sessions::sessions::load(root)
        .map_err(|error| crate::error(error.class, error.message))?;
    let matches = state
        .get("sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|session| text(field(session, "id")) == ident && text(field(session, "day")) == day)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Ok(false);
    }
    let project = text(field(matches[0], "project"));
    let records = careerbrew_projects::projects::load(root)?;
    let Some(record) = records.get(&project) else {
        return Ok(false);
    };
    let today = crate::day(today)?;
    Ok(field(record, "quiet_until")
        .and_then(Value::as_str)
        .and_then(|value| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
        .is_some_and(|until| today < until))
}

pub fn recheck(root: &Path, day: NaiveDate, provider: &dyn RecheckProvider) -> Result<Value> {
    let mut ask = Vec::new();
    let mut dropped = Vec::new();
    let mut deferred = Vec::new();
    for question in open_records(root, day)? {
        if quiet_question(
            root,
            &text(field(&question, "step")),
            &text(field(&question, "day")),
            &day.format("%Y-%m-%d").to_string(),
        )? {
            deferred.push(text(field(&question, "id")));
            continue;
        }
        let (state, why) = check_one(root, &question, provider, None)?;
        if state == "closed" {
            let is_review = kind(&text(field(&question, "recheck"))) == "review-pr";
            let expected = if is_review { Some(&question) } else { None };
            let question_day = crate::day(&text(field(&question, "day")))?;
            let closed = close(
                root,
                &text(field(&question, "id")),
                &why,
                question_day,
                expected,
            )?;
            if is_review && !closed {
                let mut changed = question;
                changed["state"] = json!("unknown");
                changed["why"] = json!("review recheck: question changed");
                ask.push(changed);
                continue;
            }
            dropped.push(json!({"id": question["id"], "step": question["step"], "age": question["age"], "why": why}));
        } else {
            let mut question = question;
            question["state"] = json!(state);
            question["why"] = json!(why);
            ask.push(question);
        }
    }
    let agent = ask
        .iter()
        .filter(|question| text(field(question, "state")) == "agent")
        .map(|question| text(field(question, "id")))
        .collect::<Vec<_>>();
    let unknown = ask
        .iter()
        .filter(|question| text(field(question, "state")) == "unknown")
        .count();
    let out = json!({"checked": ask.len() + dropped.len(), "ask": ask, "dropped": dropped, "agent": agent, "deferred": deferred});
    emit(
        root,
        "workday.recheck",
        json!({
            "checked": out["checked"], "ask": out["ask"].as_array().map_or(0, Vec::len),
            "dropped": out["dropped"].as_array().map_or(0, Vec::len),
            "agent": out["agent"].as_array().map_or(0, Vec::len), "unknown": unknown,
        }),
    )?;
    Ok(out)
}
