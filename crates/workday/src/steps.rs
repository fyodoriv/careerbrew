//! Work-step schema and workflow-resource resolution.

use crate::{field, step_error, text, Result};
use careerbrew_vault::cli::editor_argv;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const BUILTIN_STEPS: &[&str] = &["/careerbrew"];
pub const MAX_STEPS: usize = 30;
pub const MAX_ABOUT_WORDS: usize = 15;
pub const USE_KINDS: &[&str] = &["browser", "slack-draft", "file", "checkout"];
pub const MAX_LIST: usize = 10;
pub const MAX_VALUE: usize = 200;
pub const MAX_VERIFY: usize = 500;
pub const MAX_TIMEOUT: i64 = 1440;
const FIELDS: &[&str] = &[
    "scope",
    "after",
    "long_running",
    "uses",
    "verify",
    "timeout",
    "cycle",
    "review_ask_cycle",
    "watch",
    "about",
    "operator_turn",
];

pub fn normalize(name: &str) -> Result<String> {
    let name = name.trim();
    let pattern = Regex::new(r"^/?[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$").expect("step name");
    if !pattern.is_match(name) || name.contains("..") {
        return Err(step_error(format!(
            "step {:?}: use a command or skill name such as /standup (letters, digits, . _ : -)",
            crate::chars(name, 40)
        )));
    }
    Ok(format!("/{}", name.trim_start_matches('/')))
}

fn strings(value: Option<&Value>, field_name: &str, where_: &str) -> Result<Vec<String>> {
    let values = match value {
        None => Vec::new(),
        Some(Value::String(value)) => vec![Value::String(value.clone())],
        Some(Value::Array(values)) => values.clone(),
        Some(_) => {
            return Err(step_error(format!(
                "{where_}: `{field_name}` must be a string or a list of strings"
            )))
        }
    };
    if values.len() > MAX_LIST {
        return Err(step_error(format!(
            "{where_}: `{field_name}` holds at most {MAX_LIST} entries"
        )));
    }
    let mut out = Vec::new();
    for value in values {
        let Some(value) = value.as_str() else {
            return Err(step_error(format!(
                "{where_}: `{field_name}` must be a string or a list of strings"
            )));
        };
        if value.is_empty()
            || value.chars().count() > MAX_VALUE
            || value.chars().any(char::is_whitespace)
        {
            return Err(step_error(format!(
                "{where_}: `{field_name}` entry {:?} must be 1 to {MAX_VALUE} characters with no spaces",
                crate::chars(value, 40)
            )));
        }
        if !out.iter().any(|old| old == value) {
            out.push(value.to_owned());
        }
    }
    Ok(out)
}

fn check_verify(verify: &str, where_: &str) -> Result<()> {
    if verify.trim().is_empty() {
        return Err(step_error(format!(
            "{where_}: `verify` must be a command, such as `careerbrew feed check`"
        )));
    }
    if verify.chars().count() > MAX_VERIFY
        || verify.chars().any(|ch| matches!(ch, '\n' | '\r' | '\0'))
    {
        return Err(step_error(format!(
            "{where_}: `verify` must be one line of at most {MAX_VERIFY} characters"
        )));
    }
    let argv = editor_argv(verify)
        .map_err(|_| step_error(format!("{where_}: `verify` has an unclosed quote")))?;
    let mut head = argv;
    if head.first().is_some_and(|word| {
        Path::new(word)
            .file_name()
            .is_some_and(|name| name == "env")
    }) {
        head = head
            .into_iter()
            .skip(1)
            .filter(|word| !word.starts_with('-') && !word.contains('='))
            .collect();
    }
    if head.first().is_some_and(|word| {
        ["sh", "bash", "zsh"].contains(
            &Path::new(word)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
        )
    }) && head.iter().skip(1).any(|word| word == "-c")
    {
        let index = head.iter().position(|word| word == "-c").unwrap_or(0);
        let script = head.get(index + 1).cloned().unwrap_or_default();
        head = script
            .split_whitespace()
            .next()
            .into_iter()
            .map(str::to_owned)
            .collect();
    }
    let bare = Regex::new(r"^python(?:3(?:\.\d+)?)?$").expect("bare Python");
    if head.first().is_some_and(|word| bare.is_match(word)) {
        return Err(step_error(format!(
            "{where_}: `verify` names its interpreter: `uv run python -m unittest` or `.venv/bin/python`, never bare {} (it lacks the project's packages; issue #260)",
            head[0]
        )));
    }
    Ok(())
}

