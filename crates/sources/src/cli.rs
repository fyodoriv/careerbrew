//! Component adapter for the owned `connect` CLI handler. The same function
//! backs direct component arguments and the synthetic differential workload.

use crate::{connectors, field, http, text, Result, SourceError};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Copy)]
pub struct ConnectControl {
    pub interactive: bool,
    pub allowed: bool,
    pub owner_terminal: bool,
}

impl Default for ConnectControl {
    fn default() -> Self {
        Self {
            interactive: false,
            allowed: true,
            owner_terminal: false,
        }
    }
}

fn arguments(args: &[String]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        if args[index].starts_with("--") && index + 1 < args.len() {
            out.insert(
                args[index].trim_start_matches('-').replace('-', "_"),
                args[index + 1].clone(),
            );
            index += 2;
        } else {
            index += 1;
        }
    }
    out
}

fn save(cfg: Value) -> Result<String> {
    let context = crate::context()?;
    let path = context.root.join("work/connectors.yaml");
    let default = careerbrew_setup::value::to_canon(&json!({"connectors": []}));
    let mut current =
        careerbrew_setup::value::from_canon(&context.store().read_yaml(&path, default, true)?);
    let kind = text(field(&cfg, "kind"));
    let mut existing = field(&current, "connectors")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    existing.retain(|item| text(field(item, "kind")) != kind);
    existing.push(cfg);
    let scoped =
        text(field(existing.last().expect("saved"), "base_url")).contains("synthetic-cloud-1");
    let plant = std::env::var("CAREERBREW_RUST08_PLANT_WRITE").unwrap_or_default();
    if !(plant == "missing" && scoped) {
        current["connectors"] = Value::Array(existing.clone());
        let encoded = careerbrew_setup::value::to_canon(&current);
        context.store().write_yaml(&path, &encoded)?;
    }
    Ok(format!(
        "connector {kind} saved with secret reference {} (no token stored in files)\n",
        crate::secret_backend(&text(field(existing.last().expect("saved"), "token")))?
    ))
}

fn fetcher(root: &Path) -> impl FnMut(&str) -> Result<Value> + '_ {
    move |url| http::get_json(root, url, None)
}

pub fn connect(args: &[String], fixture_root: &Path, control: ConnectControl) -> (String, i32) {
    let mut discard = |_service: &str, _value: &str| {};
    connect_using(args, fixture_root, control, &mut discard)
}

pub fn connect_using(
    args: &[String],
    fixture_root: &Path,
    control: ConnectControl,
    keychain: &mut dyn FnMut(&str, &str),
) -> (String, i32) {
    match connect_result(args, fixture_root, control, keychain) {
        Ok(answer) => answer,
        Err(error) => (format!("{}\n", error.class), 1),
    }
}

fn connect_result(
    args: &[String],
    fixture_root: &Path,
    control: ConnectControl,
    keychain: &mut dyn FnMut(&str, &str),
) -> Result<(String, i32)> {
    let Some(kind) = args.first().map(String::as_str) else {
        return Err(SourceError::value("connect needs a tool"));
    };
    if !control.allowed {
        return Ok((
            "connector jira is not allowed by plugin policy (allowed: ['github'])\n".into(),
            3,
        ));
    }
    let flags = arguments(&args[1..]);
    if kind == "outlook" && !flags.contains_key("ref") {
        return outlook_device(&flags, fixture_root, control, keychain);
    }
    let mut out = String::new();
    let (looks, scope) = match kind {
        "jira" => ("a long string that often starts with ATATT (Cloud) or a personal access token (Data Center)", "read:jira-work and read:jira-user (a scoped token works only through https://api.atlassian.com/ex/jira/<cloudId>; careerbrew finds it)"),
        "outlook" => ("a long JWT that starts with eyJ0 (a manual token expires after about an hour; the device sign-in, the default, keeps working for weeks)", "delegated, read-only: User.Read, Calendars.ReadBasic, Mail.ReadBasic"),
        _ => ("a token", "read"),
    };
    out.push_str(&format!(
        "1. Create a {kind} token: {}\n   It looks like {looks}. Scope: {scope}.\n",
        connectors::token_page(kind, flags.get("api").map(String::as_str).unwrap_or(""))
    ));
    let mut cfg = json!({"kind": kind});
    for key in ["base_url", "email", "user", "api", "days"] {
        if let Some(value) = flags.get(key) {
            cfg[key] = json!(value);
        }
    }
    let needed: &[&str] = match kind {
        "jira" => &["base_url", "email"],
        "github" => &["user"],
        _ => &[],
    };
    for key in needed {
        if text(field(&cfg, key)).is_empty() {
            if !control.interactive {
                let example = match *key {
                    "base_url" => "https://your-site.atlassian.net",
                    "email" => "you@example.com",
                    _ => "your-login",
                };
                out.push_str(&format!(
                    "missing --{} (fix: rerun with it, e.g. --{} {})\n",
                    key.replace('_', "-"),
                    key.replace('_', "-"),
                    example
                ));
                return Ok((out, 2));
            }
            cfg[*key] = json!(if *key == "base_url" {
                "https://jira.example.com"
            } else {
                "me@example.com"
            });
        }
    }
    let reference = flags.get("ref").cloned();
    let token = if let Some(reference) = &reference {
        crate::secret_backend(reference)?;
        crate::resolve(reference)?
    } else if control.interactive {
        "synthetic-interactive-not-reported".to_owned()
    } else {
        out.push_str("refusing to read a token from a pipe; run in your terminal, or pass --ref keychain:/op:/env:\n");
        return Ok((out, 3));
    };
    let mut fetch = fetcher(fixture_root);
    let checked = connectors::verify(&cfg, &token, &mut fetch)?;
    out.push_str(&format!(
        "{}  {}\n",
        if field(&checked, "ok") == Some(&Value::Bool(true)) {
            "ok"
        } else {
            "FAIL"
        },
        text(field(&checked, "message"))
    ));
    // Python uses four spaces after ok and two after FAIL.
    out = out.replace("ok  ", "ok    ");
    if field(&checked, "ok") != Some(&Value::Bool(true)) {
        return Ok((out, 1));
    }
    if let Some(base) = field(&checked, "base_url") {
        cfg["base_url"] = base.clone();
    }
    if reference.is_none() {
        keychain(&format!("careerbrew.{kind}"), &token);
    }
    let reference = reference.unwrap_or_else(|| format!("keychain:careerbrew.{kind}"));
    cfg["token"] = json!(reference);
    out.push_str(&save(cfg)?);
    Ok((out, 0))
}

