//! MCP preflight, explicitly confirmed repair, self-update and workflow checks.

use crate::value;
use crate::{Result, SetupError};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn value_error(message: impl Into<String>) -> SetupError {
    SetupError::new("ValueError", message)
}

pub fn vars_in(server: &Value) -> Vec<String> {
    let mut names = BTreeSet::new();
    let mut texts = vec![value::text(value::get(server, "url"))];
    for key in ["headers", "http_headers"] {
        if let Some(headers) = value::get(server, key).and_then(Value::as_object) {
            texts.extend(headers.values().map(|value| value::text(Some(value))));
        }
    }
    for text in texts {
        let mut rest = text.as_str();
        while let Some(start) = rest.find("${") {
            rest = &rest[start + 2..];
            let Some(end) = rest.find('}') else {
                break;
            };
            let token = &rest[..end];
            let name = token.split(":-").next().unwrap_or("");
            if name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                names.insert(name.to_owned());
            }
            rest = &rest[end + 1..];
        }
    }
    if let Some(references) = value::get(server, "env_http_headers").and_then(Value::as_object) {
        for reference in references.values().filter_map(Value::as_str) {
            if !reference.is_empty() {
                names.insert(reference.to_owned());
            }
        }
    }
    if let Some(reference) = value::get(server, "bearer_token_env_var").and_then(Value::as_str) {
        if !reference.is_empty() {
            names.insert(reference.to_owned());
        }
    }
    names.into_iter().collect()
}

struct Url<'a> {
    scheme: &'a str,
    host: &'a str,
    port: Option<u16>,
    userinfo: bool,
}

fn parse_url(text: &str) -> Option<Url<'_>> {
    let (scheme, rest) = text.split_once("://")?;
    let authority = rest.split('/').next().unwrap_or(rest);
    let userinfo = authority.contains('@');
    let hostport = authority.rsplit('@').next().unwrap_or(authority);
    let (host, port) = if hostport.starts_with('[') {
        let end = hostport.find(']')?;
        let host = &hostport[1..end];
        let port = hostport[end + 1..]
            .strip_prefix(':')
            .and_then(|value| value.parse().ok());
        (host, port)
    } else if let Some((host, port)) = hostport.rsplit_once(':') {
        (host, port.parse().ok())
    } else {
        (hostport, None)
    };
    if host.is_empty() {
        return None;
    }
    Some(Url {
        scheme,
        host,
        port,
        userinfo,
    })
}

pub fn github_auth_header(server: &Value) -> Option<String> {
    let url_text = value::text(value::get(server, "url"));
    let url = parse_url(&url_text)?;
    if url.scheme != "https"
        || !matches!(
            url.host,
            "github.com" | "api.github.com" | "api.githubcopilot.com"
        )
        || url.userinfo
        || !matches!(url.port, None | Some(443))
        || !matches!(
            value::get(server, "type")
                .and_then(Value::as_str)
                .unwrap_or("http"),
            "http" | "sse"
        )
        || value::get(server, "headersHelper").is_some()
    {
        return None;
    }
    let headers = value::get(server, "headers")?.as_object()?;
    let names = headers
        .keys()
        .filter(|key| key.to_lowercase() == "authorization")
        .collect::<Vec<_>>();
    if names.len() != 1 {
        return None;
    }
    let value = headers[names[0]].as_str()?;
    let mut parts = value.split_whitespace();
    if parts
        .next()
        .is_some_and(|part| part.eq_ignore_ascii_case("bearer"))
        && parts.next() == Some("${GITHUB_TOKEN}")
        && parts.next().is_none()
    {
        Some(names[0].clone())
    } else {
        None
    }
}

