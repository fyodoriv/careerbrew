"""Issue #254: a live-feed cycle that pulled a source but stored nothing fails; raw tool replies store as-is.

`feed.ingest` records each source it stored in the cycle (`work/feed/feed-cycle.json`). `feed.mark_cycle`
returns `not_stored` (every required source with no ingest this cycle) and clears the record. A tool
reply saved exactly as returned, an object with a `tickets` list (a ticket search) or a `messages`
list (a mail search), stores one event per item through `work feed add` and `work feed transcript`."""

import json
import shutil
from pathlib import Path

import pytest

from careerbrew import cli, config, server
from careerbrew.work import feed, transcript

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "transcripts"
REQUIRED = ["chat", "jira", "mail"]
TICKETS = {
    "total": 3,
    "tickets": [
        {"key": "PROJ-7", "summary": "Fix the login bug", "description": "Steps CANARYTICKET", "status": "In Progress", "updated": "2026-10-21T12:00:00Z", "url": "https://tickets.example.com/browse/PROJ-7"},
        {"key": "PROJ-8", "fields": {"summary": "Write the API docs", "status": {"name": "To Do"}, "updated": "2026-10-21T12:30:00Z", "project": {"name": "Docs"}}},
        {"key": "PROJ-9", "summary": "no time on this one"},
    ],
}
MAIL = {
    "messages": [
        {"id": "AAMk-1", "subject": "Quarterly plan", "receivedDateTime": "2026-10-21T11:00:00Z", "from": {"emailAddress": {"name": "Sam Lee", "address": "sam@example.com"}}, "importance": "high", "bodyPreview": "CANARYMAIL body"},
        {"id": "AAMk-2", "subject": "", "receivedDateTime": "2026-10-21T11:30:00Z", "flag": {"flagStatus": "flagged"}},
    ]
}


