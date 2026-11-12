"""Promises the owner makes become tracked projects (issue #268).

A promise is a commitment in the owner's own sent message or reply: first person plus a future action
("I'll look into the timeout", "I will send it tomorrow", "let me check"). `detect` finds one in a text;
`track` turns each own feed event that holds one into a project of kind `promise` in `work/projects.yaml`
(the one project store; `projects.add_promise` writes it under the file lock):
  promise-<8 hex of the event id>:
    kind: promise
    title: the promised action, one line ("Look into the timeout")
    parent: the project whose title or name the event's project label names, else none
    sources: [{kind: chat | url, ref: the message link, reference: true}]  # a pointer, never read, never stale
    promise: {event, to, due, made_at, steps: {research|draft|status: ISO UTC}, closed: done | dropped}

The title is the only text kept from the message, and it holds only the promised action: the feed's
analysis still holds no message text. An open promise is one that is not archived. `close` archives it
(done, or dropped: it was not a promise), so it leaves the Commitments block and the handoff at once.
The handoff's "Promises you made" section lists every open promise live (`handoff_items`), so it is
never stale. The work-day cycle works each open promise in parallel: `next_step` names its next step
(research, draft the follow-up, report status) and `step` records each one.

Lifecycle: files under the data root only; nothing is kept in process memory between calls.
"""

from __future__ import annotations

import hashlib
import re
from datetime import date, datetime, timedelta
from statistics import median
from typing import Any

from .. import events
from . import projects

TITLE_MAX = 120
STEPS = ("research", "draft", "status")
STEP_TEXT = {
    "research": "research what the promise needs (the thread, the links, the code)",
    "draft": "draft the follow-up message (a draft only; the owner sends it)",
    "status": "report status to the owner",
}
CLOSED = ("done", "dropped")
# Own events that can hold a promise: messages, DMs, replies, mentions, and sent mail. Never a page or a meeting.
KINDS = ("dm", "message", "mention", "reply", "mail", "comment", "event")
_LEAD = re.compile(
    r"(?:^|(?<=[\s,;:(\"'“]))(?:I(?:'|’)ll|I\s+will|I(?:'|’)m\s+going\s+to|I\s+am\s+going\s+to|let\s+me|I\s+can)\s+(?P<what>[^.!?\n]+)",
    re.I,
)
# What follows the lead that is not a promised action: a negation, being away, or asking the other person.
_NOT = re.compile(r"^(?:not|never|no\b|probably\s+not|be\s+(?:out|off|away|back|ooo|on\s+(?:leave|vacation|holiday|pto))|know|have\s+to\s+check\s+with\s+you|let\s+you\s+decide)\b", re.I)
_SLACK_LINK = re.compile(r"<(https?://[^|>\s]+)(?:\|([^>]*))?>", re.I)
_MENTION = re.compile(r"<[@#!][^>]*>")
_WEEKDAYS = ("monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday")
_SAME_DAY = re.compile(r"\b(?:today|tonight|this\s+(?:morning|afternoon|evening)|eod|end\s+of\s+(?:the\s+)?day|asap|in\s+(?:an?|a\s+few|\d+)\s+(?:minutes?|mins?|hours?|hrs?))\b", re.I)
_IN_DAYS = re.compile(r"\bin\s+(\d{1,2})\s+days?\b", re.I)
_ISO_DAY = re.compile(r"\b(20\d\d-\d\d-\d\d)\b")
_WEEKDAY = re.compile(r"\b(next\s+)?(" + "|".join(_WEEKDAYS) + r")\b", re.I)


def _plain(text: str) -> str:
    """Slack markup out: a link becomes its label (or nothing), a mention goes."""
    text = _SLACK_LINK.sub(lambda m: m.group(2) or "", text or "")
    return " ".join(_MENTION.sub("", text).split())


