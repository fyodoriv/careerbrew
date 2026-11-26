//! `work.session` component handler over the public rust-10 session API.

use crate::{emit, field, read_json, session_error, text, write_json, Result};
use careerbrew_sessions::{agents, sessions};
use chrono::{DateTime, Duration};
use serde_json::{json, Map, Value};
use std::path::Path;

pub const STORY: &str = "US-107";

fn invocation_text(exit: i32, stdout: String) -> Value {
    json!({"exit": exit, "stdout": stdout})
}

fn invocation_json(exit: i32, value: Value) -> Value {
    json!({"exit": exit, "value": value})
}

fn failure(error: &crate::ProjectError) -> Value {
    let exit = if error.class == "ClaimHeld" { 1 } else { 2 };
    invocation_text(exit, format!("error: {} (breaks {STORY})\n", error.message))
}

fn validate_state(root: &Path) -> Result<Value> {
    let state = sessions::load(root).map_err(|error| crate::error(error.class, error.message))?;
    for session in state
        .get("sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let ended = text(field(session, "ended"));
        if !ended.is_empty() && DateTime::parse_from_rfc3339(&ended).is_err() {
            return Err(session_error(
                "saved session records are invalid; repair the session store before continuing",
            ));
        }
    }
    for claim in state
        .get("claims")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !claim.is_object()
            || ["resource", "session", "until"].iter().any(|key| {
                field(claim, key)
                    .and_then(Value::as_str)
                    .is_none_or(|value| value.is_empty())
            })
            || DateTime::parse_from_rfc3339(&text(field(claim, "until"))).is_err()
        {
            return Err(session_error(
                "saved claims are invalid; repair the session store before continuing",
            ));
        }
    }
    if state
        .get("quiet_receipts")
        .is_some_and(|value| !value.is_object())
    {
        return Err(session_error(
            "saved quiet receipts are invalid; repair the session record",
        ));
    }
    Ok(state)
}

pub fn open(root: &Path, project: &str, ident: &str) -> Result<Value> {
    validate_state(root)?;
    let result = sessions::open_session_with_loop(root, project, ident)
        .map_err(|error| crate::error(error.class, error.message));
    match result {
        Ok((session, loop_added)) => {
            if loop_added {
                emit(
                    root,
                    "workday.loop",
                    json!({"status": "added", "kind_owned": false}),
                )?;
            }
            let status = if field(&session, "reused")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "reused"
            } else {
                "ok"
            };
            emit(
                root,
                "work.session",
                json!({"action": "open", "status": status}),
            )?;
            Ok(session)
        }
        Err(error) => {
            if error.message.contains("sessions are live") {
                let live = sessions::load(root)
                    .ok()
                    .and_then(|state| state.get("sessions").and_then(Value::as_array).cloned())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|session| sessions::session_state(session, crate::today()) == "live")
                    .count();
                emit(
                    root,
                    "work.session",
                    json!({"action": "open", "status": "refused", "error": "cap", "live": live}),
                )?;
            } else if error.message.contains("already has a live session") {
                emit(
                    root,
                    "work.session",
                    json!({"action": "open", "status": "refused", "error": "one_per_project"}),
                )?;
            }
            Err(error)
        }
    }
}