fn check_cycle(fields: &Map<String, Value>, key: &str, where_: &str) -> Result<Option<String>> {
    let Some(value) = fields.get(key) else {
        return Ok(None);
    };
    let Some(cycle) = value.as_str() else {
        return Err(step_error(format!(
            "{where_}: `{key}` must be a command, such as `careerbrew work feed cycle`"
        )));
    };
    if cycle.trim().is_empty() {
        return Err(step_error(format!(
            "{where_}: `{key}` must be a command, such as `careerbrew work feed cycle`"
        )));
    }
    if cycle.chars().count() > MAX_VERIFY
        || cycle.chars().any(|ch| matches!(ch, '\n' | '\r' | '\0'))
    {
        return Err(step_error(format!(
            "{where_}: `{key}` must be one line of at most {MAX_VERIFY} characters"
        )));
    }
    editor_argv(cycle)
        .map_err(|_| step_error(format!("{where_}: `{key}` has an unclosed quote")))?;
    Ok(Some(cycle.trim().to_owned()))
}

pub fn entry(raw: &Value, origin: &str) -> Result<Value> {
    let mut fields = Map::new();
    let raw = match raw {
        Value::Object(items) => {
            if !items.contains_key("step") {
                return Err(step_error(format!(
                    "a work step from {origin} is a mapping without `step:`"
                )));
            }
            for (key, value) in items {
                if key == "step" {
                    continue;
                }
                let normalized = match key.as_str() {
                    "longRunning" => "long_running",
                    "operatorTurn" => "operator_turn",
                    other => other,
                };
                fields.insert(normalized.to_owned(), value.clone());
            }
            &items["step"]
        }
        value => value,
    };
    let Some(raw) = raw.as_str() else {
        return Err(step_error(format!(
            "a work step from {origin} must be a name or a mapping with `step:`"
        )));
    };
    let step = normalize(raw)?;
    let where_ = format!("work step {step} from {origin}");
    let mut unknown = fields
        .keys()
        .filter(|key| !FIELDS.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    unknown.sort();
    if !unknown.is_empty() {
        return Err(step_error(format!(
            "{where_}: unknown field(s) {}; a step declares {} (docs/plugins.md)",
            unknown.join(", "),
            FIELDS.join(", ")
        )));
    }
    let long_running = fields
        .get("long_running")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if fields
        .get("long_running")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err(step_error(format!(
            "{where_}: `longRunning` is true or false"
        )));
    }
    let operator_turn = fields
        .get("operator_turn")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if fields
        .get("operator_turn")
        .is_some_and(|value| !value.is_boolean())
        || (operator_turn && long_running)
    {
        return Err(step_error(format!("{where_}: operatorTurn is true or false and cannot be longRunning; it only dispatches quick work")));
    }
    let uses = strings(fields.get("uses"), "uses", &where_)?;
    let use_pattern = Regex::new(r"^(?:browser|slack-draft|file|checkout):\S+$").expect("resource");
    for use_ in &uses {
        if !use_pattern.is_match(use_) {
            return Err(step_error(format!(
                "{where_}: `uses` entry {:?} must be <kind>:<name> with kind {} (for example browser:sso)",
                crate::chars(use_, 40),
                USE_KINDS.join(", ")
            )));
        }
    }
    if let Some(verify) = fields.get("verify") {
        let Some(verify) = verify.as_str() else {
            return Err(step_error(format!(
                "{where_}: `verify` must be a command, such as `careerbrew feed check`"
            )));
        };
        check_verify(verify, &where_)?;
    }
    let about = fields.get("about").and_then(Value::as_str).unwrap_or("");
    if fields.get("about").is_some_and(|value| !value.is_string())
        || (fields.contains_key("about") && about.trim().is_empty())
        || about.chars().any(|ch| matches!(ch, '\n' | '\r' | '\0'))
        || about.split_whitespace().count() > MAX_ABOUT_WORDS
    {
        return Err(step_error(format!("{where_}: `about` is one plain line of at most {MAX_ABOUT_WORDS} words that says what the step does")));
    }
    let timeout = fields
        .get("timeout")
        .and_then(Value::as_i64)
        .or(if operator_turn { Some(3) } else { None });
    if fields
        .get("timeout")
        .is_some_and(|value| value.is_boolean() || value.as_i64().is_none())
    {
        return Err(step_error(format!("{where_}: `timeout` is whole minutes from 1 to {MAX_TIMEOUT}, for example `timeout: 30`")));
    }
    if timeout.is_some() && long_running {
        return Err(step_error(format!(
            "{where_}: a long-running step never ends, so it takes no `timeout`"
        )));
    }
    if timeout.is_some_and(|value| !(1..=MAX_TIMEOUT).contains(&value)) {
        return Err(step_error(format!("{where_}: `timeout` is whole minutes from 1 to {MAX_TIMEOUT}, for example `timeout: 30`")));
    }
    let after = strings(fields.get("after"), "after", &where_)?
        .iter()
        .map(|value| normalize(value))
        .collect::<Result<Vec<_>>>()?;
    let watch = strings(fields.get("watch"), "watch", &where_)?;
    let watch_pattern = Regex::new(r"^(?:repo|feed|file):\S+$").expect("watch input");
    for input in &watch {
        if !watch_pattern.is_match(input) {
            return Err(step_error(format!(
                "{where_}: `watch` entry {:?} must start with repo:, feed:, or file: (for example feed:chat)",
                crate::chars(input, 40)
            )));
        }
    }
    let mut out = json!({
        "step": step,
        "from": origin,
        "scope": strings(fields.get("scope"), "scope", &where_)?,
        "after": after,
        "long_running": long_running,
        "operator_turn": operator_turn,
        "uses": uses,
        "verify": fields.get("verify").and_then(Value::as_str).unwrap_or("").trim(),
        "timeout": timeout,
        "about": about.trim(),
        "cycle": "",
        "watch": watch,
    });
    for key in ["cycle", "review_ask_cycle"] {
        if let Some(command) = check_cycle(&fields, key, &where_)? {
            out[key] = json!(command);
        }
    }
    Ok(out)
}

