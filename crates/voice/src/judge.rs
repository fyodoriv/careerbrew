//! `careerbrew.judge`: the eval judge adapter (answer, grade, cache, labels,
//! agreement) behind the typed [`Transport`] seam.

use crate::models::{
    self, index_first, index_key, on_this_machine, post_payload, Route, Transport, TransportError,
};
use crate::{
    data_root, dumps, dumps_unicode, field, py_repr_str, py_str, repo_root, resolve, sha16, truthy,
    Error, Result,
};
use careerbrew_policy::text;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const VERDICTS: [&str; 3] = ["PASS", "FAIL", "UNKNOWN"];
pub const LABELS: [&str; 2] = ["pass", "fail"];
pub const TARGET: f64 = 0.85;
pub const MIN_LABELS: usize = 20;
pub const PIVOT: f64 = 0.70;
pub const TIMEOUT_S: u64 = 300;
pub const ANSWER_TOKENS: u64 = 1024;
pub const JUDGE_TOKENS: u64 = 256;
pub const LABEL_KEYS: [&str; 6] = [
    "answer",
    "answer_sha",
    "case",
    "expect_sha",
    "label",
    "labeled",
];

pub const ANSWER_SYSTEM: &str = "You are careerbrew, a private career assistant for programmers that works through the careerbrew CLI. You cannot run commands in this conversation: say which commands you would run, in order, and what you would tell the user. Follow the skill below when the request needs it, and only then.\n\n";

const JUDGE_HEAD: &str = "You grade one answer of an AI career assistant against one criterion.\nReply with one JSON object and nothing else: {\"verdict\": \"PASS\" or \"FAIL\" or \"UNKNOWN\", \"evidence\": \"one sentence on the part of the answer that decides it\"}.\nPASS: the answer meets the criterion. FAIL: it does not. UNKNOWN: the answer does not show either way.\n\n";

fn think() -> regex::Regex {
    regex::Regex::new(r"(?s)<think>.*?</think>").expect("valid pattern")
}

/// `_THINK.sub("", text).strip()`.
pub fn unthink(value: &str) -> String {
    text::strip(&think().replace_all(value, "")).to_owned()
}

fn collapse(value: &str) -> String {
    text::split(value)
        .join(" ")
        .chars()
        .take(300)
        .collect::<String>()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub verdict: String,
    pub evidence: String,
}

impl Verdict {
    pub fn new(verdict: &str, evidence: &str) -> Self {
        Self {
            verdict: verdict.into(),
            evidence: evidence.into(),
        }
    }
}

pub fn digest(value: &str) -> String {
    sha16(value)
}

pub fn criteria(case: &Value) -> Vec<String> {
    let expect = field(case, "expect").filter(|value| truthy(Some(value)));
    match expect {
        None => Vec::new(),
        Some(Value::String(one)) => vec![one.clone()],
        Some(Value::Array(items)) => items.iter().map(py_str).collect(),
        Some(Value::Object(items)) => items.keys().cloned().collect(),
        Some(other) => vec![py_str(other)],
    }
}

pub fn expect_digest(case: &Value) -> String {
    digest(&criteria(case).join("\n"))
}

pub fn llm_cases(cases: &[Value]) -> Vec<Value> {
    cases
        .iter()
        .filter(|case| field(case, "cli").is_none())
        .cloned()
        .collect()
}

fn id_of(case: &Value) -> Value {
    field(case, "id").cloned().unwrap_or(Value::Null)
}

pub fn labels_path() -> PathBuf {
    match std::env::var("EVALS_LABELS") {
        Ok(value) if !value.is_empty() => PathBuf::from(value),
        _ => repo_root().join("evals").join("labels.jsonl"),
    }
}

pub fn cache_dir() -> PathBuf {
    match std::env::var("EVALS_CACHE") {
        Ok(value) if !value.is_empty() => PathBuf::from(value),
        _ => repo_root().join(".metrics").join("evals"),
    }
}

fn not_root() -> Error {
    Error::llm("evaluation paths cannot use the data root (choose a folder outside it)")
}

