//! Component port of plugin validation, policy, lifecycle and command sources.

use crate::value;
use crate::{context, inside, resolved, Result, SetupError};
use careerbrew_vault::transfer::Context;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

pub const MANIFEST: &str = "careerbrew-plugin.yaml";
pub const RESOURCE_KINDS: &[&str] = &[
    "ladders",
    "job_sources",
    "rules",
    "prompts",
    "skills",
    "evidence_sources",
    "knowledge",
    "rubrics",
    "commands",
];
const LIST_POLICY: &[&str] = &[
    "allowed_routes",
    "allowed_connectors",
    "allowed_hosts",
    "secret_backends",
    "backup_targets",
];
const BOOL_POLICY: &[&str] = &[
    "agent_may_read_business",
    "require_encrypted_backup",
    "public_metrics",
];
const EXPORT_LEVELS: &[&str] = &["none", "aggregates", "redacted", "full"];

fn error(message: impl Into<String>) -> SetupError {
    SetupError::plugin(message)
}

fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (2..=40).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn string_list(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.iter().all(Value::is_string))
}

fn version_tuple(text: &str) -> Vec<u64> {
    let mut parts = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_ascii_digit() {
            current.push(character);
        } else if !current.is_empty() {
            parts.push(current.parse().unwrap_or(0));
            current.clear();
        }
        if parts.len() == 3 {
            break;
        }
    }
    if !current.is_empty() && parts.len() < 3 {
        parts.push(current.parse().unwrap_or(0));
    }
    parts
}

fn has_parent(path: &Path) -> bool {
    path.components()
        .any(|part| matches!(part, Component::ParentDir))
}

fn check_shapes(data: &Value, terms_inside: bool) -> Result<()> {
    let map = data.as_object().ok_or_else(|| error("manifest shape"))?;
    if let Some(required) = map.get("requires").filter(|v| !v.is_null()) {
        let text = match required {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            _ => String::new(),
        };
        let empty_literal = matches!(required, Value::String(value) if value.is_empty());
        if !empty_literal
            && (!text.bytes().any(|byte| byte.is_ascii_digit())
                || text.chars().any(|c| c.is_numeric() && !c.is_ascii()))
        {
            return Err(error(
                "requires must name a version with ASCII digits, e.g. >=0.1.0",
            ));
        }
    }
    if map
        .get("provides")
        .is_some_and(|v| !v.is_null() && !v.is_object())
    {
        return Err(error(
            "provides must be a mapping of resource kind to paths",
        ));
    }
    if let Some(policy) = map.get("policy").and_then(Value::as_object) {
        for key in LIST_POLICY {
            if policy.contains_key(*key) && !string_list(&policy[*key]) {
                return Err(error(format!("policy.{key} must be a list of strings")));
            }
        }
        for key in BOOL_POLICY {
            if policy.contains_key(*key) && !policy[*key].is_boolean() {
                return Err(error(format!("policy.{key} must be true or false")));
            }
        }
        if policy.get("export_to_personal").is_some_and(|level| {
            !level
                .as_str()
                .is_some_and(|level| EXPORT_LEVELS.contains(&level))
        }) {
            return Err(error(
                "policy.export_to_personal must be one of ['none', 'aggregates', 'redacted', 'full']",
            ));
        }
    }
    if let Some(terms) = map.get("sensitive_terms").filter(|v| !v.is_null()) {
        let path = terms.as_str().map(Path::new);
        if !terms_inside
            || path.is_none_or(|path| {
                path.as_os_str().is_empty() || path.is_absolute() || has_parent(path)
            })
        {
            return Err(error(
                "sensitive_terms must be a relative file path inside the plugin folder",
            ));
        }
    }
    if let Some(detect) = map.get("detect").filter(|v| !v.is_null()) {
        let detect = detect
            .as_object()
            .ok_or_else(|| error("detect must be a mapping with env and paths lists"))?;
        for key in ["env", "paths"] {
            if detect.contains_key(key) && !string_list(&detect[key]) {
                return Err(error(format!("detect.{key} must be a list of strings")));
            }
        }
    }
    Ok(())
}

