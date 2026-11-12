"""Proactive listening (issue #266): the follow-up work every live cycle does for each new event, as drafts only.

The live feed (`live.py`) runs from the start of the work day until wrap-up (`work live stop`). After a cycle
stores and triages its events, `follow_up` works each event it has not seen before:
  1. class    needs you (an open ask), worth knowing (news, your own action, an answered ask), noise, or
              personal (kept out of everything, no work done);
  2. reply    each open ask gets a reply draft through the safe save (`drafts.replace`), judged on the
              thread's newest state (`drafts.fit`): an emoji suggestion for a small ack, careerbrew's ack
              after a release started, else a holding reply for the agent or owner to rewrite. A target
              that already holds a draft keeps it;
  3. research each link in a kept event gets a one-line summary, read through `research.read` (the host
              policy, sign-in pages, plugin `agent_reads`), or `opened: false` with the reason. A reason
              the cycle stores carries `by: cycle`: a placeholder the agent may still replace
              (`feed.needs_research` lists it), since the agent's own tools may open what careerbrew cannot;
  4. review   a review asked of you (a review-request event, or an ask to review) gets review notes,
              drafted to `review:<digest>`, with the link research and a checklist;
  5. project  the event is attached to its project (a link that matches a project's source, else the
              event's project label) and the change is recorded (`projects.record_update`).
After that, `goal_updates` gives one line per goal for today (`goals.load`: queue goals not past, long-term
goals not past their `by` day): what moved (your actions today), other updates, and what waits on whom.

Draft only: nothing here sends, posts, or reacts. A draft lives in the local draft store (`drafts.LocalBackend`,
encrypted at rest) until the owner reads it (`careerbrew draft show <target>`) and sends it themselves.

Privacy: the cycle record holds event ids, classes, verdicts, the actions taken (draft targets, project
names), goal ids, counts, and the names a goal waits on; never message text. Reading links is off unless
the caller may read business data (`read_allowed`, the CLI's `may_show_business()`).

Lifecycle: `live.py` owns the state; this module keeps nothing in process memory. The ids followed and
each event's attached project live in the live state (`followed`, `attached`, the last `KEEP_FOLLOWED`).
"""

from __future__ import annotations

import hashlib
import http.client
import ipaddress
import os
import re
import socket
import urllib.error
import urllib.parse
import urllib.request
from typing import Any

from .. import calendar, config, connectors, events, goals, paths, store
from . import drafts, feed, projects, research

CLASSES = {"answer": "needs you", "done": "worth knowing", "read": "worth knowing", "evidence": "worth knowing", "log": "worth knowing", "skip": "noise", "personal": "personal"}
CLASS_ORDER = ("needs you", "worth knowing", "noise", "personal")
HOLDING = "Thanks, I saw this. I'm looking into it and will get back to you soon."
REVIEW_KINDS = ("review", "review-request", "review_request")
_REVIEW_ASK = re.compile(r"\b(review|take a look|have a look|look over|feedback on|sign[ -]off)\b", re.I)
READING_OFF = "not opened: reading business data is off here (an agent opens it: careerbrew work feed links --pending)"
REVIEW_CHECKS = (
    "Does the change do what its description says, and nothing else?",
    "Do tests cover the change, including the error case?",
    "Risks: data, security, rollout, and how to roll back.",
    "Names, docs, and anything a reader would trip on.",
)
KEEP_FOLLOWED = 5000
LINKS_OFF = "not opened: the live cycle opens links only with live_open_links on"
MAX_PAGE_BYTES = 1_000_000
LINK_TIMEOUT_SECONDS = 5


