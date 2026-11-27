//! `web-component --all`: the native half of the rust-25 differential.
//!
//! It seeds the synthetic data root from `tests/fixtures/web/rust25-cases.json`
//! when the Python oracle has not already done so, starts the native server on
//! a free OS port with no Python handler behind it, sends every native-owned
//! case and prints one normalized JSON line per case. `tests/oracles/rust25.py
//! --all` prints the same lines for the Python server.

use crate::server::{self, State};
use careerbrew_vault::canon::Canon;
use careerbrew_vault::json_yaml::json_read;
use careerbrew_vault::transfer::Context;
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

pub const FIXTURE: &str = include_str!("../../../tests/fixtures/web/rust25-cases.json");
pub const SEEDED: &str = ".rust25-seeded";
pub const DIST: &str = ".rust25-dist";
pub const BROWSER: &str = ".rust25-browser";

fn fixture() -> Result<Value, String> {
    serde_json::from_str(FIXTURE).map_err(|_| "the rust-25 case fixture is not JSON".into())
}

fn canon(value: &Value) -> Result<Canon, String> {
    json_read(&value.to_string()).map_err(|_| "a fixture value is not storable".into())
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Write the fixture's seed files: sealed ones through the store, plain ones as is.
pub fn seed(ctx: &Context) -> Result<(), String> {
    let root = &ctx.root;
    if root.join(SEEDED).exists() {
        return Ok(());
    }
    let fx = fixture()?;
    let store = ctx.store();
    let token = &fx["seed"]["token"];
    let path = crate::token::web_token_path(root);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|_| "cannot make day/")?;
    store
        .write_json(&path, &canon(token)?)
        .map_err(|_| "cannot write the token record")?;
    for file in fx["seed"]["files"].as_array().into_iter().flatten() {
        let target = root.join(text(file, "path"));
        std::fs::create_dir_all(target.parent().unwrap()).map_err(|_| "cannot make a seed dir")?;
        if file["sealed"] == true {
            store
                .write_text(&target, text(file, "text"))
                .map_err(|_| "cannot write a sealed seed file")?;
        } else {
            std::fs::write(&target, text(file, "text")).map_err(|_| "cannot write a seed file")?;
        }
    }
    for file in fx["seed"]["dist"].as_array().into_iter().flatten() {
        let target = root.join(DIST).join(text(file, "path"));
        std::fs::create_dir_all(target.parent().unwrap()).map_err(|_| "cannot make dist")?;
        std::fs::write(&target, text(file, "text")).map_err(|_| "cannot write dist")?;
    }
    let browser = root.join(BROWSER);
    std::fs::write(
        &browser,
        "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$0.log\"\nexit 0\n",
    )
    .map_err(|_| "cannot write the browser stub")?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o755))
        .map_err(|_| "cannot mark the browser stub")?;
    std::fs::write(root.join(SEEDED), "").map_err(|_| "cannot mark the seed")?;
    Ok(())
}

fn apply_setup(ctx: &Context, setup: &Value) -> Result<(), String> {
    let store = ctx.store();
    if let Some(settings) = setup.get("settings") {
        careerbrew_vault::settings::write_settings(&store, &canon(settings)?)
            .map_err(|_| "cannot write settings")?;
    }
    if let Some(role) = setup.get("role") {
        store
            .write_yaml(
                &ctx.root.join("machine.yaml"),
                &canon(&json!({"role": role}))?,
            )
            .map_err(|_| "cannot write the machine record")?;
    }
    Ok(())
}

fn clear_setup(root: &Path, setup: &Value) {
    if setup.get("settings").is_some() {
        let _ = std::fs::remove_file(root.join("settings.yaml"));
    }
    if setup.get("role").is_some() {
        let _ = std::fs::remove_file(root.join("machine.yaml"));
    }
}

