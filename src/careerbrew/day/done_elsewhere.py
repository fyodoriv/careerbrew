"""Is an inbox suggestion already done elsewhere? Signals from data careerbrew already reads (issue #147).

careerbrew reads no new source for this. A signal comes from:
- evidence (`work collect`, the watcher): a pull request that `gh` reports merged (sure) or closed (unsure),
  a GitHub pull request the connector saw merged or closed (unsure), a Jira ticket resolved as done (sure);
- the work feed analysis (`work/feed/events.jsonl`, written by connectors and plugins): a review you gave
  (kind review, actor me), a pull request merged (status merged), and a CI run (kind ci, build, check,
  pipeline, workflow) whose latest status for that ref is green (a later red one cancels it).
Each signal names refs: a pull request as `pr:owner/repo#N`, a web address as `url:host/path`, a ticket
key as `key:PROJ-12`, and a feed thread or event id.

A suggestion names refs too: its title, its links, and a plugin's `ref`. Refs only in an agent's why
are weaker: a match through them is never sure. `check()` matches each open ask:
- sure: a sure signal answers the ask's own verb (a review ask and a review you gave, a CI ask and CI
  green, a merge ask and the PR merged, an "add a task for this ticket" ask and the ticket done), and a
  review or CI signal came after the suggestion was made. The suggestion expires: it is kept (never
  deleted), shows under "Done elsewhere" with the signal, and Bring back (`day reopen`) opens it again.
- unsure: any other match. The suggestion stays open and says "maybe done", with the signal.
"""

from __future__ import annotations

import re
from datetime import UTC, datetime, tzinfo
from typing import Any
from urllib.parse import urlsplit

from ..work import evidence, feed, priorities

_URL = re.compile(r"https?://[^\s<>\"'`]+")
_PR_URL = re.compile(r"^/([\w.-]+)/([\w.-]+)/pulls?/(\d+)")
_BROWSE = re.compile(r"/browse/([A-Z][A-Z0-9]+-\d+)\b")
_PR_REF = re.compile(r"(?<![\w/.-])([\w.-]+/[\w.-]+)#(\d+)\b")
_KEY = re.compile(r"\b([A-Z][A-Z0-9]+-\d+)\b")
STANDARDS = ("AES-", "ANSI-", "CVE-", "CWE-", "ECMA-", "IEC-", "IEEE-", "ISO-", "PEP-", "RFC-", "RSA-", "SHA-", "TLS-", "UCS-", "UTF-")

REVIEW_KINDS = {"review", "review-given", "review_given", "review-submitted", "review_submitted", "approval", "approve", "approved"}
CI_KINDS = {"ci", "build", "check", "checks", "pipeline", "workflow", "ci-run"}
GREEN = {"success", "succeeded", "passed", "pass", "green", "fixed", "ok"}
RED = {"failure", "failed", "fail", "red", "broken", "error", "errored", "timed_out", "timed-out"}

# The verb of an ask, from its title: which signal answers it.
VERBS = {
    "review": re.compile(r"\breview", re.I),
    "ci": re.compile(r"\b(ci|builds?|pipelines?|checks|flaky|failing)\b", re.I),
    "merge": re.compile(r"\b(merge|land|ship)\b", re.I),
}
ANSWERS = {"review": "review", "ci-green": "ci", "merged": "merge", "ticket-done": "ticket"}
CHECKED = ("agent-task", "plugin", "new-task")  # the kinds that are asks; a task-done, source, or goal suggestion is not


def refs_in(text: str) -> set[str]:
    """The refs a text names: web addresses (and the pull request or ticket a link names), `owner/repo#N`, ticket keys."""
    out: set[str] = set()
    text = str(text or "")
    for raw in _URL.findall(text):
        out |= url_refs(raw)
    for owner_repo, n in _PR_REF.findall(text):
        out.add(f"pr:{owner_repo.lower()}#{n}")
    for key in _KEY.findall(text):
        if not key.startswith(STANDARDS):
            out.add(f"key:{key}")
    return out


def url_refs(raw: str) -> set[str]:
    raw = raw.rstrip(".,;:!?)]}")
    try:
        p = urlsplit(raw)
    except ValueError:
        return set()
    if p.scheme.lower() not in ("http", "https") or not p.hostname:
        return set()
    path = p.path.rstrip("/")
    out = {f"url:{p.hostname.lower()}{path}"}
    if m := _PR_URL.match(path):
        out.add(f"pr:{m.group(1).lower()}/{m.group(2).lower()}#{m.group(3)}")
    if m := _BROWSE.search(path):
        out.add(f"key:{m.group(1)}")
    return out


def _when(value: Any, tz: tzinfo) -> datetime | None:
    """An ISO time as an aware datetime (a time with no offset is in `tz`, the local zone), or None."""
    try:
        t = datetime.fromisoformat(str(value).replace("Z", "+00:00"))
    except ValueError:
        return None
    return t if t.tzinfo else t.replace(tzinfo=tz)


def _sig(kind: str, sure: bool, what: str, at: Any, url: str, refs: set[str]) -> dict[str, Any]:
    return {"type": kind, "sure": sure, "what": what, "at": str(at or ""), "url": url if priorities.is_web(url) else "", "refs": refs}