/// Pure manifest decision over the supplied filesystem facts.
pub fn validate_manifest_data(
    data: &Value,
    facts: &Value,
    version: &str,
    step_error: &dyn Fn(Option<&Value>, &str) -> Option<String>,
) -> Result<Value> {
    let map = data
        .as_object()
        .ok_or_else(|| error("careerbrew-plugin.yaml must be a mapping"))?;
    let name = map.get("name").and_then(Value::as_str).unwrap_or("");
    if !valid_name(name) {
        return Err(error(
            "plugin name must match ^[a-z][a-z0-9-]{1,39}$ (fix: set `name:` in the manifest)",
        ));
    }
    if !map.contains_key("version") {
        return Err(error("plugin manifest needs `version:`"));
    }
    check_shapes(data, value::bool_value(value::get(facts, "terms_inside")))?;
    let required = value::text(map.get("requires"))
        .trim_start_matches(['>', '=', ' '])
        .to_owned();
    if !required.is_empty() && version_tuple(version) < version_tuple(&required) {
        return Err(error(format!(
            "plugin needs careerbrew >= {required}; this is {version} (fix: update careerbrew)"
        )));
    }
    if let Some(links) = value::get(facts, "symlinks").and_then(Value::as_array) {
        if let Some(link) = links.first().and_then(Value::as_str) {
            return Err(error(format!(
                "plugin contains a symlink ({link}); plugins must be plain files"
            )));
        }
    }
    let empty = json!({});
    let provides = map.get("provides").and_then(Value::as_object);
    if let Some(provides) = provides {
        for kind in provides.keys() {
            if !RESOURCE_KINDS.contains(&kind.as_str()) {
                return Err(error(format!("unknown provides key: {kind}")));
            }
        }
        for (kind, paths) in provides {
            for item in value::strings(Some(paths)) {
                let fact = value::get(facts, "provides")
                    .and_then(Value::as_object)
                    .and_then(|items| items.get(&item))
                    .unwrap_or(&empty);
                let path = Path::new(&item);
                if path.is_absolute()
                    || has_parent(path)
                    || !value::bool_value(value::get(fact, "inside"))
                {
                    return Err(error(format!(
                        "provides.{kind} escapes the plugin folder: {item}"
                    )));
                }
                if !value::bool_value(value::get(fact, "exists")) {
                    return Err(error(format!(
                        "provides.{kind} points to a missing path: {item}"
                    )));
                }
            }
        }
    }
    if map
        .get("policy")
        .is_some_and(|v| !v.is_null() && !v.is_object())
    {
        return Err(error("policy must be a mapping"));
    }
    if map.get("connector_defaults").is_some_and(|defaults| {
        !defaults.is_null()
            && (defaults.as_object().is_none()
                || defaults
                    .as_object()
                    .is_some_and(|items| items.values().any(|item| !item.is_object())))
    }) {
        return Err(error(
            "connector_defaults maps a connector kind to its fields",
        ));
    }
    if map.get("agent_reads").is_some_and(|reads| {
        !reads.is_null()
            && reads
                .as_array()
                .is_none_or(|items| items.iter().any(|item| !item.is_string()))
    }) {
        return Err(error("agent_reads is a list of connector kinds"));
    }
    if let Some(live) = map.get("live").filter(|v| !v.is_null()) {
        let live = live
            .as_object()
            .ok_or_else(|| error("live maps sources and team to files in the plugin"))?;
        for (key, rel) in live {
            if !matches!(key.as_str(), "sources" | "team") {
                return Err(error("live maps only sources and team"));
            }
            let rel = value::text(Some(rel));
            let fact = value::get(facts, "live")
                .and_then(Value::as_object)
                .and_then(|items| items.get(&rel))
                .unwrap_or(&empty);
            if Path::new(&rel).is_absolute()
                || !value::bool_value(value::get(fact, "inside"))
                || !value::bool_value(value::get(fact, "is_file"))
            {
                return Err(error(format!(
                    "live.{key} must be a file inside the plugin folder: {rel}"
                )));
            }
        }
    }
    if let Some(problem) = step_error(map.get("work_steps"), &format!("plugin:{name}")) {
        return Err(error(format!(
            "{problem} (fix: list names like /standup under `work_steps:`)"
        )));
    }
    Ok(data.clone())
}

fn symlink_paths(root: &Path, current: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(current)? {
        let path = entry?.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            out.push(
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned(),
            );
        } else if metadata.is_dir() {
            symlink_paths(root, &path, out)?;
        }
    }
    Ok(())
}

/// Probe only the facts the pure manifest decision consumes.
pub fn manifest_facts(data: &Value, root: &Path) -> Result<Value> {
    let mut links = Vec::new();
    symlink_paths(root, root, &mut links)?;
    links.sort();
    let mut facts = json!({"symlinks": links, "provides": {}, "live": {}, "terms_inside": true});
    if let Some(provides) = value::get(data, "provides").and_then(Value::as_object) {
        for paths in provides.values() {
            for item in value::strings(Some(paths)) {
                let path = Path::new(&item);
                let target = root.join(&item);
                let valid = !path.is_absolute() && !has_parent(path) && inside(&target, root);
                let exists = if valid {
                    glob::glob(&target.to_string_lossy())
                        .ok()
                        .is_some_and(|mut matches| matches.next().is_some())
                } else {
                    false
                };
                value::set(
                    value::get_mut(&mut facts, "provides").unwrap(),
                    &item,
                    json!({"inside": valid, "exists": exists}),
                );
            }
        }
    }
    if let Some(live) = value::get(data, "live").and_then(Value::as_object) {
        for rel in live.values() {
            let rel = value::text(Some(rel));
            let target = root.join(&rel);
            value::set(
                value::get_mut(&mut facts, "live").unwrap(),
                &rel,
                json!({"inside": inside(&target, root), "is_file": target.is_file()}),
            );
        }
    }
    if let Some(terms) = value::get(data, "sensitive_terms").and_then(Value::as_str) {
        let path = Path::new(terms);
        if !terms.is_empty() && !path.is_absolute() && !has_parent(path) {
            value::set(
                &mut facts,
                "terms_inside",
                Value::Bool(inside(&root.join(path), root)),
            );
        }
    }
    Ok(facts)
}

