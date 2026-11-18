//! Set `paths`: `careerbrew.paths` over an injected environment map, with no
//! file system access (no `mkdir`, no `chmod`, no `resolve`; those are
//! rust-05). Ops:
//! - `data-root`: the data folder `data_root(create=False)` names, as
//!   pathlib prints it; the repo root reads `<repo>`;
//! - `data-root-refusal`: the refusal text when `CAREERBREW_HOME` is set but
//!   holds only whitespace (Python's `str.strip`), else null;
//! - `extra-knowledge`: false only for `CAREERBREW_EXTRA_KNOWLEDGE` exactly `0`;
//! - `config-dir`: `<repo>/config`.
//!
//! Known gaps: `~user` names a real account through `pwd` in Python; here
//! it is refused (Python refuses it only when no such account exists). A
//! `~` path with no `HOME` in the map reads the account database in Python;
//! here it is refused.
use serde_json::{json, Map, Value};

const REFUSAL: &str =
    "CAREERBREW_HOME is set but empty; unset it to use private/, or point it at a folder";

pub fn map(op: &str, input: &Value) -> Result<Value, String> {
    let env = environment(input)?;
    let text = |v: String| json!({"t": "str", "v": v});
    match op {
        "data-root" => resolve_root(&env).map(text),
        "data-root-refusal" => match resolve_root(&env) {
            Err(message) if message == REFUSAL => Ok(text(message)),
            Err(message) => Err(message),
            Ok(_) => Ok(json!({"t": "null"})),
        },
        "extra-knowledge" => Ok(json!({"t": "bool", "v": extra_knowledge(&env)})),
        "config-dir" => Ok(text("<repo>/config".into())),
        _ => Err("unknown op".into()),
    }
}

/// The input must be exactly `{"env": {name: text}}`.
fn environment(input: &Value) -> Result<Map<String, Value>, String> {
    let object = input.as_object().ok_or("input is not a map")?;
    let env = object.get("env").and_then(Value::as_object);
    let env = env
        .filter(|_| object.len() == 1)
        .ok_or("input must be {env}")?;
    if env.values().any(|v| !v.is_string()) {
        return Err("env values must be text".into());
    }
    Ok(env.clone())
}

fn var<'a>(env: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    env.get(name).and_then(Value::as_str)
}

/// Python's `str.isspace`: Unicode White_Space plus the separators U+001C
/// to U+001F.
fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

pub fn resolve_root(env: &Map<String, Value>) -> Result<String, String> {
    match var(env, "CAREERBREW_HOME") {
        Some(value) if value.chars().all(py_space) => Err(REFUSAL.into()),
        Some(value) => expanduser(value, env),
        None => Ok("<repo>/private".into()),
    }
}

pub fn extra_knowledge(env: &Map<String, Value>) -> bool {
    var(env, "CAREERBREW_EXTRA_KNOWLEDGE").unwrap_or("1") != "0"
}

/// pathlib's POSIX parse: the root (`/`, or `//` but not `///`) and the
/// parts with empty and `.` parts dropped.
fn parse(text: &str) -> (&'static str, Vec<&str>) {
    let (root, rest) = if !text.starts_with('/') {
        ("", text)
    } else if text.starts_with("//") && !text.starts_with("///") {
        ("//", &text[2..])
    } else {
        ("/", &text[1..])
    };
    let parts = rest.split('/').filter(|p| !p.is_empty() && *p != ".");
    (root, parts.collect())
}

fn render(root: &str, parts: &[&str]) -> String {
    let text = format!("{root}{}", parts.join("/"));
    if text.is_empty() {
        ".".into()
    } else {
        text
    }
}

/// `Path(value).expanduser()` with `HOME` from the injected map.
fn expanduser(value: &str, env: &Map<String, Value>) -> Result<String, String> {
    let (root, parts) = parse(value);
    let tilde = parts.first().is_some_and(|first| first.starts_with('~'));
    if !root.is_empty() || !tilde {
        return Ok(render(root, &parts));
    }
    if parts[0] != "~" {
        return Err("a ~user path reads the account database".into());
    }
    let home = var(env, "HOME").ok_or("a ~ path with no HOME reads the account database")?;
    let trimmed = home.trim_end_matches('/');
    let home = if trimmed.is_empty() { "/" } else { trimmed };
    if home.starts_with('~') {
        return Err("HOME starts with ~".into());
    }
    let (home_root, mut home_parts) = parse(home);
    home_parts.extend_from_slice(&parts[1..]);
    Ok(render(home_root, &home_parts))
}