def signals() -> list[dict[str, Any]]:
    """Every done-elsewhere signal in evidence and the work feed analysis."""
    out: list[dict[str, Any]] = []
    for e in evidence.load():
        kind, url, state = e.get("kind"), str(e.get("url") or ""), str(e.get("state") or "").lower()
        refs = url_refs(url) if url else set()
        if kind == "pr" and (m := re.match(r"^pr:([\w.-]+/[\w.-]+)#(\d+)$", str(e.get("id", "")))):
            ref = f"{m.group(1)}#{m.group(2)}"
            refs.add(f"pr:{ref.lower()}")
            if state == "merged":
                out.append(_sig("merged", True, f"PR {ref} merged", e.get("closedAt") or e.get("at"), url, refs))
            elif state == "closed":
                out.append(_sig("merged", False, f"PR {ref} closed", e.get("closedAt") or e.get("at"), url, refs))
        elif kind == "github" and "delivery" in (e.get("tags") or []) and refs:
            out.append(_sig("merged", False, "PR merged or closed (GitHub)", e.get("at"), url, refs))
        elif kind == "jira" and (m := re.match(r"^jira:([A-Z][A-Z0-9]+-\d+)$", str(e.get("id", "")))):
            out.append(_sig("ticket-done", True, f"{m.group(1)} resolved (Jira)", e.get("at"), url, refs | {f"key:{m.group(1)}"}))
    ci: dict[str, tuple[str, dict[str, Any]]] = {}  # ref -> (time, the latest CI run on it)
    for r in feed.load():
        kind, status = str(r.get("kind") or "").lower(), str(r.get("status") or "").lower()
        url = str(r.get("url") or "")
        refs = (url_refs(url) if url else set()) | refs_in(str(r.get("title") or "")) | {f"thread:{r['id']}"}
        if r.get("thread"):
            refs.add(f"thread:{r['thread']}")
        src = str(r.get("source") or "the feed")
        if r.get("actor") == "me" and kind in REVIEW_KINDS:
            out.append(_sig("review", True, f"you gave a review ({src})", r.get("at"), url, refs))
        elif status == "merged" or kind in ("merge", "merged", "pr-merged"):
            out.append(_sig("merged", True, f"PR merged ({src})", r.get("at"), url, refs))
        elif kind in CI_KINDS and status in GREEN | RED:
            for ref in refs:
                if ref not in ci or str(r.get("at") or "") >= ci[ref][0]:
                    ci[ref] = (str(r.get("at") or ""), {**r, "_green": status in GREEN, "_src": src})
    green: dict[str, dict[str, Any]] = {}  # one signal per green run, with each ref it is the latest run on
    for ref, (_at, r) in ci.items():
        if r["_green"]:
            if r["id"] not in green:
                green[r["id"]] = _sig("ci-green", True, f"CI green ({r['_src']})", r.get("at"), str(r.get("url") or ""), set())
            green[r["id"]]["refs"].add(ref)
    return out + list(green.values())


def _ask(item: dict[str, Any], tasks: dict[str, dict[str, Any]]) -> tuple[str, set[str], set[str], str]:
    """An open ask's text, its own refs (title, links, a plugin's ref), the weaker refs of an agent's why, and its verb."""
    d = item.get("data") or {}
    kind = item.get("kind")
    if kind == "agent-task":
        t = tasks.get(str(d.get("task") or "")) or {}
        title = str(t.get("title") or d.get("title") or "")
        own = refs_in(title) | {r for x in priorities.links(t) for r in url_refs(x["url"])}
        weak = refs_in(str(t.get("why") or d.get("why") or ""))
    elif kind == "plugin":
        title = str(d.get("what") or "")
        own = refs_in(title) | url_refs(str(d.get("link") or "")) | refs_in(str(d.get("ref") or "")) | ({f"thread:{d['ref']}"} if d.get("ref") else set())
        weak = refs_in(str(item.get("why") or ""))
    else:  # new-task: one ticket assigned to you
        title = str(d.get("title") or "")
        own = {f"key:{d['key']}"} if d.get("key") else refs_in(title.split(":", 1)[0])
        weak = set()
    verb = "ticket" if kind == "new-task" else next((v for v, rx in VERBS.items() if rx.search(title)), "")
    return title, own, weak - own, verb


def check(items: list[dict[str, Any]], now: datetime, sigs: list[dict[str, Any]] | None = None) -> dict[str, int]:
    """Expire open asks that a sure signal shows done; mark the unsure ones "maybe done". Changes `items` in place.

    An item the owner brought back (`keep`) is never checked again. Returns {expired, maybe}.
    """
    from ..calendar import local_tz

    sigs = signals() if sigs is None else sigs
    tasks, tz = {t["id"]: t for t in priorities.load()}, local_tz()
    by_ref: dict[str, list[dict[str, Any]]] = {}
    for s in sigs:
        for ref in s["refs"]:
            by_ref.setdefault(ref, []).append(s)
    counts = {"expired": 0, "maybe": 0}
    for i in items:
        if i.get("status") != "open" or i.get("type") != "proposal" or i.get("kind") not in CHECKED or i.get("keep"):
            continue
        _title, own, weak, verb = _ask(i, tasks)
        made = _when(i.get("created_at"), tz)
        sure, maybe = None, None
        for ref in sorted(own | weak):
            for s in by_ref.get(ref, []):
                after = s["type"] in ("merged", "ticket-done") or (made is not None and (_when(s["at"], tz) or made) >= made)
                if s["sure"] and ref in own and ANSWERS[s["type"]] == verb and after:
                    sure = sure or s
                elif maybe is None or s["at"] > maybe["at"]:
                    maybe = s
        if sure:
            i["status"] = "expired"
            i["expired"] = {"what": sure["what"], "url": sure["url"], "at": now.astimezone(UTC).isoformat(timespec="seconds")}
            i.pop("maybe_done", None)
            counts["expired"] += 1
        elif maybe:
            i["maybe_done"] = {"what": maybe["what"], "url": maybe["url"], "at": maybe["at"]}
            counts["maybe"] += 1
        else:
            i.pop("maybe_done", None)
    return counts