fn step_name(raw: &str) -> Result<String> {
    let name = raw.trim();
    let body = name.strip_prefix('/').unwrap_or(name);
    if body.is_empty()
        || body.len() > 64
        || !body
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        || !body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
        || body.contains("..")
    {
        return Err(error("use a command or skill name such as /standup"));
    }
    Ok(format!("/{body}"))
}

fn step_strings(value: Option<&Value>, field: &str) -> Result<Vec<String>> {
    let items: Vec<Value> = match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(item)) => vec![Value::String(item.clone())],
        Some(Value::Array(items)) => items.clone(),
        _ => {
            return Err(error(format!(
                "`{field}` must be a string or a list of strings"
            )))
        }
    };
    if items.len() > 10 {
        return Err(error(format!("`{field}` holds at most 10 entries")));
    }
    let mut out = Vec::new();
    for item in items {
        let item = item
            .as_str()
            .ok_or_else(|| error(format!("`{field}` must contain strings")))?;
        if item.is_empty() || item.len() > 200 || item.chars().any(char::is_whitespace) {
            return Err(error(format!(
                "`{field}` entries need 1 to 200 characters with no spaces"
            )));
        }
        if !out.iter().any(|known| known == item) {
            out.push(item.to_owned());
        }
    }
    Ok(out)
}

fn command_line(value: Option<&Value>, field: &str) -> Result<()> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let command = value
        .as_str()
        .ok_or_else(|| error(format!("`{field}` must be a command")))?;
    if command.trim().is_empty()
        || command.len() > 500
        || command.chars().any(|c| matches!(c, '\n' | '\r' | '\0'))
    {
        return Err(error(format!(
            "`{field}` must be one line of at most 500 characters"
        )));
    }
    let mut quoted = None;
    let mut escaped = false;
    for character in command.chars() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if matches!(character, '\'' | '"') {
            if quoted == Some(character) {
                quoted = None;
            } else if quoted.is_none() {
                quoted = Some(character);
            }
        }
    }
    if quoted.is_some() {
        return Err(error(format!("`{field}` has an unclosed quote")));
    }
    let first = command
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");
    if matches!(first, "python" | "python3") || first.starts_with("python3.") {
        return Err(error(format!(
            "`{field}` names its interpreter; never bare {first}"
        )));
    }
    Ok(())
}

/// Real provider for the pure manifest decision; no work-step field is ignored.
pub fn work_step_error(raw: Option<&Value>, origin: &str) -> Option<String> {
    fn validate(raw: Option<&Value>, origin: &str) -> Result<()> {
        let Some(raw) = raw.filter(|value| !value.is_null()) else {
            return Ok(());
        };
        if raw.as_str() == Some("") {
            return Ok(());
        }
        let entries = raw.as_array().ok_or_else(|| {
            error(format!(
                "work_steps from {origin} must be a list of step names"
            ))
        })?;
        if entries.len() > 30 {
            return Err(error(format!("work_steps from {origin}: at most 30 steps")));
        }
        for entry in entries {
            let (raw_name, fields) = match entry {
                Value::String(name) => (name.clone(), Map::new()),
                Value::Object(fields) => {
                    let Some(name) = fields.get("step").and_then(Value::as_str) else {
                        return Err(error(format!(
                            "a work step from {origin} needs a text `step:`"
                        )));
                    };
                    (name.to_owned(), fields.clone())
                }
                _ => {
                    return Err(error(format!(
                        "a work step from {origin} must be a name or a mapping with `step:`"
                    )))
                }
            };
            let step = step_name(&raw_name)?;
            let where_ = format!("work step {step} from {origin}");
            let aliases = [
                ("longRunning", "long_running"),
                ("operatorTurn", "operator_turn"),
            ];
            let mut normalized = Map::new();
            for (key, value) in fields {
                if key.as_str() == "step" {
                    continue;
                }
                let key = aliases
                    .iter()
                    .find_map(|(old, new)| (key.as_str() == *old).then_some(*new))
                    .unwrap_or(key.as_str());
                normalized.insert(key.to_owned(), value);
            }
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
            if let Some(unknown) = normalized
                .keys()
                .find(|key| !FIELDS.contains(&key.as_str()))
            {
                return Err(error(format!("{where_}: unknown field(s) {unknown}")));
            }
            let long = normalized
                .get("long_running")
                .unwrap_or(&Value::Bool(false));
            if !long.is_boolean() {
                return Err(error(format!("{where_}: `longRunning` is true or false")));
            }
            let operator = normalized
                .get("operator_turn")
                .unwrap_or(&Value::Bool(false));
            if !operator.is_boolean()
                || (operator == &Value::Bool(true) && long == &Value::Bool(true))
            {
                return Err(error(format!(
                    "{where_}: operatorTurn cannot be longRunning"
                )));
            }
            for field in ["scope", "after", "uses", "watch"] {
                for item in step_strings(normalized.get(field), field)? {
                    if field == "after" {
                        step_name(&item)?;
                    } else if field == "uses"
                        && !["browser", "slack-draft", "file", "checkout"]
                            .iter()
                            .any(|kind| {
                                item.starts_with(&format!("{kind}:")) && item.len() > kind.len() + 1
                            })
                    {
                        return Err(error(format!("{where_}: invalid `uses` entry")));
                    } else if field == "watch"
                        && !["repo", "feed", "file"].iter().any(|kind| {
                            item.starts_with(&format!("{kind}:")) && item.len() > kind.len() + 1
                        })
                    {
                        return Err(error(format!("{where_}: invalid `watch` entry")));
                    }
                }
            }
            for field in ["verify", "cycle", "review_ask_cycle"] {
                command_line(normalized.get(field), field)?;
            }
            if let Some(about) = normalized.get("about") {
                let about = about
                    .as_str()
                    .ok_or_else(|| error(format!("{where_}: `about` must be text")))?;
                if about.trim().is_empty()
                    || about.chars().any(|c| matches!(c, '\n' | '\r' | '\0'))
                    || about.split_whitespace().count() > 15
                {
                    return Err(error(format!(
                        "{where_}: `about` is one plain line of at most 15 words"
                    )));
                }
            }
            if let Some(timeout) = normalized.get("timeout").filter(|value| !value.is_null()) {
                if long == &Value::Bool(true)
                    || timeout.is_boolean()
                    || !timeout
                        .as_u64()
                        .is_some_and(|minutes| (1..=1440).contains(&minutes))
                {
                    return Err(error(format!("{where_}: `timeout` is whole minutes from 1 to 1440 and never applies to a long-running step")));
                }
            }
        }
        Ok(())
    }
    validate(raw, origin).err().map(|error| error.message)
}

