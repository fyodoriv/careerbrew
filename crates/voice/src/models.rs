//! `careerbrew.models`: routes from `config/models.yaml`, the privacy-guarded
//! route choice and `complete()` behind the typed [`Transport`] seam.

use crate::{data_root, field, http, py_repr_str, py_str, repo_root, truthy, Error, Result};
use careerbrew_vault::canon::Canon;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub const SENSITIVITIES: [&str; 3] = ["public", "personal", "business"];

/// The config folder override. Lifecycle: component-invocation scope, set only
/// by the synthetic workload (like the oracle's `paths.config_dir` patch) and
/// never read by a production route.
static CONFIG_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn set_config_dir(path: Option<PathBuf>) {
    *CONFIG_DIR.lock().expect("config lock") = path;
}

pub fn config_dir() -> PathBuf {
    CONFIG_DIR
        .lock()
        .expect("config lock")
        .clone()
        .unwrap_or_else(|| repo_root().join("config"))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub private_data: bool,
    pub base_url: String,
    pub api_key_env: String,
}

impl Route {
    pub fn new(name: &str, provider: &str, model: &str, private: bool, base_url: &str) -> Self {
        Self {
            name: name.into(),
            provider: provider.into(),
            model: model.into(),
            private_data: private,
            base_url: base_url.into(),
            api_key_env: String::new(),
        }
    }
}

/// One failure a transport can raise, named by its Python class.
#[derive(Clone, Debug, PartialEq)]
pub enum TransportError {
    /// `urllib.error.HTTPError` with its code and reason phrase.
    Http(u16, String),
    /// `urllib.error.URLError`; `timeout` marks a `TimeoutError` reason.
    Url {
        reason: String,
        timeout: bool,
    },
    Timeout(String),
    Os(String),
    Key(String),
    Index(String),
    Type(String),
    Value(String),
    Llm(String),
    Runtime(String),
}

impl TransportError {
    pub fn class(&self) -> &'static str {
        match self {
            Self::Http(..) => "HTTPError",
            Self::Url { .. } => "URLError",
            Self::Timeout(_) => "TimeoutError",
            Self::Os(_) => "OSError",
            Self::Key(_) => "KeyError",
            Self::Index(_) => "IndexError",
            Self::Type(_) => "TypeError",
            Self::Value(_) => "ValueError",
            Self::Llm(_) => "LLMError",
            Self::Runtime(_) => "RuntimeError",
        }
    }

    /// `str(exc)`.
    pub fn message(&self) -> String {
        match self {
            Self::Http(code, reason) => format!("HTTP Error {code}: {reason}"),
            Self::Url { reason, .. } => format!("<urlopen error {reason}>"),
            Self::Key(key) => py_repr_str(key),
            Self::Timeout(text)
            | Self::Os(text)
            | Self::Index(text)
            | Self::Type(text)
            | Self::Value(text)
            | Self::Llm(text)
            | Self::Runtime(text) => text.clone(),
        }
    }

    /// The classes `complete()` treats as "this route is unavailable".
    fn unavailable(&self) -> bool {
        matches!(
            self,
            Self::Http(..)
                | Self::Url { .. }
                | Self::Timeout(_)
                | Self::Os(_)
                | Self::Key(_)
                | Self::Value(_)
        )
    }
}

impl From<TransportError> for Error {
    fn from(error: TransportError) -> Self {
        Error::new(error.class(), error.message())
    }
}

/// The model request boundary: one payload and URL in, the reply value out.
pub trait Transport {
    fn call(&mut self, payload: &Value, url: &str) -> std::result::Result<Value, TransportError>;

    /// True only for the judge's completion-checked HTTP transport.
    fn verified_http(&self) -> bool {
        false
    }
}