pub fn check_rows(servers: &Value, facts: &Value, repair_host: &str) -> Vec<Value> {
    let Some(servers) = servers.as_object() else {
        return Vec::new();
    };
    if servers.is_empty() {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for (name, server) in servers {
        let empty = json!({});
        let fact = value::get(facts, "servers")
            .and_then(Value::as_object)
            .and_then(|items| items.get(name))
            .unwrap_or(&empty);
        let vars = value::get(fact, "vars")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let empty_names = vars_in(server)
            .into_iter()
            .filter(|name| vars.get(name) == Some(&Value::String(String::new())))
            .collect::<Vec<_>>();
        if !empty_names.is_empty() {
            let names = empty_names.join(", ");
            let mut safe = format!(
                "the desktop app sees {names} empty: `launchctl setenv {} <value>`, then restart the app",
                empty_names[0]
            );
            if repair_host == "claude"
                && empty_names == ["GITHUB_TOKEN"]
                && github_auth_header(server).is_some()
            {
                safe = match value::get(fact, "ownership_error").and_then(Value::as_str) {
                    Some(error) => error.to_owned(),
                    None => "after the owner approves changing Claude connector settings, run careerbrew day start --fix --yes; reconnect with the host's supported tool and recheck; no token belongs in chat".into(),
                };
            }
            rows.push(json!({"check": format!("mcp {name}"), "value": "empty launchd var", "ok": false, "safe": safe}));
            continue;
        }
        let launchd = vars
            .get("GITHUB_TOKEN")
            .and_then(Value::as_str)
            .unwrap_or("");
        if repair_host == "claude" && !launchd.is_empty() && github_auth_header(server).is_some() {
            if let Some(github) = value::get(fact, "github_token").and_then(Value::as_str) {
                if github != launchd {
                    rows.push(json!({"check": format!("mcp {name}"), "value": "GitHub credential mismatch", "ok": false, "safe": "the desktop app and GitHub CLI use different credentials; reconnect the GitHub connector and recheck"}));
                    continue;
                }
            }
        }
        let url_text = value::text(value::get(server, "url"));
        if !url_text.is_empty() {
            if let Some(url) = parse_url(&url_text) {
                if matches!(url.host, "localhost" | "127.0.0.1" | "::1") {
                    let port = url
                        .port
                        .unwrap_or(if url.scheme == "https" { 443 } else { 80 });
                    if !value::bool_value(value::get(fact, "port_open")) {
                        rows.push(json!({"check": format!("mcp {name}"), "value": format!("port {port} closed"), "ok": false, "safe": format!("nothing answers on localhost:{port}: start the app that serves it (an IDE), or remove the server")}));
                    }
                }
            }
            continue;
        }
        let command = value::text(value::get(server, "command"));
        if !command.is_empty() && !value::bool_value(value::get(fact, "command_found")) {
            rows.push(json!({"check": format!("mcp {name}"), "value": "command not on launchd PATH", "ok": false, "safe": format!("the desktop app cannot find `{command}`: use its full path in the config, or `launchctl setenv PATH ...`, then restart the app")}));
        }
    }
    if rows.is_empty() {
        rows.push(json!({"check": "mcp", "value": format!("{} servers", servers.len()), "ok": true, "safe": "every MCP server resolves in the desktop app's launchd env"}));
    }
    rows
}

pub fn servers(contents: &str, strict: bool) -> std::result::Result<Value, String> {
    let data: Value = serde_json::from_str(contents).map_err(|_| "invalid JSON")?;
    let map = data.as_object().ok_or("MCP config is not an object")?;
    let found = map.get("mcpServers").cloned().unwrap_or_else(|| json!({}));
    let servers = found.as_object().ok_or("MCP server map is not an object")?;
    if strict {
        for entry in servers.values() {
            let entry = entry
                .as_object()
                .ok_or("MCP server entry is not an object")?;
            if entry.get("enabled") == Some(&Value::Bool(false)) {
                continue;
            }
            if entry.contains_key("enabled") && !entry["enabled"].is_boolean() {
                return Err("MCP enabled flag is not a boolean".into());
            }
            for field in ["headers", "http_headers", "env_http_headers"] {
                if let Some(value) = entry.get(field) {
                    if value
                        .as_object()
                        .is_none_or(|headers| headers.values().any(|value| !value.is_string()))
                    {
                        return Err("MCP header map must contain string keys and values".into());
                    }
                }
            }
        }
    }
    Ok(Value::Object(
        servers
            .iter()
            .filter(|(_, entry)| {
                entry.is_object() && value::get(entry, "enabled") != Some(&Value::Bool(false))
            })
            .map(|(name, entry)| (name.clone(), entry.clone()))
            .collect(),
    ))
}

fn check_source_ownership(home: &Path, name: &str) -> Result<()> {
    let path = home.join(".config/agentbrew/state.yaml");
    if !path.exists() && !path.is_symlink() {
        return Ok(());
    }
    let text = std::fs::read_to_string(path)
        .map_err(|_| value_error("connector source ownership is unreadable; repair the agentbrew source registry and retry"))?;
    let state = value::yaml_read(&text)
        .map_err(|_| value_error("connector source ownership is unreadable; repair the agentbrew source registry and retry"))?;
    let registered = value::get(&state, "mcpServers")
        .and_then(Value::as_array)
        .ok_or_else(|| value_error("connector source ownership is unreadable; repair the agentbrew source registry and retry"))?;
    if registered
        .iter()
        .any(|item| !item.is_object() || value::get(item, "name").and_then(Value::as_str).is_none())
    {
        return Err(value_error("connector source ownership is unreadable; repair the agentbrew source registry and retry"));
    }
    if registered
        .iter()
        .any(|item| value::get(item, "name").and_then(Value::as_str) == Some(name))
    {
        return Err(value_error("agentbrew owns this connector: change its registered source, then run agentbrew sync; do not edit the generated Claude settings"));
    }
    Ok(())
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn helper_command(gh: &Path) -> Result<String> {
    let current = std::env::current_exe()?.canonicalize()?;
    Ok(format!(
        "{} github-header-helper {}",
        shell_quote(&current.to_string_lossy()),
        shell_quote(&gh.to_string_lossy())
    ))
}

/// Repair only an empty Claude GitHub token reference after explicit yes.
/// The symlink itself is never replaced; its target retains its mode.
pub fn fix_github_headers(
    yes: bool,
    requested: &Path,
    probe: Option<&str>,
    gh: &Path,
) -> Result<Value> {
    if !yes {
        return Err(value_error("--fix changes Claude connector settings; use --yes only after the owner approves this repair"));
    }
    let target = requested.canonicalize().map_err(|_| {
        value_error(
            "Claude connector settings are unavailable or malformed; no repair was completed",
        )
    })?;
    let lock_path = target.with_file_name(format!(
        ".{}.careerbrew-mcp.lock",
        target.file_name().unwrap().to_string_lossy()
    ));
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    // SAFETY: this invocation owns the lock descriptor; the OS releases the
    // advisory lock when the descriptor closes on every return path.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(SetupError::io());
    }
    if requested.canonicalize().ok().as_ref() != Some(&target) {
        return Err(value_error(
            "connector settings changed during repair; retry after the other writer finishes",
        ));
    }
    let original = std::fs::read(&target)?;
    let before = std::fs::metadata(&target)?;
    let text = String::from_utf8(original.clone()).map_err(|_| {
        value_error(
            "Claude connector settings are unavailable or malformed; no repair was completed",
        )
    })?;
    let mut data: Value = serde_json::from_str(&text).map_err(|_| {
        value_error(
            "Claude connector settings are unavailable or malformed; no repair was completed",
        )
    })?;
    let configured = servers(&text, true).map_err(|_| {
        value_error(
            "Claude connector settings are unavailable or malformed; no repair was completed",
        )
    })?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    let mut repaired = Vec::new();
    for (name, server) in configured.as_object().unwrap() {
        let Some(auth) = github_auth_header(server) else {
            continue;
        };
        if probe != Some("") {
            continue;
        }
        check_source_ownership(&home, name)?;
        let entry = data
            .get_mut("mcpServers")
            .and_then(Value::as_object_mut)
            .and_then(|items| items.get_mut(name))
            .ok_or_else(|| value_error("connector settings changed during repair"))?;
        let headers = value::get(server, "headers")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let mut kept = Map::new();
        for (key, value) in headers {
            if key != auth {
                kept.insert(key, value);
            }
        }
        value::set(entry, "headers", Value::Object(kept));
        value::set(entry, "headersHelper", json!(helper_command(gh)?));
        repaired.push(name.clone());
    }
    let changed = repaired.len();
    if changed > 0 {
        let mut temporary = tempfile::Builder::new()
            .prefix(".careerbrew-mcp-")
            .suffix(".tmp")
            .tempfile_in(target.parent().unwrap())?;
        temporary.write_all((serde_json::to_string_pretty(&data).unwrap() + "\n").as_bytes())?;
        temporary.as_file().sync_all()?;
        std::fs::set_permissions(
            temporary.path(),
            std::fs::Permissions::from_mode(before.permissions().mode() & 0o7777),
        )?;
        let latest = std::fs::metadata(&target)?;
        if requested.canonicalize().ok().as_ref() != Some(&target)
            || std::fs::read(&target)? != original
            || (
                latest.ino(),
                latest.mode(),
                latest.mtime(),
                latest.mtime_nsec(),
            ) != (
                before.ino(),
                before.mode(),
                before.mtime(),
                before.mtime_nsec(),
            )
        {
            return Err(value_error(
                "connector settings changed during repair; retry after the other writer finishes",
            ));
        }
        for name in repaired {
            check_source_ownership(&home, &name)?;
        }
        temporary.persist(&target).map_err(|_| SetupError::io())?;
    }
    Ok(json!({
        "changed": changed,
        "status": if changed > 0 { "configured" } else { "unchanged" },
        "verification": "configuration only; live authentication is not checked",
        "next": if changed > 0 { "reconnect the GitHub connector with the host's supported tool, then run careerbrew day start" } else { "no automatic change; rerun careerbrew day start and keep any remaining recovery question" },
    }))
}

pub fn launcher_ready() -> bool {
    let folder = std::env::var_os("CAREERBREW_BIN_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/bin")))
        .unwrap_or_else(|| PathBuf::from(".local/bin"));
    let launcher = folder.join("careerbrew");
    std::fs::read(&launcher).ok().is_some_and(|bytes| {
        let marker = b"# careerbrew launcher, written by scripts/install-launcher.sh";
        bytes.windows(marker.len()).any(|window| window == marker)
            && std::fs::metadata(launcher)
                .ok()
                .is_some_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    })
}

