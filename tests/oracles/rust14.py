"""rust-14 Python oracle for live listening, health, review asks and notifications.

All inputs are invocation-owned synthetic records and recorded HTTP fixtures.
Reports contain typed facts, not message bodies, access tokens or raw replies.
No production route, owner file, real notifier or live account is used.
"""

from __future__ import annotations

import contextlib
import io
import ipaddress
import json
import os
import socket
import sys
from datetime import datetime
from pathlib import Path
from typing import Any

from careerbrew import cli, config, events, goals, notify, paths, plugins, store
from careerbrew.day import inbox
from careerbrew.work import drafts, feed, health, listen, live, priorities, projects, review_ask, team

CASES = (
    "source-roster",
    "live-cycle",
    "acquisition-health",
    "direct-health",
    "listen-drafts",
    "link-review",
    "review-selection",
    "notify-dedup",
    "notify-time",
    "notify-project-quiet",
    "notify-suggestions",
    "cli-adapters",
)
FIX = Path(__file__).resolve().parents[1] / "fixtures"
LIVE = FIX / "live"
SHOP = "https://github.com/example/shop/pull/7"
TOOLS = "https://github.com/example/tools/pull/3"
MARKERS = ("LIVEZEBRA", "LISTENZEBRA", "PROJECTZEBRA", "CANARYNOTIFY", "synthetic-not-reported")


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


def _fields(row: dict[str, Any], keys: tuple[str, ...]) -> dict[str, Any]:
    return {key: row[key] for key in keys if key in row}


def _cycle(rec: dict[str, Any]) -> dict[str, Any]:
    return {
        **_fields(rec, ("n", "at", "status", "not_stored", "degraded", "verdicts", "counts", "actions", "goals", "review_sessions", "standup")),
        "sources": {
            name: _fields(row, ("pulled", "stored", "items", "new", "updated", "skipped", "error", "error_kind", "unchanged", "validated_at", "health", "max_age_minutes", "checked_at", "acquired_at", "age_seconds"))
            for name, row in rec["sources"].items()
        },
        "followups": [_fields(row, ("id", "source", "class", "verdict", "actions", "project")) for row in rec["followups"]],
    }


def _state() -> dict[str, Any]:
    return {
        "running": live._state()["running"],
        "cursors": live._state()["cursors"],
        "cycles": live._state()["cycles"],
        "followed": live._state()["followed"],
        "attached": live._state()["attached"],
    }


def _clean_text() -> dict[str, bool]:
    blobs = []
    for path in (live.state_path(), live.cycles_path(), notify.path(), events.log_path()):
        if path.exists():
            blobs.append(store.read_text(path) if path != events.log_path() else path.read_text())
    joined = "\n".join(blobs)
    return {"any": any(marker in joined for marker in MARKERS)}


def _file_facts(root: Path) -> dict[str, str]:
    import hashlib

    return {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(root.rglob("*")) if path.is_file()}


def _put(path: Path, text: str, at: str | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    stamp = datetime.fromisoformat(at or os.environ["CAREERBREW_NOW"]).timestamp()
    os.utime(path, (stamp, stamp))


def _clock(text: str) -> None:
    os.environ["CAREERBREW_NOW"] = text


class Workspace:
    def __init__(self, base: Path, name: str, *, notifier: bool = False):
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
                "CAREERBREW_NOW": "2026-10-25T16:00:00+00:00",
                "CAREERBREW_TEST_OWNER": "1",
                "CAREERBREW_NOTIFY": "off",
                "CAREERBREW_HTTP_FIXTURES": str(FIX / "http"),
                "CB_T": "synthetic-not-reported",
                "TZ": "UTC",
            }
        )
        os.environ.pop("FAKE_NOTIFIER_CLICK", None)
        os.environ.pop("FAKE_NOTIFIER_LOG", None)
        if notifier:
            binary = self.base / "bin"
            binary.mkdir()
            (binary / "alerter").symlink_to(FIX / "notifier" / "fake-notifier.sh")
            os.environ["PATH"] = f"{binary}:/usr/bin:/bin"
            os.environ["FAKE_NOTIFIER_LOG"] = str(self.base / "notifier.log")
            os.environ["CAREERBREW_NOTIFY"] = "on"

    def notifier_calls(self) -> list[list[str]]:
        path = self.base / "notifier.log"
        out: list[list[str]] = []
        for line in path.read_text().splitlines() if path.exists() else []:
            if line.startswith("--- "):
                out.append([line[4:]])
            else:
                out[-1].append(line)
        return out

    def use_notifier(self, **settings: Any) -> None:
        values = {
            "notify_events": "true",
            "notify_backend": "alerter",
            "notify_quiet_hours": "none",
            "notify_batch_minutes": "0",
            "notify_min_priority": "1",
            **settings,
        }
        for key, value in values.items():
            config.set_value(key, str(value))