/// `os.path.expandvars` (POSIX): `$name` and `${name}`, ASCII word names;
/// unknown variables stay as they are.
pub fn expandvars(text: &str) -> String {
    if !text.contains('$') {
        return text.to_owned();
    }
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'$' {
            let next = text[at..].find('$').map_or(text.len(), |n| at + n);
            out.push_str(&text[at..next]);
            at = next;
            continue;
        }
        let rest = &text[at + 1..];
        let (name, length) = if let Some(inner) = rest.strip_prefix('{') {
            match inner.find('}') {
                Some(close) => (Some(&inner[..close]), close + 2),
                None => (None, 0),
            }
        } else {
            let length = rest
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                .count();
            if length == 0 {
                (None, 0)
            } else {
                (Some(&rest[..length]), length)
            }
        };
        match name.and_then(|name| std::env::var(name).ok().map(|value| (name, value))) {
            Some((_, value)) => {
                out.push_str(&value);
                at += 1 + length;
            }
            None => {
                let skip = if name.is_some() { 1 + length } else { 1 };
                out.push_str(&text[at..at + skip]);
                at += skip;
            }
        }
    }
    out
}

pub fn routes() -> Result<Vec<Route>> {
    let cfg = careerbrew_tutor::read_yaml_abs(&config_dir().join("models.yaml"), json!({}))?;
    let cfg = if truthy(Some(&cfg)) { cfg } else { json!({}) };
    let mut out = Vec::new();
    for r in field(&cfg, "routes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let name = field(&r, "name").ok_or_else(|| Error::key("name"))?;
        let get = |key: &str| field(&r, key).map(py_str).unwrap_or_default();
        out.push(Route {
            name: py_str(name),
            provider: get("provider"),
            model: get("model"),
            private_data: truthy(field(&r, "private_data")),
            base_url: expandvars(&get("base_url")),
            api_key_env: get("api_key_env"),
        });
    }
    Ok(out)
}

/// `urllib.parse.urlparse(url).hostname`.
pub fn hostname(url: &str) -> Option<String> {
    let colon = url.find(':')?;
    let scheme = &url[..colon];
    let valid = scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    let rest = if valid { &url[colon + 1..] } else { url };
    let rest = rest.strip_prefix("//")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let netloc = &rest[..end];
    let host = netloc.rsplit_once('@').map_or(netloc, |(_, host)| host);
    let host = if let Some(inner) = host.strip_prefix('[') {
        inner.split(']').next().unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    (!host.is_empty()).then(|| host.to_lowercase())
}

pub fn on_this_machine(route: &Route) -> bool {
    matches!(
        hostname(&route.base_url).as_deref(),
        Some("localhost" | "127.0.0.1" | "::1")
    )
}

pub fn allowed_routes(sensitivity: &str) -> Result<Vec<Route>> {
    if !SENSITIVITIES.contains(&sensitivity) {
        return Err(Error::value(format!(
            "unknown sensitivity {}; use one of ('public', 'personal', 'business')",
            py_repr_str(sensitivity)
        )));
    }
    let policy = careerbrew_setup::plugins::effective_policy()?;
    let corporate = careerbrew_setup::machine::role()? == "corporate";
    let allowed = field(&policy, "allowed_routes").filter(|value| !value.is_null());
    let mut out = Vec::new();
    for r in routes()? {
        if sensitivity != "public" && !r.private_data {
            continue;
        }
        if let Some(allowed) = allowed {
            let listed = allowed
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(&r.name)));
            if !listed {
                continue;
            }
        }
        if corporate && sensitivity != "public" && !on_this_machine(&r) {
            continue;
        }
        out.push(r);
    }
    Ok(out)
}

/// Python `obj[key]` over a decoded JSON reply.
pub fn index_key(value: &Value, key: &str) -> std::result::Result<Value, TransportError> {
    match value {
        Value::Object(map) => map
            .get(key)
            .cloned()
            .ok_or_else(|| TransportError::Key(key.into())),
        Value::Array(_) => Err(TransportError::Type(
            "list indices must be integers or slices, not str".into(),
        )),
        Value::String(_) => Err(TransportError::Type(
            "string indices must be integers, not 'str'".into(),
        )),
        _ => Err(TransportError::Type("object is not subscriptable".into())),
    }
}

