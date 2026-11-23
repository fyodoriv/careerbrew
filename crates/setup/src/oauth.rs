//! OAuth reference decisions. Credential values are inputs to adapters only;
//! no classifier or report returns them.

use crate::value;
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;

pub const DEFAULT_TENANT: &str = "organizations";

pub fn classify_poll(answer: &Value) -> String {
    if value::bool_value(value::get(answer, "access_token"))
        && value::bool_value(value::get(answer, "refresh_token"))
    {
        return "done".into();
    }
    let error = value::get(answer, "error")
        .and_then(Value::as_str)
        .unwrap_or("");
    match error {
        "slow_down" => return "slow".into(),
        "authorization_pending" => return "wait".into(),
        _ => {}
    }
    let reason = match error {
        "authorization_declined" => "you declined the sign-in".into(),
        "expired_token" => "the code expired".into(),
        "bad_verification_code" => "Microsoft did not know the code".into(),
        "" => "Microsoft answered with no token".into(),
        other => format!("Microsoft answered {other}"),
    };
    format!(
        "outlook: sign-in did not finish: {reason} (fix: run `careerbrew connect outlook` again)"
    )
}

pub fn classify_refresh(answer: &Value) -> String {
    if value::bool_value(value::get(answer, "access_token")) {
        return "token".into();
    }
    let error = value::text(value::get(answer, "error"));
    if matches!(error.as_str(), "invalid_grant" | "interaction_required") {
        return "reconnect".into();
    }
    format!(
        "outlook: Microsoft refused the token ({}) (fix: check outlook_client_id and outlook_tenant; the app registration must allow public client flows)",
        if error.is_empty() { "no token" } else { &error }
    )
}

/// Flag, owner setting, plugin default, then the standard tenant.
pub fn settings(
    flag_client: &str,
    flag_tenant: &str,
    setting_client: &str,
    setting_tenant: &str,
    plugin_client: &str,
    plugin_tenant: &str,
) -> (String, String) {
    (
        if !flag_client.is_empty() {
            flag_client
        } else if !setting_client.is_empty() {
            setting_client
        } else {
            plugin_client
        }
        .trim()
        .to_owned(),
        if !flag_tenant.is_empty() {
            flag_tenant
        } else if !setting_tenant.is_empty() {
            setting_tenant
        } else if !plugin_tenant.is_empty() {
            plugin_tenant
        } else {
            DEFAULT_TENANT
        }
        .trim()
        .to_owned(),
    )
}

pub fn secret_backend(reference: &str) -> String {
    if reference.starts_with("keychain:") {
        "keychain".into()
    } else if reference.starts_with("op://") {
        "1password".into()
    } else if reference.starts_with("env:") {
        "env".into()
    } else {
        "unknown".into()
    }
}

/// Record only the reconnect state after a classified refresh answer.
pub fn write_reconnect(on: bool) -> crate::Result<()> {
    let ctx = crate::context()?;
    std::fs::create_dir_all(&ctx.root)?;
    std::fs::set_permissions(&ctx.root, std::fs::Permissions::from_mode(0o700))?;
    let path = ctx.root.join("work/outlook-auth.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if on || path.exists() {
        ctx.store().write_json_if_changed(
            &path,
            &value::to_canon(&serde_json::json!({"reconnect": on})),
        )?;
    }
    Ok(())
}
