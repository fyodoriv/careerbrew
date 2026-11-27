//! Session ownership, expiring claims and graceful handoff lifecycle.

use crate::{
    error, field, micro_stamp, now, read_json, setting, stamp, stamp_at, text, today, write_json,
    Result,
};
use careerbrew_projects::projects;
use chrono::{DateTime, Duration, FixedOffset};
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const STORY: &str = "US-107";
pub const CONTRACT: &str = "Project sessions: the main /work session runs only the work process (stages, rows, the watch loop, the one batched ask). Read `careerbrew work project list --json` first. Skip quiet projects and their owner questions until the return date; report any PM promise as deferred. Resume from the saved handoff. Each pressing project gets one long-lived session, reused all day: `careerbrew work session open <project> --id <session id>` records it or returns the live one (at most `work_session_cap` live; record nothing else for it with `day loop add`). Give that session the project's work and this contract. Read each task's full scope in its ticket or issue, not only the brief. Deliver tasks one at a time, in order. Finish every scope item, shipped and verified, before the next task. A plan, dry run, or list of questions alone is not delivery. Pause an affected task when blocked by an operator-only decision, approval, or outside wait; continue independent work. Batch foreseeable approvals up front, record blockers with `careerbrew work session ask`, and continue work that does not depend on them. Report each task with each scope item marked done or not done and its verification or blocker. After each subtask, state the goal status in one line and start or name the next goal step. End an owner reply with ranked next steps they can approve or pick now. If no owner action is needed, say so and name the next autonomous step; do not invent an ask. Before ending a turn, watch each external wait with a real monitor, scheduled wakeup, or the existing work-day loop with a recheck that covers it. Record new loops with `careerbrew day loop add`; do not create a second work-day timer. If the host cannot register a watch, say the wait is unwatched and why. Stop watching when work is done, blocked on the owner, or the work day ends. At the start of each project-session pass, read `careerbrew work project rules <project>`; if business-data policy refuses, block that project's pass and continue independent work. A project session writes through careerbrew, never through notes: each step `careerbrew work agent event <agent> \"<one line>\" --session <id>`, each project change `careerbrew work project update <project> <kind> \"<one line>\"`, each question `careerbrew work session ask <id> \"<the ask>\" --option ... --pick ... --recheck ...` (it never asks the user). Before it writes a shared thread or draft it claims it: `careerbrew work claim take slack-draft:<target> --session <id>` (exit 1: another session holds it, so skip it) and saves the draft with `careerbrew draft replace <target> --file <file> --session <id>`. Read every session with `careerbrew work session list --json`; the batched ask (`careerbrew day question recheck`) holds every session's questions. End of day: each session writes its project's handoff (`careerbrew work session handoff <id> \"<where it stands; the next step>\"`), then `careerbrew work session end --all` archives them and releases their claims.";

const PATH: &str = "work/sessions.json";

pub fn root() -> Result<PathBuf> {
    crate::root()
}

pub fn path(root: &Path) -> PathBuf {
    root.join(PATH)
}

pub fn check_id(value: &str) -> Result<String> {
    let value = value.trim();
    if !Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$")
        .expect("session id")
        .is_match(value)
    {
        return Err(error("SessionError", format!("session id {:?}: use the agent tool's own session id, 1 to 64 letters, digits, . _ : - (no spaces)", careerbrew_projects::chars(value, 40))));
    }
    Ok(value.to_owned())
}

pub fn load(root: &Path) -> Result<Value> {
    let state = read_json(root, PATH, json!({"sessions": [], "claims": []}))?;
    if !state.is_object()
        || !state.get("sessions").is_none_or(Value::is_array)
        || !state.get("claims").is_none_or(Value::is_array)
    {
        return Err(error(
            "SessionError",
            "saved session records are invalid; repair the session store before continuing",
        ));
    }
    let mut identities = Vec::new();
    for session in state
        .get("sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !session.is_object()
            || ["id", "project", "day", "started", "ended"]
                .iter()
                .any(|key| !field(session, key).is_some_and(Value::is_string))
            || text(field(session, "id")).is_empty()
            || text(field(session, "project")).is_empty()
            || DateTime::parse_from_rfc3339(&text(field(session, "started"))).is_err()
            || chrono::NaiveDate::parse_from_str(&text(field(session, "day")), "%Y-%m-%d").is_err()
        {
            return Err(error(
                "SessionError",
                "saved session records are invalid; repair the session store before continuing",
            ));
        }
        if session.get("record_id").is_some() != session.get("project_incarnation").is_some() {
            return Err(error(
                "SessionError",
                "saved session identity is incomplete; repair the session store before continuing",
            ));
        }
        if let Some(record) = field(session, "record_id") {
            if !valid_identity(record) {
                return Err(error("SessionError", "saved session identities are invalid or duplicated; repair the session store before continuing"));
            }
            identities.push(text(Some(record)));
        }
    }
    let unique = identities.iter().collect::<std::collections::BTreeSet<_>>();
    if unique.len() != identities.len() {
        return Err(error("SessionError", "saved session identities are invalid or duplicated; repair the session store before continuing"));
    }
    Ok(state)
}