def due_from(text: str, said: date) -> str:
    """The due day a promise states, as YYYY-MM-DD, counted from the day it was made; "" when it states none."""
    if m := _ISO_DAY.search(text):
        try:
            return date.fromisoformat(m.group(1)).isoformat()
        except ValueError:
            return ""
    low = text.lower()
    if _SAME_DAY.search(low):
        return said.isoformat()
    if re.search(r"\btomorrow\b", low):
        return (said + timedelta(days=1)).isoformat()
    if m := _IN_DAYS.search(low):
        return (said + timedelta(days=int(m.group(1)))).isoformat()
    if re.search(r"\b(?:this\s+week|end\s+of\s+(?:the\s+)?week|eow)\b", low):
        return (said + timedelta(days=max(0, 4 - said.weekday()))).isoformat()  # Friday of this week
    if re.search(r"\bnext\s+week\b", low):
        return (said + timedelta(days=7 - said.weekday())).isoformat()  # next Monday
    if m := _WEEKDAY.search(low):
        ahead = (_WEEKDAYS.index(m.group(2).lower()) - said.weekday()) % 7
        return (said + timedelta(days=ahead + (7 if m.group(1) else 0))).isoformat()
    return ""


def detect(text: str, said: date) -> dict[str, str] | None:
    """The first promise in a message: {what, due}, or None. `said` is the local day it was sent."""
    plain = _plain(text)
    for m in _LEAD.finditer(plain):
        what = m.group("what").strip(" ,;:-—")
        lead = m.group(0)[: m.start("what") - m.start()].strip().lower()
        end = m.end("what")
        if end < len(plain) and plain[end] == "?":  # "I can do that?" asks; it does not promise
            continue
        if _NOT.match(what) or len(what.split()) < 2:  # "I'll check" alone names no action to track
            continue
        if lead == "i can" and not due_from(what, said):  # "I can" is a promise only with a deadline
            continue
        title = what[:TITLE_MAX].rstrip()
        return {"what": title[:1].upper() + title[1:], "due": due_from(what, said)}
    return None


def name_for(event_id: str) -> str:
    return "promise-" + hashlib.sha256(event_id.encode()).hexdigest()[:8]


def _said(at: str) -> date:
    from ..calendar import local_tz

    return datetime.fromisoformat(str(at).replace("Z", "+00:00")).astimezone(local_tz()).date()


def _to_whom(row: dict[str, Any], rows: list[dict[str, Any]]) -> str:
    """Who the promise was made to: the last other person in the same thread before it, else the channel."""
    from . import feed

    key = feed.thread_key(row)
    if key:
        before = [r for r in rows if r.get("actor") == "other" and r["at"] <= row["at"] and r["id"] != row["id"] and feed.thread_key(r) == key and r.get("from")]
        if before:
            return str(max(before, key=lambda r: (r["at"], r["id"]))["from"])[:80]
    return f"#{row['channel']}"[:80] if row.get("channel") else ""


def _parent(label: str, every: dict[str, Any]) -> str:
    """The project an event's project label names (its title or its name, any case), else ""."""
    want = label.strip().lower()
    if not want:
        return ""
    for name, p in sorted(every.items()):
        if p.get("kind") != "promise" and want in (name.lower(), str(p.get("title") or "").strip().lower()):
            return name
    return ""


def candidate(row: dict[str, Any]) -> bool:
    """An own, sent, non-personal event: the only kind a promise can come from."""
    return row.get("actor") == "me" and str(row.get("kind") or "") in KINDS and not row.get("personal") and row.get("source") != "pagerduty"


def track(pairs: list[tuple[dict[str, Any], str]]) -> list[str]:
    """Make a promise project for each (analysis row, original text) that holds a promise. Idempotent by event id:
    a promise already tracked (open or closed) is never made again. Returns the new project names."""
    from . import feed

    pairs = [(r, t) for r, t in pairs if candidate(r) and t]
    if not pairs:
        return []
    rows, every = feed.load(), projects.records()
    made: list[str] = []
    failed = 0
    for row, text in pairs:
        name = name_for(row["id"])
        if name in every:
            continue
        found = detect(text, _said(row["at"]))
        if not found:
            continue
        url = str(row.get("url") or "")
        try:
            projects.add_promise(
                name,
                title=found["what"],
                parent=_parent(str(row.get("project") or ""), every),
                link=url if url.startswith(("https://", "http://")) else "",
                source_kind="chat" if row.get("source") == "slack" else "url",
                promise={"event": row["id"], "to": _to_whom(row, rows), "due": found["due"], "made_at": row["at"], "steps": {}},
            )
        except projects.ProjectError:
            if name not in projects.records():  # another session tracked it meanwhile: that is a skip, not a failure
                failed += 1  # one bad item never drops the rest; `work promise scan` retries while the original is kept
            continue
        every[name] = {}
        made.append(name)
    if made:
        events.emit("work.promise", status="ok", action="track", count=len(made))
    if failed:
        events.emit("work.promise", status="error", action="track", count=failed, error="ProjectError", story="us-99")
    return made


