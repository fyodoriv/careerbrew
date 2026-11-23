//! Component adapters for rust-10's six assigned CLI handlers.

use crate::{agents, drafts, field, sessions, text};
use serde_json::{json, Value};
use std::path::Path;

pub fn invocation(exit: i32, stdout: String) -> Value {
    json!({"exit": exit, "stdout": stdout})
}

fn quoted(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}

fn agent_json(rows: &[Value]) -> String {
    let counts = agents::counts(rows);
    let counts_text = format!(
        "{{\"running\": {}, \"quiet\": {}, \"done\": {}, \"failed\": {}}}",
        counts["running"], counts["quiet"], counts["done"], counts["failed"]
    );
    let rows = rows.iter().map(|row| {
        format!(
            "{{\"agent\": {}, \"run\": {}, \"project\": {}, \"status\": {}, \"at\": {}, \"started_at\": {}, \"events\": {}}}",
            quoted(&row["agent"]), quoted(&row["run"]), quoted(&row["project"]), quoted(&row["status"]),
            quoted(&row["at"]), quoted(&row["started_at"]), row["events"]
        )
    }).collect::<Vec<_>>().join(", ");
    format!("{{\"counts\": {counts_text}, \"runs\": [{rows}]}}\n")
}

#[allow(clippy::too_many_arguments)] // Keeps the CLI handler's generated argument boundary explicit.
pub fn work_agent_event(
    root: &Path,
    agent: &str,
    title: &str,
    status: &str,
    url: &str,
    project: &str,
    run: &str,
    session: &str,
) -> (i32, String) {
    let mut run = run.to_owned();
    let mut project = project.to_owned();
    if !session.is_empty() {
        match sessions::live(root, session) {
            Ok(row) => {
                if (!run.is_empty() && run != text(field(&row, "id")))
                    || (!project.is_empty() && project != text(field(&row, "project")))
                {
                    return (2, "error: --session sets the run and the project: leave out --run and --project (breaks US-91)\n".into());
                }
                run = text(field(&row, "id"));
                project = text(field(&row, "project"));
            }
            Err(error) => return (2, format!("error: {} (breaks US-107)\n", error.message)),
        }
    }
    match agents::record(root, agent, title, status, url, &project, &run) {
        Ok(row) => {
            let agent = text(field(&row, "agent"));
            let run = text(field(&row, "run"));
            let run_text = if run != agent {
                format!(" (run {run})")
            } else {
                String::new()
            };
            (0, format!("event stored: {agent} {}{run_text}; Agent activity on Work shows it within seconds\n", text(field(&row, "status"))))
        }
        Err(error) => (2, format!("error: {} (breaks US-91)\n", error.message)),
    }
}

pub fn work_agent_show(root: &Path, as_json: bool) -> (i32, String) {
    let rows = match agents::runs(root) {
        Ok(rows) => rows,
        Err(error) => return (2, format!("error: {} (breaks US-91)\n", error.message)),
    };
    if as_json {
        return (0, agent_json(&rows));
    }
    if rows.is_empty() {
        return (0, "no agent activity yet: an agent reports each step with careerbrew work agent event <agent> \"<what it did>\" [--project <name>] [--url <link>]; --status done ends the run\n".into());
    }
    let counts = agents::counts(&rows);
    let mut out = format!(
        "{} run(s): {} running, {} quiet, {} done, {} failed\n",
        rows.len(),
        counts["running"],
        counts["quiet"],
        counts["done"],
        counts["failed"]
    );
    for row in rows {
        let agent = text(field(&row, "agent"));
        let run = text(field(&row, "run"));
        let run = if run != agent {
            format!(" ({run})")
        } else {
            String::new()
        };
        let project = text(field(&row, "project"));
        out.push_str(&format!(
            "  {:<8} {agent}{run}  {}  {} event(s), last {}\n",
            text(field(&row, "status")),
            if project.is_empty() {
                "(no project)".into()
            } else {
                project
            },
            row["events"],
            text(field(&row, "at"))
        ));
    }
    out.push_str("titles and links hidden: stdout is not a terminal and no plugin policy allows agents to read business data\n");
    (0, out)
}