fn careerbrew_version() -> Result<String> {
    let text = std::fs::read_to_string(std::env::current_dir()?.join("pyproject.toml"))?;
    text.lines()
        .find_map(|line| line.trim().strip_prefix("version = "))
        .map(|value| value.trim_matches('"').to_owned())
        .ok_or_else(|| error("careerbrew version is unavailable"))
}

pub fn validate_manifest(data: &Value, root: &Path) -> Result<Value> {
    let facts = manifest_facts(data, root)?;
    validate_manifest_data(data, &facts, &careerbrew_version()?, &work_step_error)
}

pub fn refresh_order(lock: &Value) -> Vec<String> {
    let plugins = value::get(lock, "plugins")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    fn visit(
        name: &str,
        plugins: &serde_json::Map<String, Value>,
        order: &mut Vec<String>,
        seen: &mut Vec<String>,
    ) {
        if order.iter().any(|item| item == name)
            || seen.iter().any(|item| item == name)
            || !plugins.contains_key(name)
        {
            return;
        }
        seen.push(name.to_owned());
        let base = value::get(&plugins[name], "extends")
            .and_then(Value::as_str)
            .unwrap_or("");
        visit(base, plugins, order, seen);
        order.push(name.to_owned());
    }
    let mut order = Vec::new();
    for name in plugins.keys() {
        visit(name, &plugins, &mut order, &mut Vec::new());
    }
    order
}

/// Strictest policy merge over supplied manifests and expanded target paths.
pub fn merge_policy(manifests: &[Value], expansions: &BTreeMap<String, String>) -> Value {
    let mut policy = json!({
        "allowed_routes": null,
        "allowed_connectors": null,
        "allowed_hosts": null,
        "secret_backends": null,
        "require_encrypted_backup": false,
        "backup_targets": null,
        "export_to_personal": null,
        "agent_may_read_business": false,
        "public_metrics": true,
    });
    let mut votes = Vec::new();
    for manifest in manifests {
        let empty = json!({});
        let pol = value::get(manifest, "policy")
            .and_then(Value::as_object)
            .unwrap_or_else(|| empty.as_object().unwrap());
        for key in &LIST_POLICY[..4] {
            if let Some(value) = pol.get(*key) {
                let mine = value::strings(Some(value))
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                let current = value::get(&policy, key)
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect::<BTreeSet<_>>()
                    });
                let merged = current.map_or(mine.clone(), |current| {
                    current.intersection(&mine).cloned().collect()
                });
                value::set(
                    &mut policy,
                    key,
                    Value::Array(merged.into_iter().map(Value::String).collect()),
                );
            }
        }
        if let Some(level) = pol.get("export_to_personal").and_then(Value::as_str) {
            let level = if EXPORT_LEVELS.contains(&level) {
                level
            } else {
                "none"
            };
            let current = value::get(&policy, "export_to_personal")
                .and_then(Value::as_str)
                .unwrap_or("");
            let index = |name: &str| {
                EXPORT_LEVELS
                    .iter()
                    .position(|item| *item == name)
                    .unwrap_or(usize::MAX)
            };
            let selected = if current.is_empty() {
                level
            } else {
                EXPORT_LEVELS[index(current).min(index(level))]
            };
            value::set(&mut policy, "export_to_personal", json!(selected));
        }
        if pol.get("public_metrics") == Some(&Value::Bool(false)) {
            value::set(&mut policy, "public_metrics", Value::Bool(false));
        }
        if let Some(vote) = pol.get("agent_may_read_business") {
            votes.push(value::bool_value(Some(vote)));
        }
        if value::bool_value(pol.get("require_encrypted_backup")) {
            value::set(&mut policy, "require_encrypted_backup", Value::Bool(true));
        }
        if let Some(targets) = pol.get("backup_targets") {
            let mine = value::strings(Some(targets))
                .into_iter()
                .map(|target| expansions.get(&target).cloned().unwrap_or(target))
                .collect::<BTreeSet<_>>();
            let current = value::get(&policy, "backup_targets")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<BTreeSet<_>>()
                });
            let merged = current.map_or(mine.clone(), |current| {
                current.intersection(&mine).cloned().collect()
            });
            value::set(
                &mut policy,
                "backup_targets",
                Value::Array(merged.into_iter().map(Value::String).collect()),
            );
        }
    }
    value::set(
        &mut policy,
        "agent_may_read_business",
        Value::Bool(!votes.is_empty() && votes.iter().all(|vote| *vote)),
    );
    policy
}

