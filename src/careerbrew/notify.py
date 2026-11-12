"""Notifications about important new events (issue #74): opt-in, useful, and actionable.

What becomes a notification: an open ask in the work feed (a page on you, a direct message, a
review request, a mention, important mail; personal events never) and an open inbox suggestion.
Each gets a priority from 1 to 5 (`PRIORITY`, +1 when it belongs to your rank-1 project in
`project_priority`); only those at or above `notify_min_priority` count. `notify_events` (off by
default) turns the feature on.

Useful: each notification says what happened, why it matters, and the one next step. Pop-up text
names kinds and sources only (never a title, a name, a project or plugin name, or message text)
and passes `privacy.redact`; titles, the rank-1 project's name (`priority_why`), and the plugin's
name show only in the web app's Notifications view (titles also in `notify list --show` in your
own terminal). One notification per thread (or inbox item): a repeat
while it is unread is merged into it, and the same event never notifies twice. Several at once,
or within `notify_batch_minutes` of the last pop-up, arrive together in one pop-up.

When: never in `notify_quiet_hours` (held, then one "while you were away" pop-up), at most
`notify_daily_cap` pop-ups a day (past it they wait in the web app), never for an event older
than a day. An ask you answered, a page resolved, or a suggestion decided in the inbox closes its
notification.

How (`backend()`): alerter (buttons Open, Accept, Dismiss, Snooze), else terminal-notifier (a
click opens the notification in the web app), else the plain osascript notification the watcher
already uses (`day.watch.notify`). A click runs `careerbrew notify open <id>`, which opens the web
app through the same flow as `careerbrew open`: the token is never in a pop-up or a command line.
`CAREERBREW_NOTIFY=off` turns pop-ups off; the list still fills.

Lifecycle: `day/notifications.json` (encrypted at rest) holds the list and the pop-up history.
`_locked()` serializes read-modify-write between the watcher, the CLI, and the web app with an
fcntl lock on `day/notifications.lock`, which the OS releases when the process exits. Nothing is
kept in process memory between calls.
"""

from __future__ import annotations

import contextlib
import fcntl
import os
import shlex
import shutil
import subprocess
import sys
import urllib.parse
import uuid
from collections.abc import Iterator
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any

from . import calendar, config, events, paths, privacy, store

ACTIONS = ("read", "accept", "dismiss", "snooze")
OPEN = ("new", "snoozed", "read")
PENDING = (None, "held:quiet", "held:batch", "held:project", "held:snooze")  # popup states that still wait for a pop-up
MAX_AGE = timedelta(hours=24)  # an older event is not news
KEEP = timedelta(days=14)
ALERTER_TIMEOUT_S = 3600

# kind: (what happened, why it matters, the one next step, priority)
KINDS: dict[str, tuple[str, str, str, int]] = {
    "page": ("a page is assigned to you", "an incident on you is open and nobody has acknowledged it", "acknowledge it in {src}, then fix it or hand it off", 5),
    "page-ack": ("a page you acknowledged is still open", "it stays on you until it is resolved", "resolve it in {src} or hand it off", 3),
    "review": ("a review is requested from you", "someone is waiting on your review to move on", "open the review in {src} and approve it or comment", 4),
    "dm": ("a direct message asks you", "it was sent to you directly and you have not replied", "reply in {src}", 4),
    "mention": ("someone mentioned you", "a mention asks you and you have not replied in the thread", "reply in the thread in {src}", 3),
    "mail": ("important mail for you", "it is marked important or flagged and you have not replied", "reply to it in {src}", 3),
    "ask": ("something asks you", "a rule marks it as an ask and you have not replied", "open it in {src} and reply", 2),
}
# inbox proposal kind: (what happened, why it matters, priority)
SUGGESTIONS: dict[str, tuple[str, str, int]] = {
    "agent-task": ("an agent proposes a task", "it is never planned until you accept it", 3),
    "plugin": ("a plugin suggests something", "the plugin waits for your decision; nothing changes until you decide", 3),
    "new-task": ("a ticket assigned to you has no task", "no plan includes work that is assigned to you", 3),
    "goal": ("a new goal is suggested", "your top promotion gap has no upcoming goal", 3),
    "task-done": ("a task looks done", "marking it done keeps your plan and the PM update right", 2),
    "source": ("a project has a new related source", "its work then counts for the project", 2),
}
LABELS = {
    "page": ("page", "pages"),
    "review": ("review request", "review requests"),
    "dm": ("direct message", "direct messages"),
    "mention": ("mention", "mentions"),
    "mail": ("important mail", "important mails"),
    "ask": ("ask", "asks"),
    "suggestion": ("suggestion", "suggestions"),
}
SOURCES = {"slack": "Slack", "pagerduty": "PagerDuty", "outlook": "Outlook", "github": "GitHub", "gitlab": "GitLab", "jira": "Jira", "careerbrew": "careerbrew"}
REVIEW_KINDS = {"review", "review-request", "review_request", "pr-review"}
CLICKS = {"@CONTENTCLICKED": "open", "Open": "open", "Accept": "accept", "Dismiss": "dismiss", "Snooze": "snooze"}  # alerter prints the clicked label


class NotifyError(ValueError):
    pass


def _dir() -> Path:
    return paths.sub("day", ".keep").parent


def path() -> Path:
    return _dir() / "notifications.json"


