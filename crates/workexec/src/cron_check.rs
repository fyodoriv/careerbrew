//! The content-free host goal/cron warning (`workday/cron_check.py`). The
//! caller supplies the goal-hook boolean; no host file is read.

use crate::{field, text, watch, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const SAFE: &str = "session cron jobs fire only while idle: run careerbrew day loop catch-up at each turn start; the host supplies CAREERBREW_GOAL_ACTIVE=1 only while its goal hook is active";

pub fn check(root: &Path, env: &BTreeMap<String, String>) -> Result<Vec<Value>> {
    let active = env
        .get("CAREERBREW_GOAL_ACTIVE")
        .map(|v| v.to_lowercase())
        .unwrap_or_default();
    let known = ["0", "1", "true", "false"].contains(&active.as_str());
    let goal = active == "1" || active == "true";
    let state = watch::load(root)?;
    let cron = state["loops"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|lp| text(field(lp, "kind")) == "cron" && text(field(lp, "stopped")).is_empty())
        .count();
    let shown = if goal {
        "active"
    } else if known {
        "inactive"
    } else {
        "unknown"
    };
    Ok(vec![json!({
        "check": "goal and cron",
        "ok": cron == 0 || (known && !goal),
        "value": format!("goal hook {shown}; {cron} active session cron(s)"),
        "safe": SAFE,
    })])
}