pub fn lock(ctx: &Context) -> Result<Value> {
    let store = ctx.store();
    let value = store.read_json(
        &ctx.root.join("plugins.lock.json"),
        careerbrew_vault::canon::Canon::Map(vec![(
            careerbrew_vault::canon::Canon::Str("plugins".into()),
            careerbrew_vault::canon::Canon::Map(Vec::new()),
        )]),
        true,
    )?;
    Ok(value::from_canon(&value))
}

fn save_lock(ctx: &Context, value: &Value) -> Result<()> {
    ctx.store()
        .write_json(&ctx.root.join("plugins.lock.json"), &value::to_canon(value))?;
    Ok(())
}

pub fn plugins_dir(ctx: &Context) -> Result<PathBuf> {
    std::fs::create_dir_all(&ctx.root)?;
    std::fs::set_permissions(&ctx.root, std::fs::Permissions::from_mode(0o700))?;
    let folder = ctx.root.join("plugins");
    std::fs::create_dir_all(&folder)?;
    Ok(folder)
}

fn all_files(root: &Path, current: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(current)? {
        let path = entry?.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            all_files(root, &path, out)?;
        } else if metadata.is_file()
            && !path
                .strip_prefix(root)
                .unwrap_or(&path)
                .components()
                .any(|part| part.as_os_str() == ".git")
        {
            out.push(path);
        }
    }
    Ok(())
}

pub fn tree_hash(root: &Path) -> Result<String> {
    let mut files = Vec::new();
    all_files(root, root, &mut files)?;
    files.sort();
    let mut hash = Sha256::new();
    for path in files {
        hash.update(
            path.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .as_bytes(),
        );
        hash.update([0]);
        hash.update(std::fs::read(path)?);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn copy_tree(source: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let target = dest.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(entry.path())?, &target)?;
        } else if metadata.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
            std::fs::set_permissions(&target, metadata.permissions())?;
        }
    }
    Ok(())
}

pub fn command_files(manifest: &Value, root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let provides = value::get(manifest, "provides").and_then(Value::as_object);
    for item in provides
        .and_then(|items| items.get("commands"))
        .map(|value| value::strings(Some(value)))
        .unwrap_or_default()
    {
        let pattern = root.join(item);
        let mut matches = glob::glob(&pattern.to_string_lossy())
            .map_err(|_| error("invalid command glob"))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| error("command glob failed"))?;
        matches.sort();
        for path in matches {
            if path.is_dir() {
                let mut files = std::fs::read_dir(path)?
                    .collect::<std::result::Result<Vec<_>, _>>()?
                    .into_iter()
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
                    .collect::<Vec<_>>();
                files.sort();
                out.extend(files);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                out.push(path);
            }
        }
    }
    Ok(out)
}

fn commands_dir() -> PathBuf {
    std::env::var_os("CAREERBREW_COMMANDS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".claude/commands"))
}

fn check_commands(name: &str, manifest: &Value, root: &Path, lock: &Value) -> Result<()> {
    let mut owned = BTreeMap::new();
    if let Some(plugins) = value::get(lock, "plugins").and_then(Value::as_object) {
        for (other, entry) in plugins {
            for file in value::strings(value::get(entry, "commands")) {
                owned.insert(file, other.clone());
            }
        }
    }
    for file in command_files(manifest, root)? {
        let filename = file.file_name().unwrap().to_string_lossy().into_owned();
        let owner = owned.get(&filename);
        if owner.is_some_and(|owner| owner != name)
            || (commands_dir().join(&filename).exists() && owner != Some(&name.to_owned()))
        {
            return Err(error(format!(
                "command /{} already exists and plugin {name} does not own it",
                file.file_stem().unwrap().to_string_lossy()
            )));
        }
    }
    Ok(())
}

fn migrate_legacy(name: &str, lock: &Value) -> Result<()> {
    let ctx = context()?;
    let Some(entry) = value::get(lock, "plugins")
        .and_then(Value::as_object)
        .and_then(|items| items.get(name))
    else {
        return Ok(());
    };
    let owned = value::strings(value::get(entry, "commands"));
    let old_root = plugins_dir(&ctx)?.join(name);
    let manifest_path = old_root.join(MANIFEST);
    let manifest = if manifest_path.is_file() {
        value::from_canon(&ctx.store().read_yaml(
            &manifest_path,
            careerbrew_vault::canon::Canon::Map(Vec::new()),
            true,
        )?)
    } else {
        json!({})
    };
    let originals = command_files(&manifest, &old_root)?
        .into_iter()
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                path,
            )
        })
        .collect::<BTreeMap<_, _>>();
    for filename in &owned {
        let leaf = commands_dir().join(filename);
        if !leaf.exists() && !leaf.is_symlink() {
            continue;
        }
        let original = originals.get(filename);
        if leaf.is_symlink()
            || original.is_none_or(|path| {
                !path.is_file() || std::fs::read(path).ok() != std::fs::read(&leaf).ok()
            })
        {
            return Err(error("legacy command edited or prior source unavailable; preserve it and adopt changes into its source before refresh"));
        }
    }
    for filename in owned {
        let leaf = commands_dir().join(&filename);
        let Some(original) = originals.get(&filename) else {
            continue;
        };
        if !leaf.is_file() {
            continue;
        }
        let content = std::fs::read(&leaf)?;
        let digest = format!("{:x}", Sha256::digest(&content));
        let archive = ctx
            .root
            .join("command-archive")
            .join(name)
            .join(digest)
            .join(&filename);
        let text =
            String::from_utf8(content.clone()).map_err(|_| error("legacy command is not UTF-8"))?;
        if archive.exists() {
            if ctx.store().read_text(&archive, None, true)? != Some(text.clone()) {
                return Err(error(
                    "legacy command archive verification failed; existing archive preserved",
                ));
            }
        } else {
            ctx.store().write_text(&archive, &text)?;
        }
        if ctx.store().read_text(&archive, None, true)? != Some(text.clone()) {
            return Err(error(
                "legacy command archive verification failed; original preserved",
            ));
        }
        let mut receipt_path = archive.clone();
        receipt_path.set_extension("receipt.json");
        let receipt = json!({
            "sha256": format!("{:x}", Sha256::digest(&content)),
            "archive": archive.to_string_lossy(),
            "restore_target": leaf.to_string_lossy(),
            "source": original.to_string_lossy(),
        });
        ctx.store()
            .write_json(&receipt_path, &value::to_canon(&receipt))?;
        let readback = value::from_canon(&ctx.store().read_json(
            &receipt_path,
            careerbrew_vault::canon::Canon::Map(Vec::new()),
            true,
        )?);
        if readback != receipt {
            return Err(error(
                "legacy command archive receipt verification failed; original preserved",
            ));
        }
        std::fs::remove_file(leaf)?;
    }
    Ok(())
}