pub fn open_cli(root: &Path, project: &str, ident: &str, as_json: bool, show: bool) -> Value {
    let started = std::time::Instant::now();
    let session = match open(root, project, ident) {
        Ok(session) => session,
        Err(error) => return failure(&error),
    };
    if as_json {
        let mut result = json!({
            "id": session["id"],
            "day": session["day"],
            "started": session["started"],
            "reused": session["reused"],
        });
        if let Some(status) = field(&session, "brief_status") {
            result["brief_status"] = status.clone();
        }
        if show && !text(field(&session, "brief")).is_empty() {
            result["brief"] = session["brief"].clone();
        }
        let ms = started.elapsed().as_millis() as u64;
        if let Err(error) = emit(
            root,
            "cli.run",
            json!({"command": "work.session", "status": "ok", "via": "cli", "ms": ms}),
        ) {
            return failure(&error);
        }
        return invocation_json(0, result);
    }
    let reused = field(&session, "reused")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let how = if reused {
        "reused: the project's live session today"
    } else {
        "opened and recorded as a session loop"
    };
    let mut output = format!(
        "session {} {how}; it writes through careerbrew (careerbrew day run --json, `sessions`)\n",
        text(field(&session, "id"))
    );
    if text(field(&session, "brief_status")) == "legacy-unverified" {
        output.push_str("prior session history is unverified; check the session archive, subject to agent_keep_days\n");
    }
    if show && !text(field(&session, "brief")).is_empty() {
        output.push_str(&format!(
            "project handoff: {}\n",
            text(field(&session, "brief"))
        ));
    }
    let ms = started.elapsed().as_millis() as u64;
    if let Err(error) = emit(
        root,
        "cli.run",
        json!({"command": "work.session", "status": "ok", "via": "cli", "ms": ms}),
    ) {
        return failure(&error);
    }
    invocation_text(0, output)
}

pub fn step_of(ident: &str) -> String {
    format!("session:{ident}")
}

pub fn ask(
    root: &Path,
    session: &str,
    question: &str,
    options: &[String],
    pick: &str,
    recheck: &str,
) -> Result<Value> {
    let question = careerbrew_projects::projects::with_lock(root, || {
        let live = sessions::live(root, session)
            .map_err(|error| crate::error(error.class, error.message))?;
        crate::questions::add(
            root,
            &step_of(&text(field(&live, "id"))),
            question,
            Some(options),
            pick,
            recheck,
            "agent",
            crate::today(),
        )
    })?;
    emit(
        root,
        "work.session",
        json!({"action": "ask", "status": "ok"}),
    )?;
    Ok(question)
}

pub fn ask_cli(
    root: &Path,
    session: &str,
    question: &str,
    options: &[String],
    pick: &str,
    recheck: &str,
) -> Value {
    match ask(root, session, question, options, pick, recheck) {
        Ok(question) => invocation_text(0, format!(
            "question {} saved for {} ({} recheck); the main session asks it in the one batched ask (careerbrew day question recheck)\n",
            text(field(&question, "id")),
            text(field(&question, "step")),
            crate::questions::kind(&text(field(&question, "recheck")))
        )),
        Err(error) => failure(&error),
    }
}

pub fn handoff_cli(root: &Path, target: &str, handoff: &str, link: &str) -> Value {
    let result = sessions::write_handoff(root, target, handoff, link)
        .map_err(|error| crate::error(error.class, error.message));
    match result {
        Ok(session) => {
            if let Err(error) = emit(
                root,
                "work.session",
                json!({"action": "handoff", "status": "ok"}),
            ) {
                return failure(&error);
            }
            invocation_text(0, format!(
                "handoff written for session {} (Project sessions in careerbrew day handoff show); end it: careerbrew work session end {}\n",
                text(field(&session, "id")),
                text(field(&session, "id"))
            ))
        }
        Err(error) => failure(&error),
    }
}

fn restore_unverified_loops(root: &Path, ended: &[Value], before: &Value) -> Result<()> {
    if !ended
        .iter()
        .any(|row| text(field(row, "record_id")).is_empty())
    {
        return Ok(());
    }
    let mut current = read_json(root, "day/loops.json", before.clone())?;
    let original = current.clone();
    for session in ended
        .iter()
        .filter(|row| text(field(row, "record_id")).is_empty())
    {
        let ident = text(field(session, "id"));
        let Some(source) = before
            .get("loops")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|row| text(field(row, "id")) == ident)
            .cloned()
        else {
            continue;
        };
        let mut found = false;
        for row in current
            .get_mut("loops")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            if text(field(row, "id")) == ident {
                *row = source.clone();
                found = true;
            }
        }
        if !found {
            if let Some(rows) = current.get_mut("loops").and_then(Value::as_array_mut) {
                rows.push(source);
            }
        }
    }
    if current != original {
        write_json(root, "day/loops.json", &current)?;
    }
    Ok(())
}