fn valid_identity(value: &Value) -> bool {
    value.as_str().is_some_and(|value| {
        value.len() == 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn with_lock<T>(root: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let path = path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    careerbrew_vault::lock::with_lock(&path, f)?
}

fn parse_time(value: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(value).ok()
}

fn valid_claim(claim: &Value, at: DateTime<FixedOffset>) -> bool {
    field(claim, "until")
        .and_then(Value::as_str)
        .and_then(parse_time)
        .is_some_and(|until| until > at)
}

fn prune(root: &Path, state: &mut Value) -> Result<()> {
    let at = now();
    let open = state["sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|session| text(field(session, "ended")).is_empty())
        .map(|session| text(field(session, "id")))
        .collect::<std::collections::BTreeSet<_>>();
    state["claims"] = Value::Array(
        state["claims"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|claim| valid_claim(claim, at) && open.contains(&text(field(claim, "session"))))
            .cloned()
            .collect(),
    );
    let keep = (at - Duration::days(setting(root, "agent_keep_days", 7)?)).date_naive();
    state["sessions"] = Value::Array(
        state["sessions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|session| {
                text(field(session, "ended")).is_empty()
                    || chrono::NaiveDate::parse_from_str(&text(field(session, "day")), "%Y-%m-%d")
                        .is_ok_and(|day| day >= keep)
            })
            .cloned()
            .collect(),
    );
    Ok(())
}

fn changing<T>(root: &Path, f: impl FnOnce(&mut Value) -> Result<T>) -> Result<T> {
    with_lock(root, || {
        let mut state = load(root)?;
        let before = state.clone();
        let out = f(&mut state)?;
        prune(root, &mut state)?;
        if state != before {
            write_json(root, PATH, &state)?;
        }
        Ok(out)
    })
}

pub fn session_state(session: &Value, day: chrono::NaiveDate) -> &'static str {
    if !text(field(session, "ended")).is_empty() {
        "ended"
    } else if text(field(session, "day")) == day.format("%Y-%m-%d").to_string() {
        "live"
    } else {
        "stale"
    }
}

pub fn find<'a>(state: &'a Value, target: &str, open_only: bool) -> Option<&'a Value> {
    let rows = state["sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .filter(|session| !open_only || text(field(session, "ended")).is_empty());
    let mut project_match = None;
    for session in rows {
        if text(field(session, "id")) == target {
            return Some(session);
        }
        if project_match.is_none() && text(field(session, "project")) == target {
            project_match = Some(session);
        }
    }
    project_match
}

fn must<'a>(state: &'a Value, target: &str) -> Result<&'a Value> {
    find(state, target, true).ok_or_else(|| {
        error("SessionError", format!("no open session {target:?}: open one with `careerbrew work session open <project> --id <session id>` (careerbrew work session list shows them)"))
    })
}