def _live_sources(workspace: Workspace, listen_case: bool = False) -> Path:
    source = LIVE / ("listen" if listen_case else "") / "sources.yaml"
    text = source.read_text().replace("@DIR@", str(workspace.base / "inbox"))
    path = workspace.base / "sources.yaml"
    path.write_text(text)
    return path


def _fill_live(workspace: Workspace, listen_case: bool = False) -> None:
    folder = LIVE / ("listen" if listen_case else "")
    for path in folder.glob("*.json"):
        _put(workspace.base / "inbox" / path.name, path.read_text())


def _source_roster(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "source-roster")
    plugin = workspace.base / "plug"
    (plugin / "live").mkdir(parents=True)
    (plugin / "careerbrew-plugin.yaml").write_text("name: acme-live\nversion: 0.1.0\nlive: {sources: live/sources.yaml, team: live/team.yaml}\n")
    (plugin / "live" / "sources.yaml").write_text(
        f"sources:\n  - {{name: paging, kind: paging, required: true, adapter: file, inbox: {workspace.base}/inbox/paging.json}}\n  - {{name: chat, kind: chat, adapter: file, inbox: /nowhere.json}}\n"
    )
    (plugin / "live" / "team.yaml").write_text("team:\n  - {name: Sam Lee, chat: U0SAM}\n")
    plugins.add(str(plugin))
    live.save_sources(json.dumps({"sources": [{"name": "chat", "kind": "chat", "required": True, "inbox": str(workspace.base / "inbox/chat.json")}]}))
    bad = []
    for source in (
        {"name": "Chat Room", "kind": "chat", "inbox": "x"},
        {"name": "chat", "kind": "pager", "inbox": "x"},
        {"name": "chat", "kind": "chat"},
        {"name": "chat", "kind": "chat", "inbox": "x", "host": "y"},
        {"name": "chat", "kind": "chat", "inbox": "x", "max_age_minutes": None},
        {"name": "chat", "kind": "chat", "inbox": "x", "max_age_minutes": True},
    ):
        bad.append(_error(lambda source=source: live.check_source(source, "synthetic")))
    rows = [{**_fields(row, ("name", "kind", "required", "adapter", "origin")), "since": ""} for row in live.sources()]
    return {"sources": rows, "team": team.roster(), "own_roster": team.load(), "bad": bad}


def _live_cycle(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "live-cycle")
    live.start(_live_sources(workspace).read_text())
    _fill_live(workspace)
    store.write_yaml(feed.rules_path(), {"rules": [{"name": "shipped", "when": {"source": "tickets", "actor": "me"}, "evidence": True}]})
    first = live.cycle()
    before = _state()
    second = live.cycle()
    after = _state()
    stopped = live.stop()
    stopped_error = _error(live.cycle)
    restarted = live.start()
    lock_error = "not-tested"
    with live.cycle_lock():
        lock_error = _error(live.cycle)
    return {
        "first": _cycle(first),
        "second": _cycle(second),
        "unchanged_state": {"seen": before["cursors"] == after["cursors"], "followed": before["followed"] == after["followed"]},
        "stopped": stopped,
        "stopped_error": stopped_error,
        "restarted": restarted,
        "lock_error": lock_error,
        "clean": _clean_text(),
    }


def _healthy_workspace(base: Path, name: str) -> Workspace:
    workspace = Workspace(base, name)
    path = workspace.base / "inbox.json"
    _put(path, '{"tickets": []}', "2026-11-05T12:00:00+00:00")
    _clock("2026-11-05T12:00:00+00:00")
    live.start(json.dumps({"sources": [{"name": "tickets", "kind": "tickets", "required": True, "inbox": str(path)}]}))
    return workspace