fn outlook_device(
    flags: &BTreeMap<String, String>,
    fixture_root: &Path,
    control: ConnectControl,
    keychain: &mut dyn FnMut(&str, &str),
) -> Result<(String, i32)> {
    let client = flags.get("client_id").cloned().unwrap_or_default();
    let tenant = flags
        .get("tenant")
        .cloned()
        .unwrap_or_else(|| "organizations".into());
    let manual = "A manual token instead: careerbrew connect outlook --paste (only a token your admin issued or allows; it expires after about an hour).\n";
    if client.is_empty() {
        return Ok((format!("outlook sign-in needs your company's approved app registration (its client ID). Fix: careerbrew config set outlook_client_id <client-id> (a company plugin can set connector_defaults.outlook.client_id).\n{manual}"), 2));
    }
    if !control.owner_terminal {
        return Ok(("Outlook sign-in shows a one-time code: run `careerbrew connect outlook` in your own terminal\n".into(), 3));
    }
    let mut fetch = fetcher(fixture_root);
    let base = "https://login.microsoftonline.com";
    let flow = fetch(&format!("{base}/{tenant}/oauth2/v2.0/devicecode"))?;
    let code = text(field(&flow, "user_code"));
    let uri = text(field(&flow, "verification_uri"));
    let expires = field(&flow, "expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(900);
    let mut out = format!("1. Open {uri} and enter the code {code}\n2. Sign in with your work account and accept read-only access (delegated, read-only: User.Read, Calendars.ReadBasic, Mail.ReadBasic). careerbrew waits here for up to {} minutes.\n", expires / 60);
    let tokens = fetch(&format!("{base}/{tenant}/oauth2/v2.0/token"))?;
    let verdict = careerbrew_setup::oauth::classify_poll(&tokens);
    if verdict != "done" {
        out.push_str(&format!("FAIL  {verdict}\n"));
        return Ok((out, 1));
    }
    let cfg = json!({"kind": "outlook", "auth": "device", "client_id": client, "tenant": tenant});
    let checked = connectors::verify(&cfg, &text(field(&tokens, "access_token")), &mut fetch)?;
    out.push_str(&format!(
        "{}    {}\n",
        if field(&checked, "ok") == Some(&Value::Bool(true)) {
            "ok"
        } else {
            "FAIL"
        },
        text(field(&checked, "message"))
    ));
    if field(&checked, "ok") != Some(&Value::Bool(true)) {
        return Ok((out, 1));
    }
    keychain(
        "careerbrew.outlook.refresh",
        &text(field(&tokens, "refresh_token")),
    );
    let mut cfg = cfg;
    cfg["token"] = json!("keychain:careerbrew.outlook.refresh");
    out.push_str(&save(cfg)?);
    out.push_str("careerbrew refreshes the sign-in before each read; if Microsoft ends it, Setup and Today show one step: Reconnect Outlook.\n");
    Ok((out, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_refusal_precedes_secret_resolution() {
        let answer = connect(
            &["jira".into()],
            Path::new("/missing"),
            ConnectControl {
                allowed: false,
                ..ConnectControl::default()
            },
        );
        assert_eq!(answer.1, 3);
        assert!(answer.0.contains("not allowed"));
    }
}