pub fn end_cli(root: &Path, target: &str, every: bool, as_json: bool) -> Value {
    if target.is_empty() == !every {
        return failure(&session_error(
            "end needs a session id (or its project) or --all",
        ));
    }
    let loop_before = read_json(
        root,
        "day/loops.json",
        json!({"passes": [], "inputs": {}, "loops": []}),
    );
    let result = sessions::end(root, target, every)
        .map_err(|error| crate::error(error.class, error.message));
    let result = match result {
        Ok(result) => result,
        Err(error) => return failure(&error),
    };
    let ended = result
        .get("ended")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let missing = result
        .get("missing")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Ok(before) = loop_before {
        if let Err(error) = restore_unverified_loops(root, &ended, &before) {
            return failure(&error);
        }
    }
    if let Err(error) = emit(
        root,
        "work.session",
        json!({
            "action": "end", "status": if missing.is_empty() { "ok" } else { "missing_handoff" },
            "ended": ended.len(), "missing": missing.len(),
        }),
    ) {
        return failure(&error);
    }
    if as_json {
        return invocation_json(
            if missing.is_empty() { 0 } else { 1 },
            json!({
                "ended": ended.iter().map(|row| text(field(row, "id"))).collect::<Vec<_>>(),
                "missing": missing.iter().map(|row| text(field(row, "id"))).collect::<Vec<_>>(),
                "unverified_loops": ended.iter().filter(|row| text(field(row, "record_id")).is_empty()).map(|row| text(field(row, "id"))).collect::<Vec<_>>(),
            }),
        );
    }
    let mut output = String::new();
    for session in &ended {
        output.push_str(&format!(
            "  ended {}: archived with its handoff, claims released\n",
            text(field(session, "id"))
        ));
        if text(field(session, "record_id")).is_empty() {
            output.push_str(&format!("  older loop ownership is unverified: check the host loop, then run careerbrew day loop stop --id {} if it is stopped\n", text(field(session, "id"))));
        }
    }
    for session in &missing {
        let remedy = if text(field(session, "handoff")).is_empty() {
            "write its handoff first"
        } else {
            "write a fresh handoff"
        };
        output.push_str(&format!("  still open {}: {remedy}: careerbrew work session handoff {} \"<where it stands; the next step>\"\n", text(field(session, "id")), text(field(session, "id"))));
    }
    output.push_str(&format!(
        "sessions: {} ended, {} still open without a current handoff\n",
        ended.len(),
        missing.len()
    ));
    invocation_text(if missing.is_empty() { 0 } else { 1 }, output)
}

pub fn public(row: &Value) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), row["id"].clone());
    out.insert(
        "project".into(),
        json!(&crate::digest(&text(field(row, "project")))[..8]),
    );
    for key in [
        "day",
        "started",
        "ended",
        "state",
        "questions",
        "has_handoff",
        "wrap_requested_at",
        "handoff_at",
    ] {
        out.insert(
            key.into(),
            row.get(key).cloned().unwrap_or_else(|| json!("")),
        );
    }
    out.insert(
        "activity".into(),
        row.get("activity")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter(|(key, _)| key.as_str() != "title")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Map<_, _>>()
            .into(),
    );
    out.insert(
        "claims".into(),
        json!(row
            .get("claims")
            .and_then(Value::as_array)
            .map_or(0, Vec::len)),
    );
    Value::Object(out)
}