pub fn validate(raw: &Value, origin: &str) -> Result<Vec<Value>> {
    if raw.is_null() || raw.as_str().is_some_and(str::is_empty) {
        return Ok(Vec::new());
    }
    let Some(raw) = raw.as_array() else {
        return Err(step_error(format!(
            "work_steps from {origin} must be a list of step names"
        )));
    };
    if raw.len() > MAX_STEPS {
        return Err(step_error(format!(
            "work_steps from {origin}: at most {MAX_STEPS} steps"
        )));
    }
    raw.iter().map(|value| entry(value, origin)).collect()
}

pub fn first_sentence(value: &str) -> String {
    let value = value
        .trim()
        .trim_matches(|ch| matches!(ch, '"' | '\''))
        .trim();
    let mut end = value.len();
    let chars = value.char_indices().collect::<Vec<_>>();
    for (index, (at, ch)) in chars.iter().enumerate() {
        if matches!(ch, '.' | '!' | '?') {
            let next = chars.get(index + 1).map(|(_, ch)| *ch);
            if next.is_none_or(char::is_whitespace) {
                end = at + ch.len_utf8();
                break;
            }
        }
    }
    value[..end].chars().take(160).collect()
}

fn resolution(
    path: Option<PathBuf>,
    user_only: bool,
    description: String,
    source_error: Option<String>,
) -> Value {
    let mut out = json!({"path": path.map(|path| path.to_string_lossy().into_owned()), "user_only": user_only, "description": description});
    if let Some(source_error) = source_error {
        out["source_error"] = json!(source_error);
    }
    out
}