def _acquisition_health(base: Path) -> dict[str, Any]:
    workspace = _healthy_workspace(base, "acquisition-health")
    path = workspace.base / "inbox.json"
    first = live.cycle()
    unchanged = live.cycle()
    _clock("2026-11-05T12:15:00+00:00")
    inclusive = live.cycle()
    _clock("2026-11-05T12:15:01+00:00")
    expired = live.cycle()
    _clock("2026-11-05T12:16:00+00:00")
    older = live.cycle()
    stamp = path.stat().st_mtime_ns
    path.write_text("not JSON")
    os.utime(path, ns=(stamp, stamp))
    malformed = live.cycle()

    future = _healthy_workspace(base, "future-clock")
    _put(future.base / "inbox.json", '{"tickets": []}', "2026-11-05T12:01:00+00:00")
    future_first = live.cycle()
    future_second = live.cycle()

    _healthy_workspace(base, "backwards-clock")
    _clock("2026-11-05T12:16:00+00:00")
    live.cycle()
    _clock("2026-11-05T12:14:00+00:00")
    reversed_row = live.cycle()

    optional = _healthy_workspace(base, "optional-gap")
    live.save_sources(json.dumps({"sources": [{"name": "tickets", "kind": "tickets", "required": True, "inbox": str(optional.base / "inbox.json")}, {"name": "optional", "kind": "chat", "inbox": str(optional.base / "missing.json")}]}))
    optional_row = live.cycle()
    return {
        "first": _cycle(first),
        "unchanged": _cycle(unchanged),
        "inclusive": _cycle(inclusive),
        "expired": _cycle(expired),
        "older": _cycle(older),
        "malformed": _cycle(malformed),
        "future_first": _cycle(future_first),
        "future_second": _cycle(future_second),
        "reversed": _cycle(reversed_row),
        "optional": _cycle(optional_row),
        "invalid_limit": _error(lambda: live.save_sources(json.dumps({"sources": [{"name": "bad", "kind": "chat", "inbox": "synthetic", "max_age_minutes": 1.5}]}))),
    }


def _direct_health(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "direct-health")
    _clock("2026-11-05T12:00:00+00:00")
    config.set_value("feed_tool_sources", "mcp__tickets__*=tickets,mcp__chat__*=chat")
    feed.ingest([], sources=["tickets", "chat"])
    completed = feed.mark_cycle()
    healthy = health.snapshot()
    before = _file_facts(workspace.root)
    _clock("2026-11-05T12:15:00+00:00")
    inclusive = health.snapshot()
    _clock("2026-11-05T12:15:01+00:00")
    stale = health.snapshot()
    stale_again = health.snapshot()
    after = _file_facts(workspace.root)
    config.set_value("feed_tool_sources", "mcp__other__*=tickets,mcp__chat__*=chat")
    changed = health.snapshot()
    config.set_value("feed_tool_sources", "mcp__tickets__*=tickets,mcp__chat__*=chat")
    feed.ingest([], sources=["tickets"])
    feed.mark_cycle()
    failed = health.snapshot()
    exact = health.result_health({"feed_source": "chat"}, healthy)
    unknown = health.result_health({"id": "chat:ambiguous"}, healthy)
    invalid_workspace = Workspace(base, "invalid-health")
    store.write_yaml(invalid_workspace.root / "work/live-sources.yaml", {"sources": [{"name": "invalid-canary", "inbox": "/synthetic/private/input"}]})
    invalid = health.snapshot()
    return {
        "completed": completed,
        "healthy": healthy,
        "inclusive": inclusive,
        "stale": stale,
        "stale_again": stale_again,
        "read_only": before == after,
        "changed": changed,
        "failed": failed,
        "exact": exact,
        "unknown": unknown,
        "invalid": invalid,
        "invalid_has_path": "/synthetic/private/input" in json.dumps(invalid),
    }


def _goals() -> None:
    store.write_yaml(
        goals._path(),
        {
            "long_term": [{"id": "reviewer", "text": "Grow as a code reviewer", "by": None}],
            "queue": [{"id": "api-rollout", "kind": "custom", "title": "Ship the API rollout", "date": "2026-11-20", "topics": ["rollout"], "minutes_per_day": 30}],
            "history": [],
        },
    )


