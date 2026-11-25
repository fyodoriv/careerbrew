//! Component CLI adapters. Production argument parsing and routes stay unchanged.

use crate::{field, listen, live, notify, read_plain, review, text, truthy, Result};
use careerbrew_feed::cli::invocation;
use serde_json::{json, Value};
use std::path::Path;

pub fn live_start(root: &Path, sources: &Path) -> Value {
    match read_plain(sources).and_then(|text| live::start(root, &text)) {
        Ok(result) => {
            let sources = result["sources"].as_array().cloned().unwrap_or_default();
            let mut out = format!("live feed running: {} source(s), {} required; each cycle: careerbrew work live cycle\n", sources.len(), sources.iter().filter(|source| truthy(field(source, "required"))).count());
            for source in &sources {
                let where_ = if text(field(source, "adapter")) == "file" {
                    format!("inbox {}", text(field(source, "inbox")))
                } else {
                    format!("connector {}", text(field(source, "connector")))
                };
                out.push_str(&format!(
                    "  {:<10} {:<8} {}  {}; since {}  ({})\n",
                    text(field(source, "name")),
                    text(field(source, "kind")),
                    if truthy(field(source, "required")) {
                        "required"
                    } else {
                        "optional"
                    },
                    where_,
                    text(field(source, "since")).pipe_empty("the start"),
                    text(field(source, "origin"))
                ));
            }
            if sources.iter().any(|source| {
                !field(source, "pull")
                    .and_then(Value::as_object)
                    .is_none_or(serde_json::Map::is_empty)
            }) {
                out.push_str("pull hosts and channels hidden: stdout is not a terminal and no plugin policy allows agents to read business data\n");
            }
            invocation(0, out)
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

pub fn live_cycle(root: &Path) -> Value {
    match live::cycle(root, false) {
        Ok(rec) => {
            let stored = rec["sources"]
                .as_object()
                .into_iter()
                .flat_map(|rows| rows.values())
                .map(|row| {
                    row["new"].as_i64().unwrap_or_default()
                        + row["updated"].as_i64().unwrap_or_default()
                })
                .sum::<i64>();
            let counts = &rec["counts"];
            let verdicts = live::VERDICTS
                .iter()
                .filter(|verdict| counts[**verdict].as_i64().unwrap_or_default() > 0)
                .map(|verdict| {
                    format!(
                        "{verdict}={}",
                        counts[*verdict].as_i64().unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let mut out = format!(
                "cycle {} {}: {stored} event(s) stored; verdicts: {}\n",
                rec["n"].as_i64().unwrap_or_default(),
                text(field(&rec, "status")),
                if verdicts.is_empty() {
                    "none".into()
                } else {
                    verdicts
                }
            );
            for (name, row) in rec["sources"]
                .as_object()
                .into_iter()
                .flat_map(|rows| rows.iter())
            {
                let error_kind = text(field(row, "error_kind"));
                let error = if matches!(error_kind.as_str(), "LiveError" | "FeedError") {
                    text(field(row, "error"))
                } else {
                    format!("{error_kind} (details hidden: stdout is not a terminal)")
                };
                let state = if !text(field(row, "error")).is_empty() {
                    format!("error: {error}")
                } else if truthy(field(row, "pulled")) {
                    format!(
                        "{} item(s), {} new, {} updated, {} skipped",
                        row["items"].as_i64().unwrap_or_default(),
                        row["new"].as_i64().unwrap_or_default(),
                        row["updated"].as_i64().unwrap_or_default(),
                        row["skipped"].as_i64().unwrap_or_default()
                    )
                } else if truthy(field(row, "unchanged")) {
                    "no new intake".into()
                } else {
                    "no fresh reply".into()
                };
                out.push_str(&format!(
                    "  {name:<10} {state}; {}{}\n",
                    text(field(row, "health")),
                    if truthy(field(row, "unchanged")) {
                        "; unchanged"
                    } else {
                        ""
                    }
                ));
            }
            for (index, followup) in rec["followups"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                out.push_str(&format!(
                    "  event {:<3} {:<13} {:<8} {}: {}\n",
                    index + 1,
                    text(field(followup, "class")),
                    text(field(followup, "verdict")),
                    text(field(followup, "source")),
                    followup["actions"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|action| text(Some(action)))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            for goal in rec["goals"].as_array().into_iter().flatten() {
                out.push_str(&format!("  {}\n", listen::goal_line(goal, false)));
            }
            if rec.get("standup").is_some() {
                out.push_str(&format!("  {}\n", live::standup_line(&rec["standup"])));
            }
            for source in rec["not_stored"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "not_stored  {}: a required source is not healthy; the cycle failed\n",
                    text(Some(source))
                ));
            }
            for (index, action) in rec["actions"].as_array().into_iter().flatten().enumerate() {
                out.push_str(&format!("next {}. {}\n", index + 1, text(Some(action))));
            }
            invocation(
                if rec["not_stored"].as_array().is_none_or(Vec::is_empty) {
                    0
                } else {
                    1
                },
                out,
            )
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

pub fn live_status(root: &Path, json_output: bool) -> Value {
    match live::status(root) {
        Ok(state) => {
            if json_output {
                let canon = careerbrew_setup::value::to_canon(&state);
                return match careerbrew_vault::json_yaml::json_write(&canon) {
                    Ok(text) => invocation(0, text + "\n"),
                    Err(_) => invocation(2, "error: cannot render status\n".into()),
                };
            }
            let last = &state["last"];
            let mut out = format!(
                "live feed: {}; {} cycle(s)",
                if truthy(field(&state, "running")) {
                    "running"
                } else {
                    "stopped"
                },
                state["cycles"].as_i64().unwrap_or_default()
            );
            if !last.as_object().is_none_or(serde_json::Map::is_empty) {
                out.push_str(&format!(
                    "; last {} {} at {}",
                    last["n"].as_i64().unwrap_or_default(),
                    text(field(last, "status")),
                    text(field(last, "at"))
                ));
            }
            out.push('\n');
            let health = &state["health"];
            out.push_str(&format!(
                "source health: {}; {} of {} required source(s); {} coverage\n",
                text(field(health, "status")),
                health["healthy"].as_i64().unwrap_or_default(),
                health["required"].as_i64().unwrap_or_default(),
                text(field(health, "mode"))
            ));
            for command in health["commands"].as_array().into_iter().flatten() {
                out.push_str(&format!("  fix: {}\n", text(Some(command))));
            }
            for source in last["not_stored"].as_array().into_iter().flatten() {
                out.push_str(&format!("  not_stored: {}\n", text(Some(source))));
            }
            for source in state["sources"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "  {:<10} {:<8} {}  {}; since {}; last cycle {}  ({})\n",
                    text(field(source, "name")),
                    text(field(source, "kind")),
                    if truthy(field(source, "required")) {
                        "required"
                    } else {
                        "optional"
                    },
                    text(field(source, "adapter")),
                    text(field(source, "since")).pipe_empty("-"),
                    if truthy(field(source, "last_stored")) {
                        "stored"
                    } else {
                        "stored nothing"
                    },
                    text(field(source, "origin"))
                ));
            }
            if state["sources"].as_array().is_none_or(Vec::is_empty)
                && text(field(health, "status")) == "missing"
            {
                out.push_str("no live sources: careerbrew work live start --sources <file.yaml> (or a plugin with live sources)\n");
            }
            for (index, action) in state["actions"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                out.push_str(&format!("next {}. {}\n", index + 1, text(Some(action))));
            }
            invocation(0, out)
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

pub fn live_stop(root: &Path) -> Value {
    match live::stop(root) {
        Ok(result) => invocation(
            0,
            if truthy(field(&result, "was_running")) {
                format!("live feed stopped after {} cycle(s); cursors kept, so the next start continues\n", result["cycles"].as_i64().unwrap_or_default())
            } else {
                "the live feed was not running\n".into()
            },
        ),
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

trait PipeEmpty {
    fn pipe_empty(self, default: &str) -> String;
}
impl PipeEmpty for String {
    fn pipe_empty(self, default: &str) -> String {
        if self.is_empty() {
            default.into()
        } else {
            self
        }
    }
}

pub fn review_ask(root: &Path, url: &str, extra: &[String], check: Option<&Path>) -> Value {
    let result = if let Some(check) = check {
        read_plain(check).and_then(|text| review::check_pr(root, url, &text)).map(|result| {
            if !truthy(field(&result, "ok")) {
                return invocation(1, format!("refused: the draft names {} person(s) and {} group handle(s); this PR needs {} named people (group handles do not count) (breaks US-90)\n", result["people"].as_i64().unwrap_or_default(), result["groups"].as_i64().unwrap_or_default(), result["needed"].as_i64().unwrap_or_default()));
            }
            invocation(0, format!("ok: the draft names {} person(s) for {} required approvals\n", result["people"].as_i64().unwrap_or_default(), result["needed"].as_i64().unwrap_or_default()))
        })
    } else {
        review::build(root, url, extra).map(|result| {
            let source = if text(field(&result, "source")) != "unknown" {
                format!("{} required by {}", result["required"].as_i64().unwrap_or_default(), text(field(&result, "source")))
            } else {
                "required count unreadable".into()
            };
            let people = result["people"].as_array().cloned().unwrap_or_default();
            let mut out = format!("review ask: {}; needs {} approvals ({}; never fewer than {}); names {} person(s); {} group handle(s) not counted; {} changed file(s)\n", text(field(&result, "status")), result["needed"].as_i64().unwrap_or_default(), source, review::MIN_PEOPLE, people.len(), result["groups"].as_array().map(Vec::len).unwrap_or_default(), result["files"].as_i64().unwrap_or_default());
            if text(field(&result, "status")) == "short" {
                out.push_str(&format!("refused: {} more person(s) needed (fix: add --reviewer <login> per person) (breaks US-90)\n", result["needed"].as_i64().unwrap_or_default() - people.len() as i64));
            }
            out.push_str("draft hidden: stdout is not a terminal and no plugin policy allows agents to read business data\n");
            invocation(if text(field(&result, "status")) == "ready" { 0 } else { 1 }, out)
        })
    };
    result
        .unwrap_or_else(|error| invocation(2, format!("error: {} (breaks US-90)\n", error.message)))
}

pub fn notify_check(root: &Path, wait: bool) -> Value {
    match notify::run(root, wait, None) {
        Ok(result) => {
            if !truthy(field(&result, "enabled")) {
                return invocation(0, "notifications are off: turn them on with careerbrew config set notify_events true (careerbrew notify status shows the rules)\n".into());
            }
            let state = notify::status(root).unwrap_or_else(|_| json!({}));
            let batched = result["batched"].as_i64().unwrap_or_default();
            let reason = match text(field(&result, "reason")).as_str() {
                "sent" => format!("1 pop-up ({batched} together)"),
                "scheduled" => format!("1 pop-up scheduled ({batched} together); delivery is not yet confirmed"),
                "not_sent" => "pop-up not sent; check the Notifications view".into(),
                "quiet" => format!("no pop-up: quiet hours ({}); {batched} arrive together after them", text(field(&state, "quiet_hours"))),
                "cap" => format!("no pop-up: the daily cap ({}) is reached; {batched} wait in the Notifications view", state["daily_cap"].as_i64().unwrap_or_default()),
                "batch" => format!("no pop-up yet: {batched} arrive with the next one (notify_batch_minutes)"),
                "off" => format!("no pop-up ({}); {batched} wait in the Notifications view", text(field(&state, "backend_note")).split(';').next().unwrap_or("")),
                _ => "nothing new to pop up".into(),
            };
            invocation(
                0,
                format!(
                    "checked: {} new, {} open; {reason}\n",
                    result["new"].as_i64().unwrap_or_default(),
                    result["open"].as_i64().unwrap_or_default()
                ),
            )
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

pub fn notify_list(root: &Path, show: bool, all: bool) -> Value {
    match notify::view(root) {
        Ok(view) => {
            let state = &view["status"];
            let rows = view["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| all || notify::OPEN.contains(&text(field(row, "status")).as_str()))
                .collect::<Vec<_>>();
            let mut out = format!(
                "notifications: {} open ({} unread), {}\n",
                state["open"].as_i64().unwrap_or_default(),
                state["unread"].as_i64().unwrap_or_default(),
                if truthy(field(state, "enabled")) {
                    "on"
                } else {
                    "off"
                }
            );
            if show {
                out.push_str("titles hidden: stdout is not a terminal and no plugin policy allows agents to read business data\n");
            }
            for row in rows {
                out.push_str(&format!(
                    "- {} [{}] p{} {}",
                    text(field(row, "id")),
                    text(field(row, "status")),
                    row["priority"].as_i64().unwrap_or_default(),
                    text(field(row, "headline"))
                ));
                if row["count"].as_i64().unwrap_or_default() > 1 {
                    out.push_str(&format!(
                        " ({}x)",
                        row["count"].as_i64().unwrap_or_default()
                    ));
                }
                out.push('\n');
                out.push_str(&format!(
                    "    why: {}. next: {}. act: careerbrew notify act {} {}\n",
                    text(field(row, "why")),
                    text(field(row, "next")),
                    text(field(row, "id")),
                    if row["actions"].as_array().is_none_or(Vec::is_empty) {
                        "read".into()
                    } else {
                        row["actions"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|value| text(Some(value)))
                            .collect::<Vec<_>>()
                            .join("|")
                    }
                ));
            }
            if view["items"].as_array().is_none_or(Vec::is_empty)
                && !truthy(field(state, "enabled"))
            {
                out.push_str("nothing open: careerbrew config set notify_events true turns notifications on\n");
            }
            invocation(0, out)
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

pub fn notify_status(root: &Path) -> Value {
    match notify::status(root) {
        Ok(state) => {
            let quiet = text(field(&state, "quiet_hours"));
            let mut out = format!(
                "notifications: {}\n",
                if truthy(field(&state, "enabled")) {
                    "on"
                } else {
                    "off (opt in: careerbrew config set notify_events true)"
                }
            );
            out.push_str(&format!(
                "pop-ups: {}\n",
                text(field(&state, "backend_note"))
            ));
            out.push_str(&format!("rules: priority >= {} (5 a page on you, 4 a DM or review request, 3 a mention, important mail, or suggestion), quiet hours {}{}, cap {} a day ({} today), batch {} min, snooze {} min\n", state["min_priority"].as_i64().unwrap_or_default(), if quiet.is_empty() { "none".into() } else { quiet }, if truthy(field(&state, "quiet_now")) { " (now)" } else { "" }, state["daily_cap"].as_i64().unwrap_or_default(), state["sent_today"].as_i64().unwrap_or_default(), state["batch_minutes"].as_i64().unwrap_or_default(), state["snooze_minutes"].as_i64().unwrap_or_default()));
            out.push_str(&format!("open: {} ({} unread): careerbrew notify list, or the web app's Notifications view\n", state["open"].as_i64().unwrap_or_default(), state["unread"].as_i64().unwrap_or_default()));
            invocation(0, out)
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

pub fn notify_act(root: &Path, id: &str, action: &str) -> Value {
    match notify::act(root, id, action, "cli", None) {
        Ok(row) => {
            let mut out = format!("{id}: {}", text(field(&row, "status")));
            if text(field(&row, "status")) == "snoozed" {
                out.push_str(&format!(
                    " until {}",
                    &text(field(&row, "snooze_until"))[11..16]
                ));
            }
            if text(field(&row, "kind")) == "suggestion" && ["accept", "dismiss"].contains(&action)
            {
                out.push_str(" (the suggestion is decided in the inbox too)");
            }
            out.push('\n');
            invocation(0, out)
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

pub fn invocation_json(value: Value) -> Result<String> {
    Ok(invocation(
        0,
        careerbrew_vault::json_yaml::json_write(&careerbrew_setup::value::to_canon(&value))
            .map_err(|_| crate::error("ValueError", "cannot render JSON"))?
            + "\n",
    )
    .to_string())
}