fn quiet(record: &Value) -> bool {
    field(record, "quiet_until")
        .and_then(Value::as_str)
        .and_then(|value| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
        .is_some_and(|until| today() < until)
}

pub fn live(root: &Path, target: &str) -> Result<Value> {
    let state = load(root)?;
    let session = must(&state, target)?.clone();
    if session_state(&session, today()) != "live" {
        return Err(error("SessionError", format!("session {} was opened on {}: end it (careerbrew work session end {}) and open today's", text(field(&session, "id")), text(field(&session, "day")), text(field(&session, "id")))));
    }
    let every = projects::load(root)?;
    let record = every
        .get(text(field(&session, "project")))
        .ok_or_else(|| error("SessionError", "unknown project"))?;
    if quiet(record) {
        return Err(error(
            "SessionError",
            "this project is quiet until its saved date; clear --quiet-until to resume it",
        ));
    }
    Ok(session)
}

fn load_loops(root: &Path) -> Result<Value> {
    read_json(
        root,
        "day/loops.json",
        json!({"passes": [], "inputs": {}, "loops": []}),
    )
}

fn validate_loop_state(state: &Value, id: &str, record: &str) -> Result<()> {
    let running = state["loops"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| text(field(row, "stopped")).is_empty() && text(field(row, "id")) == id)
        .cloned()
        .collect::<Vec<_>>();
    if running.iter().any(|row| {
        text(field(row, "kind")) != "session" || text(field(row, "session_record")) != record
    }) {
        return Err(error("WatchError", "session loop id conflicts with another running loop; resolve that loop before continuing"));
    }
    Ok(())
}

fn commit_session_loop(
    root: &Path,
    id: &str,
    record: &str,
    before: &Value,
    state: &Value,
) -> Result<bool> {
    let path = root.join("day/loops.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    careerbrew_vault::lock::with_lock(&path, || -> Result<bool> {
        let mut loops = load_loops(root)?;
        validate_loop_state(&loops, id, record)?;
        if state != before {
            write_json(root, PATH, state)?;
        }
        let running = loops["loops"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| text(field(row, "stopped")).is_empty() && text(field(row, "id")) == id)
            .cloned()
            .collect::<Vec<_>>();
        if !running.is_empty() {
            return Ok(false);
        }
        loops["loops"].as_array_mut().expect("loops").push(json!({
            "kind": "session", "id": id, "step": "", "day": today().format("%Y-%m-%d").to_string(),
            "started": now().format("%Y-%m-%dT%H:%M%:z").to_string(), "stopped": "", "session_record": record,
        }));
        write_json(root, "day/loops.json", &loops)?;
        Ok(true)
    })?
}