def track_safely(pairs: list[tuple[dict[str, Any], str]]) -> int:
    """`track` for the feed's ingest: the events are stored already, so no error here may fail the ingest."""
    try:
        return len(track(pairs))
    except Exception as exc:  # noqa: BLE001 -- best effort after the store: a decrypt or disk error is logged, never raised
        events.emit("work.promise", status="error", action="track", error=type(exc).__name__, story="us-99")
        return 0


def scan() -> dict[str, int]:
    """Look for promises in every own event whose original text is still kept (`feed_keep_days`): a first run, or after a fix."""
    from . import feed

    origs = feed.originals()
    rows = [r for r in feed.load() if candidate(r) and r["id"] in origs]
    made = track([(r, str(origs[r["id"]].get("text") or "")) for r in rows])
    return {"events": len(rows), "new": len(made)}


def add(title: str, to: str = "", due: str = "", link: str = "", parent: str = "") -> str:
    """A promise made outside the feed (a meeting, a call): the owner or an agent adds it by hand."""
    from ..calendar import now

    if due:
        try:
            due = date.fromisoformat(due.strip()).isoformat()
        except ValueError:
            raise projects.ProjectError(f"the due day looks like 2026-11-03, not {due!r}") from None
    if link and not link.startswith(("https://", "http://")):
        raise projects.ProjectError("a promise's link is an http(s) link")
    made_at = now().isoformat(timespec="seconds")
    every = projects.records()
    n = 0
    while (name := name_for(f"manual:{made_at}:{title}:{n}")) in every:  # the same text twice in one second is two promises
        n += 1
    projects.add_promise(name, title=title, parent=parent, link=link, source_kind="url", promise={"event": "", "to": " ".join(to.split())[:80], "due": due, "made_at": made_at, "steps": {}})
    events.emit("work.promise", status="ok", action="add", count=1)
    return name


def next_step(p: dict[str, Any]) -> str:
    """The next step the work-day cycle takes on an open promise: research, then draft, then status."""
    done = (p.get("promise") or {}).get("steps") or {}
    return next((s for s in STEPS if s not in done), "status")


def view(name: str, p: dict[str, Any], today: date, business: bool) -> dict[str, Any]:
    pr = p.get("promise") or {}
    link = next((s["ref"] for s in p.get("sources") or [] if s.get("kind") in ("chat", "url")), "")
    due = str(pr.get("due") or "")
    row = {
        "name": name,
        "due": due,
        "overdue": bool(due) and due < today.isoformat(),
        "made_at": pr.get("made_at", ""),
        "event": pr.get("event", ""),
        "url": link,
        "parent": p.get("parent", ""),
        "steps": dict(pr.get("steps") or {}),
        "next": next_step(p),
        "closed": pr.get("closed", "") if p.get("archived_at") else "",
    }
    if business:
        row["title"] = p.get("title", "")
        row["to"] = pr.get("to", "")
    return row


def open_list(business: bool = True, include_closed: bool = False) -> list[dict[str, Any]]:
    """Open promises (not archived): overdue first, then by due day (none last), then oldest. Without `business`,
    the title and who it was made to stay out."""
    from ..calendar import now

    today = now().date()
    rows = [view(n, p, today, business) for n, p in projects.records().items() if p.get("kind") == "promise" and (include_closed or not p.get("archived_at"))]
    return sorted(rows, key=lambda r: (bool(r["closed"]), r["due"] == "", r["due"], r["made_at"], r["name"]))