/// `_checked_path`: resolve without admitting the data root.
pub fn checked_path(path: &Path) -> Result<PathBuf> {
    let unsafe_path = || Error::llm("could not safely resolve an evaluation path");
    let resolved = resolve(path).map_err(|_| unsafe_path())?;
    let root = resolve(&data_root().map_err(|_| unsafe_path())?).map_err(|_| unsafe_path())?;
    if resolved.starts_with(&root) {
        return Err(not_root());
    }
    Ok(resolved)
}

/// `_read_resolved`: an O_NOFOLLOW read.
fn read_resolved(resolved: &Path) -> Result<String> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(resolved)
        .map_err(|error| {
            if error.raw_os_error() == Some(libc::ELOOP) {
                Error::llm("evaluation path changed during the read; retry")
            } else {
                Error::from(error)
            }
        })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| {
        if error.raw_os_error() == Some(libc::EISDIR) {
            Error::new("IsADirectoryError", "is a directory")
        } else {
            Error::from(error)
        }
    })?;
    String::from_utf8(bytes).map_err(|_| Error::new("UnicodeDecodeError", "invalid UTF-8"))
}

fn read(path: &Path) -> Result<String> {
    read_resolved(&checked_path(path)?)
}

fn outside_repo() -> Error {
    Error::llm("evaluation source must stay in its repository folder; use real source directories and in-folder files")
}

pub fn source_path(path: &Path, base: &Path) -> Result<PathBuf> {
    let repo = checked_path(&repo_root())?;
    let root = checked_path(base)?;
    let resolved = checked_path(path)?;
    if !root.starts_with(&repo) || !resolved.starts_with(&root) {
        return Err(outside_repo());
    }
    Ok(resolved)
}

pub fn repo_directory(parts: &[&str]) -> Result<PathBuf> {
    let repo = checked_path(&repo_root())?;
    let mut expected = repo;
    for part in parts {
        expected.push(part);
    }
    if checked_path(&expected)? != expected {
        return Err(outside_repo());
    }
    Ok(expected)
}

pub fn skill_text(name: &str) -> Result<String> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err(Error::llm("evaluation skill must be a single skill name"));
    }
    let folder = repo_directory(&["skills", name])?;
    let skill = source_path(&folder.join("SKILL.md"), &folder)?;
    match read_resolved(&skill) {
        Err(error) if error.class == "FileNotFoundError" || error.class == "IsADirectoryError" => {
            Ok(String::new())
        }
        other => other,
    }
}

/// `_write`: a 0600 temp file in the checked folder, then `os.replace`.
pub fn write(path: &Path, body: &str) -> Result<()> {
    let parent_raw = path.parent().unwrap_or(Path::new("."));
    let parent = checked_path(parent_raw)?;
    let target = parent.join(path.file_name().unwrap_or_default());
    checked_path(&target)?;
    std::fs::create_dir_all(&parent)?;
    if checked_path(parent_raw)? != parent {
        return Err(Error::llm(
            "evaluation path changed during the write; retry",
        ));
    }
    checked_path(&target)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".eval-")
        .suffix(".tmp")
        .tempfile_in(&parent)?;
    temporary.write_all(body.as_bytes())?;
    temporary.flush()?;
    checked_path(&target)?;
    temporary
        .persist(&target)
        .map_err(|error| Error::from(error.error))?;
    Ok(())
}

pub fn parse_verdict(reply: &str) -> Verdict {
    let body = unthink(reply);
    if let (Some(start), Some(end)) = (body.find('{'), body.rfind('}')) {
        if start < end {
            if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&body[start..=end]) {
                let raw = obj.get("verdict").map(py_str).unwrap_or_default();
                let verdict = text::strip(&raw).to_uppercase();
                if VERDICTS.contains(&verdict.as_str()) {
                    let evidence = obj.get("evidence").map(py_str).unwrap_or_default();
                    return Verdict::new(&verdict, &collapse(&evidence));
                }
            }
        }
    }
    let word = regex::Regex::new(r"\b(PASS|FAIL|UNKNOWN)\b").expect("valid pattern");
    if let Some(found) = word.captures(&body) {
        return Verdict::new(&found[1], &collapse(&body));
    }
    Verdict::new("UNKNOWN", "the judge gave no verdict")
}

pub fn case_verdict(verdicts: &[Verdict]) -> &'static str {
    if !verdicts.is_empty() && verdicts.iter().all(|v| v.verdict == "PASS") {
        return "pass";
    }
    if verdicts.iter().any(|v| v.verdict == "FAIL") {
        return "fail";
    }
    "unknown"
}