pub fn open_session_with_loop(root: &Path, project: &str, ident: &str) -> Result<(Value, bool)> {
    let project = project.trim();
    let every = projects::load(root)?;
    if every.get(project).is_none() {
        return Err(error("SessionError", "unknown project: add it first with `careerbrew work project add <name>` (careerbrew work project list shows them)"));
    }
    let ident = if ident.is_empty() {
        String::new()
    } else {
        check_id(ident)?
    };
    let cap = setting(root, "work_session_cap", 3)?;
    projects::with_lock(root, || {
        with_lock(root, || {
            let (record, _migrated) = projects::session_record(root, project)?;
            let every = projects::load(root)?;
            let incarnation = text(field(&record, "incarnation_id"));
            if every
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(_, row)| {
                    field(row, "incarnation_id") == Some(&json!(incarnation.clone()))
                })
                .count()
                != 1
            {
                return Err(error("SessionError", "saved project identity is ambiguous; repair the project record before opening a session"));
            }
            if quiet(&record) {
                return Err(error(
                    "SessionError",
                    "this project is quiet until its saved date; clear --quiet-until to resume it",
                ));
            }
            let mut state = load(root)?;
            let before = state.clone();
            state
                .as_object_mut()
                .expect("session state")
                .entry("quiet_receipts")
                .or_insert_with(|| json!({}));
            let day = today();
            if let Some(ambiguous) =
                state["sessions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|session| {
                        text(field(session, "project")) == project
                            && session_state(session, day) == "live"
                            && text(field(session, "project_incarnation")) != incarnation
                    })
            {
                return Err(error("SessionError", format!("an older or unverified session is still live; record its handoff and end it with careerbrew work session end {} before opening another", text(field(ambiguous, "id")))));
            }
            let mine = state["sessions"]
                .as_array()
                .into_iter()
                .flatten()
                .rev()
                .find(|session| {
                    text(field(session, "project")) == project
                        && session_state(session, day) == "live"
                })
                .cloned();
            let mut got = if let Some(existing) = &mine {
                if !ident.is_empty() && ident != text(field(existing, "id")) {
                    let old = text(field(existing, "id"));
                    return Err(error("SessionError", format!("this project already has a live session today, {old}: send its work there (one session per project, reused all day). If that session cannot respond, record its status and next step with careerbrew work session handoff {old} \"<status and next step>\", then run careerbrew work session end {old} and open the new session")));
                }
                let mut got = existing.clone();
                got["reused"] = json!(true);
                got
            } else {
                if ident.is_empty() {
                    return Err(error("SessionError", "no live session for this project yet: pass --id <session id> (the agent tool's own id for the new session)"));
                }
                if state["sessions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|session| {
                        text(field(session, "id")) == ident
                            && text(field(session, "ended")).is_empty()
                    })
                {
                    return Err(error("SessionError", format!("session id {ident} already belongs to another open session: each project gets its own session")));
                }
                let live_count = state["sessions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|session| session_state(session, day) == "live")
                    .count() as i64;
                if live_count >= cap {
                    return Err(error("SessionError", format!("{live_count} of {cap} sessions are live (work_session_cap): end one first (careerbrew work session handoff <id> \"<text>\", then careerbrew work session end <id>), or raise the cap: careerbrew config set work_session_cap <n>")));
                }
                let record_id = uuid::Uuid::new_v4().simple().to_string();
                if state["sessions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|session| text(field(session, "record_id")) == record_id)
                {
                    return Err(error(
                        "SessionError",
                        "new session identity is invalid or duplicated; try again",
                    ));
                }
                let row = json!({"id": ident, "record_id": record_id, "project": project, "project_incarnation": incarnation, "day": day.format("%Y-%m-%d").to_string(), "started": stamp(), "ended": "", "handoff": "", "handoff_link": "", "handoff_id": ""});
                state["sessions"]
                    .as_array_mut()
                    .expect("sessions")
                    .push(row.clone());
                let mut got = row;
                got["reused"] = json!(false);
                got
            };
            let current_id = text(field(&got, "id"));
            let current_record = text(field(&got, "record_id"));
            let latest = state["sessions"]
                .as_array()
                .into_iter()
                .flatten()
                .rev()
                .find(|session| {
                    text(field(session, "project")) == project
                        && text(field(session, "project_incarnation")) == incarnation
                        && text(field(session, "record_id")) != current_record
                })
                .cloned();
            let legacy_history =
                state["sessions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|session| {
                        text(field(session, "project")) == project
                            && text(field(session, "record_id")) != current_record
                            && text(field(session, "project_incarnation")).is_empty()
                    });
            let generation = match field(&record, "quiet_generation") {
                Some(value) => {
                    let generation = text(Some(value));
                    if !Regex::new(r"^[0-9a-f]{32}$")
                        .expect("quiet identity")
                        .is_match(&generation)
                    {
                        return Err(error(
                            "SessionError",
                            "saved quiet identity is invalid; set the quiet period again",
                        ));
                    }
                    generation
                }
                None => String::new(),
            };
            let generations = every
                .as_object()
                .into_iter()
                .flatten()
                .map(|(_, row)| text(field(row, "quiet_generation")))
                .filter(|generation| !generation.is_empty())
                .collect::<Vec<_>>();
            if generations
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != generations.len()
            {
                return Err(error(
                    "SessionError",
                    "saved quiet identities are ambiguous; set the affected quiet periods again",
                ));
            }
            let mut receipts = state["quiet_receipts"]
                .as_object()
                .cloned()
                .ok_or_else(|| {
                    error(
                        "SessionError",
                        "saved quiet receipts are invalid; repair the session record",
                    )
                })?;
            receipts.retain(|key, _| generations.contains(key));
            let quiet_first = !generation.is_empty() && !receipts.contains_key(&generation);
            let current = state["sessions"]
                .as_array_mut()
                .expect("sessions")
                .iter_mut()
                .find(|session| text(field(session, "record_id")) == current_record)
                .expect("current session");
            if current.get("opening_brief").is_none() {
                let brief = if quiet_first {
                    latest
                        .as_ref()
                        .map(|row| text(field(row, "handoff")))
                        .unwrap_or_default()
                } else {
                    let yesterday = day - Duration::days(1);
                    latest
                        .as_ref()
                        .filter(|row| {
                            text(field(row, "day")) == yesterday.format("%Y-%m-%d").to_string()
                        })
                        .map(|row| text(field(row, "handoff")))
                        .unwrap_or_default()
                };
                current["opening_brief"] = json!(brief);
                if (legacy_history && latest.is_none())
                    || (generation.is_empty()
                        && (!text(field(&record, "quiet_until")).is_empty()
                            || !text(field(&record, "quiet_resumed")).is_empty()))
                {
                    current["opening_brief"] = json!("");
                    current["brief_status"] = json!("legacy-unverified");
                }
            } else if quiet_first {
                current["opening_brief"] = json!(latest
                    .as_ref()
                    .map(|row| text(field(row, "handoff")))
                    .unwrap_or_default());
            }
            if quiet_first {
                receipts.insert(generation, json!(current_record));
            }
            got["brief"] = current["opening_brief"].clone();
            if let Some(status) = current.get("brief_status") {
                got["brief_status"] = status.clone();
            }
            state["quiet_receipts"] = Value::Object(receipts);
            prune(root, &mut state)?;
            let loop_added =
                commit_session_loop(root, &current_id, &current_record, &before, &state)?;
            Ok((got, loop_added))
        })
    })
}

