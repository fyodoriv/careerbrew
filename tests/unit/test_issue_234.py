"""Issue #234: store live-feed tool replies straight from the agent session transcript.

A live feed driven by an agent pulls events with MCP tools. `work feed transcript` reads the session
transcript (.jsonl), maps each tool to a feed source (`feed_tool_sources`), follows the "saved to
<file>" pointer of a large reply, stores each event once, and names every source that stored
nothing this cycle, so a silent gap fails the run."""

import json
import shutil
from pathlib import Path

import pytest

from careerbrew import cli, config, paths
from careerbrew.work import feed, transcript

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "transcripts"
MAP = "mcp__chat__*=chat, mcp__tickets__*=tickets, mcp__mail__*=mail"


@pytest.fixture
def session(home, tmp_path, monkeypatch):
    """The fixture transcript copied to a temp folder, its pointer made absolute as Claude Code writes it."""
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T16:00")
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    root = tmp_path / "projects" / "p"
    shutil.copytree(FIX / "feed-cycle", root / "feed-cycle")
    path = root / "feed-cycle.jsonl"
    path.write_text((FIX / "feed-cycle.jsonl").read_text().replace("@DIR@", str(root)))
    config.set_value("feed_tool_sources", MAP)
    return path


def test_the_setting_maps_tool_globs_to_feed_sources(home):
    assert config.set_value("feed_tool_sources", MAP) == ["mcp__chat__*=chat", "mcp__tickets__*=tickets", "mcp__mail__*=mail"]
    with pytest.raises(ValueError, match="glob=source"):
        config.set_value("feed_tool_sources", "mcp__chat__search")
    with pytest.raises(ValueError, match="source"):
        config.set_value("feed_tool_sources", "mcp__chat__*=Chat Room")


def test_each_mapped_reply_is_stored_once_and_the_silent_source_is_a_gap(session):
    r = transcript.store(session)
    assert (r["new"], r["already"], r["skipped"]) == (4, 0, 1), "2 chat + 2 ticket events; the ticket with no time is skipped"
    assert r["replies"] == {"chat": 2, "tickets": 1, "mail": 1}, "the Bash call is not a feed source"
    assert r["errors"] == 1 and r["pointers"] == {"followed": 1, "refused": 0}
    assert r["stored"] == {"chat": 2, "tickets": 2, "mail": 0}
    assert r["gaps"] == ["mail"], "the mail search failed, so nothing was stored for mail this cycle"
    rows = {e["id"]: e for e in feed.load()}
    assert set(rows) == {"chat:C1-1001", "chat:C1-1002", "tickets:PROJ-7", "tickets:PROJ-8"}
    assert rows["chat:C1-1001"]["triage"] == "act", "a mention to me is triaged like any other event"
    assert rows["tickets:PROJ-7"]["source"] == "tickets"
    assert feed.originals()["tickets:PROJ-7"]["original"]["pad"] == "x" * 2000, "the original is kept as the tool returned it"


def test_a_rerun_stores_nothing_twice(session):
    transcript.store(session)
    r = transcript.store(session)
    assert (r["new"], r["updated"], r["already"]) == (0, 0, 4)
    assert r["stored"]["chat"] == 2, "an event already stored counts as stored, so a rerun is no gap"
    assert len(feed.load()) == 4


def test_text_and_payload_stay_encrypted_and_out_of_the_analysis(session):
    transcript.store(session)
    root = paths.data_root()
    for f in (root / "work" / "feed").glob("*.jsonl"):
        assert b"CANARYCHAT" not in f.read_bytes(), f"{f.name} is plaintext on disk"
    assert "CANARYCHAT" not in json.dumps(feed.load()[0]["id"])
    assert "Can you look" in feed.originals()["chat:C1-1001"]["text"]


def test_a_pointer_outside_the_transcripts_folder_is_not_followed(session, tmp_path):
    outside = tmp_path / "elsewhere.txt"
    outside.write_text(json.dumps([{"id": "X-1", "at": "2026-10-21T12:00:00Z", "title": "outside"}]))
    session.write_text(session.read_text().replace(str(session.parent / "feed-cycle" / "tool-results" / "big.txt"), str(outside)))
    r = transcript.store(session)
    assert r["pointers"] == {"followed": 0, "refused": 1}
    assert "tickets" in r["gaps"] and "tickets:X-1" not in {e["id"] for e in feed.load()}


def test_since_limits_the_cycle(session):
    r = transcript.store(session, since="2026-10-21T13:13:30Z")
    assert r["replies"] == {"chat": 1, "tickets": 0, "mail": 0}
    assert r["gaps"] == ["tickets", "mail"]
    r = transcript.store(session, since="30m")  # 16:00 minus 30 minutes: nothing in this transcript
    assert r["gaps"] == ["chat", "tickets", "mail"]