/// Write only the private source fragment. The synthetic adapter deliberately
/// does not invoke an external deployer; production activation is out of scope.
pub fn sync_command_sources(lock: &Value) -> Result<()> {
    let ctx = context()?;
    let mut files = Vec::new();
    let empty = json!({});
    let plugins = value::get(lock, "plugins")
        .and_then(Value::as_object)
        .unwrap_or_else(|| empty.as_object().unwrap());
    for (name, entry) in plugins {
        let root = plugins_dir(&ctx)?.join(name);
        let manifest = value::from_canon(&ctx.store().read_yaml(
            &root.join(MANIFEST),
            careerbrew_vault::canon::Canon::Map(Vec::new()),
            true,
        )?);
        let selected = command_files(&manifest, &root)?;
        files.extend(
            selected
                .iter()
                .map(|path| resolved(path).to_string_lossy().into_owned()),
        );
        if !value::strings(value::get(entry, "commands")).is_empty() && selected.is_empty() {
            return Err(error(
                "plugin command source missing; run careerbrew plugin refresh",
            ));
        }
    }
    files.sort();
    files.dedup();
    let folder = ctx.root.join("agentbrew");
    let fragment = folder.join("Agentfile.yaml");
    if files.is_empty() && !fragment.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(&folder)?;
    if folder.is_symlink() || !folder.is_dir() {
        return Err(error(
            "unsafe agent metadata directory; inspect the plugin data folder",
        ));
    }
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700))?;
    let document = json!({"commands": files});
    let text = value::yaml_write(&document).map_err(SetupError::plugin)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".Agentfile.")
        .suffix(".tmp")
        .tempfile_in(&folder)?;
    temporary.write_all(text.as_bytes())?;
    temporary.flush()?;
    let owner = temporary.as_file().metadata()?;
    let current = std::fs::symlink_metadata(temporary.path())?;
    if !current.is_file() || (current.dev(), current.ino()) != (owner.dev(), owner.ino()) {
        return Err(error("agent metadata temporary ownership changed"));
    }
    temporary.persist(&fragment).map_err(|_| SetupError::io())?;
    Ok(())
}

fn resource_ids(manifest: &Value, root: &Path) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let Some(paths) = value::get(manifest, "provides")
        .and_then(Value::as_object)
        .and_then(|items| items.get("ladders"))
    else {
        return Ok(ids);
    };
    for item in value::strings(Some(paths)) {
        for path in glob::glob(&root.join(item).to_string_lossy())
            .map_err(|_| error("invalid ladder glob"))?
            .flatten()
        {
            if path.is_file() {
                let ladder = value::from_canon(&context()?.store().read_yaml(
                    &path,
                    careerbrew_vault::canon::Canon::Map(Vec::new()),
                    true,
                )?);
                let id = value::get(&ladder, "id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| path.file_stem().map(|v| v.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                ids.insert(format!("ladder:{id}"));
            }
        }
    }
    Ok(ids)
}

