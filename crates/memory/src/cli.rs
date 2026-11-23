//! Component adapters for rust-11 import, absorb, check-since and sync output.

use crate::{field, memory_sync, project_import, research, text};
use serde_json::{json, Value};
use std::path::Path;

pub fn invocation(exit: i32, stdout: String) -> Value {
    json!({"exit": exit, "stdout": stdout})
}

pub fn project_import_command(root: &Path, source: &str, dry_run: bool) -> Value {
    match project_import::run(root, source, dry_run) {
        Ok(result) => {
            let mut out = format!(
                "{}{} row(s): {} added, {} updated, {} unchanged, {} tracker source(s) added\n",
                if dry_run { "dry run: " } else { "" },
                result["rows"].as_u64().unwrap_or(0),
                result["added"].as_array().map(Vec::len).unwrap_or(0),
                result["updated"].as_array().map(Vec::len).unwrap_or(0),
                result["unchanged"].as_array().map(Vec::len).unwrap_or(0),
                result["sources"].as_u64().unwrap_or(0),
            );
            for key in ["added", "updated"] {
                let names = crate::strings(result.get(key));
                if !names.is_empty() {
                    out.push_str(&format!("  {key}: {}\n", names.join(", ")));
                }
            }
            let ignored = crate::strings(result.get("ignored"));
            if !ignored.is_empty() {
                out.push_str(&format!(
                    "  ignored columns: {} (import reads {})\n",
                    ignored.join(", "),
                    project_import::COLUMNS.join(", ")
                ));
            }
            out.push_str(if dry_run {
                "nothing written: run it again without --dry-run to save\n"
            } else {
                "see them: careerbrew work project list, or the Work view\n"
            });
            invocation(0, out)
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

pub fn project_sync_command(root: &Path, allowed: bool) -> Value {
    match memory_sync::run(root, allowed) {
        Ok(result) => {
            if result["enabled"] == Value::Bool(false) {
                return invocation(0, "project memory sync is off (turn it on: careerbrew config set project_memory_sync true)\n".into());
            }
            let input = &result["in"];
            let output = &result["out"];
            let mut out = format!(
                "memory in: {} changed file(s): {} new source(s), {} proposed, {} link(s), {} status line(s)",
                input["files"].as_u64().unwrap_or(0),
                input["sources"].as_u64().unwrap_or(0),
                input["proposed"].as_u64().unwrap_or(0),
                input["links"].as_u64().unwrap_or(0),
                input["status"].as_u64().unwrap_or(0),
            );
            if input["errors"].as_u64().unwrap_or(0) > 0 {
                out.push_str(&format!(
                    ", {} unreadable",
                    input["errors"].as_u64().unwrap_or(0)
                ));
            }
            out.push('\n');
            out.push_str(&format!(
                "memory out: {} new note(s) in {} project(s)",
                output["notes"].as_u64().unwrap_or(0),
                output["projects"].as_u64().unwrap_or(0)
            ));
            if output["removed"].as_u64().unwrap_or(0) > 0 {
                out.push_str(&format!(
                    ", {} note(s) of personal events removed",
                    output["removed"].as_u64().unwrap_or(0)
                ));
            }
            out.push('\n');
            let reason = text(field(output, "reason"));
            if !reason.is_empty() {
                out.push_str(&format!("note: {reason}\n"));
            }
            invocation(0, out)
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

pub fn project_absorb_command(root: &Path, name: &str, reference: &str, allowed: bool) -> Value {
    match research::absorb(root, reference, Some(name), allowed) {
        Ok(result) => {
            let mut out = format!(
                "absorbed {} {} into {}\n",
                text(field(&result, "kind")),
                text(field(&result, "ref")),
                text(field(&result, "project"))
            );
            if result["memory"] == Value::Bool(true) {
                out = out.trim_end().to_owned();
                out.push_str(&format!(
                    ": {} note(s) written to the project memory\n",
                    result["notes"].as_u64().unwrap_or(0)
                ));
            }
            let reason = text(field(&result, "reason"));
            if !reason.is_empty() {
                out.push_str(&format!("note: {reason}\n"));
            } else if result["read"] == Value::Bool(true)
                && result["memory"] == Value::Bool(false)
                && result["notes"].as_u64().unwrap_or(0) > 0
            {
                out.push_str(&format!("note: no memory path, so the notes were not saved (fix: careerbrew work project memory {} <path>)\n", text(field(&result, "project"))));
            }
            invocation(0, out)
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}

pub fn project_since_command(
    root: &Path,
    name: &str,
    reference: Option<&str>,
    allowed: bool,
    status: bool,
) -> Value {
    if status {
        return invocation(2, format!("error: --since stamps nothing, so it takes no --status (fix: read what changed, then `careerbrew work project check {name} <ref> --status ...`)\n"));
    }
    match research::changes(root, name, reference, allowed) {
        Ok(rows) => {
            let mut out =
                format!("{name}: what changed since each source's last check (nothing stamped)\n");
            for row in &rows {
                let state = text(field(row, "state"));
                let kind = text(field(row, "kind"));
                let source = text(field(row, "ref"));
                let since = text(field(row, "since"));
                let when = format!(
                    "since {} UTC",
                    &since.get(..16).unwrap_or(&since).replace('T', " ")
                );
                let detail = match state.as_str() {
                    "changed" => format!("{} change(s) {when}", row["count"].as_u64().unwrap_or(0)),
                    "unchanged" => when,
                    "never read" => "never checked".into(),
                    "unknown" => format!("why: {}", text(field(row, "reason"))),
                    "reference" => format!("skipped: {}", text(field(row, "reason"))),
                    _ => String::new(),
                };
                out.push_str(&format!("  {state:<10} {kind:<8} {source}  {detail}\n"));
            }
            let todo = rows
                .iter()
                .filter(|row| {
                    matches!(
                        text(field(row, "state")).as_str(),
                        "changed" | "never read" | "unknown"
                    )
                })
                .count();
            let tracked = rows
                .iter()
                .filter(|row| text(field(row, "state")) != "reference")
                .count();
            if todo > 0 {
                out.push_str(&format!("read these {todo} of {tracked} (changed, never read, unknown), then stamp each one you read: careerbrew work project check {name} <ref> --status \"<one line>\"\n"));
            } else {
                out.push_str(&format!(
                    "nothing changed in {tracked} source(s): no read needed now\n"
                ));
            }
            invocation(0, out)
        }
        Err(error) => invocation(2, format!("error: {}\n", error.message)),
    }
}