def _listen_drafts(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "listen-drafts")
    live.start(_live_sources(workspace, True).read_text())
    _fill_live(workspace, True)
    projects.add("shop-api")
    projects.add_source("shop-api", "repo", "example-shop/api")
    _goals()
    first = live.cycle()
    holding = drafts.LocalBackend().read("chat:a1")
    review_id = listen.review_target(next(row for row in feed.load() if row["id"] == "chat:1792936860.000200"))
    notes = drafts.LocalBackend().read(review_id) or ""
    updates = projects.records()["shop-api"].get("updates") or []
    sessions = listen.review_sessions()
    second = live.cycle()
    goal_lines = [listen.goal_line(row, False) for row in first["goals"]]
    kept_workspace = Workspace(base, "existing-draft")
    drafts.replace("chat:a1", "my own words, written earlier")
    live.start(_live_sources(kept_workspace, True).read_text())
    _fill_live(kept_workspace, True)
    kept = live.cycle()
    return {
        "first": _cycle(first),
        "holding": holding == listen.HOLDING,
        "review_notes": {"link": "https://github.com/example-shop/api/pull/9" in notes, "draft": "not posted" in notes, "verdict": "Verdict:" in notes},
        "updates": updates,
        "sessions": sessions,
        "second": _cycle(second),
        "goal_lines": goal_lines,
        "kept": _cycle(kept),
        "kept_text": drafts.LocalBackend().read("chat:a1"),
        "clean": _clean_text(),
    }


def _link_review(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "link-review")
    live.start(_live_sources(workspace, True).read_text())
    _fill_live(workspace, True)
    config.set_value("live_open_links", "true")
    real_addresses = listen._addresses
    listen._addresses = lambda host: [info[4][0] for info in socket.getaddrinfo("localhost" if host == "private.example" else "cloudflare.com", None)]
    try:
        rec = live.cycle(read_allowed=True)
    finally:
        listen._addresses = real_addresses
    listen._addresses = lambda host: [info[4][0] for info in socket.getaddrinfo("localhost" if host == "private.example" else "cloudflare.com", None)]
    rows = {row["id"]: row for row in feed.load()}
    research_rows = {key: rows[key].get("research") for key in ("chat:1792936800.000100", "chat:1792936860.000200", "chat:1792936920.000300")}
    private = []
    for url in ("http://localhost:8080/x", "http://private.example/y", "http://localhost/z", "https://[draft"):
        try:
            private.append({"url": url, "reason": listen.private_reason(url)})
        except Exception as exc:
            private.append({"url": url, "error": type(exc).__name__})
    projects.add("ops")
    projects.add_source("ops", "url", "https://status.example.com/budget")
    projects.set_reference("ops", "https://status.example.com/budget")
    unresearched = {**rows["chat:1792936800.000100"]}
    unresearched.pop("research", None)
    reference = listen.research_links(unresearched, True)
    ping = {"id": "chat:ping", "source": "chat", "kind": "mention", "at": "2026-10-25T16:01:00Z", "title": "re-review", "actor": "other", "to_me": True, "url": "https://github.com/example-shop/api/pull/9"}
    feed.ingest([ping])
    verdict = live.verdict(feed.load()[-1], {row["id"] for row in feed.open_asks()})
    follow, _attached = listen.follow_up({ping["id"]: verdict}, set(), False)
    partial = {"id": "chat:partial", "source": "chat", "kind": "dm", "at": "2026-10-25T16:02:00Z", "title": "partial research", "actor": "other", "to_me": True, "text": "see https://public.example/first"}
    feed.ingest([partial])
    feed.research("chat:partial", [{"url": "https://public.example/first", "summary": "agent original"}])
    feed.ingest([{**partial, "text": "see https://public.example/first and https://public.example/second"}])
    feed.cycle_research("chat:partial", [{"url": "https://public.example/second", "opened": False, "reason": "offline"}])
    feed.ingest([{**partial, "text": "see https://public.example/first and https://public.example/second and https://public.example/third"}])
    research_merge = next(row for row in feed.load() if row["id"] == "chat:partial")["research"]
    public_summary = listen._read_url("https://public.example/budget")
    listen._addresses = real_addresses
    range_controls = []
    for value in (0xC0000201, 0x64000001, 0xC6120001, 0xF0000001, 0xCB007101, 0x20010DB8000000000000000000000001, 0x26064700470000000000000000001111):
        url = _range_url(value)
        range_controls.append((url, listen.private_reason(url)))
    return {
        "cycle": _cycle(rec),
        "research": research_rows,
        "private": private,
        "range_controls": range_controls,
        "research_merge": research_merge,
        "public_summary": public_summary,
        "reference": reference,
        "re_review": follow,
        "sessions": listen.review_sessions(),
        "clean": _clean_text(),
    }