@contextlib.contextmanager
def _locked() -> Iterator[None]:
    with (_dir() / "notifications.lock").open("a") as fh:
        fcntl.flock(fh, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(fh, fcntl.LOCK_UN)


def load() -> dict[str, Any]:
    st = store.read_json(path(), {}) or {}
    return {"items": st.get("items", []), "popups": st.get("popups", [])}


def _save(st: dict[str, Any]) -> None:
    store.write_json_if_changed(path(), st)  # the watcher checks every cycle: a check with nothing new is no change (US-64)


def _dt(value: str) -> datetime:
    dt = datetime.fromisoformat(str(value).replace("Z", "+00:00"))
    return dt if dt.tzinfo else dt.replace(tzinfo=calendar.local_tz())


def _iso(dt: datetime) -> str:
    return dt.isoformat(timespec="seconds")


def settings() -> dict[str, Any]:
    v = {r["key"]: r["value"] for r in config.effective()}
    return {
        "enabled": bool(v["notify_events"]),
        "min_priority": int(v["notify_min_priority"]),
        "quiet_hours": str(v["notify_quiet_hours"] or ""),
        "daily_cap": int(v["notify_daily_cap"]),
        "batch_minutes": int(v["notify_batch_minutes"]),
        "snooze_minutes": int(v["notify_snooze_minutes"]),
        "backend": str(v["notify_backend"]),
    }


def in_quiet(now: datetime, spec: str) -> bool:
    """True inside quiet hours `HH:MM-HH:MM` (local time); a range like 19:00-08:00 wraps past midnight."""
    if not spec:
        return False
    start, end = spec.split("-")
    t = now.strftime("%H:%M")
    return start <= t < end if start < end else t >= start or t < end


# ---------- what is important ----------


def kind_of(row: dict[str, Any]) -> str:
    """The notification kind of a feed ask: page, review, dm, mention, mail, or ask."""
    kind = str(row.get("kind") or "")
    if kind == "page":
        return "page-ack" if row.get("status") == "acknowledged" else "page"
    if kind in REVIEW_KINDS or "review" in (row.get("tags") or []):
        return "review"
    return kind if kind in ("dm", "mention", "mail") else "ask"


def _top_project(ks: list[str], entries: list[dict[str, Any]]) -> str:
    from .work import project_priority

    hit = project_priority.match(ks, entries) if entries else None
    return hit["name"] if hit and hit["rank"] == 1 else ""


def _source_name(source: str) -> str:
    return SOURCES.get(source, source or "a tool")


def ask_key(row: dict[str, Any], entries: list[dict[str, Any]] | None = None) -> str:
    """One notification per chat thread, PR, or ticket (`feed.thread_key`, issue #228); else per event."""
    from .work import feed

    return f"feed:{feed.thread_key(row, entries) or row['id']}"


def candidates(now: datetime | None = None) -> list[dict[str, Any]]:
    """Every open ask and suggestion, scored, with its what / why / next. Below the threshold too (they keep old ones open)."""
    from .day import inbox
    from .work import evidence, feed, project_priority, projects, research

    now = now or calendar.now()
    entries = project_priority.entries()
    known = projects.records()

    def https_host(value: str) -> str:
        try:
            parsed = urllib.parse.urlsplit(value)
            if parsed.scheme == "https" and parsed.hostname and not parsed.username and not parsed.password and parsed.port in (None, 443):
                return parsed.hostname
        except ValueError:
            pass
        return ""

    github_hosts: set[str] = set()
    if any(source.get("kind") == "repo" for project in known.values() for source in project.get("sources", [])):
        for connector in evidence.sources_list():
            if connector.get("kind") != "github":
                continue
            host = https_host(str(connector.get("api") or "https://api.github.com"))
            if host:
                github_hosts.add("github.com" if host == "api.github.com" else host)

    def is_quiet(keys: list[str], explicit: str = "", url: str = "") -> bool:
        if explicit in known:
            return projects.quiet(explicit, now.date(), records=known)
        host = https_host(url)
        item = research.github_item(url) if host else None
        if item:
            identity = (host, item[0].lower(), item[1].lower(), item[2], item[3])
            owners = set()
            for name, project in known.items():
                for source in project.get("sources", []):
                    ref = str(source.get("ref", ""))
                    if source.get("kind") != "url" or not (source_host := https_host(ref)) or not (source_item := research.github_item(ref)):
                        continue
                    if (source_host, source_item[0].lower(), source_item[1].lower(), source_item[2], source_item[3]) == identity:
                        owners.add(name)
            if len(owners) > 1:
                return False
            if owners:
                return projects.quiet(next(iter(owners)), now.date(), records=known)
        if item and host in github_hosts:
            repo = "/".join(item[:2]).lower()
            owners = {name for name, project in known.items() for source in project.get("sources", []) if source.get("kind") == "repo" and not source.get("reference") and str(source.get("ref", "")).lower() == repo}
            if len(owners) > 1:
                return False
            if owners:
                return projects.quiet(next(iter(owners)), now.date(), records=known)
        hit = project_priority.match(keys, entries) if entries else None
        name = hit["name"] if hit else ""
        return name in known and projects.quiet(name, now.date(), records=known)

    out: list[dict[str, Any]] = []
    for r in feed.open_asks():  # personal events are never in it
        k = kind_of(r)
        what, why, nxt, base = KINDS[k]
        src = _source_name(str(r.get("source", "")))
        url = str(r.get("url") or "")
        keys = project_priority.keys(str(r.get("title") or ""), project_priority.repo_of(url=url), url) + [str(r.get("channel") or "").lower()]
        project_quiet = is_quiet(keys, url=url)
        top = _top_project(keys, entries)
        out.append(
            {
                "key": ask_key(r, entries),
                "ref": r["id"],
                "project_quiet": project_quiet,
                "at": r["at"],
                "source": str(r.get("source", "")),
                "kind": "page" if k == "page-ack" else k,
                "sub": k,
                "headline": f"{src}: {what}",
                "why": why + ("; it is on your rank-1 project" if top else ""),  # pop-up text: never the project's name
                "next": nxt.format(src=src),
                "priority": min(5, base + (1 if top else 0)),
                "priority_why": f"{base} for a {LABELS['page' if k == 'page-ack' else k][0]}" + (f", +1 for {top}, your rank-1 project" if top else ""),
            }
        )
    for i in inbox.load():
        if i.get("status") != "open" or i.get("type") != "proposal":
            continue
        d = i.get("data") or {}
        what, why, base = SUGGESTIONS.get(i.get("kind", ""), ("careerbrew has a suggestion", "nothing changes until you decide", 3))
        link = str(d.get("link") or "")
        keys = project_priority.keys(str(d.get("title") or d.get("task") or d.get("what") or ""), project_priority.repo_of(url=link), link, str(d.get("project") or ""))
        project_quiet = is_quiet(keys, str(d.get("project") or ""), link)
        top = _top_project(keys, entries)
        goal = i.get("kind") == "goal"
        out.append(
            {
                "key": f"inbox:{i['id']}",
                "ref": i["id"],
                "project_quiet": project_quiet,
                "item": i["id"],
                "at": i.get("created_at") or _iso(now),
                "source": "careerbrew",
                "kind": "suggestion",
                "sub": str(i.get("kind", "")),
                "headline": f"careerbrew: {what}",
                "why": why + ("; it is on your rank-1 project" if top else ""),
                "next": "approve or dismiss it in the web app" if goal else "accept or dismiss it",
                "priority": min(5, base + (1 if top else 0)),
                "priority_why": f"{base} for a suggestion" + (f", +1 for {top}, your rank-1 project" if top else ""),
            }
        )
    tired = _snoozed_kinds(now)
    for c in out:
        c["fresh"] = now - _dt(c["at"]) <= MAX_AGE
        if c["sub"] in tired:  # the owner keeps snoozing this kind: it matters less to them (issue #185)
            c["priority"] = max(1, c["priority"] - 1)
            c["priority_why"] += f", -1: you snoozed {tired[c['sub']]} like it in {SNOOZE_DAYS} days"
    return out


SNOOZE_TIMES, SNOOZE_DAYS = 3, 14  # this many snoozes of one kind in SNOOZE_DAYS lower its priority by one


def _snoozed_kinds(now: datetime) -> dict[str, int]:
    """Kinds (an ask's sub kind, a suggestion's kind) the owner snoozed SNOOZE_TIMES times in SNOOZE_DAYS days."""
    cut = now - timedelta(days=SNOOZE_DAYS)
    counts: dict[str, int] = {}
    for r in load()["items"]:
        n = sum(1 for at in r.get("snoozes", []) if _dt(at) >= cut)
        if n:
            counts[str(r.get("sub") or r.get("kind") or "")] = counts.get(str(r.get("sub") or r.get("kind") or ""), 0) + n
    return {k: n for k, n in counts.items() if n >= SNOOZE_TIMES}


# ---------- the list and the pop-ups ----------

TEXT = ("source", "kind", "sub", "headline", "why", "next", "priority", "priority_why")


def _nid(key: str) -> str:
    return f"n-{store.digest('notify', key)[:10]}"


def _optional(row: dict[str, Any], key: str, value: Any) -> None:
    if value:
        row[key] = value
    else:
        row.pop(key, None)


def _merge(st: dict[str, Any], cands: list[dict[str, Any]], now: datetime, s: dict[str, Any]) -> int:
    """Add new notifications, merge repeats, reopen a thread with a new ask, and close what no longer asks you."""
    items = st["items"]
    by_key = {r["key"]: r for r in items}
    live = {c["key"] for c in cands}
    grouped: dict[str, list[dict[str, Any]]] = {}
    for c in cands:
        grouped.setdefault(c["key"], []).append(c)
    for r in items:
        current = grouped.get(r["key"], [])
        quiet_refs = {c["ref"] for c in current if c.get("project_quiet")}
        ready_refs = {c["ref"] for c in current if not c.get("project_quiet")}
        held = r.get("held_refs", [])
        released = [ref for ref in held if ref in ready_refs]
        held = [ref for ref in held if ref in quiet_refs]
        if "pending_refs" not in r and not r.get("held_refs") and not r.get("snoozed_until_by_ref") and r["status"] == "new" and r.get("popup") in PENDING:
            _optional(r, "pending_refs", [ref for ref in r["refs"] if ref in quiet_refs | ready_refs])
        if r["status"] == "new":
            pending = r.get("pending_refs", [])
            snoozed = dict(r.get("snoozed_until_by_ref") or {})
            due = [ref for ref, until in snoozed.items() if _dt(until) <= now]
            held = list(dict.fromkeys([*held, *(ref for ref in due if ref in quiet_refs)]))[-50:]
            pending = [*pending, *(ref for ref in due if ref in ready_refs)]
            _optional(r, "snoozed_until_by_ref", {ref: until for ref, until in snoozed.items() if ref not in due and ref in quiet_refs | ready_refs})
            if any(ref in ready_refs for ref in due):
                r["popup"] = None
            newly_held = [ref for ref in pending if ref in quiet_refs]
            held = list(dict.fromkeys([*held, *newly_held]))
            _optional(r, "pending_refs", list(dict.fromkeys([*(ref for ref in pending if ref in ready_refs), *released])))
            if released:
                r["popup"] = None
        _optional(r, "held_refs", held)
    stamp = _iso(now)
    created = 0
    for c in cands:
        r = by_key.get(c["key"])
        if r is None:
            if c["priority"] < s["min_priority"] or not c["fresh"]:
                continue
            r = {"id": _nid(c["key"]), "key": c["key"], **{k: c[k] for k in TEXT}, "item": c.get("item", ""), "refs": [c["ref"]], "count": 1, "at": c["at"]}
            if c.get("project_quiet"):
                r["held_refs"] = [c["ref"]]
            else:
                r["pending_refs"] = [c["ref"]]
            r.update(status="new", popup=None, created_at=stamp, updated_at=stamp, acted_at="", snooze_until="", resolved="")
            items.append(r)
            by_key[c["key"]] = r
            created += 1
            continue
        if not c.get("project_quiet") or not any(not other.get("project_quiet") for other in grouped[c["key"]]):
            r.update({k: c[k] for k in TEXT})  # a page acknowledged changes its words and priority
        if c["ref"] in r["refs"]:
            continue  # the same event again: never a second notification
        r["refs"] = [*r["refs"], c["ref"]][-50:]
        if c.get("project_quiet"):
            r["held_refs"] = [*r.get("held_refs", []), c["ref"]][-50:]
        elif r["status"] == "new" and (r.get("held_refs") or r.get("snoozed_until_by_ref")):
            r["pending_refs"] = [*r.get("pending_refs", []), c["ref"]][-50:]
            r["popup"] = None
        r["count"] += 1
        r["updated_at"] = stamp
        if _dt(c["at"]) > _dt(r["at"]):
            r["at"] = c["at"]
        handled = r["status"] not in ("new", "snoozed")
        if handled and c["fresh"] and c["priority"] >= s["min_priority"] and _dt(c["at"]) > _dt(r.get("acted_at") or r["created_at"]):
            r.update(status="new", popup=None, resolved="")  # a new ask in a thread you had handled
            if not c.get("project_quiet"):
                r["pending_refs"] = [*r.get("pending_refs", []), c["ref"]][-50:]
    for r in items:
        if r["status"] in OPEN and r["key"] not in live:
            r.update(status="done", resolved="closed" if r["key"].startswith("feed:") else "decided", updated_at=stamp, acted_at=stamp)
        elif r["status"] == "snoozed" and r.get("snooze_until") and _dt(r["snooze_until"]) <= now:
            r.update(status="new", popup=None, snooze_until="")
    return created


def _count(rows: list[dict[str, Any]]) -> str:
    n: dict[str, int] = {}
    for r in rows:
        n[r["kind"]] = n.get(r["kind"], 0) + 1
    order = sorted(n, key=lambda k: (-max(x["priority"] for x in rows if x["kind"] == k), k))
    return ", ".join(f"{n[k]} {LABELS.get(k, (k, k + 's'))[0 if n[k] == 1 else 1]}" for k in order)


def _actions(r: dict[str, Any]) -> list[str]:
    if r["kind"] != "suggestion":
        return ["Open", "Snooze"]
    return ["Open", "Dismiss", "Snooze"] if r.get("sub") == "goal" else ["Accept", "Dismiss", "Snooze"]  # a goal is approved in the web app only


def _compose(rows: list[dict[str, Any]], now: datetime, quiet: bool) -> dict[str, Any]:
    """One pop-up for these notifications: what happened, why it matters, the next step. No titles, no names."""
    rows = sorted(rows, key=lambda r: (-r["priority"], r["at"]))
    top = rows[0]
    if len(rows) == 1:
        subtitle, message, actions, target = top["headline"], f"Why: {top['why']}. Next: {top['next']}.", _actions(top), top["id"]
    else:
        lead = "While you were away" if quiet else f"{len(rows)} new for you"
        subtitle = f"{lead}: {_count(rows)}"
        message, actions, target = f"Most urgent: {top['headline']}. Next: open careerbrew to act on each one.", ["Open", "Snooze"], ""
    ids = [r["id"] for r in rows]
    return {
        "id": f"p-{store.digest(_iso(now), ids)[:10]}",
        "at": _iso(now),
        "ids": ids,
        "target": target,
        "quiet": quiet,
        "subtitle": privacy.redact(subtitle)[:120],
        "message": privacy.redact(message[:1].upper() + message[1:])[:240],
        "actions": actions,
    }


def _decide(st: dict[str, Any], now: datetime, s: dict[str, Any], kind: str) -> tuple[str, dict[str, Any] | None, int]:
    """Pop up the waiting notifications now (one pop-up), or say why not: off, quiet, cap, or batch."""
    waiting = [r for r in st["items"] if r["status"] == "new" and r.get("popup") in PENDING]
    for r in waiting:
        if not r.get("pending_refs") and (r.get("held_refs") or r.get("snoozed_until_by_ref")):
            r["popup"] = "held:project" if r.get("held_refs") else "held:snooze"
    pending = [r for r in waiting if r.get("pending_refs") or not (r.get("held_refs") or r.get("snoozed_until_by_ref"))]
    if not pending:
        return "", None, 0
    today = [p for p in st["popups"] if _dt(p["at"]).astimezone(now.tzinfo).date() == now.date()]
    last = max((_dt(p["at"]) for p in st["popups"]), default=None)
    reason, mark = "", ""
    if kind == "none":
        reason, mark = "off", "off"
    elif in_quiet(now, s["quiet_hours"]):
        reason, mark = "quiet", "held:quiet"
    elif len(today) >= s["daily_cap"]:
        reason, mark = "cap", "capped"
    elif last is not None and now - last < timedelta(minutes=s["batch_minutes"]):
        reason, mark = "batch", "held:batch"
    if reason:
        for r in pending:
            r["popup"] = mark
        return reason, None, len(pending)
    popup = _compose(pending, now, quiet=any(r.get("popup") == "held:quiet" for r in pending))
    popup["refs"] = {r["id"]: list(r.get("pending_refs") or r["refs"]) for r in pending}
    popup["observed_refs"] = {r["id"]: list(r["refs"]) for r in pending}
    popup["launch_state"] = "reserved"
    for r in pending:
        r.update(popup="sent", popup_id=popup["id"], delivered_at=popup["at"])
        r.pop("pending_refs", None)
    st["popups"] = [*st["popups"], popup][-100:]
    return "sent", popup, len(pending)


def process(now: datetime | None = None) -> tuple[dict[str, Any], dict[str, Any] | None]:
    """Update the list from the feed and the inbox, and decide the pop-up (if any). Shows nothing itself."""
    now = now or calendar.now()
    s = settings()
    kind = backend()[0]
    stats: dict[str, Any] = {"enabled": s["enabled"], "new": 0, "open": 0, "reason": "", "batched": 0, "backend": kind}
    if not s["enabled"]:
        return stats, None
    cands = candidates(now)  # reads the feed and the inbox: outside the lock
    with _locked():
        st = load()
        stats["new"] = _merge(st, cands, now, s)
        stats["reason"], popup, stats["batched"] = _decide(st, now, s, kind)
        st["items"] = [r for r in st["items"] if r["status"] in OPEN or now - _dt(r["at"]) <= KEEP]
        stats["open"] = sum(1 for r in st["items"] if r["status"] in OPEN)
        _save(st)
    return stats, popup


def run(wait: bool = False, now: datetime | None = None) -> dict[str, Any]:
    """One check: update the list, then show at most one pop-up. The watcher calls this every cycle."""
    stats, popup = process(now)
    launched = bool(popup and deliver(popup, wait=wait))
    if popup and not launched:
        stats["reason"] = "not_sent"
    elif popup and launched and stats["backend"] == "alerter" and not wait:
        stats["reason"] = "scheduled"
    events.emit(
        "notify.check",
        status=(stats["reason"] or "ok") if stats["enabled"] else "off",
        source_kind=stats["backend"],
        new=stats["new"],
        sent=1 if launched else 0,
        batched=stats["batched"] if launched else 0,
        held=stats["batched"] if stats["reason"] in ("quiet", "batch") else 0,
        capped=stats["batched"] if stats["reason"] == "cap" else 0,
    )
    return stats


# ---------- how pop-ups show ----------


def backend() -> tuple[str, str, str]:
    """(kind, executable, note): alerter, terminal-notifier, osascript, or none."""
    if os.environ.get("CAREERBREW_NOTIFY", "on") == "off":
        return "none", "", "off (CAREERBREW_NOTIFY=off); every notification is in the web app's Notifications view"
    want = str(config.get("notify_backend"))
    for kind in ("alerter", "terminal-notifier") if want == "auto" else (want,) if want != "osascript" else ():
        exe = shutil.which(kind)
        if exe:
            note = "buttons Open, Accept, Dismiss, and Snooze" if kind == "alerter" else "a click opens the notification in the web app (buttons need alerter: brew install vjeantet/tap/alerter)"
            return kind, exe, f"{kind}: {note}"
    if want in ("alerter", "terminal-notifier"):
        fix = "brew install vjeantet/tap/alerter" if want == "alerter" else "brew install terminal-notifier"
        return "none", "", f"none: {want} is not installed (fix: {fix}, or careerbrew config set notify_backend auto)"
    if sys.platform == "darwin" and shutil.which("osascript"):
        return "osascript", "osascript", "osascript: plain pop-ups that do not open careerbrew (for clickable ones: brew install terminal-notifier, or alerter for buttons)"
    return "none", "", "none: no notifier on this system; every notification is in the web app's Notifications view"


def _click_command(nid: str) -> str:
    """The shell command a terminal-notifier click runs. It carries ids and paths only, never the token."""
    env = [f"{k}={os.environ[k]}" for k in ("CAREERBREW_HOME", "CAREERBREW_KEY_FILE") if os.environ.get(k)]
    return shlex.join(["env", *env, sys.executable, "-m", "careerbrew", "notify", "open", *([nid] if nid else [])])


def _prepare_launch(popup_id: str, claim: str) -> dict[str, Any] | None:
    """Recheck a reserved pop-up while the caller holds the project lock."""
    current = {(c["key"], c["ref"]): c for c in candidates()}
    with _locked():
        st = load()
        p = next((x for x in st["popups"] if x["id"] == popup_id), None)
        if p is None or p.get("launch_state", "launched") != "reserved":
            return None
        rows = {r["id"]: r for r in st["items"]}
        ready = []
        selected: dict[str, list[str]] = {}
        for ident in p["ids"]:
            r = rows.get(ident)
            if r is None or r["status"] != "new":
                continue
            refs = p.get("refs", {}).get(ident, r["refs"])
            live = [current[(r["key"], ref)] for ref in refs if (r["key"], ref) in current]
            held = [c["ref"] for c in live if c.get("project_quiet")]
            eligible = [c for c in live if not c.get("project_quiet")]
            _optional(r, "held_refs", list(dict.fromkeys([*r.get("held_refs", []), *held]))[-50:])
            if eligible:
                r.update({k: eligible[-1][k] for k in TEXT})
                selected[ident] = [c["ref"] for c in eligible]
                ready.append(r)
            elif held:
                r.update(popup="held:project", popup_id="")
            elif not live:
                observed = p.get("observed_refs", {}).get(ident, refs)
                other = [c for (key, ref), c in current.items() if key == r["key"] and ref not in observed]
                remaining = [c for (key, _ref), c in current.items() if key == r["key"]]
                if other:
                    for c in other:
                        if c["ref"] not in r["refs"]:
                            r["refs"] = [*r["refs"], c["ref"]][-50:]
                            r["count"] += 1
                    _optional(r, "held_refs", list(dict.fromkeys([*r.get("held_refs", []), *(c["ref"] for c in other if c.get("project_quiet"))]))[-50:])
                    _optional(r, "pending_refs", list(dict.fromkeys([*r.get("pending_refs", []), *(c["ref"] for c in other if not c.get("project_quiet"))]))[-50:])
                    r.update(popup=None if r.get("pending_refs") else "held:project", popup_id="")
                elif remaining:
                    quiet_refs = [c["ref"] for c in remaining if c.get("project_quiet") and c["ref"] in r.get("held_refs", [])]
                    r.update(popup="held:project" if quiet_refs else "sent", popup_id="")
                else:
                    r.update(status="done", resolved="closed" if r["key"].startswith("feed:") else "decided", popup_id="")
        if not ready:
            st["popups"] = [x for x in st["popups"] if x["id"] != popup_id]
            _save(st)
            return None
        fresh = _compose(ready, calendar.now(), quiet=p.get("quiet", False))
        fresh.update(id=popup_id, refs=selected, observed_refs=p.get("observed_refs", {}), launch_state="launching", launch_claim=claim)
        p.clear()
        p.update(fresh)
        _save(st)
        return dict(p)


def _rollback_launch(popup_id: str, claim: str = "") -> None:
    """Restore an owned reservation after the notifier could not be started."""
    with _locked():
        st = load()
        p = next((x for x in st["popups"] if x["id"] == popup_id), None)
        if p is None:
            return
        state = p.get("launch_state", "launched")
        if state != "reserved" and not (state == "launching" and claim and p.get("launch_claim") == claim):
            return
        for r in st["items"]:
            if r.get("popup_id") == popup_id and r["status"] == "new":
                refs = p.get("refs", {}).get(r["id"], r["refs"])
                r.update(popup=None, popup_id="", pending_refs=list(refs))
        st["popups"] = [x for x in st["popups"] if x["id"] != popup_id]
        _save(st)


def _mark_launched(popup_id: str, claim: str) -> None:
    with _locked():
        st = load()
        p = next((x for x in st["popups"] if x["id"] == popup_id), None)
        if p is not None and p.get("launch_state") == "launching" and p.get("launch_claim") == claim:
            p["launch_state"] = "launched"
            p.pop("launch_claim", None)
            _save(st)


def deliver(popup: dict[str, Any], wait: bool = False) -> bool:
    """Show one pop-up. alerter waits for a click, so it runs in its own process unless `wait`."""
    from .work import projects

    kind, exe, _note = backend()
    started = False
    claim = ""
    try:
        if kind == "alerter":
            if wait:
                present(popup["id"])
                return any(p["id"] == popup["id"] for p in load()["popups"])
            else:
                subprocess.Popen([sys.executable, "-m", "careerbrew.notify", "present", popup["id"]], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)  # noqa: S603
                started = True
        elif kind in ("terminal-notifier", "osascript"):
            with projects.locked():
                claim = uuid.uuid4().hex
                current = _prepare_launch(popup["id"], claim)
                if current is None:
                    return False
                if kind == "terminal-notifier":
                    group = f"careerbrew-{current['target'] or 'list'}"
                    cmd = [exe, "-title", "careerbrew", "-subtitle", current["subtitle"], "-message", current["message"], "-group", group, "-execute", _click_command(current["target"])]
                    process = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)  # noqa: S603
                else:
                    from .day import watch

                    watch.notify(f"{current['subtitle']}. {current['message']}")
                started = True
                _mark_launched(popup["id"], claim)
            if kind == "terminal-notifier":
                try:
                    process.communicate(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.communicate()
                    raise
    except (OSError, subprocess.SubprocessError) as exc:
        if not started:
            with projects.locked():
                _rollback_launch(popup["id"], claim)
        events.emit("notify.error", source_kind=kind, error=type(exc).__name__, story="us-62")
    if not started:
        with projects.locked():
            _rollback_launch(popup["id"], claim)
    return started


def remind(subtitle: str, message: str) -> str:
    """One plain reminder pop-up, outside the list (issue #265, the work day's wrap-up). The opt-in rules hold:
    `notify_events` off, quiet hours, or no notifier shows nothing. Returns `sent`, `off`, `quiet`, `none`, or `error`."""
    s = settings()
    if not s["enabled"]:
        return "off"
    if in_quiet(calendar.now(), s["quiet_hours"]):
        return "quiet"
    kind, exe, _note = backend()
    if kind == "none":
        return "none"
    subtitle, message = privacy.redact(subtitle)[:120], privacy.redact(message)[:240]
    try:
        if kind == "alerter":  # alerter waits for a click: its own process, so the caller never waits
            subprocess.Popen(
                [exe, "-title", "careerbrew", "-subtitle", subtitle, "-message", message, "-group", "careerbrew-reminder", "-closeLabel", "Close", "-timeout", "600"],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )  # noqa: S603
        elif kind == "terminal-notifier":
            subprocess.run([exe, "-title", "careerbrew", "-subtitle", subtitle, "-message", message, "-group", "careerbrew-reminder", "-execute", _click_command("")], check=False, capture_output=True, timeout=10)  # noqa: S603
        else:
            from .day import watch

            watch.notify(f"{subtitle}. {message}")
    except (OSError, subprocess.SubprocessError) as exc:
        events.emit("notify.error", source_kind=kind, error=type(exc).__name__, story="us-106")
        return "error"
    events.emit("notify.remind", status="sent", source_kind=kind)
    return "sent"


def present(popup_id: str) -> str:
    """Show an alerter pop-up, wait for the click, and apply it. Returns the action (empty: closed or timed out)."""
    from .work import projects

    kind, exe, _note = backend()
    if kind != "alerter":
        with projects.locked():
            _rollback_launch(popup_id)
        return ""
    started = False
    claim = uuid.uuid4().hex
    try:
        with projects.locked():
            p = _prepare_launch(popup_id, claim)
            if p is None:
                return ""
            cmd = [
                exe,
                "-title",
                "careerbrew",
                "-subtitle",
                p["subtitle"],
                "-message",
                p["message"],
                "-actions",
                ",".join(p["actions"]),
                "-closeLabel",
                "Close",
                "-group",
                f"careerbrew-{p['target'] or 'list'}",
                "-timeout",
                str(ALERTER_TIMEOUT_S),
            ]
            process = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)  # noqa: S603
            started = True
            _mark_launched(popup_id, claim)
        try:
            out = process.communicate(timeout=ALERTER_TIMEOUT_S + 30)[0].strip()
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate()
            raise
    except (OSError, subprocess.SubprocessError) as exc:
        if not started:
            with projects.locked():
                _rollback_launch(popup_id, claim)
        events.emit("notify.error", source_kind=kind, error=type(exc).__name__, story="us-62")
        return ""
    return _apply_click(p, out, kind)


