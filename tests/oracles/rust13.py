"""rust-13 Python oracle for feed ingestion and transcript normalization.

Every case uses invocation-owned synthetic data and fixed recorded fixtures.
Reports contain typed metadata and booleans, never reply bodies or secrets.
No production route, owner file, or live account is used.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import shutil
import sys
from datetime import datetime
from pathlib import Path
from typing import Any

from careerbrew import cli, config, connectors, events, paths, store
from careerbrew.work import evidence, feed, projects, team, transcript

CASES = (
    "source-ingestion",
    "edited-upsert",
    "raw-replies",
    "claude-transcript",
    "codex-transcript",
    "containment-since",
    "asks-cycle",
    "team-feed",
    "local-time-and-nan",
    "source-failures",
    "source-inventory",
    "partial-transport",
)
FIX = Path(__file__).resolve().parents[1] / "fixtures"
MAP = "mcp__chat__*=chat, mcp__tickets__*=tickets, mcp__mail__*=mail"
TOKEN = "env:CB_TEST_TOKEN"


def _typed(value: Any) -> Any:
    if value is None or isinstance(value, bool | str):
        return value
    if isinstance(value, int):
        return {"int": str(value)}
    if isinstance(value, float):
        return {"float": repr(value)}
    if isinstance(value, list | tuple):
        return [_typed(item) for item in value]
    if isinstance(value, set):
        return _typed(sorted(value))
    if isinstance(value, dict):
        return {str(key): _typed(value[key]) for key in sorted(value, key=str)}
    if hasattr(value, "isoformat"):
        return value.isoformat()
    return {"type": type(value).__name__}


def _call(cases: list[dict[str, Any]], name: str, fn: Any) -> None:
    try:
        value = fn()
    except Exception as exc:
        value = {"error": type(exc).__name__, "message": str(exc)}
    cases.append({"id": name, "result": _typed(value)})


def _error(fn: Any) -> str:
    try:
        fn()
    except Exception as exc:
        return type(exc).__name__
    return "accepted"


def _invoke(args: list[str]) -> dict[str, Any]:
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        rc = cli.main(args)
    return {"exit": rc, "stdout": output.getvalue()}


class Workspace:
    def __init__(self, base: Path, name: str):
        self.base = base / name
        self.root = self.base / "data"
        self.home = self.base / "home"
        self.root.mkdir(parents=True)
        self.home.mkdir(parents=True)
        os.environ.update(
            {
                "HOME": str(self.home),
                "CAREERBREW_HOME": str(self.root),
                "CAREERBREW_KEY_FILE": str(self.base / "keys" / "backup.key"),
                "CAREERBREW_EVENTS": "off",
                "CAREERBREW_ENCRYPT": "on",
                "CAREERBREW_NOW": "2026-10-21T16:00:00+00:00",
                "CAREERBREW_TEST_OWNER": "1",
                "CAREERBREW_HTTP_FIXTURES": str(FIX / "http" / "feed"),
                "CB_TEST_TOKEN": "synthetic-not-reported",
                "CB_T": "synthetic",
                "TZ": "UTC",
            }
        )


def _connectors(kinds: list[str]) -> None:
    store.write_json(paths.sub("work", "connectors.yaml"), {"connectors": [{"kind": kind, "token": TOKEN} for kind in kinds]})


def _row(row: dict[str, Any]) -> dict[str, Any]:
    return {
        key: row.get(key)
        for key in (
            "id",
            "source",
            "kind",
            "at",
            "actor",
            "actor_id",
            "to_me",
            "channel",
            "from",
            "status",
            "importance",
            "thread",
            "triage",
            "rule",
            "personal",
            "personal_by",
            "team",
            "links",
            "signal",
            "evidence",
            "evidence_rule",
            "ask_mark",
        )
        if key in row
    }


def _source_ingestion(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "source-ingestion")
    _connectors(["slack", "pagerduty", "outlook"])
    first = feed.pull()
    repeated = feed.pull()
    rows = {row["id"]: row for row in feed.load()}
    dm = _row(rows["slack:D0ALEX:1792591200.000100"])
    original = store.read_text(feed.originals_path())
    analysis = store.read_text(feed.events_path())
    sealed = feed.originals_path().read_bytes().startswith(b"CBV1\n") and feed.events_path().read_bytes().startswith(b"CBV1\n")
    return {
        "first": first,
        "repeated": repeated,
        "count": len(rows),
        "triages": {key: rows[key]["triage"] for key in sorted(rows)},
        "dm": dm,
        "open": [row["id"] for row in feed.open_asks()],
        "summary": feed.summary(),
        "sealed": sealed,
        "original_kept": "can you review the rollout plan today?" in original,
        "analysis_separated": "can you review the rollout plan today?" not in analysis and "PROJECTZEBRA" not in analysis,
        "lock": (workspace.root / "work" / "feed" / ".lock").is_file(),
    }


def _edited_upsert(base: Path) -> dict[str, Any]:
    Workspace(base, "edited-upsert")
    item = {
        "id": "slack:C1:1",
        "source": "slack",
        "kind": "mention",
        "at": "2026-10-21T12:00:00Z",
        "title": "Review the rollout",
        "actor": "other",
        "to_me": True,
        "channel": "eng",
        "thread": "slack:C1:1",
        "text": "spec <https://docs.example.com/d/1|spec>",
    }
    first = feed.add_lines(json.dumps(item))
    feed.mark_ask(item["id"], "done")
    feed.research(item["id"], [{"url": "https://docs.example.com/d/1", "summary": "approved spec"}])
    edited = {**item, "title": "Review the edited rollout", "text": "edited <https://docs.example.com/d/1|spec>", "original": {"edited": True}}
    second = feed.add_lines(json.dumps(edited))
    row = _row(feed.load()[0])
    original = feed.originals()[item["id"]]
    repeated = feed.add_lines(json.dumps(edited))
    retriaged = feed.retriage()
    return {
        "first": first,
        "second": second,
        "repeated": repeated,
        "retriaged": retriaged,
        "row": row,
        "title_edited": feed.load()[0]["title"] == "Review the edited rollout",
        "research_kept": feed.load()[0].get("research") == [{"url": "https://docs.example.com/d/1", "opened": True, "summary": "approved spec"}],
        "original_edited": original["original"] == {"edited": True},
        "one_row": len(feed.load()) == 1,
        "open": feed.open_asks(),
    }


def _raw_replies(base: Path) -> dict[str, Any]:
    Workspace(base, "raw-replies")
    tickets = {
        "total": 3,
        "tickets": [
            {"key": "PROJ-7", "summary": "Fix the login bug", "description": "Steps CANARYTICKET", "status": "In Progress", "updated": "2026-10-21T12:00:00Z", "url": "https://tickets.example.com/browse/PROJ-7"},
            {"key": "PROJ-8", "fields": {"summary": "Write the API docs", "status": {"name": "To Do"}, "updated": "2026-10-21T12:30:00Z", "project": {"name": "Docs"}}},
            {"key": "PROJ-9", "summary": "no time on this one"},
        ],
    }
    mail = {
        "messages": [
            {"id": "AAMk-1", "subject": "Quarterly plan", "receivedDateTime": "2026-10-21T11:00:00Z", "from": {"emailAddress": {"name": "Sam Lee", "address": "sam@example.com"}}, "importance": "high", "bodyPreview": "CANARYMAIL body"},
            {"id": "AAMk-2", "subject": "", "receivedDateTime": "2026-10-21T11:30:00Z", "flag": {"flagStatus": "flagged"}},
        ]
    }
    calendar = {
        "events": [
            {"id": "abc", "start": {"dateTime": "2026-10-29T15:00:00Z"}, "summary": "Design review", "htmlLink": "https://cal.example.com/abc"},
            {"start": {"dateTime": "2026-10-29T16:00:00Z"}, "summary": "No id"},
            {"id": "nostart", "summary": "No start"},
            {"id": "allday", "start": {"date": "2026-10-30"}, "summary": "Offsite"},
        ]
    }
    added_tickets = feed.add_lines(json.dumps(tickets), source="jira")
    added_mail = feed.add_lines(json.dumps(mail))
    added_calendar = feed.add_lines(json.dumps(calendar), source="calendar")
    quiet = feed.add_lines(json.dumps({"messages": []}), source="chat")
    rows = {row["id"]: _row(row) for row in feed.load()}
    originals = feed.originals()
    not_calendar = feed.raw_events({"events": [{"id": "pg-1", "at": "2026-10-29T15:00:00Z", "title": "Disk full"}]}, "pager")
    return {
        "tickets": added_tickets,
        "mail": added_mail,
        "calendar": added_calendar,
        "quiet": quiet,
        "rows": rows,
        "ticket_original": originals["jira:PROJ-7:2026-10-21T12:00:00Z:In Progress"]["original"]["description"].endswith("CANARYTICKET"),
        "mail_original": originals["mail:AAMk-1"]["text"] == "CANARYMAIL body",
        "analysis_separated": all("CANARY" not in json.dumps(row) for row in feed.load()),
        "not_calendar": not_calendar is None,
        "bad_source": _error(lambda: feed.add_lines(json.dumps(tickets), source="Bad Source")),
    }


def _session(workspace: Workspace, name: str) -> Path:
    root = workspace.base / name
    shutil.copytree(FIX / "transcripts" / "feed-cycle", root / "feed-cycle")
    path = root / "feed-cycle.jsonl"
    path.write_text((FIX / "transcripts" / "feed-cycle.jsonl").read_text().replace("@DIR@", str(root)))
    config.set_value("feed_tool_sources", MAP)
    return path


def _claude_transcript(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "claude-transcript")
    session = _session(workspace, "session")
    first = transcript.store(session)
    repeated = transcript.store(session)
    cli_result = _invoke(["work", "feed", "transcript", str(session)])
    rows = {row["id"]: _row(row) for row in feed.load()}
    original = feed.originals()["tickets:PROJ-7"]
    return {
        "first": first,
        "repeated": repeated,
        "cli": cli_result,
        "rows": rows,
        "original_padded": original["original"]["pad"] == "x" * 2000,
        "sealed": feed.originals_path().read_bytes().startswith(b"CBV1\n"),
        "analysis_separated": "CANARYCHAT" not in store.read_text(feed.events_path()),
    }


def _codex_transcript(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "codex-transcript")
    session = _session(workspace, "source")
    expected = transcript.store(session)
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
    native.write_text("\n".join(json.dumps(row) for row in rows))
    response_items = transcript.store(native)
    quiet = workspace.base / "events.jsonl"
    quiet.write_text(
        "\n".join(
            json.dumps(row)
            for row in [
                {
                    "type": "event_msg",
                    "timestamp": "2026-10-21T15:59:00Z",
                    "payload": {"type": "mcp_tool_call_end", "call_id": "q", "invocation": {"server": "mail", "tool": "search", "arguments": {}}, "result": {"Ok": {"content": [{"type": "text", "text": '{"messages": []}'}]}}},
                },
                {
                    "type": "event_msg",
                    "timestamp": "2026-10-21T15:59:00Z",
                    "payload": {"type": "mcp_tool_call_end", "call_id": "e", "invocation": {"server": "tickets", "tool": "search", "arguments": {}}, "result": {"Err": "CANARY-NATIVE-ERROR"}},
                },
            ]
        )
    )
    end_events = transcript.store(quiet)
    malformed = workspace.base / "malformed.jsonl"
    malformed.write_text('{"type":"future_format","payload":{}}\n')
    return {
        "expected": expected,
        "response_items": response_items,
        "same_facts": response_items["stored"] == expected["stored"] and response_items["replies"] == expected["replies"] and response_items["errors"] == expected["errors"],
        "end_events": end_events,
        "unsupported": _error(lambda: transcript.store(malformed)),
    }


def _containment_since(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "containment-since")
    session = _session(workspace, "session")
    outside = workspace.base / "elsewhere.json"
    outside.write_text(json.dumps([{"id": "X-1", "at": "2026-10-21T12:00:00Z", "title": "outside"}]))
    pointer = str(session.parent / "feed-cycle" / "tool-results" / "big.txt")
    original = session.read_text()
    session.write_text(original.replace(pointer, str(outside)))
    refused = transcript.store(session)
    session.write_text(original)
    since = transcript.store(session, since="2026-10-21T13:13:30Z")
    duration = transcript.store(session, since="30m")
    bad = _error(lambda: transcript.store(session, since="yesterday"))
    return {
        "refused": refused,
        "outside_stored": "tickets:X-1" in feed.originals(),
        "since": since,
        "duration": duration,
        "bad": bad,
    }


def _asks_cycle(base: Path) -> dict[str, Any]:
    Workspace(base, "asks-cycle")
    ask = {"id": "rr-12", "source": "github", "kind": "review-request", "to_me": True, "at": "2026-10-21T13:00:00Z", "title": "Review requested", "url": "https://github.example.com/acme/api/pull/12"}
    first = feed.ingest([ask])
    open_first = [row["id"] for row in feed.open_asks()]
    idle = feed.ingest([ask])
    feed.mark_ask("rr-12", "done")
    marked_closed = feed.open_asks() == []
    feed.mark_ask("rr-12", "ask")
    reply = {"id": "rv-12", "source": "github", "kind": "review", "actor": "me", "at": "2026-10-21T14:00:00Z", "title": "Reviewed", "url": "https://github.example.com/acme/api/pull/12/files#pullrequestreview-9"}
    feed.ingest([reply])
    reply_closed = feed.open_asks() == []
    legacy = {"id": "legacy", "thread": "synthetic-thread", "triage": "act", "at": None}
    valid = {"id": "valid", "thread": "synthetic-thread", "triage": "act", "at": "2026-10-19T15:00:00-04:00"}
    legacy_sibling = [row["id"] for row in feed.open_asks([legacy, valid])]
    cycle = feed.mark_cycle(["github", "jira", "mail"])
    next_cycle = feed.mark_cycle(["github", "jira", "mail"])
    cli_cycle = _invoke(["work", "feed", "cycle", "--require", "github,jira,mail"])
    return {
        "first": first,
        "idle": idle,
        "open_first": open_first,
        "marked_closed": marked_closed,
        "reply_closed": reply_closed,
        "legacy_sibling": legacy_sibling,
        "cycle": cycle,
        "next_cycle": next_cycle,
        "cli_cycle": cli_cycle,
        "lock": (paths.data_root() / "work" / "feed" / ".lock").is_file(),
    }


def _team_feed(base: Path) -> dict[str, Any]:
    Workspace(base, "team-feed")
    os.environ["CAREERBREW_HTTP_FIXTURES"] = str(FIX / "http" / "team-feed")
    _connectors(["slack"])
    team.add("Sam Lee", chat="U0SAM", login="samlee", email="Sam@Example.com")
    pulled = feed.pull()
    rows = {row["id"]: _row(row) for row in feed.load()}
    reply_id = "slack:C0ENG:1792591260.000300"
    feed.research(reply_id, [{"url": "https://tickets.example.com/PROJ-7", "summary": "the flag rollout ticket"}])
    kept = [{"id": row["id"], "projects": row["projects"], "to_research": row["to_research"]} for row in team.feed()]
    cli_list = _invoke(["work", "team", "list"])
    cli_feed = _invoke(["work", "team", "feed"])
    os.environ["CAREERBREW_HTTP_FIXTURES"] = str(FIX / "http" / "team-feed-noscope")
    without_scope = [item["id"] for item in connectors.slack_feed({"kind": "slack", "token": TOKEN})]
    projects.add("export")
    projects.add_source("export", "chat", "#eng")
    projects.add("archived")
    projects.add_source("archived", "chat", "#eng")
    projects.archive("archived")
    linked = {row["id"]: row["projects"] for row in team.feed()}
    linked_cli = _invoke(["work", "team", "feed"])
    return {
        "pulled": pulled,
        "rows": rows,
        "kept": kept,
        "cli_list": cli_list,
        "cli_feed": cli_feed,
        "without_scope": without_scope,
        "linked": linked,
        "linked_cli": linked_cli,
    }


def _local_time_and_nan(base: Path) -> dict[str, Any]:
    Workspace(base, "local-time-and-nan")
    config.set_value("timezone", "America/New_York")
    os.environ["CAREERBREW_NOW"] = "2026-10-21T16:00:00"
    naive = feed.clean({"id": "naive", "at": "2026-10-21T12:00:00", "title": "naive"})["at"]
    aware = feed.clean({"id": "aware", "at": "2026-10-21T12:00:00-04:00", "title": "aware"})["at"]
    row = {"at": "2026-10-19T16:30:00-04:00"}
    minutes = feed.work_minutes(row, datetime.fromisoformat("2026-10-19T17:30:00-04:00"), feed.work_hours())
    raw = []
    for stamp in ("NaN", "inf"):
        events, skipped, source = feed.raw_events({"messages": [{"id": stamp, "title": "bad", "ts": stamp}]}, "chat")
        raw.append({"events": len(events), "skipped": skipped, "source": source})
    transcript_results = []
    for stamp in ("NaN", "inf"):
        try:
            event = transcript._event({"id": stamp, "at": stamp, "title": "bad"}, "chat")
            transcript_results.append("skipped" if event is None else "accepted")
        except Exception as exc:
            transcript_results.append(type(exc).__name__)
    config.set_value("timezone", "")
    os.environ["TZ"] = ":America/New_York"
    colon = feed._now_iso()
    os.environ.pop("TZ")
    system = feed._now_iso()
    config.set_value("timezone", "America/New_York")
    gap = feed._utc("2026-03-08T02:30:00")
    return {
        "now": feed._now_iso(),
        "naive": naive,
        "aware": aware,
        "work_minutes": minutes,
        "raw": raw,
        "transcript": transcript_results,
        "colon": colon,
        "system": system,
        "gap": gap,
    }


def _source_failures(base: Path) -> dict[str, Any]:
    Workspace(base, "source-failures")
    _connectors(["slack", "pagerduty", "outlook"])
    store.write_text(paths.sub("sensitive-terms.txt"), "CANARYSOURCE\n")
    plugin_root = paths.sub("plugins", "feedplug")
    store.write_yaml(plugin_root / "careerbrew-plugin.yaml", {"name": "feedplug", "version": "0.1.0", "sensitive_terms": "sensitive-terms.txt"})
    store.write_text(plugin_root / "sensitive-terms.txt", "CANARYPLUGIN\n")
    store.write_json(paths.sub("plugins.lock.json"), {"plugins": {"feedplug": {"origin": "plugin:feedplug"}}})
    real = connectors.feed

    def fail(_cfg):
        raise RuntimeError("CANARYSOURCE CANARYPLUGIN " + "x" * 250)

    connectors.feed = fail
    try:
        result = feed.pull()
    finally:
        connectors.feed = real
    Workspace(base, "absent-plugin-term")
    _connectors(["slack", "pagerduty", "outlook"])
    store.write_text(paths.sub("sensitive-terms.txt"), "CANARYSOURCE\n")
    plugin_root = paths.sub("plugins", "feedplug")
    store.write_yaml(plugin_root / "careerbrew-plugin.yaml", {"name": "feedplug", "version": "0.1.0", "sensitive_terms": "missing.txt"})
    store.write_json(paths.sub("plugins.lock.json"), {"plugins": {"feedplug": {"origin": "plugin:feedplug"}}})

    def fail_absent(_cfg):
        raise RuntimeError("CANARYSOURCE " + "x" * 250)

    connectors.feed = fail_absent
    try:
        absent = feed.pull()
    finally:
        connectors.feed = real
    return {"result": result, "lengths": [len(reason) for reason in result["reasons"]], "absent": absent}


def _source_inventory(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "source-inventory")
    _connectors(["slack"])
    slack = {"kind": "slack", "token": TOKEN}
    store.write_yaml(paths.sub("work", "sources.yaml"), {"sources": [slack]})
    plugin_root = workspace.root / "plugins" / "feedplug"
    store.write_yaml(plugin_root / "careerbrew-plugin.yaml", {"name": "feedplug", "version": "0.1.0", "provides": {"evidence_sources": "evidence.yaml"}})
    store.write_yaml(plugin_root / "evidence.yaml", {"connectors": [slack]})
    store.write_json(paths.sub("plugins.lock.json"), {"plugins": {"feedplug": {"origin": "plugin:feedplug"}}})
    result = feed.pull()
    inventory = [{"kind": row["kind"], "origin": row["origin"]} for row in evidence.sources_list()]
    origins = {row["id"]: row["origin"] for row in feed.load()}
    return {"result": result, "inventory": inventory, "origins": origins}


def _partial_transport(base: Path) -> dict[str, Any]:
    Workspace(base, "partial-transport")
    os.environ["CAREERBREW_HTTP_FIXTURES"] = str(FIX / "http" / "team-feed")
    os.environ["CAREERBREW_EVENTS"] = "on"
    team.add("Sam Lee", chat="U0SAM")
    # Compare feed diagnostics, not the roster setup event or its receipt.
    (paths.data_root() / "logs" / "events.jsonl").write_text("")
    if trace := os.environ.get("CAREERBREW_RUST13_WRITE_TRACE"):
        Path(trace).write_text("")
    real = connectors.http_json

    def fail_team(url, *args, **kwargs):
        if "search.messages" in url and "from%3A%3C%40U0SAM" in url:
            raise RuntimeError("CANARYTRANSPORT")
        return real(url, *args, **kwargs)

    connectors.http_json = fail_team
    try:
        team_search = [row["id"] for row in connectors.slack_feed({"kind": "slack", "token": TOKEN})]
    finally:
        connectors.http_json = real

    def fail_replies(url, *args, **kwargs):
        if "conversations.replies" in url:
            raise RuntimeError("CANARYTRANSPORT")
        return real(url, *args, **kwargs)

    connectors.http_json = fail_replies
    try:
        replies = [row["id"] for row in connectors.slack_feed({"kind": "slack", "token": TOKEN})]
    finally:
        connectors.http_json = real
    logged = [json.loads(line) for line in (paths.data_root() / "logs" / "events.jsonl").read_text().splitlines()]
    diagnostics = [row["event"] for row in logged if row["event"] in {"work.feed.team_search", "work.feed.replies"}]
    return {"team_search": team_search, "replies": replies, "diagnostics": diagnostics}


def run_all(selected: str | None = None) -> dict[str, Any]:
    trace = os.environ.get("CAREERBREW_RUST13_WRITE_TRACE")
    if trace:
        real_write_text = store.write_text

        def traced_write_text(path: Path, text: str) -> None:
            real_write_text(path, text)
            with Path(trace).open("a", encoding="utf-8") as log:
                log.write(f"{path}\n")

        store.write_text = traced_write_text
        real_event_write_row = events.write_row

        def traced_event_write_row(row: dict[str, Any]) -> None:
            real_event_write_row(row)
            with Path(trace).open("a", encoding="utf-8") as log:
                log.write(f"{events.log_path()}\n")

        events.write_row = traced_event_write_row
    base = Path(os.environ["CAREERBREW_HOME"]) / "rust13-workload"
    base.mkdir(parents=True, exist_ok=True)
    functions = {
        "source-ingestion": _source_ingestion,
        "edited-upsert": _edited_upsert,
        "raw-replies": _raw_replies,
        "claude-transcript": _claude_transcript,
        "codex-transcript": _codex_transcript,
        "containment-since": _containment_since,
        "asks-cycle": _asks_cycle,
        "team-feed": _team_feed,
        "local-time-and-nan": _local_time_and_nan,
        "source-failures": _source_failures,
        "source-inventory": _source_inventory,
        "partial-transport": _partial_transport,
    }
    cases: list[dict[str, Any]] = []
    for name in CASES:
        if selected is None or name == selected:
            _call(cases, name, lambda fn=functions[name]: fn(base))
    return {"text": json.dumps({"cases": cases}, sort_keys=True) + "\n"}


def main() -> int:
    args = sys.argv[1:]
    if args == ["--all"]:
        selected = None
    elif len(args) == 2 and args[0] == "--case" and args[1] in CASES:
        selected = args[1]
    else:
        return 2
    sys.stdout.write(run_all(selected)["text"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