def test_no_tool_map_is_an_error_with_the_fix(home, tmp_path):
    path = tmp_path / "t.jsonl"
    path.write_text("")
    with pytest.raises(feed.FeedError, match="careerbrew config set feed_tool_sources"):
        transcript.store(path)


def test_cli_prints_counts_and_gaps_only_and_fails_on_a_gap(session, capsys):
    rc = cli.main(["work", "feed", "transcript", str(session)])
    out = capsys.readouterr().out
    assert rc == 1, "a source that stored nothing fails the cycle"
    assert "stored 4 new event(s), 0 updated, 0 already stored, 1 skipped" in out
    assert "gap  mail: nothing stored this cycle" in out
    for secret in ("CANARYCHAT", "CANARYTICKET", "Can you look", str(session.parent)):
        assert secret not in out
    config.set_value("feed_tool_sources", "mcp__chat__*=chat")
    assert cli.main(["work", "feed", "transcript", str(session)]) == 0


def test_native_call_results_match_claude_feed_facts(session):
    original = transcript.store(session)
    rows = []
    for raw in session.read_text().splitlines():
        row = json.loads(raw)
        content = row.get("message", {}).get("content", [])
        for block in content if isinstance(content, list) else []:
            if block.get("type") == "tool_use":
                payload = {"type": "function_call", "name": block["name"], "call_id": block["id"], "arguments": "{}"}
            elif block.get("type") == "tool_result":
                payload = {"type": "function_call_output", "call_id": block["tool_use_id"], "output": block.get("content"), "is_error": block.get("is_error", False)}
            else:
                continue
            rows.append({"type": "response_item", "timestamp": row.get("timestamp"), "payload": payload})
    native = session.with_name("native.jsonl")
    native.write_text("\n".join(json.dumps(r) for r in rows))
    result = transcript.store(native)
    assert result["stored"] == original["stored"]
    assert result["replies"] == original["replies"]
    assert result["errors"] == original["errors"]
    assert result["pointers"] == original["pointers"]
    assert result["new"] == 0 and result["already"] == 4
    assert transcript.store(native, since="30m")["gaps"] == ["chat", "tickets", "mail"]


def test_native_input_text_result_pairs_and_quiet_sources(home, tmp_path):
    config.set_value("feed_tool_sources", "mcp__mail__*=mail")
    path = tmp_path / "native.jsonl"
    rows = [
        {"type": "response_item", "payload": {"type": "function_call", "name": "mcp__mail__search", "call_id": "quiet", "arguments": "{}"}},
        {"type": "response_item", "payload": {"type": "function_call_output", "call_id": "quiet", "output": [{"type": "input_text", "text": '{"messages": []}'}]}},
    ]
    path.write_text("\n".join(json.dumps(r) for r in rows))
    result = transcript.store(path)
    assert result["gaps"] == [] and result["replies"] == {"mail": 1}


def test_unknown_transcript_format_names_the_unsupported_input(session):
    session.write_text('{"type":"future_format","payload":{}}\n')
    with pytest.raises(feed.FeedError, match="unsupported transcript format"):
        transcript.store(session)


def test_native_mcp_end_events_keep_quiet_and_error_distinct(home, tmp_path):
    # Codex rust-v0.166.0 protocol.rs McpToolCallEndEvent / mcp.rs CallToolResult.
    config.set_value("feed_tool_sources", "mcp__mail__*=mail, mcp__tickets__*=tickets")
    path = tmp_path / "native.jsonl"
    rows = [
        {
            "type": "event_msg",
            "timestamp": "2026-10-21T15:59:00Z",
            "payload": {"type": "mcp_tool_call_end", "call_id": "q", "invocation": {"server": "mail", "tool": "search", "arguments": {}}, "result": {"Ok": {"content": [{"type": "text", "text": '{"messages": []}'}]}}},
        },
        {"type": "event_msg", "timestamp": "2026-10-21T15:59:00Z", "payload": {"type": "mcp_tool_call_end", "call_id": "e", "invocation": {"server": "tickets", "tool": "search", "arguments": {}}, "result": {"Err": "CANARY-NATIVE-ERROR"}}},
    ]
    path.write_text("\n".join(json.dumps(row) for row in rows))
    result = transcript.store(path)
    assert result["replies"] == {"mail": 1, "tickets": 1}
    assert result["errors"] == 1 and result["gaps"] == ["tickets"]
    assert "CANARY-NATIVE-ERROR" not in json.dumps(result)