def _apply_click(p: dict[str, Any], out: str, kind: str) -> str:
    """Apply a response from the already launched pop-up."""
    action = CLICKS.get(out.splitlines()[-1].strip() if out else "", "")
    if not action or (not p["target"] and action not in ("open", "snooze")):
        return ""  # closed, timed out, or a label a batch pop-up does not offer
    try:
        if p["target"]:
            act(p["target"], "read" if action == "open" else action, via="popup", selected_refs=p.get("refs", {}).get(p["target"]))
        elif action == "snooze":
            for nid in p["ids"]:
                with contextlib.suppress(NotifyError):  # one already closed elsewhere
                    act(nid, "snooze", via="popup", selected_refs=p.get("refs", {}).get(nid))
        else:
            events.emit("notify.act", status="open", source_kind="popup")
    except NotifyError as exc:  # decided elsewhere since the pop-up showed
        events.emit("notify.error", source_kind=kind, error=type(exc).__name__, story="us-62")
        return ""
    if action == "open":
        open_view(f"notifications/{p['target']}" if p["target"] else "notifications")
    return action


def open_view(view: str) -> bool:
    """Open the web app at `view` in the default browser, the way `careerbrew open` does: the token goes only to the browser."""
    import webbrowser

    from . import server

    return bool(webbrowser.open(server.app_url(int(config.get("serve_port")), view)))