fn check_conflicts(name: &str, manifest: &Value, root: &Path, lock: &Value) -> Result<()> {
    let mine = resource_ids(manifest, root)?;
    let ctx = context()?;
    for (other, _) in value::get(lock, "plugins")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if other == name {
            continue;
        }
        let other_root = plugins_dir(&ctx)?.join(other);
        let manifest = value::from_canon(&ctx.store().read_yaml(
            &other_root.join(MANIFEST),
            careerbrew_vault::canon::Canon::Map(Vec::new()),
            true,
        )?);
        if !mine
            .intersection(&resource_ids(&manifest, &other_root)?)
            .next()
            .is_none()
        {
            return Err(error(format!("plugin {name} conflicts with {other}")));
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct Installed {
    pub name: String,
    pub entry: Value,
    pub manifest: Value,
    pub root: PathBuf,
}

pub fn installed() -> Result<Vec<Installed>> {
    installed_with_check(true)
}

pub fn installed_with_check(check: bool) -> Result<Vec<Installed>> {
    let ctx = context()?;
    let lock = lock(&ctx)?;
    let mut out = Vec::new();
    for (name, entry) in value::get(&lock, "plugins")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let root = plugins_dir(&ctx)?.join(name);
        let manifest = value::from_canon(&ctx.store().read_yaml(
            &root.join(MANIFEST),
            careerbrew_vault::canon::Canon::Map(Vec::new()),
            true,
        )?);
        if check {
            validate_manifest(&manifest, &root).map_err(|e| {
                error(format!(
                    "installed plugin {name} has an invalid manifest: {} (fix: `careerbrew plugin add <its folder>` again, or `careerbrew plugin remove {name}`)",
                    e.message
                ))
            })?;
        }
        out.push(Installed {
            name: name.clone(),
            entry: entry.clone(),
            manifest,
            root,
        });
    }
    Ok(out)
}

fn stamp() -> String {
    std::env::var("CAREERBREW_NOW").unwrap_or_else(|_| "1970-01-01T00:00:00+00:00".into())
}

pub fn add(source: &Path, allow_exec: bool) -> Result<Value> {
    let ctx = context()?;
    let staging = plugins_dir(&ctx)?.join(".staging");
    let _ = std::fs::remove_dir_all(&staging);
    if !source.is_dir() {
        return Err(error("could not clone plugin source"));
    }
    copy_tree(source, &staging)?;
    let manifest_path = staging.join(MANIFEST);
    if !manifest_path.exists() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error("no careerbrew-plugin.yaml at the plugin root"));
    }
    let manifest = value::from_canon(&ctx.store().read_yaml(
        &manifest_path,
        careerbrew_vault::canon::Canon::Map(Vec::new()),
        true,
    )?);
    if let Err(problem) = validate_manifest(&manifest, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(problem);
    }
    let name = value::text(value::get(&manifest, "name"));
    let mut lock = lock(&ctx)?;
    let base = value::get(&manifest, "extends")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !base.is_empty()
        && !value::get(&lock, "plugins")
            .and_then(Value::as_object)
            .is_some_and(|items| items.contains_key(base))
    {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error(format!(
            "plugin {name} extends {base}, which is not installed"
        )));
    }
    check_conflicts(&name, &manifest, &staging, &lock)?;
    if let Err(problem) = check_commands(&name, &manifest, &staging, &lock) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(problem);
    }
    migrate_legacy(&name, &lock)?;
    let dest = plugins_dir(&ctx)?.join(&name);
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&staging, &dest)?;
    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o700))?;
    let commands = command_files(&manifest, &dest)?
        .iter()
        .map(|path| Value::String(path.file_name().unwrap().to_string_lossy().into_owned()))
        .collect::<Vec<_>>();
    if let Some(defaults) = value::get(&manifest, "settings_defaults").and_then(Value::as_object) {
        let path = ctx.root.join("settings.yaml");
        let mut settings = value::from_canon(&ctx.store().read_yaml(
            &path,
            careerbrew_vault::canon::Canon::Map(Vec::new()),
            true,
        )?);
        for key in ["ladder", "target_level"] {
            if let Some(default) = defaults.get(key).filter(|v| v.is_string()) {
                let current = value::get(&settings, key).and_then(Value::as_str);
                let generic = match key {
                    "ladder" => "generic-swe",
                    _ => "senior",
                };
                let base_default = if base.is_empty() {
                    None
                } else {
                    let base_manifest = value::from_canon(&ctx.store().read_yaml(
                        &plugins_dir(&ctx)?.join(base).join(MANIFEST),
                        careerbrew_vault::canon::Canon::Map(Vec::new()),
                        true,
                    )?);
                    value::get(&base_manifest, "settings_defaults")
                        .and_then(|items| items.get(key))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                };
                if current.is_none_or(|current| {
                    current == generic || base_default.as_deref() == Some(current)
                }) {
                    value::set(&mut settings, key, default.clone());
                }
            }
        }
        ctx.store().write_yaml(&path, &value::to_canon(&settings))?;
    }
    let now = stamp();
    let prior = value::get(&lock, "plugins")
        .and_then(Value::as_object)
        .and_then(|items| items.get(&name))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let first = value::get(&prior, "first_installed_at")
        .or_else(|| value::get(&prior, "installed_at"))
        .and_then(Value::as_str)
        .unwrap_or(&now);
    let entry = json!({
        "name": name,
        "version": value::text(value::get(&manifest, "version")),
        "source": "local-folder",
        "local_path": resolved(source).to_string_lossy(),
        "tree_sha256": tree_hash(&dest)?,
        "installed_at": now,
        "first_installed_at": first,
        "exec_allowed": allow_exec,
        "extends": if base.is_empty() { Value::Null } else { json!(base) },
        "commands": commands,
        "origin": format!("plugin:{name}"),
    });
    value::set(
        value::get_mut(&mut lock, "plugins").unwrap(),
        &name,
        entry.clone(),
    );
    save_lock(&ctx, &lock)?;
    sync_command_sources(&lock)?;
    Ok(entry)
}