/// Run one self-update repair with no input or captured payload and a bounded
/// process lifetime. Output is discarded because it may contain private paths.
pub fn repair(argv: &[String], cwd: &Path) -> bool {
    if argv.is_empty() {
        return false;
    }
    let mut child = match Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn command_report_ready(report: &Value, source: &Path, required: &BTreeSet<String>) -> bool {
    if report.get("version") != Some(&json!(1)) {
        return false;
    }
    let Some(commands) = report.get("commands").and_then(Value::as_array) else {
        return false;
    };
    let expected = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    let rows = commands
        .iter()
        .filter(|row| {
            value::get(row, "sourcePath").and_then(Value::as_str)
                == Some(expected.to_string_lossy().as_ref())
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return false;
    }
    for agent in required {
        if !rows.iter().any(|row| {
            value::get(row, "targets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|target| {
                    value::get(target, "agent").and_then(Value::as_str) == Some(agent)
                        && value::get(target, "ready") == Some(&Value::Bool(true))
                        && value::get(target, "status").and_then(Value::as_str) == Some("ready")
                        && value::get(target, "path")
                            .and_then(Value::as_str)
                            .is_some_and(|path| Path::new(path).is_absolute())
                })
        }) {
            return false;
        }
    }
    true
}

fn reported_bodies(report: &Value, source: &Path, required: &BTreeSet<String>) -> Vec<String> {
    let mut texts = Vec::new();
    let expected = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    for row in report
        .get("commands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if value::get(row, "sourcePath").and_then(Value::as_str)
            != Some(expected.to_string_lossy().as_ref())
        {
            continue;
        }
        for target in value::get(row, "targets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let agent = value::get(target, "agent")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !required.contains(agent) {
                continue;
            }
            let Some(path) = value::get(target, "path").and_then(Value::as_str) else {
                continue;
            };
            let selected = Path::new(path);
            if !selected.is_absolute() || selected.is_symlink() {
                continue;
            }
            if selected.is_file() {
                if let Ok(text) = std::fs::read_to_string(selected) {
                    texts.push(text);
                }
            } else if selected.is_dir() {
                for entry in std::fs::read_dir(selected).into_iter().flatten().flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "md") && !path.is_symlink() {
                        if let Ok(text) = std::fs::read_to_string(path) {
                            texts.push(text);
                        }
                    }
                }
            }
        }
    }
    texts
}