def _range_url(value: int) -> str:
    address = ipaddress.ip_address(value)
    return f"http://{address}/x" if address.version == 4 else f"http://[{address}]/x"


def _github() -> None:
    store.write_yaml(paths.sub("work", "connectors.yaml"), {"connectors": [{"kind": "github", "token": "env:CB_T", "user": "me"}]})


def _review_selection(base: Path) -> dict[str, Any]:
    Workspace(base, "review-selection")
    _github()
    shop = review_ask.build(SHOP)
    tools = review_ask.build(TOOLS)
    extra = review_ask.build(TOOLS, ["@eli-dev", "example/maintainers"])
    checks = [
        review_ask.check("Please review @ana-dev and @example/backend", 2),
        review_ask.check_pr(SHOP, "@author-dev @ana-dev"),
        review_ask.check_pr(SHOP, "@ana-dev and @ben-dev, please"),
        review_ask.check("@Ana-Dev and @ana-dev", 2),
        review_ask.check("@here please review @ana-dev", 2),
    ]
    patterns = []
    for pattern, path in (("*", "a/b.py"), ("/src/api/", "src/api/x/y.py"), ("/src/api/", "lib/src/api/x.py"), ("docs/*", "docs/sub/a.md"), ("**/logs", "a/b/logs/x")):
        patterns.append(bool(review_ask.owners_of(path, review_ask.parse_codeowners(f"{pattern} @someone"))))
    last = review_ask.parse_codeowners("* @all-dev\n/src/ @src-dev @org/team owner@example.com\n")
    mismatch = _error(lambda: review_ask.build("https://evil.example.com/example/shop/pull/7"))
    links = [review_ask.recheck_pr_link(SHOP), _error(lambda: review_ask.recheck_pr_link("http://github.com/example/shop/pull/7"))]
    return {
        "shop": shop,
        "tools": tools,
        "extra": extra,
        "checks": checks,
        "patterns": patterns,
        "last": review_ask.owners_of("src/a.py", last),
        "mismatch": mismatch,
        "links": links,
        "recheck_unknown": review_ask.recheck_pr(SHOP),
    }


def _event(eid: str, kind: str, at: str = "2026-10-21T13:30:00Z", **kw: Any) -> dict[str, Any]:
    return {"id": eid, "at": at, "title": f"title of {eid}", "source": "slack", "kind": kind, "actor": "other", "to_me": True, "thread": f"t-{eid}", **kw}


def _notification_view() -> dict[str, Any]:
    state = notify.load()
    return {
        "items": [
            _fields(
                row,
                (
                    "id",
                    "key",
                    "ref",
                    "refs",
                    "pending_refs",
                    "held_refs",
                    "snoozed_until_by_ref",
                    "kind",
                    "sub",
                    "headline",
                    "why",
                    "next",
                    "priority",
                    "priority_why",
                    "count",
                    "at",
                    "status",
                    "popup",
                    "popup_id",
                    "created_at",
                    "updated_at",
                    "acted_at",
                    "snooze_until",
                    "resolved",
                    "snoozes",
                ),
            )
            for row in state["items"]
        ],
        "popups": state["popups"],
    }


def _notify_dedup(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "notify-dedup", notifier=True)
    _clock("2026-10-21T10:00:00+00:00")
    workspace.use_notifier(notify_batch_minutes="10")
    config.set_value("project_priority", "billing=*invoice*")
    store.write_text(workspace.root / "sensitive-terms.txt", "CANARYNOTIFY\n")
    feed.ingest(
        [
            _event("pagerduty:P1", "page", source="pagerduty", status="triggered"),
            _event("slack:C1:1.0", "dm"),
            _event("github:pr-9", "review-request", source="CANARYNOTIFY"),
            _event("slack:C3:3.0", "mention", title="Mention about the invoice export"),
            _event("slack:C4:4.0", "message", actor="bot"),
            _event("slack:old", "dm", at="2026-10-19T13:30:00Z"),
            _event("slack:personal", "dm", personal=True),
        ]
    )
    priorities = {row["ref"]: row["priority"] for row in notify.candidates()}
    first = notify.run(wait=True)
    first_view = _notification_view()
    repeated = notify.run(wait=True)
    _clock("2026-10-21T10:11:00+00:00")
    feed.ingest([_event("slack:C1:2.0", "dm", thread="t-slack:C1:1.0", at="2026-10-21T14:10:00Z")])
    merged = notify.run(wait=True)
    feed.ingest([_event("slack:C1:reply", "message", actor="me", to_me=False, thread="t-slack:C1:1.0", at="2026-10-21T14:11:00Z")])
    closed = notify.run(wait=True)
    _clock("2026-10-21T10:30:00+00:00")
    feed.ingest([_event("slack:C1:new", "dm", thread="t-slack:C1:1.0", at="2026-10-21T14:20:00Z")])
    reopened = notify.run(wait=True)
    return {
        "priorities": priorities,
        "first": first,
        "first_view": first_view,
        "first_calls": workspace.notifier_calls(),
        "repeated": repeated,
        "merged": merged,
        "merged_view": _notification_view(),
        "closed": closed,
        "closed_view": _notification_view(),
        "reopened": reopened,
        "reopened_view": _notification_view(),
        "calls": workspace.notifier_calls(),
        "notifier_clean": "CANARYNOTIFY" not in (workspace.base / "notifier.log").read_text(),
        "clean": _clean_text(),
        "sealed": notify.path().read_bytes().startswith(b"CBV1\n"),
    }