fn read_resolution(path: &Path, owned: bool) -> Result<Value> {
    if !owned && !path.is_file() {
        return Ok(resolution(None, false, String::new(), None));
    }
    let head = match std::fs::read_to_string(path) {
        Ok(text) => text.chars().take(2000).collect::<String>(),
        Err(_) if owned => {
            return Ok(resolution(None, false, String::new(), Some("own declared workflow source changed or is unreadable; repair it before this step".into())));
        }
        Err(_) => String::new(),
    };
    let normalized = head.replace("\r\n", "\n");
    let front = Regex::new(r"(?s)^---\n(.*?)\n---")
        .expect("frontmatter")
        .captures(&normalized)
        .and_then(|parts| parts.get(1).map(|value| value.as_str().to_owned()));
    let user_only = front.as_ref().is_some_and(|front| {
        Regex::new(r#"(?m)^disable-model-invocation:\s*[\"']?true[\"']?\s*(#.*)?$"#)
            .expect("user only")
            .is_match(front)
    });
    let raw_description = front.as_ref().and_then(|front| {
        Regex::new(r"(?m)^description:\s*(.+)$")
            .expect("description")
            .captures(front)
            .and_then(|parts| parts.get(1).map(|value| value.as_str().to_owned()))
    });
    let mut description = raw_description.clone().unwrap_or_default();
    if let (Some(front), Some(raw)) = (front, raw_description.as_ref()) {
        if Regex::new(r">(?:[+-]?[1-9]?|[1-9][+-]?)(?:\s+#.*)?\s*")
            .expect("odd description")
            .is_match(raw)
        {
            if let Ok(metadata) = careerbrew_setup::value::yaml_read(&front) {
                if let Some(parsed) = field(&metadata, "description").and_then(Value::as_str) {
                    description = parsed.to_owned();
                }
            }
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let raw_path = path.to_string_lossy().into_owned();
    let shown = if !home.is_empty() && raw_path.starts_with(&(home.clone() + "/")) {
        format!("~{}", &raw_path[home.len()..])
    } else {
        raw_path
    };
    Ok(resolution(
        Some(PathBuf::from(shown)),
        user_only,
        if raw_description.is_some() {
            first_sentence(&description)
        } else {
            String::new()
        },
        None,
    ))
}

pub fn resolve(step: &str, roots: &[PathBuf]) -> Result<Value> {
    let step = normalize(step)?;
    let name = step.trim_start_matches('/').replace(':', "/");
    if name.is_empty()
        || Path::new(&name).is_absolute()
        || name.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Ok(resolution(
            None,
            false,
            String::new(),
            Some("workflow resource name is empty or unsafe; use a relative step name".into()),
        ));
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let mut candidates = Vec::new();
    for root in roots {
        let skills = root.join("skills");
        let mut skill_names = vec![name.clone()];
        if skills.canonicalize().unwrap_or_else(|_| skills.clone()) == repo.join("skills")
            && !name.contains('/')
            && !name.starts_with("careerbrew-")
        {
            skill_names.insert(0, format!("careerbrew-{name}"));
        }
        for skill in skill_names {
            candidates.push((skills.join(skill).join("SKILL.md"), false));
        }
        let commands = root.join("commands");
        if commands.is_file() {
            if commands
                .file_name()
                .is_some_and(|file| file.to_string_lossy() == format!("{name}.md"))
            {
                candidates.push((commands, false));
            }
        } else {
            candidates.push((commands.join(format!("{name}.md")), false));
        }
        candidates.push((root.join("prompts").join(format!("{name}.md")), false));
    }
    for (path, owned) in candidates {
        if path.is_file() {
            return read_resolution(&path, owned);
        }
    }
    Ok(resolution(None, false, String::new(), None))
}

fn plugin_dirs(origin: &str) -> Result<Vec<(Option<PathBuf>, Option<PathBuf>)>> {
    let installed = careerbrew_setup::plugins::installed()
        .map_err(|error| crate::error(error.class, error.message))?;
    let mut out = Vec::new();
    for plugin in installed {
        if !origin.is_empty() && format!("plugin:{}", plugin.name) != origin {
            continue;
        }
        let provides = field(&plugin.manifest, "provides")
            .cloned()
            .unwrap_or_else(|| json!({}));
        for kind in ["skills", "commands"] {
            let raw = field(&provides, kind).cloned().unwrap_or(Value::Null);
            let items = match raw {
                Value::Array(items) => items,
                Value::Null => Vec::new(),
                item => vec![item],
            };
            for item in items
                .into_iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
            {
                let pattern = plugin.root.join(item).to_string_lossy().into_owned();
                for path in glob::glob(&pattern)
                    .map_err(|_| step_error("invalid plugin resource pattern"))?
                    .flatten()
                {
                    if path.is_dir()
                        || (kind == "commands"
                            && path.is_file()
                            && path.extension().is_some_and(|ext| ext == "md"))
                    {
                        out.push(if kind == "skills" {
                            (Some(path), None)
                        } else {
                            (None, Some(path))
                        });
                    }
                }
            }
        }
    }
    Ok(out)
}

pub fn resolve_origin(step: &str, origin: &str) -> Result<Value> {
    if origin.starts_with("plugin:") {
        let step = normalize(step)?;
        let name = step.trim_start_matches('/').replace(':', "/");
        for (skills, commands) in plugin_dirs(origin)? {
            let mut candidates = Vec::new();
            if let Some(skills) = skills {
                candidates.push(skills.join(&name).join("SKILL.md"));
            }
            if let Some(commands) = commands {
                if commands.is_file() {
                    if commands
                        .file_name()
                        .is_some_and(|file| file.to_string_lossy() == format!("{name}.md"))
                    {
                        candidates.push(commands);
                    }
                } else {
                    candidates.push(commands.join(format!("{name}.md")));
                }
            }
            for path in candidates {
                if path.is_file() {
                    return read_resolution(&path, false);
                }
            }
        }
        return Ok(resolution(None, false, String::new(), Some(format!("plugin source unavailable for {step}; repair its declared plugin resource before this step"))));
    }
    let roots = vec![
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".agents"),
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".codex"),
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".claude"),
    ];
    resolve(step, &roots)
}