pub fn verify() -> Result<Vec<(String, bool)>> {
    installed()?
        .into_iter()
        .map(|plugin| {
            let expected = value::text(value::get(&plugin.entry, "tree_sha256"));
            Ok((plugin.name.clone(), tree_hash(&plugin.root)? == expected))
        })
        .collect()
}

pub fn refresh() -> Result<Vec<String>> {
    let lock = lock(&context()?)?;
    let order = refresh_order(&lock);
    for name in &order {
        let entry = &value::get(&lock, "plugins").unwrap()[name];
        let source = value::get(entry, "local_path")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .ok_or_else(|| error("plugin was installed before refresh existed"))?;
        if !source.is_dir() {
            return Err(error(format!(
                "plugin {name}: its source folder was moved or deleted; the installed copy is unchanged"
            )));
        }
        add(
            &source,
            value::bool_value(value::get(entry, "exec_allowed")),
        )?;
    }
    if order.is_empty() {
        sync_command_sources(&lock)?;
    }
    Ok(order)
}

fn purge_origin(ctx: &Context, origin: &str) -> Result<usize> {
    let day = ctx.root.join("day");
    std::fs::create_dir_all(&day)?;
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(day.join("inbox.lock"))?;
    let mut files = Vec::new();
    all_files(&ctx.root, &ctx.root, &mut files)?;
    let mut purged = 0;
    for path in files {
        if path
            .extension()
            .is_none_or(|extension| extension != "jsonl")
            || path
                .strip_prefix(&ctx.root)
                .unwrap_or(&path)
                .components()
                .any(|part| part.as_os_str() == "logs")
        {
            continue;
        }
        let rows = ctx.store().read_jsonl(&path, true)?;
        let before = rows.len();
        let mut kept = Vec::new();
        for row in rows {
            let value = value::from_canon(&row);
            if value::get(&value, "origin").and_then(Value::as_str) == Some(origin) {
                purged += 1;
            } else {
                kept.push(row);
            }
        }
        if kept.len() != before {
            ctx.store().write_jsonl(&path, &kept)?;
        }
    }
    Ok(purged)
}

pub fn remove(name: &str, purge_data: bool) -> Result<Value> {
    let ctx = context()?;
    let mut lock = lock(&ctx)?;
    if !value::get(&lock, "plugins")
        .and_then(Value::as_object)
        .is_some_and(|items| items.contains_key(name))
    {
        return Err(error(format!("plugin {name} is not installed")));
    }
    let dependents = installed_with_check(false)?
        .into_iter()
        .filter(|plugin| {
            value::get(&plugin.manifest, "extends").and_then(Value::as_str) == Some(name)
        })
        .map(|plugin| plugin.name)
        .collect::<Vec<_>>();
    if !dependents.is_empty() {
        return Err(error(format!(
            "plugin {name} is extended by {dependents:?}; remove those first"
        )));
    }
    migrate_legacy(name, &lock)?;
    let _ = std::fs::remove_dir_all(plugins_dir(&ctx)?.join(name));
    value::get_mut(&mut lock, "plugins")
        .and_then(Value::as_object_mut)
        .unwrap()
        .remove(name);
    save_lock(&ctx, &lock)?;
    sync_command_sources(&lock)?;
    let purged = if purge_data {
        purge_origin(&ctx, &format!("plugin:{name}"))?
    } else {
        0
    };
    for folder in ["out", "cache"] {
        let _ = std::fs::remove_dir_all(ctx.root.join(folder));
    }
    let _ = std::fs::remove_file(ctx.root.join("jobs/strategy.json"));
    Ok(json!({"removed": 1, "purged_records": purged}))
}

pub fn effective_policy() -> Result<Value> {
    let plugins = installed()?;
    let manifests = plugins
        .iter()
        .map(|plugin| plugin.manifest.clone())
        .collect::<Vec<_>>();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    let mut expansions = BTreeMap::new();
    for manifest in &manifests {
        if let Some(targets) =
            value::get(manifest, "policy").and_then(|policy| value::get(policy, "backup_targets"))
        {
            for target in value::strings(Some(targets)) {
                expansions.insert(
                    target.clone(),
                    crate::expand_user(Path::new(&target), &home)
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    Ok(merge_policy(&manifests, &expansions))
}

pub fn detect() -> Result<Vec<(String, Vec<String>)>> {
    let mut out = Vec::new();
    for plugin in installed()? {
        let mut fired = Vec::new();
        for var in value::strings(
            value::get(&plugin.manifest, "detect").and_then(|det| value::get(det, "env")),
        ) {
            if std::env::var_os(&var).is_some_and(|value| !value.is_empty()) {
                fired.push(format!("env:{var}"));
            }
        }
        out.push((plugin.name, fired));
    }
    Ok(out)
}