# ---------- actions (pop-up buttons, the web app, the CLI) ----------


def act(nid: str, action: str, via: str = "cli", *, selected_refs: list[str] | None = None) -> dict[str, Any]:
    """read, accept, dismiss, or snooze one notification. Accept and Dismiss on a suggestion decide it in the inbox."""
    from .day import inbox

    if action not in ACTIONS:
        raise NotifyError(f"unknown action {action}; use one of {', '.join(ACTIONS)}")
    now = calendar.now()
    with _locked():
        st = load()
        r = next((x for x in st["items"] if x["id"] == nid), None)
        if r is None:
            raise NotifyError(f"no notification {nid} (see: careerbrew notify list)")
        if action != "read" and r["status"] not in OPEN:
            raise NotifyError(f"{nid} is already {r['status']}")
        if action == "accept" and r["kind"] != "suggestion":
            raise NotifyError(f"{nid} is not a suggestion: open it, snooze it, or dismiss it")
        if r["kind"] == "suggestion" and action in ("accept", "dismiss"):
            if not any(i["id"] == r["item"] and i["status"] == "open" for i in inbox.load()):  # decided in Today or the CLI since it showed
                r.update(status="done", resolved="decided", acted_at=_iso(now), updated_at=_iso(now))
                _save(st)
                raise NotifyError(f"{nid}: the suggestion is already decided in the inbox; nothing changed")
            try:
                inbox.accept(r["item"], from_ui=via == "ui") if action == "accept" else inbox.dismiss(r["item"])
            except inbox.InboxError as exc:
                raise NotifyError(str(exc)) from None
        deferred_refs = set(r.get("held_refs") or []) | set(r.get("snoozed_until_by_ref") or {})
        partial = bool(selected_refs and deferred_refs and not set(selected_refs) & deferred_refs)
        deferred_popup = "held:project" if r.get("held_refs") else "held:snooze"
        if action == "read":
            if r["status"] in OPEN and partial:
                r.update(status="new", popup=deferred_popup, popup_id="")
            else:
                r["status"] = "read" if r["status"] in OPEN else r["status"]
        elif action == "snooze":
            if partial:
                snoozed = dict(r.get("snoozed_until_by_ref") or {})
                snoozed.update({ref: _iso(now + timedelta(minutes=settings()["snooze_minutes"])) for ref in selected_refs or []})
                r.update(status="new", popup=deferred_popup, popup_id="", snoozed_until_by_ref=snoozed)
            else:
                r.update(status="snoozed", snooze_until=_iso(now + timedelta(minutes=settings()["snooze_minutes"])), popup=None)
            r["snoozes"] = [*r.get("snoozes", []), _iso(now)][-10:]  # each snooze teaches the kind's priority (issue #185)
        else:
            r.update(status="done" if action == "accept" else "dismissed", resolved="accepted" if action == "accept" else "dismissed")
        r.update(acted_at=_iso(now), updated_at=_iso(now))
        _save(st)
    events.emit("notify.act", status=action, source_kind=via)
    return r