pub fn index_first(value: &Value) -> std::result::Result<Value, TransportError> {
    match value {
        Value::Array(items) => items
            .first()
            .cloned()
            .ok_or_else(|| TransportError::Index("list index out of range".into())),
        Value::Object(_) => Err(TransportError::Key("0".into())),
        Value::String(text) => text
            .chars()
            .next()
            .map(|c| Value::String(c.into()))
            .ok_or_else(|| TransportError::Index("string index out of range".into())),
        _ => Err(TransportError::Type("object is not subscriptable".into())),
    }
}

/// POST the payload (without `_api_key_env`) and decode the JSON reply.
pub fn post_payload(
    payload: &Value,
    url: &str,
    timeout: Duration,
) -> std::result::Result<Value, TransportError> {
    let mut body = payload.clone();
    let key_env = body
        .as_object_mut()
        .and_then(|map| map.remove("_api_key_env"))
        .map(|value| py_str(&value))
        .unwrap_or_default();
    let token = (!key_env.is_empty())
        .then(|| std::env::var(&key_env).ok())
        .flatten()
        .filter(|token| !token.is_empty())
        .map(|token| format!("Bearer {token}"));
    let raw = http::post(url, &body, token.as_deref(), timeout)?;
    serde_json::from_slice(&raw).map_err(|_| TransportError::Value("invalid JSON reply".into()))
}

/// `models._http_transport`: no completion check.
pub struct HttpTransport;

impl Transport for HttpTransport {
    fn call(&mut self, payload: &Value, url: &str) -> std::result::Result<Value, TransportError> {
        let body = post_payload(payload, url, Duration::from_secs(60))?;
        let choice = index_first(&index_key(&body, "choices")?)?;
        index_key(&index_key(&choice, "message")?, "content")
    }
}

fn emit(event: &str, route: &str) -> Result<()> {
    careerbrew_tutor::emit(
        &data_root()?,
        event,
        vec![("route", Canon::Str(route.into()))],
    )?;
    Ok(())
}

/// `models.complete`: (text, route name); route "fallback" means no model answered.
pub fn complete(
    prompt: &str,
    sensitivity: &str,
    fallback: &dyn Fn() -> String,
    transport: &mut dyn Transport,
) -> Result<(Value, String)> {
    if std::env::var("CAREERBREW_LLM").as_deref() == Ok("off") {
        return Ok((Value::String(fallback()), "fallback".into()));
    }
    for r in allowed_routes(sensitivity)? {
        if !r.private_data {
            let terms = careerbrew_feed::sources::sensitive_terms(&data_root()?)?;
            careerbrew_policy::assert_clean(
                &careerbrew_policy::Rules::python(),
                prompt,
                &format!("route:{}", r.name),
                &terms,
            )?;
        }
        let payload = json!({
            "model": r.model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0,
            "_api_key_env": r.api_key_env,
        });
        let url = format!("{}/chat/completions", r.base_url.trim_end_matches('/'));
        match transport.call(&payload, &url) {
            Ok(text) => {
                emit("llm.call", &r.name)?;
                return Ok((text, r.name));
            }
            Err(error) if error.unavailable() => emit("llm.unavailable", &r.name)?,
            Err(error) => return Err(error.into()),
        }
    }
    Ok((Value::String(fallback()), "fallback".into()))
}

pub fn route_report(route: &Route) -> Value {
    json!({
        "name": route.name,
        "provider": route.provider,
        "model": route.model,
        "private_data": route.private_data,
        "base_url": route.base_url,
        "api_key_env": route.api_key_env,
        "on_this_machine": on_this_machine(route),
    })
}

pub fn models_yaml(dir: &Path) -> PathBuf {
    dir.join("models.yaml")
}
