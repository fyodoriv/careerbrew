"""Agent activity (issue #272): background agents report each step while they work, and the web app shows it live.

One call per step: `careerbrew work agent event <agent> "<what it did>" [--status running|done|failed]
[--project <name>] [--url <link>] [--run <id>]`. Write as you go, never in a batch at the end of a cycle: each
call appends one row to `work/agents/events.jsonl` (encrypted at rest like every data file). The file is under
`work/`, so `/api/changes` moves at once and Work's Agent activity block refetches within one poll (5 s).

A run is one agent's work on one job: `(agent, run)`, where `run` defaults to the agent's name. Its state is its
last event. A run whose last event is `running` and older than `agent_quiet_minutes` shows as `quiet` (the agent
may have died), never as running forever. A `running` event after `done` or `failed` starts the run again.

Privacy: a row holds the agent's name, a run id, a status, a one-line title the agent wrote, an http(s) link,
and a project name. It is business data: the CLI prints titles and links only where business data may show
(`may_show_business`), and the event log gets the status only.

Lifecycle: files under the data root only; nothing is kept in process memory between calls. `_locked()`
serializes read-modify-write between processes with an fcntl lock on `work/agents/.lock`, which the OS
releases when the process exits. Rows older than `agent_keep_days` are dropped at the next write and never shown.
"""

from __future__ import annotations

import contextlib
import fcntl
import re
from collections.abc import Iterator
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit

from .. import calendar, config, events, paths, store

STORY = "US-91"
STATUSES = ("running", "done", "failed")
TITLE_MAX = 200
PROJECT_MAX = 80
NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,59}$")


class AgentError(ValueError):
    """A refused event: the message names the fix."""


def _dir() -> Path:
    d = paths.sub("work", "agents")
    d.mkdir(parents=True, exist_ok=True)
    return d


def events_path() -> Path:
    return _dir() / "events.jsonl"


@contextlib.contextmanager
def _locked() -> Iterator[None]:
    fh = (_dir() / ".lock").open("a+")
    try:
        fcntl.flock(fh, fcntl.LOCK_EX)
        yield
    finally:
        fcntl.flock(fh, fcntl.LOCK_UN)
        fh.close()


def load() -> list[dict[str, Any]]:
    path = paths.sub("work", "agents", "events.jsonl")
    return store.read_jsonl(path) if path.exists() else []


def _line(value: str) -> str:
    return " ".join(str(value or "").split())


def _check(agent: str, title: str, status: str, url: str, project: str, run: str) -> dict[str, str]:
    agent, run, title, project, url = agent.strip(), (run or agent).strip(), _line(title), _line(project), url.strip()
    for label, value in (("agent", agent), ("run", run)):
        if not NAME.match(value):
            raise AgentError(f"bad {label} name {value[:60]!r}: use 1 to 60 letters, digits, and _ . : / - (for example review-bot)")
    if status not in STATUSES:
        raise AgentError(f"bad status {status!r}: use one of {', '.join(STATUSES)}")
    if not title:
        raise AgentError("the title is empty: say in one line what the agent did or does now")
    if len(title) > TITLE_MAX:
        raise AgentError(f"the title has {len(title)} characters; the limit is {TITLE_MAX}: keep it to one short line")
    if len(project) > PROJECT_MAX:
        raise AgentError(f"the project name has {len(project)} characters; the limit is {PROJECT_MAX}")
    if url:
        parts = urlsplit(url)
        if parts.scheme not in ("http", "https") or not parts.netloc:
            raise AgentError("the link must be a full http(s) URL, for example https://github.example.com/org/repo/pull/7")
    return {"agent": agent, "run": run, "status": status, "title": title, "url": url, "project": project}


def _cutoff(now: datetime) -> str:
    return (now - timedelta(days=int(config.get("agent_keep_days")))).isoformat(timespec="seconds")


def record(agent: str, title: str, status: str = "running", url: str = "", project: str = "", run: str = "") -> dict[str, Any]:
    """Append one event; drop rows older than `agent_keep_days`. Raises AgentError on bad input and writes nothing."""
    row = _check(agent, title, status, url, project, run)
    now = calendar.now_utc()
    row = {"at": now.isoformat(timespec="seconds"), **row}
    with _locked():
        cutoff = _cutoff(now)
        kept = [r for r in load() if str(r.get("at", "")) >= cutoff]
        store.write_jsonl(events_path(), [*kept, row])
    events.emit("work.agent.event", status=status, story=STORY.lower())
    return row


def runs(now: datetime | None = None) -> list[dict[str, Any]]:
    """Each run's state from its last event, newest first: agent, run, project, status (running, quiet, done,
    failed), title, url, at (last event), started_at (first event of this run since it last finished), events."""
    now = now or calendar.now_utc()
    cutoff, quiet = _cutoff(now), (now - timedelta(minutes=int(config.get("agent_quiet_minutes")))).isoformat(timespec="seconds")
    out: dict[tuple[str, str], dict[str, Any]] = {}
    for r in sorted(load(), key=lambda r: str(r.get("at", ""))):
        at = str(r.get("at", ""))
        if at < cutoff:
            continue
        key = (str(r.get("agent", "")), str(r.get("run", "")))
        prev = out.get(key)
        restart = prev is None or (prev["status"] in ("done", "failed") and r.get("status", "running") == "running")
        out[key] = {
            "agent": key[0],
            "run": key[1],
            "project": str(r.get("project") or (prev["project"] if prev and not restart else "")),
            "status": str(r.get("status", "running")),
            "title": str(r.get("title", "")),
            "url": str(r.get("url") or ""),
            "at": at,
            "started_at": at if restart else prev["started_at"],  # type: ignore[index]
            "events": 1 if restart else prev["events"] + 1,  # type: ignore[index]
        }
    rows = sorted(out.values(), key=lambda r: (r["at"], r["agent"], r["run"]), reverse=True)
    for r in rows:
        if r["status"] == "running" and r["at"] < quiet:
            r["status"] = "quiet"
    return rows


def counts(rows: list[dict[str, Any]]) -> dict[str, int]:
    return {s: sum(1 for r in rows if r["status"] == s) for s in ("running", "quiet", "done", "failed")}


def view(now: datetime | None = None) -> dict[str, Any]:
    """Work's Agent activity: runs grouped by project. A group with a running run comes first, then the group
    with the newest event; inside a group, running runs first, then newest first. No project is the "" group."""
    rows = runs(now)
    groups: dict[str, list[dict[str, Any]]] = {}
    for r in rows:
        groups.setdefault(r["project"], []).append(r)
    ranked = []
    for name, items in groups.items():
        items.sort(key=lambda r: r["status"] != "running")  # stable: newest first inside each part
        ranked.append({"project": name, "running": sum(1 for r in items if r["status"] == "running"), "last_at": max(r["at"] for r in items), "runs": items})
    ranked.sort(key=lambda g: g["last_at"], reverse=True)
    ranked.sort(key=lambda g: g["running"] == 0)
    return {"counts": counts(rows), "quiet_minutes": int(config.get("agent_quiet_minutes")), "groups": ranked}