fn missing_paths(text: &str, home: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for token in text.split_whitespace() {
        if let Some(start) = token.find("~/") {
            let raw = token[start..]
                .trim_end_matches(['.', ':', ',', ')', ';', '`', '"', '\''])
                .to_owned();
            if !raw.is_empty() && !crate::expand_user(Path::new(&raw), home).exists() {
                out.push(raw);
            }
        }
    }
    out
}

/// Read-only command and plan completeness check over the current files.
pub fn workflow_check(
    declared: &[Value],
    repo: &Path,
    agentfiles: &[PathBuf],
    report: &Value,
    required: &BTreeSet<String>,
) -> Result<Vec<Value>> {
    let agentfile = std::fs::read_to_string(repo.join("Agentfile.yaml"))?;
    let config = value::yaml_read(&agentfile).map_err(SetupError::plugin)?;
    let commands = value::strings(value::get(&config, "commands"));
    let source = repo.join("commands/work.md");
    let selected = commands.len() == 1 && source.is_file();
    let want = if selected {
        std::fs::read_to_string(&source).unwrap_or_default()
    } else {
        String::new()
    };
    let ready =
        selected && (required.is_empty() || command_report_ready(report, &source, required));
    let installed = if selected {
        reported_bodies(report, &source, required)
    } else {
        Vec::new()
    };
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    let mut missing = Vec::new();
    for text in std::iter::once(&want).chain(installed.iter()) {
        missing.extend(missing_paths(text, &home));
    }
    let planned = declared
        .iter()
        .filter_map(|step| value::get(step, "step").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    let mut lost = 0;
    for path in agentfiles {
        if !path.exists() {
            continue;
        }
        let Ok(data) = value::yaml_read(&std::fs::read_to_string(path)?) else {
            lost += 1;
            continue;
        };
        let Some(steps) = value::get(&data, "workSteps").and_then(Value::as_array) else {
            continue;
        };
        for entry in steps {
            let name = if entry.is_string() {
                entry.as_str().unwrap_or("").to_owned()
            } else {
                value::text(value::get(entry, "step"))
            };
            let normalized = format!("/{}", name.trim_start_matches('/'));
            if name.is_empty() || !planned.contains(normalized.as_str()) {
                lost += 1;
            }
        }
    }
    let unresolved = declared
        .iter()
        .filter(|step| {
            value::get(step, "path")
                .and_then(Value::as_str)
                .is_none_or(|path| !Path::new(path).is_file())
        })
        .count();
    let unique_missing = missing.into_iter().collect::<BTreeSet<_>>().len();
    Ok(vec![
        json!({"check": "installed-command", "ok": ready, "value": if ready { "ready" } else { "missing, stale or readiness unavailable" }, "safe": "preserve and adopt edited /work copies into the source, then run agentbrew sync; never remove an unknown owner edit"}),
        json!({"check": "command-paths", "ok": unique_missing == 0, "value": format!("{unique_missing} missing path(s)"), "safe": "repair the missing ~/ paths in the installed /work source, then agentbrew sync"}),
        json!({"check": "plan-steps", "ok": lost == 0, "value": format!("{lost} missing legacy step(s)"), "safe": "move Agentfile workSteps into the plugin manifest work_steps, then careerbrew plugin refresh"}),
        json!({"check": "step-files", "ok": unresolved == 0, "value": format!("{unresolved} unresolved step(s)"), "safe": "repair each source_error in careerbrew day run --json; restore its declared file in its owning source"}),
    ])
}

/// Build a content-free preflight fact map. Credential comparison stays in
/// `check_rows`; this helper never prints either credential value.
pub fn facts(entries: &BTreeMap<String, Value>) -> Value {
    let mut servers = Map::new();
    for (name, server) in entries {
        let vars = vars_in(server)
            .into_iter()
            .map(|name| (name, Value::Null))
            .collect::<Map<String, Value>>();
        servers.insert(name.clone(), json!({"vars": vars}));
    }
    json!({"servers": servers})
}