pub fn open_session(root: &Path, project: &str, ident: &str) -> Result<Value> {
    open_session_with_loop(root, project, ident).map(|(session, _)| session)
}

pub fn resource(value: &str) -> Result<String> {
    let raw = value;
    let value = value.trim();
    if value.starts_with("pr:") {
        let pattern = Regex::new(r"^pr:([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)#([1-9][0-9]*)$")
            .expect("pr resource");
        let Some(parts) = pattern.captures(value) else {
            return Err(error(
                "SessionError",
                "a PR claim is pr:owner/repo#number, with no whitespace or leading zeroes",
            ));
        };
        if raw != value
            || value.len() > 200
            || [".".into(), "..".into()].contains(&parts[1].to_owned())
            || [".".into(), "..".into()].contains(&parts[2].to_owned())
        {
            return Err(error(
                "SessionError",
                "a PR claim is pr:owner/repo#number, with no whitespace or leading zeroes",
            ));
        }
        return Ok(format!(
            "pr:{}/{}#{}",
            parts[1].to_lowercase(),
            parts[2].to_lowercase(),
            &parts[3]
        ));
    }
    if !Regex::new(r"^(browser|slack-draft|file|checkout):\S+$")
        .expect("resource")
        .is_match(value)
        || value.len() > 200
    {
        return Err(error("SessionError", format!("claim {:?}: name it like a step's `uses:` entry, <kind>:<name> with kind browser, slack-draft, file, checkout (a thread's reply draft is slack-draft:<channel>/<ts>)", careerbrew_projects::chars(value, 40))));
    }
    Ok(value.to_owned())
}

