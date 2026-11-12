"""The approval queue: agent actions wait here until the owner approves or rejects them.

An agent (Claude Code, Codex) that wants to do something the owner must sign off on runs
`careerbrew approvals request "<title>" --detail "<what and why>"` and stops. The owner
decides in teacher mode (or `careerbrew approvals decide`). The agent reads the decision
with `careerbrew approvals show <id>` and acts only on "approved".

The owner's note on a decision (why they rejected it, or a condition on an approval) is standing
guidance (issue #185): `guidance()` lists the newest ones, `approvals request` prints them so an agent
reads them before it asks again, and `careerbrew approvals guidance` shows them.
"""

from __future__ import annotations

import uuid
from pathlib import Path
from typing import Any

from . import calendar, events, paths, store


def _path() -> Path:
    return paths.sub("approvals.json")


def load() -> list[dict[str, Any]]:
    return store.read_json(_path(), []) or []


def request(title: str, detail: str = "", by: str = "agent") -> dict[str, Any]:
    title = " ".join(title.split())
    if not title:
        raise ValueError("an approval request needs a title")
    item = {"id": uuid.uuid4().hex[:8], "title": title[:200], "detail": detail.strip()[:4000], "by": by[:60], "at": calendar.now_utc().isoformat(timespec="seconds"), "status": "pending", "note": ""}
    store.write_json(_path(), [*load(), item])
    events.emit("approvals.request", status="pending")
    return item


def pending() -> list[dict[str, Any]]:
    return [a for a in load() if a["status"] == "pending"]


def decide(item_id: str, decision: str, note: str = "") -> dict[str, Any]:
    if decision not in ("approved", "rejected"):
        raise ValueError("decision must be approved or rejected")
    items = load()
    for a in items:
        if a["id"] == item_id:
            if a["status"] != "pending":
                raise ValueError(f"already {a['status']}")
            a.update(status=decision, note=note.strip()[:1000], decided_at=calendar.now_utc().isoformat(timespec="seconds"))
            store.write_json(_path(), items)
            events.emit("approvals.decide", status=decision)
            return a
    raise KeyError(f"unknown approval {item_id}")


GUIDANCE_MAX = 10


def guidance(limit: int = GUIDANCE_MAX) -> list[dict[str, str]]:
    """The owner's notes on decided requests, newest first: what agents should do differently next time."""
    rows = [a for a in load() if a.get("status") in ("approved", "rejected") and a.get("note")]
    rows.sort(key=lambda a: str(a.get("decided_at") or a.get("at") or ""), reverse=True)
    return [{"status": a["status"], "title": a["title"], "note": a["note"], "at": str(a.get("decided_at") or "")} for a in rows[:limit]]


def get(item_id: str) -> dict[str, Any]:
    for a in load():
        if a["id"] == item_id:
            return a
    raise KeyError(f"unknown approval {item_id}")