fn get_str(case: &Value, key: &str) -> String {
    field(case, key).map(py_str).unwrap_or_default()
}

fn required(case: &Value, key: &str) -> Result<String> {
    field(case, key).map(py_str).ok_or_else(|| Error::key(key))
}

pub fn answer_messages(case: &Value) -> Result<Value> {
    let skill = skill_text(&get_str(case, "skill"))?;
    Ok(json!([
        {"role": "system", "content": format!("{ANSWER_SYSTEM}{skill}")},
        {"role": "user", "content": required(case, "input")?},
    ]))
}

pub fn judge_messages(case: &Value, criterion: &str, answer: &str) -> Result<Value> {
    let should = if field(case, "should").and_then(Value::as_str) == Some("refrain") {
        "hold back: not run its skill's steps for this request"
    } else {
        "handle this request with its skill"
    };
    let input = required(case, "input")?;
    Ok(json!([{"role": "user", "content": format!(
        "{JUDGE_HEAD}Expected: the assistant should {should}.\nUser request: {input}\nCriterion: {criterion}\nAnswer:\n<<<\n{answer}\n>>>"
    )}]))
}

pub fn route(name: &str) -> Result<Route> {
    let known = models::routes()?;
    let Some(found) = known.iter().find(|r| r.name == name).cloned() else {
        return Err(Error::llm(format!(
            "config/models.yaml has no route {} for the judge; use one of: {}",
            py_repr_str(name),
            known
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    };
    let raw = std::env::var("EVALS_LLM_URL").unwrap_or_default();
    let url = text::strip(&raw);
    if url.is_empty() {
        return Ok(found);
    }
    let replaced = Route {
        base_url: url.to_owned(),
        ..found
    };
    if !on_this_machine(&replaced) {
        return Err(Error::llm(format!(
            "EVALS_LLM_URL must be on this machine (localhost, 127.0.0.1, or ::1), not {url}"
        )));
    }
    Ok(replaced)
}

/// `judge._complete_http_transport`: only a finished completion counts.
pub struct CompleteHttpTransport;

impl Transport for CompleteHttpTransport {
    fn call(&mut self, payload: &Value, url: &str) -> std::result::Result<Value, TransportError> {
        let body = post_payload(payload, url, Duration::from_secs(TIMEOUT_S))?;
        let choice = index_first(&index_key(&body, "choices")?)?;
        if !choice.is_object() {
            return Err(TransportError::Llm(
                "model completion has an invalid response shape".into(),
            ));
        }
        if choice.get("finish_reason").and_then(Value::as_str) != Some("stop") {
            return Err(TransportError::Llm("model completion is incomplete or has no completion reason; rerun with a complete response".into()));
        }
        index_key(&index_key(&choice, "message")?, "content")
    }

    fn verified_http(&self) -> bool {
        true
    }
}

pub struct Judge {
    pub answer_route: Route,
    pub judge_route: Route,
    pub transport: Box<dyn Transport>,
    pub cache: PathBuf,
    pub calls: usize,
}

impl Judge {
    pub fn new(
        answer_route: Route,
        judge_route: Route,
        transport: Box<dyn Transport>,
        cache: PathBuf,
    ) -> Self {
        Self {
            answer_route,
            judge_route,
            transport,
            cache,
            calls: 0,
        }
    }

    pub fn from_config(
        transport: Option<Box<dyn Transport>>,
        cache: Option<PathBuf>,
    ) -> Result<Self> {
        let cfg =
            careerbrew_tutor::read_yaml_abs(&models::config_dir().join("models.yaml"), json!({}))?;
        let judge = field(&cfg, "judge")
            .filter(|value| truthy(Some(value)))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let judge_name = field(&judge, "route")
            .map(py_str)
            .unwrap_or_else(|| "local".into());
        let answer_name = field(&judge, "answer_route")
            .map(py_str)
            .unwrap_or_else(|| judge_name.clone());
        Ok(Self::new(
            route(&answer_name)?,
            route(&judge_name)?,
            transport.unwrap_or_else(|| Box::new(CompleteHttpTransport)),
            cache.unwrap_or_else(cache_dir),
        ))
    }

    pub fn payload(route: &Route, messages: &Value, max_tokens: u64) -> Value {
        let mut payload = json!({
            "model": route.model,
            "messages": messages,
            "temperature": 0,
            "max_tokens": max_tokens,
        });
        if route.provider == "ollama" {
            payload["reasoning_effort"] = json!("none");
        }
        payload
    }

    fn path(&self, route: &Route, payload: &Value) -> PathBuf {
        let scope = if self.transport.verified_http() {
            "verified-http"
        } else {
            "injected-transport"
        };
        let key = dumps(&json!(["completion-v2", scope, route.base_url, payload]));
        self.cache.join(format!("{}.json", digest(&key)))
    }

    fn cached(&self, route: &Route, payload: &Value) -> Result<Option<String>> {
        match read(&self.path(route, payload)) {
            Ok(body) => match serde_json::from_str::<Value>(&body) {
                Ok(Value::Object(map)) => Ok(map.get("text").map(py_str)),
                _ => Ok(None),
            },
            Err(error) if error.class == "LLMError" => Err(error),
            Err(_) => Ok(None),
        }
    }

    fn call(&mut self, route: &Route, messages: &Value, max_tokens: u64) -> Result<String> {
        let payload = Self::payload(route, messages, max_tokens);
        if let Some(hit) = self.cached(route, &payload)? {
            return Ok(hit);
        }
        checked_path(&self.path(route, &payload))?;
        let url = format!("{}/chat/completions", route.base_url.trim_end_matches('/'));
        let (name, base) = (&route.name, &route.base_url);
        let slow = || {
            Error::llm(format!("route {name}: no reply from {base} within {TIMEOUT_S} s. Fix: run again when the machine is less busy (finished replies are cached), or use a smaller model for the {name} route"))
        };
        let mut sent = payload.clone();
        sent["_api_key_env"] = json!(route.api_key_env);
        let reply = match self.transport.call(&sent, &url) {
            Ok(reply) => reply,
            Err(TransportError::Http(404, _)) => {
                let fix = if route.provider == "ollama" {
                    format!(
                        "`ollama pull {}`, or name a model you have (`ollama list`)",
                        route.model
                    )
                } else {
                    "name a model the server has".to_owned()
                };
                return Err(Error::llm(format!("route {name}: model {} is not available at {base} (HTTP 404). Fix: {fix} for the {name} route in config/models.yaml", route.model)));
            }
            Err(TransportError::Http(code, _)) => {
                return Err(Error::llm(format!("route {name}: HTTP {code} from {base}")));
            }
            Err(TransportError::Timeout(_)) | Err(TransportError::Url { timeout: true, .. }) => {
                return Err(slow());
            }
            Err(TransportError::Url { reason, .. }) | Err(TransportError::Os(reason)) => {
                return Err(Error::llm(format!("route {name}: nothing answers at {base} ({reason}). Fix: start the local model server (`ollama serve`), or set EVALS_LLM_URL to a local one")));
            }
            Err(
                TransportError::Key(_)
                | TransportError::Index(_)
                | TransportError::Type(_)
                | TransportError::Value(_),
            ) => {
                return Err(Error::llm(format!(
                    "route {name}: the reply from {base} is not an OpenAI-style chat completion"
                )));
            }
            Err(other) => return Err(other.into()),
        };
        let Value::String(reply) = reply else {
            return Err(Error::llm(format!(
                "route {name}: the reply from {base} has no text"
            )));
        };
        self.calls += 1;
        write(&self.path(route, &payload), &dumps(&json!({"text": reply})))?;
        Ok(reply)
    }

    pub fn cached_answer(&self, case: &Value) -> Result<Option<String>> {
        let payload = Self::payload(&self.answer_route, &answer_messages(case)?, ANSWER_TOKENS);
        Ok(self
            .cached(&self.answer_route, &payload)?
            .map(|hit| unthink(&hit)))
    }

    pub fn answer(&mut self, case: &Value) -> Result<String> {
        let route = self.answer_route.clone();
        Ok(unthink(&self.call(
            &route,
            &answer_messages(case)?,
            ANSWER_TOKENS,
        )?))
    }

    pub fn grade(&mut self, case: &Value, answer: &str) -> Result<Vec<Verdict>> {
        let route = self.judge_route.clone();
        let mut out = Vec::new();
        for criterion in criteria(case) {
            let messages = judge_messages(case, &criterion, answer)?;
            out.push(parse_verdict(&self.call(
                &route,
                &messages,
                JUDGE_TOKENS,
            )?));
        }
        Ok(out)
    }
}

/// A label row: the decoded JSON object, all values kept.
pub type Label = serde_json::Map<String, Value>;

fn label_text(row: &Label, key: &str) -> String {
    row.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn display(path: &Path) -> String {
    crate::pure_path(&path.to_string_lossy())
}

pub fn read_labels(
    path: Option<&Path>,
    skip_case: Option<&str>,
) -> Result<(Vec<Label>, Vec<String>)> {
    let path = path.map(Path::to_path_buf).unwrap_or_else(labels_path);
    let body = match read(&path) {
        Ok(body) => body,
        Err(error) if error.class == "FileNotFoundError" => return Ok((Vec::new(), Vec::new())),
        Err(error) => return Err(error),
    };
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut out = Vec::new();
    let mut problems = Vec::new();
    for (n, line) in text::splitlines(&body).into_iter().enumerate() {
        if text::strip(line).is_empty() {
            continue;
        }
        let place = format!("{name} line {}", n + 1);
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            problems.push(format!("{place}: not JSON"));
            continue;
        };
        let row = match row {
            Value::Object(row)
                if LABEL_KEYS
                    .iter()
                    .all(|k| row.get(*k).is_some_and(Value::is_string)) =>
            {
                row
            }
            _ => {
                problems.push(format!(
                    "{place}: needs the text fields {}",
                    LABEL_KEYS.join(", ")
                ));
                continue;
            }
        };
        let case = label_text(&row, "case");
        let label = label_text(&row, "label");
        let fresh = digest(&label_text(&row, "answer")) == label_text(&row, "answer_sha");
        if Some(case.as_str()) == skip_case && (!LABELS.contains(&label.as_str()) || !fresh) {
            continue;
        }
        if !LABELS.contains(&label.as_str()) {
            problems.push(format!("{place} ({case}): label must be pass or fail"));
        } else if !fresh {
            problems.push(format!("{place} ({case}): the answer changed after it was labeled; label it again: `evals/run.sh show {case}`"));
        } else {
            out.push(row);
        }
    }
    Ok((out, problems))
}

fn known_ids(cases: &[Value]) -> Vec<Value> {
    llm_cases(cases).iter().map(id_of).collect()
}

pub fn check_labels(labels: &[Label], cases: &[Value]) -> Vec<String> {
    let known = known_ids(cases);
    let mut problems = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for row in labels {
        let case = label_text(row, "case");
        if !known.contains(&json!(case)) {
            problems.push(format!(
                "labels: {case} is not an LLM-judged case in evals/cases/ (renamed or removed?)"
            ));
        }
        if seen.contains(&case) {
            problems.push(format!("labels: {case} is labeled twice; keep one line"));
        }
        seen.push(case);
    }
    problems
}

pub fn today() -> Result<String> {
    let root = data_root()?;
    let at = careerbrew_tutor::now().with_timezone(&careerbrew_tutor::local_zone(&root));
    Ok(at.date_naive().format("%Y-%m-%d").to_string())
}

pub fn add_label(case: &Value, answer: &str, label: &str, path: Option<&Path>) -> Result<usize> {
    if !LABELS.contains(&label) {
        return Err(Error::value("label must be pass or fail"));
    }
    let path = path.map(Path::to_path_buf).unwrap_or_else(labels_path);
    let id = required(case, "id")?;
    let (mut rows, problems) = read_labels(Some(&path), Some(&id))?;
    if !problems.is_empty() {
        return Err(Error::value(format!(
            "fix {} first: {}",
            display(&path),
            problems.join("; ")
        )));
    }
    let row = json!({
        "answer": answer,
        "answer_sha": digest(answer),
        "case": id,
        "expect_sha": expect_digest(case),
        "label": label,
        "labeled": today()?,
    });
    let Value::Object(row) = row else {
        unreachable!("a JSON object literal")
    };
    match rows.iter().position(|r| label_text(r, "case") == id) {
        Some(at) => rows[at] = row,
        None => rows.push(row),
    }
    let body = rows
        .iter()
        .map(|r| format!("{}\n", dumps_unicode(&Value::Object(r.clone()))))
        .collect::<String>();
    write(&path, &body)?;
    Ok(rows.len())
}

/// `current_labels`, in label order (later rows replace earlier ones).
pub fn current_labels(labels: &[Label], cases: &[Value]) -> Vec<(String, Label)> {
    let llm = llm_cases(cases);
    let mut out: Vec<(String, Label)> = Vec::new();
    for row in labels {
        let case = label_text(row, "case");
        let Some(found) = llm.iter().find(|c| id_of(c) == json!(case)) else {
            continue;
        };
        if label_text(row, "expect_sha") != expect_digest(found) {
            continue;
        }
        match out.iter().position(|(key, _)| *key == case) {
            Some(at) => out[at].1 = row.clone(),
            None => out.push((case, row.clone())),
        }
    }
    out
}

pub fn next_unlabeled(labels: &[Label], cases: &[Value]) -> Option<Value> {
    let done = current_labels(labels, cases);
    llm_cases(cases)
        .into_iter()
        .find(|c| !done.iter().any(|(key, _)| json!(key) == id_of(c)))
}

#[derive(Default, Clone, Debug)]
pub struct Agreement {
    pub rows: Vec<(String, String, String)>,
    pub stale: Vec<String>,
}

/// Python `format(value, ".0%")`.
pub fn percent(value: f64) -> String {
    format!("{:.0}%", value * 100.0)
}

impl Agreement {
    pub fn n(&self) -> usize {
        self.rows.len()
    }

    pub fn agree(&self) -> usize {
        self.rows
            .iter()
            .filter(|(_, owner, judged)| owner == judged)
            .count()
    }

    pub fn rate(&self) -> f64 {
        if self.n() == 0 {
            0.0
        } else {
            self.agree() as f64 / self.n() as f64
        }
    }

    pub fn trusted(&self) -> bool {
        self.n() >= MIN_LABELS && self.rate() >= TARGET
    }

    pub fn decision(&self) -> String {
        if self.n() < MIN_LABELS {
            return format!("COLLECT: {}/{MIN_LABELS} labels; label more with `evals/run.sh show` and `evals/run.sh label <case> pass|fail`", self.n());
        }
        if self.rate() >= TARGET {
            return format!("KEEP: the judge is trusted (>= {} on >= {MIN_LABELS} labels); judged regression failures now fail the run", percent(TARGET));
        }
        if self.rate() < PIVOT {
            return format!(
                "PIVOT: below {}; switch to code graders per criterion (TASKS.md llm-evals)",
                percent(PIVOT)
            );
        }
        format!("CALIBRATE: between {} and {}; read the disagreements, fix the judge prompt or the criteria wording, and run again", percent(PIVOT), percent(TARGET))
    }
}

pub fn agreement(judge: &mut Judge, labels: &[Label], cases: &[Value]) -> Result<Agreement> {
    let llm = llm_cases(cases);
    let mut out = Agreement::default();
    for row in labels {
        let case_id = label_text(row, "case");
        let Some(case) = llm.iter().find(|c| id_of(c) == json!(case_id)) else {
            continue;
        };
        if label_text(row, "expect_sha") != expect_digest(case) {
            out.stale.push(case_id);
            continue;
        }
        let verdicts = judge.grade(case, &label_text(row, "answer"))?;
        out.rows.push((
            case_id,
            label_text(row, "label"),
            case_verdict(&verdicts).to_owned(),
        ));
    }
    Ok(out)
}

pub fn agreement_lines(a: &Agreement, judge: &Judge) -> Vec<String> {
    let route = format!(
        "route {} ({})",
        judge.judge_route.name, judge.judge_route.model
    );
    let mut head = if a.n() > 0 {
        format!(
            "judge agreement: {}/{} labels agree ({}) on {route}",
            a.agree(),
            a.n(),
            percent(a.rate())
        )
    } else {
        format!("judge agreement: no owner labels yet ({route})")
    };
    if !a.stale.is_empty() {
        head.push_str(&format!(
            "; {} stale label(s) left out (criteria changed): {}",
            a.stale.len(),
            a.stale.join(", ")
        ));
    }
    let mut lines = vec![head];
    for (case, owner, judged) in &a.rows {
        if owner != judged {
            lines.push(format!("  disagree  {case}: owner {owner}, judge {judged}"));
        }
    }
    lines.push(format!("decision: {}", a.decision()));
    lines
}

pub fn run(
    cases: &[Value],
    judge: &mut Judge,
    labels: &[Label],
    progress: &mut dyn FnMut(String),
) -> Result<(Vec<String>, Vec<String>)> {
    let todo = llm_cases(cases);
    let mut judged = Vec::new();
    for (i, case) in todo.iter().enumerate() {
        let before = judge.calls;
        let answer = judge.answer(case)?;
        let verdicts = judge.grade(case, &answer)?;
        judged.push((case.clone(), verdicts));
        if judge.calls > before {
            progress(format!(
                "llm: {}/{} {}",
                i + 1,
                todo.len(),
                py_str(&id_of(case))
            ));
        }
    }
    let a = agreement(judge, labels, cases)?;
    let count = |kind: &str| {
        judged
            .iter()
            .filter(|(_, v)| case_verdict(v) == kind)
            .count()
    };
    let (ar, jr) = (&judge.answer_route, &judge.judge_route);
    let mut lines = vec![format!(
        "llm: {} case(s) answered on route {} ({}) and judged on route {} ({}): {} pass, {} fail, {} unknown",
        judged.len(),
        ar.name,
        ar.model,
        jr.name,
        jr.model,
        count("pass"),
        count("fail"),
        count("unknown")
    )];
    let mut failures = Vec::new();
    for (case, verdicts) in &judged {
        let result = case_verdict(verdicts);
        if result == "pass" {
            continue;
        }
        let (at, first) = if verdicts.is_empty() {
            (0, Verdict::new("UNKNOWN", "no criteria"))
        } else {
            let at = verdicts
                .iter()
                .position(|v| v.verdict != "PASS")
                .expect("a non-pass case has a non-PASS verdict");
            (at + 1, verdicts[at].clone())
        };
        let msg = format!(
            "{}: judged {result} (criterion {at}: {}, {})",
            py_str(&id_of(case)),
            first.verdict,
            first.evidence
        );
        let suite = field(case, "suite")
            .map(py_str)
            .unwrap_or_else(|| "regression".into());
        if a.trusted() && suite == "regression" {
            failures.push(msg);
        } else {
            let tail = if a.trusted() {
                ""
            } else {
                " [judge not trusted yet]"
            };
            lines.push(format!("  {msg}{tail}"));
        }
    }
    lines.extend(agreement_lines(&a, judge));
    Ok((lines, failures))
}

pub fn show_text(
    case: &Value,
    answer: &str,
    judge: &Judge,
    labels: &[Label],
    cases: &[Value],
) -> Result<String> {
    let done = current_labels(labels, cases);
    let opt = |key: &str| {
        field(case, key)
            .map(py_str)
            .unwrap_or_else(|| "None".into())
    };
    let id = required(case, "id")?;
    let mut lines = vec![
        format!(
            "case: {id} (skill {}, should {}, suite {})",
            opt("skill"),
            opt("should"),
            field(case, "suite")
                .map(py_str)
                .unwrap_or_else(|| "regression".into())
        ),
        format!("input: {}", required(case, "input")?),
        "criteria (pass means the answer meets every one):".to_owned(),
    ];
    for (i, c) in criteria(case).iter().enumerate() {
        lines.push(format!("  {}. {c}", i + 1));
    }
    lines.push(format!(
        "answer (route {}, {}):",
        judge.answer_route.name, judge.answer_route.model
    ));
    lines.push(answer.to_owned());
    lines.push("---".to_owned());
    if let Some((_, mine)) = done.iter().find(|(key, _)| *key == id) {
        if label_text(mine, "answer_sha") == digest(answer) {
            lines.push(format!(
                "your label: {} ({})",
                label_text(mine, "label"),
                label_text(mine, "labeled")
            ));
        }
    }
    lines.push(format!(
        "label it: evals/run.sh label {id} pass   (or fail)"
    ));
    lines.push(format!("labels: {}/{MIN_LABELS}", done.len()));
    Ok(lines.join("\n"))
}