# ---------- views ----------


def status() -> dict[str, Any]:
    s = settings()
    kind, _exe, note = backend()
    now = calendar.now()
    st = load()
    return {
        **s,
        "backend_kind": kind,
        "backend_note": note,
        "quiet_now": in_quiet(now, s["quiet_hours"]),
        "sent_today": sum(1 for p in st["popups"] if _dt(p["at"]).astimezone(now.tzinfo).date() == now.date()),
        "open": sum(1 for r in st["items"] if r["status"] in OPEN),
        "unread": sum(1 for r in st["items"] if r["status"] == "new"),
    }


POPUP_NOTE = {
    "sent": "shown as a pop-up",
    "held:quiet": "held for quiet hours: it pops up after them",
    "held:batch": "held to arrive with the next pop-up",
    "held:project": "held until the project's quiet period ends",
    "held:snooze": "held until the snoozed ask is due",
    "capped": "no pop-up: the daily cap was reached",
    "off": "no pop-up: pop-ups are off here",
    None: "waiting for the next check",
}


def view() -> dict[str, Any]:
    """The Notifications view: each notification with its title (local only), what, why, next, and the actions it allows."""
    from .day import inbox
    from .work import feed, project_priority

    all_feed = feed.load()
    feed_rows = {r["id"]: r for r in all_feed}
    entries = project_priority.entries()
    live_feed = {ask_key(r, entries) for r in feed.open_asks(all_feed)}
    items = {i["id"]: i for i in inbox.load()}
    notification_state = load()
    unconfirmed = {p["id"] for p in notification_state["popups"] if p.get("launch_state") in ("reserved", "launching")}
    rows = []
    for r in notification_state["items"]:
        r = dict(r)
        if r["status"] in OPEN and (items.get(r["item"], {}).get("status") != "open" if r["kind"] == "suggestion" else r["key"] not in live_feed):
            r.update(status="done", resolved="decided" if r["kind"] == "suggestion" else "closed")  # decided or answered since the last check
        is_open = r["status"] in OPEN
        if r["kind"] == "suggestion":
            i = items.get(r["item"]) or {}
            title = inbox.explain(i)["what"] if i else ""
            if i.get("kind") == "plugin":
                title += f" (suggested by plugin {(i.get('data') or {}).get('plugin', '')})"  # local only: a pop-up never names the plugin
            url, link_label = "#/today", "See it in Today"
            accept = "Approve goal" if i.get("kind") == "goal" else "Accept"
            actions = (["accept"] if is_open and i.get("status") == "open" else []) + (["snooze", "dismiss"] if is_open else [])
        else:
            f = feed_rows.get(r["refs"][-1]) or {}
            if f.get("personal"):
                continue  # marked personal after it notified: it leaves the list too
            title, url = str(f.get("title") or ""), str(f.get("url") or "")
            link_label, accept = f"Open in {_source_name(r['source'])}", ""
            actions = ["snooze", "dismiss"] if is_open else []
        rows.append(
            {
                **{k: r[k] for k in ("id", "kind", "source", "headline", "why", "next", "priority", "priority_why", "count", "at", "status", "snooze_until", "resolved")},
                "title": title,
                "url": url,
                "link_label": link_label,
                "accept_label": accept,
                "actions": actions,
                "popup": "delivery not confirmed" if r.get("popup_id") in unconfirmed else POPUP_NOTE.get(r.get("popup"), ""),
            }
        )
    rows.sort(key=lambda x: _dt(x["at"]), reverse=True)  # newest first, then open before closed, unread first, by priority
    rows.sort(key=lambda x: (x["status"] not in OPEN, x["status"] != "new", -x["priority"]))
    return {"items": rows, "status": status()}


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "present":  # the alerter path of deliver(): waits for the click in its own process
        present(sys.argv[2])