@pytest.fixture
def feedhome(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T16:00")
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    return home


def _chat(n: int = 1) -> list[dict]:
    return [{"id": f"chat:C{i}", "source": "chat", "at": "2026-10-21T13:00:00Z", "title": f"hello {i}", "kind": "message"} for i in range(n)]


def test_a_cycle_that_ingests_only_chat_returns_the_other_required_sources(feedhome):
    feed.ingest(_chat())
    r = feed.mark_cycle(REQUIRED)
    assert r["stored"] == ["chat"]
    assert r["not_stored"] == ["jira", "mail"], "every required source with no ingest this cycle"
    assert feed.mark_cycle(REQUIRED)["not_stored"] == REQUIRED, "mark_cycle clears the record, so the next cycle starts empty"


def test_an_event_already_stored_still_counts_as_stored_this_cycle(feedhome):
    feed.ingest(_chat())
    feed.mark_cycle(REQUIRED)
    assert feed.ingest(_chat())["new"] == 0
    assert "chat" not in feed.mark_cycle(REQUIRED)["not_stored"], "a quiet rerun of the same events is a stored source, not a gap"


def test_required_sources_default_to_the_tool_map_and_none_is_an_error_with_the_fix(feedhome):
    with pytest.raises(feed.FeedError, match="--require"):
        feed.mark_cycle()
    config.set_value("feed_tool_sources", "mcp__chat__*=chat, mcp__tickets__*=jira")
    feed.ingest(_chat())
    assert feed.mark_cycle()["not_stored"] == ["jira"]
    with pytest.raises(feed.FeedError, match="short lowercase name"):
        feed.mark_cycle(["Not A Source"])


def test_transient_intake_is_bookkeeping_but_completion_moves_health_version(feedhome):
    feed.ingest(_chat())
    before = server.changes_view()["version"]
    feed.ingest(_chat())  # already stored: only the cycle record could change
    assert server.changes_view()["version"] == before
    feed.mark_cycle(REQUIRED)
    assert server.changes_view()["version"] != before, "explicit completion writes a health input"
    assert feed.cycle_path().name in server.CHANGE_SKIP


def test_a_raw_ticket_search_reply_stores_one_event_per_ticket(feedhome):
    r = feed.add_lines(json.dumps(TICKETS), source="jira")
    assert (r["new"], r["skipped"]) == (2, 1), "the ticket with no updated time is skipped"
    rows = {e["id"]: e for e in feed.load()}
    assert set(rows) == {"jira:PROJ-7:2026-10-21T12:00:00Z:In Progress", "jira:PROJ-8:2026-10-21T12:30:00Z:To Do"}, "id from key + updated + status"
    seven = rows["jira:PROJ-7:2026-10-21T12:00:00Z:In Progress"]
    assert (seven["source"], seven["kind"], seven["status"], seven["thread"]) == ("jira", "ticket", "In Progress", "jira:PROJ-7")
    assert seven["url"] == "https://tickets.example.com/browse/PROJ-7"
    assert rows["jira:PROJ-8:2026-10-21T12:30:00Z:To Do"]["project"] == "Docs", "a search reply's `fields` object is read too"
    assert feed.originals()["jira:PROJ-7:2026-10-21T12:00:00Z:In Progress"]["original"]["description"].endswith("CANARYTICKET"), "kept as returned"
    assert "CANARYTICKET" not in json.dumps(feed.load())
    moved = {**TICKETS, "tickets": [{**TICKETS["tickets"][0], "status": "Done", "updated": "2026-10-21T15:00:00Z"}]}
    assert feed.add_lines(json.dumps(moved), source="jira")["new"] == 1, "a status change is a new event"
    assert feed.mark_cycle(REQUIRED)["not_stored"] == ["chat", "mail"]


def test_a_raw_mail_search_reply_stores_one_event_per_message(feedhome):
    r = feed.add_lines(json.dumps(MAIL))
    assert (r["new"], r["skipped"]) == (2, 0)
    rows = {e["id"]: e for e in feed.load()}
    one, two = rows["mail:AAMk-1"], rows["mail:AAMk-2"]
    assert (one["source"], one["kind"], one["from"], one["importance"], one["triage"]) == ("mail", "mail", "Sam Lee", "high", "act")
    assert (two["title"], two["status"], two["triage"]) == ("(no subject)", "flagged", "act")
    assert "CANARYMAIL" in feed.originals()["mail:AAMk-1"]["text"]
    assert "CANARYMAIL" not in json.dumps(feed.load())


def test_a_raw_reply_with_no_items_is_a_quiet_source_that_was_stored(feedhome):
    assert feed.add_lines(json.dumps({"tickets": []}), source="jira")["new"] == 0
    assert feed.mark_cycle(REQUIRED)["not_stored"] == ["chat", "mail"], "an empty search reply proves the source was pulled and stored"


def test_json_lines_still_work_and_a_bad_raw_reply_names_the_fix(feedhome):
    line = json.dumps({"id": "x1", "at": "2026-10-21T10:00:00Z", "title": "one event", "source": "chat"})
    assert feed.add_lines(line)["new"] == 1
    with pytest.raises(feed.FeedError, match="short lowercase name"):
        feed.add_lines(json.dumps(TICKETS), source="Bad Source")


def test_cli_cycle_prints_sources_only_and_fails_on_not_stored(feedhome, capsys):
    feed.ingest(_chat())
    assert cli.main(["work", "feed", "cycle", "--require", "chat,jira,mail"]) == 1
    out = capsys.readouterr().out
    assert "stored 1 of 3 required source(s)" in out
    assert "not stored  jira:" in out and "not stored  mail:" in out
    feed.ingest(_chat())
    assert cli.main(["work", "feed", "cycle", "--require", "chat", "--json"]) == 0
    assert json.loads(capsys.readouterr().out) == {"required": ["chat"], "stored": ["chat"], "not_stored": []}


def test_cli_add_takes_a_raw_reply_and_a_source(feedhome, tmp_path, capsys):
    f = tmp_path / "jira.json"
    f.write_text(json.dumps(TICKETS))
    assert cli.main(["work", "feed", "add", str(f), "--source", "jira"]) == 0
    out = capsys.readouterr().out
    assert "added 2 event(s), updated 0, 1 skipped" in out
    assert "CANARYTICKET" not in out


@pytest.fixture
def raw_session(feedhome, tmp_path):
    """A transcript whose ticket and mail tools replied with raw search results, one of them empty."""
    rows = [
        {"type": "assistant", "timestamp": "2026-10-21T13:10:00Z", "message": {"content": [{"type": "tool_use", "id": "t1", "name": "mcp__tickets__search"}]}},
        {"type": "user", "timestamp": "2026-10-21T13:10:02Z", "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": json.dumps(TICKETS)}]}]}},
        {"type": "assistant", "timestamp": "2026-10-21T13:11:00Z", "message": {"content": [{"type": "tool_use", "id": "t2", "name": "mcp__mail__search"}]}},
        {"type": "user", "timestamp": "2026-10-21T13:11:02Z", "message": {"content": [{"type": "tool_result", "tool_use_id": "t2", "content": json.dumps({"messages": []})}]}},
    ]
    path = tmp_path / "raw.jsonl"
    path.write_text("\n".join(json.dumps(r) for r in rows))
    config.set_value("feed_tool_sources", "mcp__chat__*=chat, mcp__tickets__*=jira, mcp__mail__*=mail")
    return path


def test_the_transcript_stores_raw_replies_through_the_same_wrapper(raw_session):
    r = transcript.store(raw_session)
    assert (r["new"], r["skipped"]) == (2, 1)
    assert "jira:PROJ-7:2026-10-21T12:00:00Z:In Progress" in {e["id"] for e in feed.load()}
    assert r["gaps"] == ["chat"], "the empty mail reply was pulled and stored (quiet); chat was never called"
    assert feed.mark_cycle()["not_stored"] == ["chat"], "the transcript records what it stored in the cycle"


def test_the_existing_transcript_fixture_keeps_its_ids(feedhome, tmp_path):
    root = tmp_path / "projects" / "p"
    shutil.copytree(FIX / "feed-cycle", root / "feed-cycle")
    path = root / "feed-cycle.jsonl"
    path.write_text((FIX / "feed-cycle.jsonl").read_text().replace("@DIR@", str(root)))
    config.set_value("feed_tool_sources", "mcp__chat__*=chat, mcp__tickets__*=tickets, mcp__mail__*=mail")
    r = transcript.store(path)
    assert {e["id"] for e in feed.load()} == {"chat:C1-1001", "chat:C1-1002", "tickets:PROJ-7", "tickets:PROJ-8"}
    assert r["gaps"] == ["mail"]
    assert feed.mark_cycle()["not_stored"] == ["mail"]


def test_a_messages_reply_reads_every_time_alias_the_transcript_read_before(feedhome):
    reply = {"messages": [{"id": f"m{n}", key: "2026-10-21T10:00:00Z", "title": "hi"} for n, key in enumerate(("createdAt", "created", "time", "timestamp", "created_at"))]}
    assert feed.add_lines(json.dumps(reply), source="chat") == {"new": 5, "updated": 0, "pruned": 0, "skipped": 0}