def _notify_time(base: Path) -> dict[str, Any]:
    quiet = Workspace(base, "quiet-hours", notifier=True)
    quiet.use_notifier(notify_quiet_hours="19:00-08:00")
    _clock("2026-10-21T22:00:00+00:00")
    feed.ingest([_event("slack:C1:1.0", "dm", at="2026-10-22T01:30:00Z"), _event("pagerduty:P1", "page", source="pagerduty", status="triggered", at="2026-10-22T01:45:00Z")])
    held = notify.run(wait=True)
    held_view = _notification_view()
    _clock("2026-10-22T08:30:00+00:00")
    released = notify.run(wait=True)
    away_calls = quiet.notifier_calls()

    cap = Workspace(base, "daily-cap", notifier=True)
    cap.use_notifier(notify_daily_cap="1")
    _clock("2026-10-21T10:00:00+00:00")
    feed.ingest([_event("slack:C1:1.0", "dm")])
    notify.run(wait=True)
    _clock("2026-10-21T11:00:00+00:00")
    feed.ingest([_event("slack:C2:2.0", "dm", at="2026-10-21T14:50:00Z")])
    capped = notify.run(wait=True)
    _clock("2026-10-22T10:00:00+00:00")
    feed.ingest([_event("slack:C3:3.0", "dm", at="2026-10-22T13:50:00Z")])
    next_day = notify.run(wait=True)

    snooze = Workspace(base, "snooze", notifier=True)
    snooze.use_notifier(notify_snooze_minutes="60")
    _clock("2026-10-21T10:00:00+00:00")
    feed.ingest([_event("slack:C1:1.0", "dm")])
    os.environ["FAKE_NOTIFIER_CLICK"] = "Snooze"
    notify.run(wait=True)
    os.environ.pop("FAKE_NOTIFIER_CLICK")
    snoozed = _notification_view()
    _clock("2026-10-21T10:30:00+00:00")
    early = notify.run(wait=True)
    _clock("2026-10-21T11:01:00+00:00")
    due = notify.run(wait=True)
    return {
        "held": held,
        "held_view": held_view,
        "released": released,
        "away_calls": away_calls,
        "capped": capped,
        "next_day": next_day,
        "cap_calls": cap.notifier_calls(),
        "cap_view": _notification_view(),
        "snoozed": snoozed,
        "early": early,
        "due": due,
        "snooze_calls": snooze.notifier_calls(),
        "snooze_view": _notification_view(),
        "quiet_boundaries": [notify.in_quiet(datetime.fromisoformat(f"2026-10-21T{stamp}"), "19:00-08:00") for stamp in ("07:59", "08:00", "18:59", "22:00")],
    }