def stats(days: int) -> dict[str, int | float | None]:
    """Count promises created in an inclusive local-day cohort."""
    from .. import calendar
    from ..day import pm_promises

    if not 1 <= days <= 3660:
        raise projects.ProjectError("promise stats needs --days between 1 and 3660")
    today = calendar.now().date()
    start = today - timedelta(days=days - 1)
    outcomes = pm_promises.outcomes()
    missing = {row["promise"] for row in outcomes if row["state"] == "not-mentioned"}
    invalid = {row["promise"] for row in outcomes if row["state"] == "decision-needs-review"}
    out = {"made": 0, "kept_on_time": 0, "kept_late": 0, "moved": 0, "dropped": 0, "not_mentioned": 0}
    durations = []
    for name, row in projects.records().items():
        if row.get("kind") != "promise":
            continue
        promise = row.get("promise") or {}
        try:
            made_at = datetime.fromisoformat(str(promise.get("made_at", "")))
            made = made_at.astimezone(calendar.local_tz()).date() if made_at.tzinfo else made_at.date()
        except ValueError:
            continue
        if not start <= made <= today or promise.get("pm_removed"):
            continue
        out["made"] += 1
        out["moved"] += bool(promise.get("pm_moved")) and name not in invalid
        out["not_mentioned"] += name in missing
        if promise.get("closed") == "dropped" and row.get("archived_at"):
            out["dropped"] += 1
        if promise.get("closed") == "done" and row.get("archived_at") and name not in invalid:
            decision = promise.get("pm_decision") or {}
            try:
                if promise.get("manual_kept_day"):
                    kept = date.fromisoformat(str(promise["manual_kept_day"]))
                elif decision.get("state") == "done":
                    kept = date.fromisoformat(str(decision.get("day", "")))
                else:
                    closed = datetime.fromisoformat(str(row["archived_at"]))
                    kept = closed.astimezone(calendar.local_tz()).date() if closed.tzinfo else closed.date()
            except ValueError:
                continue
            if not made <= kept <= today:
                continue
            durations.append((kept - made).days)
            try:
                due = date.fromisoformat(str(promise.get("original_due", promise.get("due", ""))))
            except ValueError:
                continue
            out["kept_on_time" if kept <= due else "kept_late"] += 1
    return {**out, "median_days_to_keep": median(durations) if durations else None}


def _get(name: str) -> dict[str, Any]:
    p = projects.records().get(name)
    if not p or p.get("kind") != "promise":
        raise projects.ProjectError(f"no promise {name} (see: careerbrew work promise list)")
    return p


def close(name: str, how: str = "done") -> None:
    """Close a promise: done (kept) or dropped (it was not a promise). It leaves Commitments and the handoff."""
    if how not in CLOSED:
        raise projects.ProjectError(f"a promise closes as {' or '.join(CLOSED)}")
    _get(name)
    projects.close_promise(name, how)
    if how == "done":  # a fixed action id per branch: event fields are allow-listed short ids (test_projects.py)
        events.emit("work.promise", status="ok", action="done", count=1)
    else:
        events.emit("work.promise", status="ok", action="dropped", count=1)


def step(name: str, which: str, text: str) -> dict[str, Any]:
    """Record one step of the cycle's work on a promise (research, draft, status) with a one-line note."""
    if which not in STEPS:
        raise projects.ProjectError(f"a promise step is one of {', '.join(STEPS)}")
    if _get(name).get("archived_at"):
        raise projects.ProjectError(f"{name} is closed (open it again: careerbrew work project unarchive {name})")
    row = projects.promise_step(name, which, text)
    if which == "research":
        events.emit("work.promise", status="ok", action="research", count=1)
    elif which == "draft":
        events.emit("work.promise", status="ok", action="draft", count=1)
    else:
        events.emit("work.promise", status="ok", action="status", count=1)
    return row


def handoff_items(business: bool = True) -> list[dict[str, Any]]:
    """Each open promise as a "Promises you made" handoff item: live, so a closed one is gone at once."""
    out = []
    for r in open_list(business=True):
        text = r["title"] + (f" (to {r['to']})" if r["to"] else "") + (f", due {r['due']}" if r["due"] else "")
        item: dict[str, Any] = {"id": r["name"], "section": "promises", "text": text if business else "", "live": True}
        if r["url"]:
            item["link"] = r["url"]
        if not business:
            item.pop("text")
        out.append(item)
    return out


def links() -> set[str]:
    """Every tracked promise's link, open or closed: a hand-added handoff item with one of them is the same promise."""
    return {r["url"] for r in open_list(include_closed=True) if r["url"]}