/// The raw request bytes of one case for a server on `port`.
pub fn request_bytes(case: &Value, port: u16, token: &str) -> Vec<u8> {
    let fill = |s: &str| {
        s.replace("{port}", &port.to_string())
            .replace("{token}", token)
    };
    let body = text(case, "body");
    let mut head = format!(
        "{} {} HTTP/1.0\r\n",
        text(case, "method"),
        text(case, "path")
    );
    let mut has_length = false;
    for pair in case["headers"].as_array().into_iter().flatten() {
        let name = pair[0].as_str().unwrap_or("");
        has_length |= name.eq_ignore_ascii_case("content-length");
        head.push_str(&format!(
            "{name}: {}\r\n",
            fill(pair[1].as_str().unwrap_or(""))
        ));
    }
    if !body.is_empty() && !has_length {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

/// One normalized line for a raw HTTP answer: status, reason, every header but
/// Server and Date in order, and the body (parsed JSON, or text).
pub fn normalize(id: &str, raw: &[u8], port: u16) -> Value {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let body = raw.get(split + 4..).unwrap_or(&[]);
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.splitn(3, ' ');
    let _version = parts.next();
    let status = parts
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let reason = parts.next().unwrap_or("").to_owned();
    let mut headers = Vec::new();
    let mut ctype = String::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value
            .trim()
            .replace(&format!("careerbrew-{port}="), "careerbrew-<port>=");
        if name == "server" || name == "date" {
            continue;
        }
        if name == "content-type" {
            ctype.clone_from(&value);
        }
        headers.push(json!([name, value]));
    }
    let mut body = if ctype.starts_with("application/json") {
        serde_json::from_slice(body)
            .unwrap_or_else(|_| json!({"text": String::from_utf8_lossy(body)}))
    } else {
        json!({"text": String::from_utf8_lossy(body)})
    };
    if let Some(key) = body.get_mut("key") {
        if key.as_str().is_some_and(|k| k.len() == 44) {
            *key = json!("<tab-key>");
        }
    }
    json!({"case": id, "status": status, "reason": reason, "headers": headers, "body": body})
}

/// `json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)`.
pub fn line(value: &Value) -> String {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(items) => {
                let mut keys = items.keys().collect::<Vec<_>>();
                keys.sort();
                let mut out = Map::new();
                for key in keys {
                    out.insert(key.clone(), sorted(&items[key]));
                }
                Value::Object(out)
            }
            Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string(&sorted(value)).expect("a JSON value serializes")
}

pub fn send_raw(port: u16, bytes: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.write_all(bytes)?;
    let mut out = Vec::new();
    stream.read_to_end(&mut out)?;
    Ok(out)
}

pub fn run_all() -> Result<String, String> {
    let ctx = Context::from_env().map_err(|_| "CAREERBREW_HOME is required")?;
    seed(&ctx)?;
    let root = ctx.root.clone();
    // Python's webbrowser reads BROWSER; the stub logs the one-time link.
    std::env::set_var("BROWSER", root.join(BROWSER));
    let fx = fixture()?;
    let token = text(&fx["seed"]["token"], "token").to_owned();
    let listener = server::bind(0).map_err(|_| "cannot bind a free port")?;
    let port = listener
        .local_addr()
        .map_err(|_| "no local address")?
        .port();
    let state = Arc::new(State::new(
        Context::from_env().map_err(|_| "CAREERBREW_HOME is required")?,
        port,
        std::env::var("CAREERBREW_SERVE_TOKEN")
            .ok()
            .filter(|t| !t.is_empty()),
        root.join(DIST),
    ));
    std::thread::spawn(move || server::run(listener, state));
    let mut out = String::new();
    for case in fx["cases"].as_array().into_iter().flatten() {
        if case.get("owner").and_then(Value::as_str) != Some("native") {
            continue;
        }
        let setup = case.get("setup").cloned().unwrap_or(Value::Null);
        apply_setup(&ctx, &setup)?;
        let raw = send_raw(port, &request_bytes(case, port, &token));
        clear_setup(&root, &setup);
        let raw = raw.map_err(|_| "a case request failed")?;
        out.push_str(&line(&normalize(text(case, "id"), &raw, port)));
        out.push('\n');
    }
    Ok(out)
}