pub fn work_claim(root: &Path, action: &str, resource: &str, session: &str) -> (i32, String) {
    let result = match action {
        "take" => sessions::take(root, resource, session, None).map(|claim| {
            let resource = text(field(&claim, "resource"));
            let session = text(field(&claim, "session"));
            format!("{} {resource} for session {session} until {}; release it when done: careerbrew work claim release {resource} --session {session}\n", if claim["renewed"] == true { "renewed" } else { "claimed" }, &text(field(&claim, "until"))[11..16])
        }),
        "release" => sessions::release(root, resource, session).and_then(|released| {
            if released {
                Ok(format!("released {resource}\n"))
            } else {
                Err(crate::error("SessionError", format!("session {session} holds no claim on {resource} (careerbrew work claim list)")))
            }
        }),
        _ => Ok(String::new()),
    };
    match result {
        Ok(output) => (0, output),
        Err(error) => (
            if error.class == "ClaimHeld" { 1 } else { 2 },
            format!("error: {} (breaks US-107)\n", error.message),
        ),
    }
}

pub fn work_claim_list(root: &Path, as_json: bool) -> (i32, String) {
    let rows = match sessions::claims(root) {
        Ok(rows) => rows,
        Err(error) => return (2, format!("error: {} (breaks US-107)\n", error.message)),
    };
    if as_json {
        let rows = rows
            .iter()
            .map(|row| {
                let kind = text(field(row, "resource"))
                    .split(':')
                    .next()
                    .unwrap_or("")
                    .to_owned();
                format!(
                    "{{\"session\": {}, \"at\": {}, \"until\": {}, \"kind\": {}}}",
                    quoted(&row["session"]),
                    quoted(&row["at"]),
                    quoted(&row["until"]),
                    quoted(&json!(kind))
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        return (0, format!("{{\"claims\": [{rows}]}}\n"));
    }
    let mut out = String::new();
    for row in &rows {
        out.push_str(&format!(
            "  {}  session {}  until {}\n",
            sessions::shown(&text(field(row, "resource"))),
            text(field(row, "session")),
            &text(field(row, "until"))[11..16]
        ));
    }
    out.push_str(&format!("claims: {} held\n", rows.len()));
    (0, out)
}

fn read_file(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| {
        format!(
            "could not read {}: {}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            match error.kind() {
                std::io::ErrorKind::NotFound => "FileNotFoundError",
                _ => "OSError",
            }
        )
    })
}

fn fit_json(row: &Value) -> String {
    format!(
        "{{\"kind\": {}, \"emoji\": {}, \"text\": {}, \"keep_draft\": {}, \"newest_ts\": {}, \"newer\": {}, \"stale\": {}, \"deploy_started\": {}, \"reason\": {}}}\n",
        quoted(&row["kind"]), quoted(&row["emoji"]), quoted(&row["text"]), row["keep_draft"],
        quoted(&row["newest_ts"]), row["newer"], row["stale"], row["deploy_started"], quoted(&row["reason"])
    )
}

pub fn draft_fit(
    root: &Path,
    thread_path: &Path,
    draft_path: Option<&Path>,
    seen: Option<&str>,
    major: Option<&str>,
    as_json: bool,
) -> (i32, String) {
    let _ = root;
    let result = (|| {
        let raw = read_file(thread_path)
            .map_err(|_| "could not read the thread file: FileNotFoundError".to_owned())?;
        let thread = serde_json::from_str::<Value>(&raw)
            .map_err(|_| "could not read the thread file: JSONDecodeError".to_owned())?;
        let thread = drafts::messages(&thread).map_err(|error| error.message)?;
        let draft = draft_path.map(read_file).transpose()?;
        drafts::fit(
            &thread,
            draft.as_deref(),
            seen.map(|value| json!(value)).as_ref(),
            major,
        )
        .map_err(|error| error.message)
    })();
    let row = match result {
        Ok(row) => row,
        Err(message) => return (2, format!("error: {message} (breaks US-98)\n")),
    };
    if as_json {
        return (0, fit_json(&row));
    }
    let mut out = format!(
        "draft: {} ({})\n",
        text(field(&row, "kind")),
        text(field(&row, "reason"))
    );
    if let Some(emoji) = field(&row, "emoji").and_then(Value::as_str) {
        out.push_str(&format!("emoji: :{emoji}:\n"));
    }
    if let Some(text) = field(&row, "text").and_then(Value::as_str) {
        out.push_str(&format!("text: {text}\n"));
    }
    if row["stale"] == true {
        out.push_str(&format!(
            "stale: {} newer message(s) since the one your draft answers\n",
            row["newer"]
        ));
    }
    if matches!(text(field(&row, "kind")).as_str(), "ack" | "stop" | "reply")
        && (!text(field(&row, "text")).is_empty() || row["keep_draft"] == true)
    {
        out.push_str("save it: careerbrew draft replace <target> --file <draft file>\n");
    }
    (0, out)
}

pub fn draft_replace(
    root: &Path,
    target: &str,
    file: &Path,
    session: &str,
    as_json: bool,
) -> (i32, String) {
    let result = read_file(file)
        .map_err(|message| crate::error("ValueError", message))
        .and_then(|text| drafts::replace(root, target, &text, false, session));
    let row = match result {
        Ok(row) => row,
        Err(error) => {
            let code = if matches!(error.class, "DraftMismatch" | "DraftClaimed") {
                1
            } else {
                2
            };
            return (code, format!("error: {} (breaks US-98)\n", error.message));
        }
    };
    let uses = drafts::uses_entry(target).unwrap_or_default();
    if as_json {
        return (0, format!("{{\"same\": true, \"sent_chars\": {}, \"uses\": {}, \"storage\": \"local\", \"chat_tool_saved\": false}}\n", row["sent_chars"], quoted(&json!(uses))));
    }
    (0, format!("draft saved and read back: {} characters match; stored locally, not saved in the chat tool; save it there separately when needed; a step that writes it declares `uses: {uses}`\n", row["sent_chars"]))
}

pub fn draft_show(root: &Path, target: &str) -> (i32, String) {
    let result = drafts::check_target(target)
        .and_then(|target| drafts::Backend::read(&drafts::LocalBackend::new(root), &target));
    let text = match result {
        Ok(text) => text,
        Err(error) => return (2, format!("error: {} (breaks US-98)\n", error.message)),
    };
    if drafts::normalize(text.as_deref()).is_empty() {
        return (1, format!("error: no draft saved for {target} (the live cycle lists each target it saved: careerbrew work live cycle) (breaks US-98)\n"));
    }
    let count = drafts::normalize(text.as_deref()).chars().count();
    (0, format!("draft {target}: {count} characters, never sent (text hidden: stdout is not a terminal and no plugin policy allows agents to read business data)\n"))
}

pub fn draft_check(sent_path: &Path, read_back_path: &Path) -> (i32, String) {
    let result = read_file(sent_path)
        .and_then(|sent| read_file(read_back_path).map(|back| drafts::compare(&sent, Some(&back))));
    let row = match result {
        Ok(row) => row,
        Err(message) => return (2, format!("error: {message} (breaks US-98)\n")),
    };
    if row["same"] != true {
        return (1, format!("error: the read-back differs at line {} ({} vs {} characters): the chat tool kept other text (breaks US-98)\n", row["line"], row["read_chars"], row["sent_chars"]));
    }
    (
        0,
        format!(
            "draft check: the read-back matches ({} characters)\n",
            row["sent_chars"]
        ),
    )
}