fn held(state: &Value, resource: &str) -> Option<Value> {
    let at = now();
    let open = state["sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|session| text(field(session, "ended")).is_empty())
        .map(|session| text(field(session, "id")))
        .collect::<std::collections::BTreeSet<_>>();
    state["claims"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|claim| {
            text(field(claim, "resource")) == resource
                && valid_claim(claim, at)
                && open.contains(&text(field(claim, "session")))
        })
        .cloned()
}

pub fn holder(root: &Path, resource: &str) -> Result<Option<Value>> {
    let resource = crate::sessions::resource(resource)?;
    Ok(held(&load(root)?, &resource))
}

pub fn shown(resource: &str) -> String {
    if std::env::var("CAREERBREW_TEST_BUSINESS").as_deref() == Ok("1") {
        resource.to_owned()
    } else {
        format!("{}:(hidden)", resource.split(':').next().unwrap_or(""))
    }
}

pub fn take(root: &Path, resource: &str, session: &str, minutes: Option<i64>) -> Result<Value> {
    let resource = crate::sessions::resource(resource)?;
    let minutes = minutes.unwrap_or(setting(root, "work_claim_minutes", 30)?);
    if !(1..=480).contains(&minutes) {
        return Err(error(
            "SessionError",
            "--minutes: a claim lasts 1 to 480 minutes",
        ));
    }
    let live_session = live(root, session)?;
    changing(root, |state| {
        let existing = held(state, &resource);
        if let Some(claim) = &existing {
            if text(field(claim, "session")) != text(field(&live_session, "id")) {
                return Err(error("ClaimHeld", format!("{} is claimed by session {} until {}: skip it (the claim ends when that session releases it, ends, or at that time)", shown(&resource), text(field(claim, "session")), &text(field(claim, "until"))[11..16])));
            }
        }
        state["claims"] = Value::Array(
            state["claims"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|claim| text(field(claim, "resource")) != resource)
                .cloned()
                .collect(),
        );
        let at = now();
        let row = json!({"resource": resource, "session": text(field(&live_session, "id")), "at": stamp_at(at), "until": stamp_at(at + Duration::minutes(minutes))});
        state["claims"]
            .as_array_mut()
            .expect("claims")
            .push(row.clone());
        let mut got = row;
        got["renewed"] = json!(existing.is_some());
        Ok(got)
    })
}

pub fn release(root: &Path, resource: &str, session: &str) -> Result<bool> {
    let resource = crate::sessions::resource(resource)?;
    let session = check_id(session)?;
    changing(root, |state| {
        let before = state["claims"].as_array().map_or(0, Vec::len);
        state["claims"] = Value::Array(
            state["claims"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|claim| {
                    !(text(field(claim, "resource")) == resource
                        && text(field(claim, "session")) == session)
                })
                .cloned()
                .collect(),
        );
        Ok(state["claims"].as_array().map_or(0, Vec::len) < before)
    })
}

pub fn claims(root: &Path) -> Result<Vec<Value>> {
    let mut state = load(root)?;
    prune(root, &mut state)?;
    Ok(state["claims"].as_array().cloned().unwrap_or_default())
}

pub fn write_handoff(root: &Path, target: &str, handoff_text: &str, link: &str) -> Result<Value> {
    let handoff_text = crate::one_line(handoff_text);
    if handoff_text.is_empty() {
        return Err(error(
            "SessionError",
            "the handoff is empty: say in one line where the project stands and the next step",
        ));
    }
    if !link.is_empty() && !(link.starts_with("https://") || link.starts_with("http://")) {
        return Err(error("SessionError", "--link must be a full http(s) URL"));
    }
    changing(root, |state| {
        let session = must(state, target)?.clone();
        let project = text(field(&session, "project"));
        let item_text = format!("{project}: {handoff_text}");
        let item_id = format!(
            "h-{}",
            &crate::digest(&format!("projects\n{item_text}"))[..8]
        );
        let day = today();
        let relative = format!("day/handoff-{}.json", day.format("%Y-%m-%d"));
        let handoff_path = root.join(&relative);
        if let Some(parent) = handoff_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        careerbrew_vault::lock::with_lock(&handoff_path, || -> Result<()> {
            let mut day_state = read_json(
                root,
                &relative,
                json!({"day": day.format("%Y-%m-%d").to_string(), "items": []}),
            )?;
            let old_id = text(field(&session, "handoff_id"));
            let mut items = day_state["items"].as_array().cloned().unwrap_or_default();
            items.retain(|item| {
                text(field(item, "id")) != item_id && text(field(item, "id")) != old_id
            });
            let mut item = json!({"id": item_id, "section": "projects", "text": item_text});
            if !link.is_empty() {
                item["link"] = json!(link);
            }
            items.push(item);
            day_state["items"] = json!(items);
            write_json(root, &relative, &day_state)
        })??;
        let current = state["sessions"]
            .as_array_mut()
            .expect("sessions")
            .iter_mut()
            .find(|row| text(field(row, "id")) == text(field(&session, "id")))
            .expect("session");
        current["handoff"] = json!(handoff_text);
        current["handoff_link"] = json!(link);
        current["handoff_id"] = json!(item_id);
        let mut at = now();
        if let Some(requested) = field(current, "wrap_requested_at")
            .and_then(Value::as_str)
            .and_then(parse_time)
        {
            at = at.max(requested + Duration::microseconds(1));
        }
        current["handoff_at"] = json!(micro_stamp(at));
        Ok(current.clone())
    })
}

pub fn fresh_handoff(session: &Value) -> bool {
    if text(field(session, "handoff")).is_empty() {
        return false;
    }
    let Some(requested) = field(session, "wrap_requested_at").and_then(Value::as_str) else {
        return true;
    };
    let Some(written) = field(session, "handoff_at")
        .and_then(Value::as_str)
        .and_then(parse_time)
    else {
        return false;
    };
    let Some(requested) = parse_time(requested) else {
        return false;
    };
    written > requested
}

pub fn request_wrap(root: &Path, request_id: &str) -> Result<Vec<String>> {
    if !path(root).exists() {
        return Ok(Vec::new());
    }
    changing(root, |state| {
        let mut requested_ids = Vec::new();
        for session in state["sessions"].as_array_mut().expect("sessions") {
            if !text(field(session, "ended")).is_empty()
                || (!text(field(session, "wrap_requested_at")).is_empty()
                    && (request_id.is_empty()
                        || text(field(session, "wrap_request_id")) == request_id))
            {
                continue;
            }
            let mut at = now();
            if let Some(prior) = field(session, "handoff_at")
                .and_then(Value::as_str)
                .and_then(parse_time)
            {
                at = at.max(prior + Duration::microseconds(1));
            }
            session["wrap_requested_at"] = json!(micro_stamp(at));
            session["wrap_request_id"] = json!(request_id);
            requested_ids.push(text(field(session, "id")));
        }
        Ok(requested_ids)
    })
}

fn stop_loop(root: &Path, id: &str, record: &str) -> Result<()> {
    let path = root.join("day/loops.json");
    careerbrew_vault::lock::with_lock(&path, || -> Result<()> {
        let mut state = load_loops(root)?;
        for row in state["loops"].as_array_mut().expect("loops") {
            if text(field(row, "id")) == id
                && text(field(row, "stopped")).is_empty()
                && text(field(row, "kind")) == "session"
                && text(field(row, "session_record")) == record
            {
                row["stopped"] = json!(now().format("%Y-%m-%dT%H:%M%:z").to_string());
            }
        }
        write_json(root, "day/loops.json", &state)
    })??;
    Ok(())
}

pub fn end(root: &Path, target: &str, every: bool) -> Result<Value> {
    changing(root, |state| {
        let rows = if every {
            state["sessions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| text(field(row, "ended")).is_empty())
                .cloned()
                .collect::<Vec<_>>()
        } else {
            vec![must(state, target)?.clone()]
        };
        let mut ended = Vec::new();
        let mut missing = Vec::new();
        for session in rows {
            if !fresh_handoff(&session) {
                missing.push(session);
                continue;
            }
            if !text(field(&session, "record_id")).is_empty() {
                stop_loop(
                    root,
                    &text(field(&session, "id")),
                    &text(field(&session, "record_id")),
                )?;
            }
            let current = {
                let current = state["sessions"]
                    .as_array_mut()
                    .expect("sessions")
                    .iter_mut()
                    .find(|row| text(field(row, "id")) == text(field(&session, "id")))
                    .expect("session");
                current["ended"] = json!(stamp());
                current.clone()
            };
            state["claims"] = Value::Array(
                state["claims"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|claim| text(field(claim, "session")) != text(field(&session, "id")))
                    .cloned()
                    .collect(),
            );
            ended.push(current);
        }
        Ok(json!({"ended": ended, "missing": missing}))
    })
}

pub fn session_states(root: &Path, include_ended: bool) -> Result<Vec<Value>> {
    let mut state = load(root)?;
    prune(root, &mut state)?;
    let day = today();
    Ok(state["sessions"].as_array().into_iter().flatten().rev().filter(|row| include_ended || text(field(row, "ended")).is_empty()).map(|row| {
        json!({"id": text(field(row, "id")), "state": session_state(row, day), "handoff": text(field(row, "handoff"))})
    }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conflicting_loop_cannot_interleave_the_session_transaction() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        // The store reads its root from the environment; a clean runner has none.
        std::env::set_var("CAREERBREW_HOME", root);
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("CAREERBREW_ENCRYPT", "off");
        std::env::set_var("CAREERBREW_EVENTS", "off");
        std::env::set_var("CAREERBREW_NOW", "2026-11-03T09:00:00+00:00");
        std::fs::create_dir_all(root.join("day")).unwrap();
        let loop_path = root.join("day/loops.json");
        std::fs::write(
            &loop_path,
            r#"{"passes":[],"inputs":{},"loops":[{"kind":"work","id":"conflict","stopped":""}]}"#,
        )
        .unwrap();
        let session_path = root.join(PATH);
        std::fs::create_dir_all(session_path.parent().unwrap()).unwrap();
        let before = json!({"sessions": []});
        std::fs::write(&session_path, serde_json::to_string(&before).unwrap()).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut child = None;
        careerbrew_vault::lock::with_lock(&loop_path, || {
            let owned = root.to_path_buf();
            let before = before.clone();
            child = Some(std::thread::spawn(move || {
                let result =
                    commit_session_loop(&owned, "conflict", "new-record", &before, &before);
                sender.send(result).unwrap();
            }));
            assert!(receiver
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err());
            Ok::<(), crate::ProjectError>(())
        })
        .unwrap()
        .unwrap();
        let result = receiver
            .recv()
            .expect("child result")
            .expect_err("conflicting loop");
        child.expect("child").join().expect("child thread");
        assert_eq!(result.class, "WatchError");
    }
}