def _notify_project_quiet(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "notify-project-quiet", notifier=True)
    _clock("2026-10-21T10:00:00+00:00")
    workspace.use_notifier(notify_snooze_minutes="15")
    projects.add("billing")
    projects.add("ledger")
    config.set_value("project_priority", "billing=*invoice*, ledger=*ledger*")
    projects.edit("billing", quiet_until="2026-10-24")
    feed.ingest([_event("slack:C1:1.0", "dm", title="invoice export", thread="shared", at="2026-10-21T09:30:00Z"), _event("slack:C1:2.0", "dm", title="unrelated B", thread="shared", at="2026-10-21T09:31:00Z")])
    candidates = [(row["ref"], row["project_quiet"]) for row in notify.candidates()]
    os.environ["FAKE_NOTIFIER_CLICK"] = "Snooze"
    notify.run(wait=True)
    os.environ.pop("FAKE_NOTIFIER_CLICK")
    projects.edit("ledger", quiet_until="2026-10-23")
    _clock("2026-10-21T10:05:00+00:00")
    feed.ingest([_event("slack:C1:3.0", "dm", title="unrelated C", thread="shared", at="2026-10-21T10:04:00Z")])
    notify.run(wait=True)
    _clock("2026-10-21T10:16:00+00:00")
    notify.run(wait=True)
    second_refs = [ref for refs in notify.load()["popups"][-1]["refs"].values() for ref in refs]
    _clock("2026-10-21T10:21:00+00:00")
    notify.run(wait=True)
    third_refs = [ref for refs in notify.load()["popups"][-1]["refs"].values() for ref in refs]
    _clock("2026-10-23T09:00:00+00:00")
    notify.run(wait=True)
    fourth_refs = [ref for refs in notify.load()["popups"][-1]["refs"].values() for ref in refs]
    _clock("2026-10-24T09:00:00+00:00")
    notify.run(wait=True)
    notify.run(wait=True)
    final_refs = [ref for refs in notify.load()["popups"][-1]["refs"].values() for ref in refs]

    reserved = Workspace(base, "reserved-quiet", notifier=True)
    _clock("2026-10-21T10:00:00+00:00")
    reserved.use_notifier()
    projects.add("billing")
    config.set_value("project_priority", "billing=*invoice*")
    feed.ingest([_event("slack:C1:1.0", "dm", title="invoice export")])
    _stats, popup = notify.process()
    projects.edit("billing", quiet_until="2026-10-23")
    presented = notify.present(popup["id"])
    held = _notification_view()
    _clock("2026-10-23T09:00:00+00:00")
    returned = notify.run(wait=True)
    return {
        "candidates": candidates,
        "second_refs": second_refs,
        "third_refs": third_refs,
        "fourth_refs": fourth_refs,
        "final_refs": final_refs,
        "calls": workspace.notifier_calls(),
        "view": _notification_view(),
        "presented": presented,
        "held": held,
        "reserved_calls": reserved.notifier_calls(),
        "returned": returned,
        "returned_view": _notification_view(),
        "returned_calls": reserved.notifier_calls(),
    }


def _agent_task(title: str) -> dict[str, Any]:
    task = priorities.add(title, 3, 3, 1.0, [], source="agent", why="the release is Thursday")
    inbox.offer_task(task)
    return task


def _seed_suggestion(kind: str, data: dict[str, Any]) -> str:
    item_id = f"{kind}-kind"
    item = {"id": item_id, "type": "proposal", "kind": kind, "status": "open", "data": data}
    store.write_json(paths.sub("day", "inbox.json"), {"items": [item]})
    notification = {
        "id": f"n-{kind}",
        "key": f"inbox:{item_id}",
        "item": item_id,
        "refs": [item_id],
        "count": 1,
        "kind": "suggestion",
        "sub": kind,
        "source": "careerbrew",
        "headline": "careerbrew: a suggestion",
        "why": "a decision waits",
        "next": "accept or dismiss it",
        "priority": 3,
        "priority_why": "3 for a suggestion",
        "at": "2026-10-21T10:00:00+00:00",
        "status": "new",
        "popup": "sent",
        "created_at": "2026-10-21T10:00:00+00:00",
        "updated_at": "2026-10-21T10:00:00+00:00",
        "acted_at": "",
        "snooze_until": "",
        "resolved": "",
    }
    store.write_json(paths.sub("day", "notifications.json"), {"items": [notification], "popups": []})
    return f"n-{kind}"


