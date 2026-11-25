//! Native rust-13 normalization and identity controls.

use careerbrew_feed::{feed, transcript};
use serde_json::json;

#[test]
fn raw_reply_identities_and_skips_match_the_contract() {
    let tickets = json!({"tickets": [
        {"key": "PROJ-7", "summary": "Fix", "status": "Open", "updated": "2026-10-21T12:00:00Z"},
        {"key": "PROJ-8", "summary": "No time"},
    ]});
    let (rows, skipped, source) = feed::raw_events(&tickets, "jira").unwrap().unwrap();
    assert_eq!(source, "jira");
    assert_eq!(skipped, 1);
    assert_eq!(rows[0]["id"], "jira:PROJ-7:2026-10-21T12:00:00Z:Open");
    assert_eq!(rows[0]["thread"], "jira:PROJ-7");

    let calendar = json!({"events": [
        {"id": "ok", "start": {"dateTime": "2026-10-29T15:00:00Z"}, "summary": "Review"},
        {"id": "bad", "summary": "No start"},
    ]});
    let (rows, skipped, _) = feed::raw_events(&calendar, "calendar").unwrap().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(skipped, 1);
    assert_eq!(rows[0]["id"], "calendar:ok:2026-10-29T15:00:00Z");
}

#[test]
fn codex_rows_normalize_calls_results_quiet_and_errors() {
    let call = transcript::normalized(&json!({
        "type": "response_item",
        "payload": {"type": "function_call", "namespace": "mcp__mail__", "name": "search", "call_id": "q"},
    }));
    assert_eq!(call["message"]["content"][0]["name"], "mcp__mail__search");
    let end = transcript::normalized(&json!({
        "type": "event_msg",
        "timestamp": "2026-10-21T15:59:00Z",
        "payload": {
            "type": "mcp_tool_call_end",
            "call_id": "q",
            "invocation": {"server": "mail", "tool": "search"},
            "result": {"Ok": {"content": [{"type": "text", "text": "{\"messages\": []}"}]}}
        }
    }));
    assert_eq!(end["message"]["content"][1]["is_error"], false);
    let error = transcript::normalized(&json!({
        "type": "event_msg", "payload": {"type": "mcp_tool_call_end", "call_id": "e", "invocation": {"server": "mail", "tool": "search"}, "result": {"Err": "not reported"}},
    }));
    assert_eq!(error["message"]["content"][1]["is_error"], true);
}

#[test]
fn malformed_times_and_link_boundaries_fail_closed() {
    assert!(feed::utc(&json!("not-a-time")).is_err());
    assert_eq!(
        feed::links("see <https://example.com/a|doc> and https://example.com/b."),
        ["https://example.com/a", "https://example.com/b"],
    );
}