pub fn about(step: &Value, resolved: &Value) -> String {
    let own = text(field(step, "about"));
    if !own.is_empty() {
        own
    } else {
        text(field(resolved, "description"))
    }
}

pub fn missing_about(found: &[Value]) -> Vec<String> {
    let mut out = found
        .iter()
        .filter(|step| text(field(step, "about")).is_empty())
        .map(|step| text(field(step, "step")))
        .collect::<Vec<_>>();
    out.sort();
    out
}

pub fn declared(
    root: &Path,
    configured: &Value,
    plugins: &[(String, Value)],
    default_timeout: i64,
) -> Result<Vec<Value>> {
    let mut found = vec![entry(&json!("/careerbrew"), "careerbrew")?];
    found.extend(validate(configured, "settings")?);
    for (origin, raw) in plugins {
        found.extend(validate(raw, origin)?);
    }
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for step in found {
        let name = text(field(&step, "step"));
        if seen.contains(&name) {
            continue;
        }
        seen.push(name);
        let origin = text(field(&step, "from"));
        let resolved = resolve_origin(&text(field(&step, "step")), &origin)?;
        let long_running = field(&step, "long_running")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let own_timeout = field(&step, "timeout")
            .and_then(Value::as_i64)
            .unwrap_or(default_timeout);
        let mut step = step;
        step["timeout"] = if long_running {
            Value::Null
        } else {
            json!(own_timeout)
        };
        if let Some(items) = resolved.as_object() {
            for (key, value) in items {
                if key != "description" {
                    step[key] = value.clone();
                }
            }
        }
        step["about"] = json!(about(&step, &resolved));
        out.push(step);
    }
    let _ = root;
    Ok(out)
}