pub fn listing(root: &Path, include_ended: bool) -> Result<Vec<Value>> {
    let state = validate_state(root)?;
    let claims =
        sessions::claims(root).map_err(|error| crate::error(error.class, error.message))?;
    let runs = agents::runs(root).map_err(|error| crate::error(error.class, error.message))?;
    let open_questions = crate::questions::open_records(root, crate::today())?;
    let keep =
        (crate::now() - Duration::days(crate::setting(root, "agent_keep_days", 7)?)).date_naive();
    let mut out = Vec::new();
    for session in state
        .get("sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .rev()
    {
        let ended = !text(field(session, "ended")).is_empty();
        if ended && (!include_ended || crate::day(&text(field(session, "day")))? < keep) {
            continue;
        }
        let ident = text(field(session, "id"));
        let run = runs.iter().find(|run| text(field(run, "run")) == ident);
        let activity = run.map(|run| json!({"status": run["status"], "at": run["at"], "events": run["events"], "title": run["title"]})).unwrap_or_else(|| json!({"status": "none", "at": "", "events": 0, "title": ""}));
        let mine = claims
            .iter()
            .filter(|claim| text(field(claim, "session")) == ident)
            .map(|claim| json!({"resource": claim["resource"], "until": claim["until"]}))
            .collect::<Vec<_>>();
        let question_count = open_questions
            .iter()
            .filter(|question| text(field(question, "step")) == step_of(&ident))
            .count();
        let mut row = json!({
            "state": sessions::session_state(session, crate::today()),
            "activity": activity,
            "claims": mine,
            "questions": question_count,
            "has_handoff": sessions::fresh_handoff(session),
        });
        for key in [
            "id",
            "project",
            "day",
            "started",
            "ended",
            "handoff",
            "handoff_link",
            "wrap_requested_at",
            "handoff_at",
        ] {
            row[key] = session.get(key).cloned().unwrap_or_else(|| json!(""));
        }
        out.push(row);
    }
    Ok(out)
}

fn session_line(row: &Value, show: bool) -> String {
    let activity = field(row, "activity").cloned().unwrap_or_else(|| json!({}));
    let events = field(&activity, "events")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let last = if events > 0 {
        format!(
            "last event {} at {}",
            text(field(&activity, "status")),
            text(field(&activity, "at"))
                .get(11..16)
                .unwrap_or("")
                .to_owned()
        )
    } else {
        "no event yet".into()
    };
    let handoff = if field(row, "has_handoff")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "written"
    } else if !text(field(row, "handoff")).is_empty() {
        "stale or unverified: write a fresh handoff"
    } else {
        "not yet"
    };
    let mut head = format!(
        "  {:<6} {}{}  {last}, {} claim(s), {} open question(s), handoff {handoff}",
        text(field(row, "state")),
        text(field(row, "id")),
        if show {
            format!("  {}", text(field(row, "project")))
        } else {
            String::new()
        },
        row.get("claims")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        field(row, "questions").and_then(Value::as_i64).unwrap_or(0),
    );
    if show && !text(field(row, "handoff")).is_empty() {
        head.push_str(&format!("  handoff: {}", text(field(row, "handoff"))));
    }
    head
}

pub fn list_cli(root: &Path, include_ended: bool, as_json: bool, show: bool) -> Value {
    let rows = match listing(root, include_ended) {
        Ok(rows) => rows,
        Err(error) => return failure(&error),
    };
    let cap = match crate::setting(root, "work_session_cap", 3) {
        Ok(cap) => cap,
        Err(error) => return failure(&error),
    };
    if as_json {
        let sessions = if show {
            rows.clone()
        } else {
            rows.iter().map(public).collect::<Vec<_>>()
        };
        return invocation_json(0, json!({"cap": cap, "sessions": sessions}));
    }
    if rows.is_empty() {
        return invocation_text(0, "no project sessions: the /work agent opens one per pressing project with careerbrew work session open <project> --id <session id>\n".into());
    }
    let mut output = rows
        .iter()
        .map(|row| session_line(row, show) + "\n")
        .collect::<String>();
    let live = rows
        .iter()
        .filter(|row| text(field(row, "state")) == "live")
        .count();
    let stale = rows
        .iter()
        .filter(|row| text(field(row, "state")) == "stale")
        .count();
    output.push_str(&format!("sessions: {live} of {cap} live"));
    if stale > 0 {
        output.push_str(&format!(", {stale} left from an earlier day"));
    }
    output.push('\n');
    if !show {
        output.push_str("projects and handoffs hidden: stdout is not a terminal and no plugin policy allows agents to read business data\n");
    }
    invocation_text(0, output)
}