def _suggestion_kinds(base: Path) -> dict[str, Any]:
    out = {}
    for kind in ("task-done", "new-task", "source", "feed-rule", "plugin", "goal", "goal-cli"):
        Workspace(base, f"kind-{kind}")
        data = {
            "task-done": {"task": "t001"},
            "new-task": {"title": "Ticket work"},
            "source": {"project": "billing", "kind": "repo", "ref": "example/added"},
            "feed-rule": {"rule": {"name": "quiet", "when": {"source": "chat"}, "triage": "fyi"}},
            "plugin": {"plugin": "acme", "task": "Document the rollout", "link": ""},
            "goal": {"id": "goal-test", "title": "Grow", "kind": "custom", "date": "2026-12-01"},
            "goal-cli": {"id": "goal-test", "title": "Grow", "kind": "custom", "date": "2026-12-01"},
        }[kind]
        if kind == "task-done":
            priorities.add("Finish me", 3, 3, 1.0, [])
        if kind == "source":
            projects.add("billing")
        nid = _seed_suggestion("goal" if kind.startswith("goal") else kind, data)
        if kind == "goal-cli":
            accepted = _error(lambda: notify.act(nid, "accept", via="cli"))
        else:
            accepted = notify.act(nid, "accept", via="ui" if kind == "goal" else "cli")
        out[kind] = {
            "accepted": accepted,
            "tasks": priorities.load(),
            "projects": projects.records(),
            "goals": goals.load(),
            "rules": feed.rules(),
        }
    return out


def _notify_suggestions(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "notify-suggestions", notifier=True)
    _clock("2026-10-21T10:00:00+00:00")
    workspace.use_notifier()
    suggestion = _agent_task("Review the rollout checklist")
    first = notify.run(wait=True)
    first_view = _notification_view()
    first_item = notify.load()["items"][0]
    inbox.dismiss(first_item["item"])
    late = notify._apply_click(notify.load()["popups"][-1], "Accept", "alerter")
    late_view = _notification_view()
    _clock("2026-10-21T10:30:00+00:00")
    _agent_task("Update the runbook")
    second = notify.run(wait=True)
    nid = next(row["id"] for row in notify.load()["items"] if row["status"] == "new" and row["kind"] == "suggestion")
    accepted = notify.act(nid, "accept")
    accepted_view = _notification_view()
    inbox_state = [(row["id"], row["status"]) for row in inbox.load()]
    repeated_offer = inbox.offer_task(suggestion)
    kinds = _suggestion_kinds(base)
    return {
        "suggestion": suggestion,
        "first": first,
        "first_view": first_view,
        "first_calls": workspace.notifier_calls(),
        "late": late,
        "late_view": late_view,
        "second": second,
        "accepted": accepted,
        "accepted_view": accepted_view,
        "inbox": inbox_state,
        "repeated_offer": repeated_offer,
        "kinds": kinds,
        "calls": workspace.notifier_calls(),
        "clean": _clean_text(),
        "sealed": notify.path().read_bytes().startswith(b"CBV1\n"),
    }


def _cli_adapters(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "cli-adapters", notifier=True)
    started = _invoke(["work", "live", "start", "--sources", str(_live_sources(workspace))])
    _fill_live(workspace)
    cycled = _invoke(["work", "live", "cycle"])
    status = _invoke(["work", "live", "status", "--json"])
    stopped = _invoke(["work", "live", "stop"])
    _github()
    review = _invoke(["work", "review-ask", SHOP])
    short = _invoke(["work", "review-ask", TOOLS])
    workspace.use_notifier()
    _clock("2026-10-21T10:00:00+00:00")
    feed.ingest([_event("slack:C1:1.0", "dm")])
    checked = _invoke(["notify", "check", "--wait"])
    listed = _invoke(["notify", "list"])
    notify_status = _invoke(["notify", "status"])
    return {"started": started, "cycled": cycled, "status": status, "stopped": stopped, "review": review, "short": short, "checked": checked, "listed": listed, "notify_status": notify_status, "clean": _clean_text()}


def run_all(selected: str | None = None) -> dict[str, Any]:
    if trace := os.environ.get("CAREERBREW_RUST14_WRITE_TRACE"):
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
    base = Path(os.environ["CAREERBREW_HOME"]) / "rust14-workload"
    base.mkdir(parents=True, exist_ok=True)
    functions = {
        "source-roster": _source_roster,
        "live-cycle": _live_cycle,
        "acquisition-health": _acquisition_health,
        "direct-health": _direct_health,
        "listen-drafts": _listen_drafts,
        "link-review": _link_review,
        "review-selection": _review_selection,
        "notify-dedup": _notify_dedup,
        "notify-time": _notify_time,
        "notify-project-quiet": _notify_project_quiet,
        "notify-suggestions": _notify_suggestions,
        "cli-adapters": _cli_adapters,
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