def _digest(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()[:16]


def reply_target(row: dict[str, Any]) -> str:
    """Where an event's reply draft lives: its thread (one reply per thread), else its id, else a digest of the id."""
    for cand in (row.get("thread"), row.get("id")):
        cand = str(cand or "")
        if cand and drafts.TARGET.match(cand) and ".." not in cand:
            return cand
    return f"feed:{_digest(str(row['id']))}"


def review_target(row: dict[str, Any]) -> str:
    return f"review:{_digest(str(row['id']))}"


_URL = re.compile(r"https?://[^\s<>\"')]+")


def review_key(request: str) -> str:
    """A review request's key: its link (a PR's URL) when it has one, else its event id, so a re-review ping on the same PR is the same request."""
    return f"review-session:{_digest(request)}"


def _review_sessions_path():
    return paths.sub("work", "live", "review-sessions.json")


def review_sessions() -> dict[str, dict[str, Any]]:
    """The review sessions opened by live cycles (issue #313): {key: {event, url, status, opened}}, never message text."""
    data = store.read_json(_review_sessions_path(), {}) or {}
    return data if isinstance(data, dict) else {}


def open_review(row: dict[str, Any], text: str) -> tuple[str, bool]:
    """Record one review session for this request, or find the one already open: (key, opened now).

    careerbrew cannot start an agent session: the live-feed agent starts a fresh reviewer for each new key, which
    writes the review to a file and saves a reply draft, and never posts. Runs inside the cycle lock (one writer)."""
    m = _URL.search(text) or _URL.search(str(row.get("url") or ""))
    url = m.group(0).rstrip(".,;:") if m else ""
    key = review_key(url or str(row["id"]))
    sessions = review_sessions()
    if key in sessions:
        return key, False
    sessions[key] = {"event": row["id"], "url": url, "status": "requested", "opened": calendar.now().isoformat(timespec="seconds")}
    store.write_json(_review_sessions_path(), sessions)
    return key, True


def _thread(row: dict[str, Any], rows: list[dict[str, Any]], keys: dict[str, str], origs: dict[str, dict[str, Any]]) -> list[dict[str, Any]]:
    key = keys.get(row["id"], "")
    same = [r for r in rows if r["id"] == row["id"] or (key and keys.get(r["id"]) == key)]
    same.sort(key=lambda r: (r["at"], r["id"]))
    return drafts.messages([{"ts": r["at"], "text": str((origs.get(r["id"]) or {}).get("text") or ""), "mine": r.get("actor") == "me"} for r in same])


def _save_draft(target: str, text: str) -> str | None:
    """Save a draft unless the target already holds one (checked under the target's lock); None when it was kept."""
    res = drafts.replace(target, text, drafts.LocalBackend(), if_empty=True)
    return None if res["kept"] else target


def draft_reply(row: dict[str, Any], thread: list[dict[str, Any]]) -> str:
    """The action taken for an open ask: a reply draft, an emoji suggestion, or nothing when your message is newest."""
    target = reply_target(row)
    try:
        res = drafts.fit(thread)
        if res["kind"] == "none":
            return "no reply draft: your message is the newest in the thread"
        if res["kind"] == "emoji":
            return f"react :{res['emoji']}: (no text draft)"
        saved = _save_draft(target, res["text"] or HOLDING)
    except drafts.DraftError as e:
        return f"reply draft not saved: {type(e).__name__}"
    return f"reply draft saved: {target}" if saved else f"draft kept: {target} already has one"


def is_review(row: dict[str, Any], verdict: str, text: str) -> bool:
    """A review asked of you and still open: an answered ask (`done`) gets no notes."""
    if verdict != "answer":
        return False
    return row.get("kind") in REVIEW_KINDS or bool(_REVIEW_ASK.search(text))


# ---------- links ----------


def _addresses(host: str) -> list[str]:
    """Every address the host resolves to (an IP literal is itself); [] when it does not resolve."""
    try:
        return sorted({str(info[4][0]) for info in socket.getaddrinfo(host, None)})
    except (OSError, UnicodeError):
        return []


def private_reason(url: str) -> str:
    """Why the cycle may not open this link's host (a loopback, private, link-local, or reserved address, or none), else ""."""
    host = (urllib.parse.urlsplit(url).hostname or "").lower().rstrip(".")
    if not host:
        return "not opened: the link has no host"
    addrs = ["127.0.0.1"] if host == "localhost" or host.endswith(".localhost") else _addresses(host)
    if not addrs:
        return "not opened: the host does not resolve"
    for a in addrs:
        ip = ipaddress.ip_address(a.split("%")[0])
        if ip.is_loopback or ip.is_private or ip.is_link_local or ip.is_reserved or ip.is_multicast or ip.is_unspecified:
            return "not opened: the link points to a private or local address"
    return ""


def _refuse_private(url: str) -> None:
    if why := private_reason(url):
        raise connectors.ConnectorError(why)


def _fetch(url: str) -> bytes:
    """GET one page under the host policy: a short timeout, and at most MAX_PAGE_BYTES read."""
    connectors.check_host(url)
    if os.environ.get("CAREERBREW_HTTP_FIXTURES"):
        return connectors.http_get(url)[:MAX_PAGE_BYTES]
    req = urllib.request.Request(url, headers={"User-Agent": "careerbrew/0.2 (live feed link summary)"})
    with connectors.open_url(req, timeout=LINK_TIMEOUT_SECONDS, also=_refuse_private) as resp:
        return resp.read(MAX_PAGE_BYTES)


def _summary_of(html: str, ref: str) -> str:
    title = re.search(r"<title>(.*?)</title>", html, re.S | re.I)
    head = " ".join((title.group(1) if title else ref).split())[:120]
    notes = research._key_lines(re.sub(r"<[^>]+>", "\n", html))
    return f"{head}: {' '.join(notes[0].split())}" if notes else head


def _read_url(url: str) -> str:
    """A web page's one-line summary; raises research.ReadError with the reason."""
    if why := private_reason(url):
        raise research.ReadError(why)
    try:
        body = _fetch(url)
    except connectors.ConnectorError as e:
        raise research.ReadError(str(e)) from None
    except urllib.error.HTTPError as e:
        raise research.ReadError(f"url answered {e.code}") from None
    except (OSError, ValueError, http.client.HTTPException) as e:
        raise research.ReadError(f"url could not be read ({type(e).__name__})") from None
    return _summary_of(body[:MAX_PAGE_BYTES].decode("utf-8", errors="replace"), url)


def _references() -> set[str]:
    """Refs the owner marked as project references (`work project reference`): never opened."""
    return {str(s.get("ref")) for p in projects.records().values() for s in p.get("sources") or [] if s.get("reference") is True}


def research_links(row: dict[str, Any], read_allowed: bool, refs: set[str] | None = None) -> list[dict[str, Any]]:
    """One entry per link not researched yet: a one-line summary, or opened: false and the reason."""
    refs = _references() if refs is None else refs
    out = []
    for url in feed.needs_research(row):
        entry: dict[str, Any] = {"url": url, "opened": False, "by": "cycle"}
        try:
            kind, ref = research.detect(url)
            if url in refs or ref in refs:
                entry["reason"] = "not opened: you marked it a project reference"
            elif why := projects.no_reader(url, kind):
                entry["reason"] = why
            elif not config.get("live_open_links"):
                entry["reason"] = LINKS_OFF
            elif not read_allowed:
                entry["reason"] = READING_OFF
            elif kind == "url" and not research.github_item(ref):
                entry = {"url": url, "opened": True, "summary": _read_url(url)[: feed.SUMMARY_MAX], "by": "cycle"}
            else:
                got = research.read(kind, ref, None)
                title = " ".join(str(got.get("title") or ref).split())
                notes = [n for n in got.get("notes") or [] if str(n).strip()]
                summary = f"{title}: {' '.join(str(notes[0]).split())}" if notes else title
                entry = {"url": url, "opened": True, "summary": summary[: feed.SUMMARY_MAX], "by": "cycle"}
        except research.ReadError as e:
            entry["reason"] = " ".join(str(e).split())
        except ValueError as e:  # a link urllib cannot parse ("https://[draft")
            entry["reason"] = f"not opened: the link is not a valid URL ({type(e).__name__})"
        if not entry["opened"]:
            entry["reason"] = entry["reason"][: feed.REASON_MAX]
        out.append(entry)
    return out


def review_notes(row: dict[str, Any], found: list[dict[str, Any]]) -> str:
    link = row.get("url") or next(iter(row.get("links") or []), "") or "none"
    lines = [f"Review notes (a draft, not posted): {row['title']}", f"Link: {link}"]
    for e in found:
        lines.append(f"- {e['url']}: " + (e["summary"] if e.get("opened") else f"not opened ({e.get('reason', '')})"))
    lines += ["", "Check:", *(f"- [ ] {c}" for c in REVIEW_CHECKS), "", "Verdict: (approve, or request changes and say why in one line)"]
    return "\n".join(lines)


def attach(row: dict[str, Any], every: dict[str, Any]) -> str:
    """The project an event belongs to: the one project a link of it matches, else the one its project label names; "" when none or a tie."""
    names: set[str] = set()
    urls: list[str] = [str(u) for u in row.get("links") or []] + ([str(row["url"])] if row.get("url") else [])
    for url in dict.fromkeys(urls):
        try:
            kind, ref = research.detect(url)
            names.add(research.match_project(kind, ref, ""))
        except (research.ReadError, projects.ProjectError, ValueError):
            continue
    if len(names) == 1:
        return names.pop()
    if names:
        return ""
    label = str(row.get("project") or "").casefold()
    if not label:
        return ""
    hits = [n for n, p in every.items() if not p.get("archived_at") and label in (n, str(p.get("title") or "").casefold())]
    return hits[0] if len(hits) == 1 else ""


# ---------- one event, and the cycle's follow-up ----------


def _one(rec: dict[str, Any], row: dict[str, Any], ctx: dict[str, Any]) -> None:
    """The follow-up work for one event; it appends to rec["actions"] and notes its project in ctx["touched"]."""
    acts, v, event_id = rec["actions"], rec["verdict"], rec["id"]
    if rec["class"] in ("personal", "noise"):
        acts.append(f"nothing: {'personal, kept out' if rec['class'] == 'personal' else 'noise'}")
        return
    text = str((ctx["origs"].get(event_id) or {}).get("text") or "")
    if v == "answer":
        acts.append(draft_reply(row, _thread(row, ctx["every"], ctx["keys"], ctx["origs"])))
    found = research_links(row, ctx["read_allowed"], ctx["refs"])
    if found:
        try:
            row = feed.cycle_research(event_id, found)
            acts.append(f"{len(found)} link(s): {sum(e['opened'] for e in found)} summarized, {sum(not e['opened'] for e in found)} with a reason")
        except feed.FeedError:
            pass
    if is_review(row, v, text):
        target = review_target(row)
        try:
            saved = _save_draft(target, review_notes(row, row.get("research") or []))
            acts.append(f"review notes drafted: {target}" if saved else f"review notes kept: {target} already has some")
        except drafts.DraftError as e:
            acts.append(f"review notes not saved: {type(e).__name__}")
        key, opened = open_review(row, text)
        acts.append(f"review session opened: {key}" if opened else "review session kept: one is already open for this request")
    if name := attach(row, ctx["projs"]):
        rec["project"] = name
        ctx["touched"].setdefault(name, []).append((rec["class"], row))
        acts.append(f"project {name} updated")
    if not acts:
        acts.append("nothing to do: noted")


def _project_updates(touched: dict[str, list[tuple[str, dict[str, Any]]]]) -> list[str]:
    """One update per project per cycle: how many events and of which class, plus the newest one. Names the projects that failed."""
    failed = []
    for name, items in touched.items():
        counts = {c: sum(1 for x, _ in items if x == c) for c in CLASS_ORDER}
        cls, newest = max(items, key=lambda it: it[1]["at"])
        what = ", ".join(f"{n} {c}" for c, n in counts.items() if n)
        text = f"live cycle: {len(items)} event(s) ({what}); newest: {cls} ({newest['source']} {newest['kind']}): {newest['title']}"
        try:
            projects.record_update(name, "project", " ".join(text.split())[: projects.UPDATE_MAX])
        except projects.ProjectError:
            failed.append(name)
    return failed


def follow_up(verdicts: dict[str, str], followed: set[str], read_allowed: bool) -> tuple[list[dict[str, Any]], dict[str, str]]:
    """The follow-up work for each event in `verdicts` not in `followed`: (one record per event, {event id: project}).

    Each event runs alone: an error in one is recorded on it as `failed: <error class>` (never its text), the event
    still counts as followed, and the other events go on."""
    from . import project_priority

    every = feed.load()
    by_id = {r["id"]: r for r in every}
    es = project_priority.entries()
    ctx: dict[str, Any] = {
        "every": every,
        "origs": feed.originals(),
        "keys": {r["id"]: feed.thread_key(r, es) for r in every},
        "projs": projects.records(),
        "refs": _references(),
        "read_allowed": read_allowed,
        "touched": {},
    }
    out: list[dict[str, Any]] = []
    for event_id, v in verdicts.items():
        row = by_id.get(event_id)
        if row is None or event_id in followed:
            continue
        rec: dict[str, Any] = {"id": event_id, "source": row["source"], "class": CLASSES[v], "verdict": v, "actions": [], "project": None}
        out.append(rec)
        try:
            _one(rec, row, ctx)
        except Exception as exc:  # noqa: BLE001 - one bad event must never lose the others or the cycle mark (issue #266)
            rec["actions"].append(f"failed: {type(exc).__name__}")
            events.emit("work.live.error", source_kind="listen", error=type(exc).__name__, story="us-101")
    try:
        failed = _project_updates(ctx["touched"])
    except Exception as exc:  # noqa: BLE001 - the events' records must survive a failed projects write
        failed = list(ctx["touched"])
        events.emit("work.live.error", source_kind="listen", error=type(exc).__name__, story="us-101")
    for name in failed:
        for rec in out:
            if rec["project"] == name:
                rec["actions"] = [a for a in rec["actions"] if a != f"project {name} updated"] + [f"project {name} not updated: ProjectError"]
                rec["project"] = None
    attached = {rec["id"]: rec["project"] for rec in out if rec["project"]}
    if out:
        events.emit("work.live.listen", status="ok", events=len(out), needs_you=sum(r["class"] == "needs you" for r in out), story="us-101")
    return out, attached


# ---------- goals ----------


def todays_goals() -> list[dict[str, Any]]:
    """Each goal still ahead today: queue goals (no date, or today or later) and long-term goals (no `by`, or not past)."""
    today = calendar.now().date().isoformat()
    g = goals.load()
    out: list[dict[str, Any]] = []
    for x in g["queue"]:
        if not x.get("date") or str(x["date"]) >= today:
            out.append({"id": str(x["id"]), "terms": [str(x["id"]), *[str(t) for t in x.get("topics") or []]]})
    for x in g["long_term"]:
        if not x.get("by") or str(x["by"]) >= today:
            out.append({"id": str(x["id"]), "terms": [str(x["id"])]})
    return list({x["id"]: x for x in reversed(out)}.values())[::-1]  # one line per id; the first one listed wins


def _hits(goal: dict[str, Any], row: dict[str, Any], project: dict[str, Any] | None, name: str) -> bool:
    if f"goal:{goal['id']}" in (row.get("tags") or []):
        return True
    if project and goal["id"] in (str(project.get("group") or "").casefold(), name):
        return True
    hay = " ".join([str(row.get("title") or ""), str(row.get("project") or ""), name, str((project or {}).get("title") or "")])
    return any(t and re.search(rf"(?<![\w-]){re.escape(t)}(?![\w-])", hay, re.I) for t in goal["terms"])


def goal_updates(attached: dict[str, str]) -> list[dict[str, Any]]:
    """One update per goal for today: what moved (your actions), other updates, and what waits on you or on whom."""
    gs = todays_goals()
    if not gs:
        return []
    from . import project_priority

    tz = calendar.local_tz()
    today = calendar.now().date()
    every = feed.load()
    es = project_priority.entries()
    keys = {r["id"]: feed.thread_key(r, es) for r in every}

    def local_day(r: dict[str, Any]) -> Any:
        from datetime import datetime

        return datetime.fromisoformat(str(r["at"]).replace("Z", "+00:00")).astimezone(tz).date()

    kept = [r for r in every if r["triage"] != "noise" and not r.get("personal") and local_day(r) == today]
    open_ids = {r["id"] for r in feed.open_asks(every)}
    projs = projects.records()
    out = []
    for g in gs:
        mine = [r for r in kept if _hits(g, r, projs.get(attached.get(r["id"], "")), attached.get(r["id"], ""))]
        moved = [r for r in mine if r.get("actor") == "me"]
        waits_on: list[str] = []
        for r in moved:
            key = keys.get(r["id"], "")
            thread = sorted((x for x in every if key and keys.get(x["id"]) == key), key=lambda x: (x["at"], x["id"]))
            if thread and thread[-1]["id"] == r["id"]:
                for x in thread:
                    who = str(x.get("from") or "someone") if x.get("actor") == "other" else ""
                    if who and who not in waits_on:
                        waits_on.append(who)
        out.append({"id": g["id"], "moved": len(moved), "news": len(mine) - len(moved), "waits_you": sum(r["id"] in open_ids for r in mine), "waits_on": waits_on})
    return out


def goal_line(g: dict[str, Any], show: bool) -> str:
    """One line for one goal; the names it waits on show only where business data may show."""
    parts = []
    if g["moved"] or g["news"]:
        parts.append(f"{g['moved']} moved (your actions today), {g['news']} other update(s)")
    else:
        parts.append("nothing moved today")
    waits = []
    if g["waits_you"]:
        waits.append(f"{g['waits_you']} on you")
    if g["waits_on"]:
        waits.append(f"{len(g['waits_on'])} on {', '.join(g['waits_on']) if show else 'others'}")
    parts.append("waits: " + ", ".join(waits) if waits else "nothing waits")
    return f"goal {g['id']}: " + "; ".join(parts)
