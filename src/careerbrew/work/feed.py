"""The work feed: one local event store for every work tool (issue #72).

Connectors (Slack reads, PagerDuty, Outlook) and plugins write events into it. Each event is kept
in two parts, apart:
- the original, as the tool sent it (`work/feed/originals.jsonl`): encrypted at rest like every
  data file and kept `feed_keep_days` days (default 30). careerbrew reads it only to triage again
  and for `work feed open` (the owner's terminal or web app). It is never logged, never printed
  to an agent, and never in evidence, the day log, or the PM update;
- the analysis (`work/feed/events.jsonl`): id, source, kind, time, a short title, who acted, the
  project title when the tool sends one (null is no project), the triage label, the personal flag,
  and the rule that decided each; the actor's id (`actor_id`), the teammates it is by or about
  (`team`), the links in its text (`links`), and what each link says (`research`, one line per link).
  It holds no message text. Views, evidence, and the standup report read only this.

Triage is by rule. Rules run in order, the owner's (`work/feed-rules.yaml`) before the built-in
ones (`BUILTIN_RULES`); for each field (triage, personal, evidence) the first matching rule that
sets it wins.
  act    something asks you: a DM or mention, a review requested from you (a plugin's `review-request`
         event), a page assigned to you, important or flagged mail
  fyi    worth a look; nothing asks you
  noise  chatter: bots and no-reply senders
  mine   your own action: a message or mail you sent, a page you handled, a meeting you organized
Teammates (issue #180, `team.py`): an event by a teammate on your roster, or one whose text names
one (their name, a <@chat-id> mention, or @login), carries `team` (the names) in its analysis and is
never `noise`: a rule that would make it noise makes it `fyi` (rule `teammate`), unless that rule
names `team` itself. A rule can match on `team: true` or `team: false`.
Links: each link in a kept event's text (Slack's `<url|label>` and bare URLs) is in `links`; an agent
or you research each one with `work feed research <id> <file>`, which is refused unless it covers
every link: a one-line `summary`, or `opened: false` with the `reason` (a sign-in page, say).
An ask is open until a later event of yours in the same thread (a reply, an acknowledge), or until the owner
marks it `not-ask` (it never asked them) or `done` (handled elsewhere) with `work feed mark` (issue #185). An event
with no chat thread takes its PR, merge request, issue, or ticket link as the thread (`thread_key`, issue #228), so your
later review or comment there closes the ask. Any other link is no thread. It is overdue
after `feed_ask_overdue_hours` work hours: only time from `work_start` to `work_end` on `work_days`
(local time) counts, so nights and weekends do not.

Personal: an event the source marks private (an Outlook private entry), a rule marks personal, or
the owner marks with `work feed mark <id> --personal`. It stays in the store, but it never becomes
evidence, never enters the day log or a project's memory file (`memory_sync`), and never reaches the
PM update. The owner's mark wins.

Signals (issue #239): your own channel post keeps one word in its analysis, `signal`: `launch` (it announces
something, or it is pinned) or `check-in` (your reply in a channel thread); the PM update names them with
their links. The word is the only thing read from the text.

Your own actions that a rule marks `evidence` (built in: pages you acknowledged or resolved)
become work evidence through `evidence.collect()`, unless they are personal.

Cycles (issue #254): `ingest` records each source it stored in the live-feed cycle (`work/feed/feed-cycle.json`),
also when every event was stored before, and a raw search reply with no items (a quiet source). `mark_cycle`
returns `not_stored`, every required source with no ingest this cycle, and clears the record; a non-empty
`not_stored` is a failed cycle, so a source pulled but never stored is never read as quiet.
Raw replies (issue #254): a tool reply saved exactly as returned, an object with a `tickets` list (a ticket
search; id from key + updated + status) or a `messages` list (a mail search; id from the message id), is
turned into one event per item by `raw_events`, for `add_lines` and the transcript path alike.

Lifecycle: files under the data root only; nothing is kept in process memory between calls.
`_locked()` serializes read-modify-write between the CLI and the watcher with an fcntl lock on
`work/feed/.lock`, which the OS releases when the process exits.
"""

from __future__ import annotations

import contextlib
import fcntl
import fnmatch
import json
import re
from bisect import bisect_left
from collections.abc import Callable, Iterable, Iterator
from datetime import UTC, datetime, time, timedelta, tzinfo
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit

from .. import config, events, paths, store

TRIAGES = ("act", "fyi", "noise", "mine")
ACTORS = ("me", "other", "bot")
SET_KEYS = ("triage", "personal", "evidence")
TEXT_KEYS = ("source", "kind", "channel", "from", "status", "importance", "title", "origin")
WHEN_KEYS = ("actor", "to_me", "team", "text", *TEXT_KEYS)
BOOL_KEYS = ("to_me", "team")
# What the analysis keeps of an event. The text and the tool's payload live only in the original.
FACT_KEYS = ("id", "source", "kind", "at", "title", "actor", "actor_id", "to_me", "channel", "from", "status", "importance", "url", "thread", "tags", "project")
# Links in an event's text: Slack's <url|label> or <url>, then bare http(s) URLs.
_SLACK_LINK = re.compile(r"<(https?://[^|>\s]+)(?:\|[^>]*)?>", re.I)
_BARE_LINK = re.compile(r"(?<![<|\w])https?://[^\s<>|\"'`,*]+", re.I)
_SLACK_ESCAPES = (("&lt;", "<"), ("&gt;", ">"), ("&amp;", "&"))  # Slack escapes these three in message text
LINKS_MAX, SUMMARY_MAX, REASON_MAX = 20, 200, 120
# Signals (issue #239): one word in the analysis for an own post the PM update names. Never the post's text.
#   launch    an own channel post that announces something ("launched", "now live") or that is pinned
#   check-in  an own reply in a channel thread someone else started (an initiative the owner follows)
SIGNALS = ("launch", "check-in")
_LAUNCH = re.compile(r"\b(?:launch(?:ed|ing)|announc(?:ing|ed|ement)|(?:now|is|went|are) live|rolled out|released)\b", re.I)
BUILTIN_RULES: list[dict[str, Any]] = [
    {"name": "own-action", "when": {"actor": "me"}, "triage": "mine"},
    {"name": "page-handled", "when": {"source": "pagerduty", "actor": "me", "status": ["acknowledged", "resolved"]}, "evidence": True},
    {"name": "bot", "when": {"actor": "bot"}, "triage": "noise"},
    {"name": "page-for-me", "when": {"source": "pagerduty", "to_me": True, "status": ["triggered", "acknowledged"]}, "triage": "act"},
    {"name": "important-mail", "when": {"kind": "mail", "importance": "high"}, "triage": "act"},
    {"name": "flagged-mail", "when": {"kind": "mail", "status": "flagged"}, "triage": "act"},
    {"name": "asked-me", "when": {"kind": ["dm", "mention"], "to_me": True}, "triage": "act"},
    {"name": "review-for-me", "when": {"kind": ["review", "review-request", "review_request"], "to_me": True}, "triage": "act"},
    {"name": "default", "when": {}, "triage": "fyi", "personal": False, "evidence": False},
]
# A PR, merge request, or issue page link: the path up to its number (`/files`, `#review-9` are the same item).
_ITEM_PATH = re.compile(r"^(/.+?/(?:pulls?|pull-requests|merge_requests|issues)/\d+)(?!\d)")
_SOURCE = re.compile(r"^[a-z0-9][a-z0-9._-]{0,39}$")


class FeedError(ValueError):
    pass


def _dir() -> Path:
    return paths.sub("work", "feed", "events.jsonl").parent


def events_path() -> Path:
    return _dir() / "events.jsonl"


def originals_path() -> Path:
    return _dir() / "originals.jsonl"


def cycle_path() -> Path:
    """The sources stored in this live-feed cycle (bookkeeping: `server.CHANGE_SKIP` holds its name)."""
    return _dir() / "feed-cycle.json"


def rules_path() -> Path:
    return paths.sub("work", "feed-rules.yaml")


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
    return store.read_jsonl(events_path())


def originals() -> dict[str, dict[str, Any]]:
    return {o["id"]: o for o in store.read_jsonl(originals_path())}


# ---------- rules ----------


def _check_rule(r: Any, where: str) -> dict[str, Any]:
    fix = "(fix: careerbrew data edit work/feed-rules.yaml; `careerbrew work feed rules` shows the shape)"
    if not isinstance(r, dict):
        raise FeedError(f"{where}: a rule is a mapping with `when` and at least one of triage, personal, evidence {fix}")
    unknown = sorted(set(r) - {"name", "when", *SET_KEYS})
    if unknown:
        raise FeedError(f"{where}: unknown key(s) {', '.join(unknown)}; a rule has name, when, triage, personal, evidence {fix}")
    when = r.get("when") or {}
    if not isinstance(when, dict):
        raise FeedError(f"{where}: `when` is a mapping such as {{source: slack, channel: 'alerts-*'}} {fix}")
    for key, val in when.items():
        if key not in WHEN_KEYS:
            raise FeedError(f"{where}: unknown key {key} in when; use {', '.join(WHEN_KEYS)} {fix}")
        if key in BOOL_KEYS and not isinstance(val, bool):
            raise FeedError(f"{where}: {key} is true or false {fix}")
        if key not in BOOL_KEYS and not (isinstance(val, str) and val or isinstance(val, list) and val and all(isinstance(x, str) and x for x in val)):
            raise FeedError(f"{where}: {key} is a word or glob, or a list of them {fix}")
    if not any(k in r for k in SET_KEYS):
        raise FeedError(f"{where}: set at least one of triage, personal, evidence {fix}")
    if "triage" in r and r["triage"] not in TRIAGES:
        raise FeedError(f"{where}: triage is one of {', '.join(TRIAGES)} {fix}")
    for key in ("personal", "evidence"):
        if key in r and not isinstance(r[key], bool):
            raise FeedError(f"{where}: {key} is true or false {fix}")
    return {**r, "when": when}


def rules() -> list[dict[str, Any]]:
    """The owner's rules (validated, in file order), then the built-in rules."""
    data = store.read_yaml(rules_path(), {}) or {}
    own = data.get("rules", []) if isinstance(data, dict) else None
    if not isinstance(own, list):
        raise FeedError("work/feed-rules.yaml: put the rules in a list under `rules:` (fix: careerbrew data edit work/feed-rules.yaml)")
    out = []
    for i, raw in enumerate(own, 1):
        r = _check_rule(raw, f"work/feed-rules.yaml rule {i}")
        out.append({**r, "name": str(r.get("name") or f"rule-{i}"), "builtin": False})
    return out + [{**r, "builtin": True} for r in BUILTIN_RULES]


def _matches(when: dict[str, Any], facts: dict[str, Any]) -> bool:
    for key, want in when.items():
        have = facts.get(key)
        if isinstance(want, bool):
            if bool(have) is not want:
                return False
            continue
        pats = [want] if isinstance(want, str) else want
        if key == "text":  # any of the words, anywhere in the original text
            low = str(have or "").lower()
            if not any(p.lower() in low for p in pats):
                return False
        elif not any(fnmatch.fnmatchcase(str(have or "").lower(), p.lower()) for p in pats):
            return False
    return True


def links(text: str) -> list[str]:
    """Every distinct link in an event's text, in order: Slack's <url|label> form first, then bare URLs."""
    out: list[str] = []
    for url in [*_SLACK_LINK.findall(text or ""), *_BARE_LINK.findall(_SLACK_LINK.sub(" ", text or ""))]:
        for code, char in _SLACK_ESCAPES:
            url = url.replace(code, char)
        url = url.rstrip(".,;:!?]}")
        while url.endswith(")") and url.count(")") > url.count("("):  # keep a balanced (...) inside a URL
            url = url[:-1].rstrip(".,;:!?")
        url = url[:500]
        if url not in out:
            out.append(url)
    return out[:LINKS_MAX]


def analyze(item: dict[str, Any], rule_list: list[dict[str, Any]], origin: str, mark: bool | None = None, team_match: Callable[[str, str, str], list[str]] | None = None) -> dict[str, Any]:
    """The analysis row of one clean event: its facts, triage, personal flag, teammates, links, and the rules that decided."""
    if team_match is None:
        from . import team

        team_match = team.matcher()
    facts = {k: item.get(k) for k in FACT_KEYS}
    # A bot's text that names a teammate is not their work: for bots only the actor's own ids count.
    text = "" if item.get("actor") == "bot" else str(item.get("text") or "")
    mates = team_match(str(item.get("actor_id") or ""), str(item.get("from") or ""), text)
    decided: dict[str, Any] = {}
    by: dict[str, str] = {}
    named_team: dict[str, bool] = {}
    for r in rule_list:
        if _matches(r["when"], {**facts, "team": mates, "origin": origin, "text": item.get("text", "")}):
            for key in SET_KEYS:
                if key in r and key not in decided:
                    decided[key], by[key] = r[key], r["name"]
                    named_team[key] = "team" in r["when"]
    if mates and decided["triage"] == "noise" and not named_team["triage"]:  # a teammate's event is worth a look (issue #180)
        decided["triage"], by["triage"] = "fyi", "teammate"
    if mark is not None:
        personal, personal_by = mark, "owner"
    elif item.get("personal"):
        personal, personal_by = True, "source"
    else:
        personal = bool(decided.get("personal"))
        personal_by = f"rule:{by['personal']}" if personal else ""
    row = {**facts, "origin": origin, "triage": decided["triage"], "rule": by["triage"], "personal": personal, "personal_by": personal_by, "team": mates, "links": links(str(item.get("text") or ""))}
    row["signal"] = signal(item)
    row["evidence_rule"] = by["evidence"] if decided.get("evidence") else ""
    row["evidence"] = bool(row["evidence_rule"]) and row["triage"] == "mine" and not personal
    if mark is not None:
        row["marked"] = mark
    return row


def signal(item: dict[str, Any]) -> str:
    """`launch`, `check-in`, or "" for one clean event (issue #239): only the owner's own posts in a channel count."""
    if item.get("actor") != "me" or not item.get("channel") or item.get("kind") in ("dm", "mail", "meeting"):
        return ""
    pinned = isinstance(item.get("original"), dict) and bool(item["original"].get("pinned_to"))
    if pinned or _LAUNCH.search(str(item.get("text") or "")):
        return "launch"
    return "check-in" if item.get("thread") and item.get("thread") != item.get("id") else ""


# ---------- events in ----------


def _utc(value: Any) -> str:
    from ..calendar import local_tz

    try:
        dt = datetime.fromisoformat(str(value).strip().replace("Z", "+00:00"))
    except ValueError:
        raise FeedError(f"at {str(value)[:40]!r} is not an ISO time such as 2026-10-21T14:03:00Z") from None
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=local_tz())
    return dt.astimezone(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


def _short(value: Any, limit: int) -> str:
    return " ".join(str(value or "").split())[:limit]


def project_label(value: Any) -> str:
    """A project's title from an event or an evidence row: a string, or {title} / {name}.
    A tool may send null for either (issue #110): that is no project, never the text "None"."""
    if isinstance(value, dict):
        value = value.get("title") or value.get("name")
    return _short(value, 120) if isinstance(value, str) else ""


def clean(item: Any, where: str = "event") -> dict[str, Any]:
    """Check one event and return it in the store's shape; the error names the field and the fix."""
    if not isinstance(item, dict):
        raise FeedError(f"{where}: an event is a JSON object with id, at, title (and source, kind, actor)")
    missing = [k for k in ("id", "at", "title") if not str(item.get(k) or "").strip()]
    if missing:
        raise FeedError(f"{where}: missing {', '.join(missing)} (fix: every event needs id, at, and title)")
    source = str(item.get("source") or "plugin").strip().lower()
    if not _SOURCE.match(source):
        raise FeedError(f"{where}: source {source[:40]!r} must be a short lowercase name such as slack or pagerduty")
    actor = str(item.get("actor") or "other")
    if actor not in ACTORS:
        raise FeedError(f"{where}: actor must be one of {', '.join(ACTORS)}")
    tags = item.get("tags") or []
    if not isinstance(tags, list) or not all(isinstance(t, str) for t in tags):
        raise FeedError(f"{where}: tags is a list of words")
    out: dict[str, Any] = {
        "id": _short(item["id"], 200),
        "source": source,
        "kind": _short(item.get("kind") or "event", 40).lower(),
        "at": _utc(item["at"]),
        "title": _short(item["title"], 200),
        "actor": actor,
        "actor_id": _short(item.get("actor_id"), 120),
        "to_me": bool(item.get("to_me")),
        "url": str(item.get("url") or "")[:500],
        "thread": _short(item.get("thread"), 200),
        "tags": sorted({t[:40] for t in tags}),
        "personal": bool(item.get("personal")),
        "text": str(item.get("text") or "")[:20000],
        "original": item.get("original", {}),
    }
    for key in ("channel", "from", "status", "importance"):
        out[key] = _short(item.get(key), 120)
    out["project"] = project_label(item.get("project"))
    return out


def _prune(origs: dict[str, dict[str, Any]]) -> int:
    """Drop originals older than `feed_keep_days`; their analysis stays."""
    from ..calendar import now

    cut = (now().astimezone(UTC) - timedelta(days=int(config.get("feed_keep_days")))).strftime("%Y-%m-%dT%H:%M:%SZ")
    old = [i for i, o in origs.items() if str(o.get("at", "")) < cut]
    for i in old:
        del origs[i]
    return len(old)


def _save(rows: dict[str, dict[str, Any]], origs: dict[str, dict[str, Any]]) -> None:
    store.write_jsonl(events_path(), sorted(rows.values(), key=lambda r: (r["at"], r["id"])))
    store.write_jsonl(originals_path(), sorted(origs.values(), key=lambda o: (o["at"], o["id"])))


def _source_name(value: Any) -> str:
    src = str(value or "").strip().lower()
    if not _SOURCE.match(src):
        raise FeedError(f"source {src[:40]!r} must be a short lowercase name such as slack or jira")
    return src


def _note_stored(sources: set[str], ids: Iterable[str] = ()) -> None:
    """Add sources to this cycle's record; call it under `_locked()`. Written only on change."""
    from ..calendar import now_utc
    from . import health

    batch = store.read_json(cycle_path(), {}) or {}
    identity = health.declared()[2]
    if batch.get("identity") != identity:
        batch = {}
    have = set(batch.get("stored") or [])
    acquired = dict(batch.get("acquired") or {})
    acquired.update(dict.fromkeys(sources, now_utc().isoformat()))
    store.write_json_if_changed(
        cycle_path(), {**batch, "identity": identity, "stored": sorted(have | sources), "acquired": acquired, "ids": sorted({*batch.get("ids", []), *ids}), "failed_sources": sorted(set(batch.get("failed_sources", [])) - sources)}
    )


def ingest(items: list[dict[str, Any]], origin: str = "user", source_kind: str = "feed", sources: Iterable[str] = ()) -> dict[str, int]:
    """Store events: the original apart from its analysis. Idempotent by id; a changed event (a page resolved) is updated.

    Each event's source, and each of `sources` (a source pulled with no events), counts as stored this cycle."""
    from . import team

    rule_list, mates = rules(), team.matcher()
    cleaned = [clean(it, f"event {n}") for n, it in enumerate(items, 1)]
    stored_now = {it["source"] for it in cleaned} | {_source_name(s) for s in sources}
    new = updated = 0
    fresh: list[tuple[dict[str, Any], str]] = []  # new events, for promises the owner made (issue #268)
    with _locked():
        if stored_now:
            _note_stored(stored_now, (it["id"] for it in cleaned))
        rows = {r["id"]: r for r in load()}
        origs = originals()
        for it in cleaned:
            old = rows.get(it["id"])
            row = _keep_marks(_keep_research(analyze(it, rule_list, origin, mark=(old or {}).get("marked"), team_match=mates), old), old)
            orig = {"id": it["id"], "origin": origin, "at": it["at"], "text": it["text"], "original": it["original"]}
            if old == row and origs.get(it["id"]) == orig:
                continue
            if old is None:
                fresh.append((row, it["text"]))
            new, updated = (new + 1, updated) if old is None else (new, updated + 1)
            rows[it["id"]], origs[it["id"]] = row, orig
        pruned = _prune(origs)
        if new or updated or pruned:
            _save(rows, origs)
    events.emit("work.feed.ingest", status="ok", source_kind=source_kind, new=new, updated=updated, pruned=pruned)
    from . import promises  # local import: promises reads this module

    promises.track_safely(fresh)
    return {"new": new, "updated": updated, "pruned": pruned}


def mark_cycle(required: Iterable[str] | None = None) -> dict[str, Any]:
    """Complete current declared coverage; extra required sources are additive.

    `not_stored` reports missing or invalid acquisition coverage, including an
    unknown clock. Physical `stored` counts remain separate. Clear the batch.
    """
    from . import health, live

    _sources, declared, _identity = health.declared()
    wanted = [*declared, *(required or [])]
    req = list(dict.fromkeys(_source_name(s) for s in wanted))
    if not req:
        raise FeedError("no required sources (fix: --require slack,jira,mail, or careerbrew config set feed_tool_sources)")
    with live.cycle_lock():
        with _locked():
            batch = store.read_json(cycle_path(), {}) or {}
            stored = sorted(set(batch.get("stored") or []))
            if batch:
                store.write_json_if_changed(cycle_path(), {"stored": []})
        health.complete(req, batch)
        assessment = health.snapshot()
    gaps = {source["name"] for source in assessment["sources"] if source["required"] and source["health"] != "healthy"} if declared else set()
    not_stored = [s for s in req if s not in stored or (declared and (s in batch.get("failed_sources", []) or batch.get("errors"))) or s in gaps]
    events.emit("work.feed.cycle", status="gap" if not_stored else "ok", required=len(req), stored=len(stored), not_stored=len(not_stored), story="us-61")
    return {"required": req, "stored": stored, "not_stored": not_stored}


# A raw tool reply's list key, and the source its events take when none is given.
RAW_LISTS = {"tickets": "jira", "messages": "mail", "events": "calendar"}


def _field(obj: dict[str, Any], *keys: str) -> Any:
    """The first non-empty value of `keys` in the item, then in its `fields` object (a ticket search's shape)."""
    sub = obj["fields"] if isinstance(obj.get("fields"), dict) else {}
    return next((v for k in keys for v in (obj.get(k), sub.get(k)) if v not in (None, "", [], {})), None)


def _name(value: Any) -> str:
    """A status, project, or sender as text: a string, or an object's name (`{"name": ...}`, `{"emailAddress": {...}}`)."""
    if isinstance(value, dict):
        inner = value["emailAddress"] if isinstance(value.get("emailAddress"), dict) else value
        value = inner.get("name") or inner.get("address") or inner.get("email") or ""
    return value.strip() if isinstance(value, str) else ""


def _when(value: Any) -> Any:
    try:
        return datetime.fromtimestamp(float(value), UTC).strftime("%Y-%m-%dT%H:%M:%SZ")  # an epoch time such as a chat ts
    except (TypeError, ValueError, OverflowError, OSError):
        return value


def _ticket(obj: dict[str, Any], source: str) -> dict[str, Any]:
    key, updated, status = _field(obj, "key", "id"), _field(obj, "updated", "updated_at", "updatedAt"), _name(_field(obj, "status"))
    summary, text = _field(obj, "summary", "title"), _field(obj, "description", "text")
    return {
        "id": f"{source}:" + ":".join(str(x) for x in (key, updated, status) if x) if key and updated else "",
        "source": source,
        "kind": "ticket",
        "at": updated,
        "title": f"{key} {summary}" if key and summary else summary,
        "status": status,
        "url": _field(obj, "url", "browse_url", "link"),
        "thread": f"{source}:{key}" if key else "",
        "project": _field(obj, "project"),
        "to_me": bool(obj.get("to_me")),
        "text": text if isinstance(text, str) else "",
        "original": obj,
    }


def _message(obj: dict[str, Any], source: str) -> dict[str, Any]:
    ident = _field(obj, "id", "messageId", "message_id", "internetMessageId", "key", "uuid", "ts")
    body = obj.get("body")
    texts = (obj.get("text"), body.get("content") if isinstance(body, dict) else body, obj.get("bodyPreview"), obj.get("snippet"))
    text = next((t for t in texts if isinstance(t, str) and t.strip()), "")
    title = _field(obj, "title", "subject", "summary") or (text.strip().splitlines()[0] if text.strip() else "(no subject)")
    flag = obj["flag"] if isinstance(obj.get("flag"), dict) else {}
    item: dict[str, Any] = {k: obj[k] for k in ("kind", "actor", "actor_id", "to_me", "channel", "importance", "personal", "tags", "project") if k in obj}
    if item.get("actor") not in ACTORS:
        item.pop("actor", None)
    item.update(
        id=f"{source}:{ident}" if ident not in (None, "") else "",
        source=source,
        kind=item.get("kind") or "mail",
        at=_when(_field(obj, "at", "receivedDateTime", "received", "sentDateTime", "date", "timestamp", "created_at", "createdAt", "created", "time", "ts")),
        title=str(title).splitlines()[0],
        url=_field(obj, "url", "webLink", "link"),
        thread=_field(obj, "thread", "conversationId", "threadId"),
        status="flagged" if flag.get("flagStatus") == "flagged" or obj.get("flagged") is True else _name(obj.get("status")),
        text=text,
        original=obj,
    )
    item["from"] = _name(obj.get("from"))
    return item


def _meeting(obj: dict[str, Any], source: str) -> dict[str, Any]:
    """A calendar event. Its id is the event id plus its start; a record missing either has no stable id (issue #316)."""
    start = obj.get("start")
    at = _field(start, "dateTime", "date") if isinstance(start, dict) else start
    ident = _field(obj, "id", "eventId", "iCalUID")
    return {
        "id": f"{source}:{ident}:{at}" if ident and at else "",
        "source": source,
        "kind": "meeting",
        "at": at,
        "title": str(_field(obj, "summary", "title", "subject") or "(no title)").splitlines()[0],
        "url": _field(obj, "htmlLink", "url", "webLink"),
        "text": obj["description"] if isinstance(obj.get("description"), str) else "",
        "original": obj,
    }


def raw_events(data: Any, source: str = "") -> tuple[list[dict[str, Any]], int, str] | None:
    """Events from a tool reply saved as returned: (events, items skipped, source), or None when it is no raw reply.

    A raw reply is an object with a `tickets` list (a ticket search), a `messages` list (a mail search), or an
    `events` list (a calendar search), and
    not itself an event. An item with no id, time, or title is skipped, not fatal."""
    if not isinstance(data, dict) or all(data.get(k) for k in ("id", "at", "title")):
        return None
    for key, default in RAW_LISTS.items():
        items = data.get(key)
        if not isinstance(items, list):
            continue
        if key == "events" and items and not any(isinstance(o, dict) and "start" in o for o in items):
            continue  # another tool's `events` list (a pager's), not a calendar reply: the generic reader keeps it
        src = _source_name(source or default)
        convert = {"tickets": _ticket, "events": _meeting}.get(key, _message)
        out, skipped = [], 0
        for obj in items:
            ev = convert(obj, src) if isinstance(obj, dict) else None
            try:
                if not ev or not ev["id"]:
                    raise FeedError("no id")
                clean(ev)
            except FeedError:
                skipped += 1
                continue
            out.append(ev)
        return out, skipped, src
    return None


def note_failure(source: str = "") -> None:
    """Invalidate the pending batch without saving input or exception text."""
    if not isinstance(source, str) or not _SOURCE.fullmatch(source):
        return  # an unbound rejected input cannot invent an error for a declared provider
    with _locked():
        batch = store.read_json(cycle_path(), {}) or {}
        store.write_json_if_changed(cycle_path(), {**batch, "failed_sources": sorted({*batch.get("failed_sources", []), source})})


def add_lines(text: str, plugin: str = "", source: str = "") -> dict[str, int]:
    try:
        return _add_lines(text, plugin, source)
    except FeedError:
        note_failure(source)
        raise


def _add_lines(text: str, plugin: str = "", source: str = "") -> dict[str, int]:
    """Events from JSON lines (a plugin's feed, or the owner's script), or one raw tool reply (`raw_events`). One bad line writes nothing."""
    from .. import plugins

    origin = "user"
    if plugin:
        if plugin not in {p["name"] for p in plugins.installed()}:
            raise FeedError(f"plugin {plugin} is not installed (see `careerbrew plugin list`)")
        origin = f"plugin:{plugin}"
    try:
        raw = raw_events(json.loads(text), source)
    except json.JSONDecodeError:
        raw = None
    if raw is not None:
        evs, skipped, src = raw
        if plugin:
            evs = [ev if ev["id"].startswith(f"{plugin}:") else {**ev, "id": f"{plugin}:{ev['id']}"} for ev in evs]
        return {**ingest(evs, origin, source_kind="plugin" if plugin else "user", sources=[src]), "skipped": skipped}
    items = []
    for n, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            raise FeedError(f"line {n}: not JSON (fix: one JSON object per line with id, at, and title)") from None
        it = clean(item, f"line {n}")
        if plugin and not it["id"].startswith(f"{plugin}:"):
            item = {**item, "id": f"{plugin}:{it['id']}"}  # a plugin's ids never collide with a connector's
        item.setdefault("original", {k: v for k, v in item.items() if k != "original"})
        items.append(item)
    if not items:
        raise FeedError("no events in the input (fix: one JSON object per line with id, at, and title)")
    return {**ingest(items, origin, source_kind="plugin" if plugin else "user"), "skipped": 0}


def pull() -> dict[str, Any]:
    """Read every feed connector (Slack, PagerDuty, Outlook) into the store. A broken source is reported, not fatal."""
    from .. import connectors, privacy
    from . import evidence

    stats: dict[str, Any] = {"sources": 0, "new": 0, "updated": 0, "errors": 0, "reasons": []}
    for cfg in evidence.sources_list():
        kind = str(cfg.get("kind", ""))
        if kind not in connectors.FEED_READERS:
            continue
        stats["sources"] += 1
        try:
            r = ingest(connectors.feed(cfg), str(cfg.get("origin", "user")), source_kind=kind, sources=[kind])
            stats["new"] += r["new"]
            stats["updated"] += r["updated"]
        except (RuntimeError, OSError, KeyError, ValueError, TypeError) as exc:  # ConnectorError and SecretError are RuntimeErrors
            note_failure(kind)
            stats["errors"] += 1
            stats["reasons"].append(f"{kind}: {privacy.redact(str(exc))[:200]}")
            events.emit("work.feed.error", source_kind=kind, error=type(exc).__name__, story="us-61")
    events.emit("work.feed.pull", status="ok", sources=stats["sources"], new=stats["new"], errors=stats["errors"])
    return stats


def retriage() -> dict[str, int]:
    """Run the rules again over every event whose original is still kept (after a rules or roster change)."""
    from . import team

    rule_list, mates = rules(), team.matcher()
    seen = changed = 0
    with _locked():
        rows = {r["id"]: r for r in load()}
        origs = originals()
        for i, o in origs.items():
            old = rows.get(i)
            if old is None:
                continue
            seen += 1
            item = {**{k: old.get(k) for k in FACT_KEYS}, "text": o.get("text", ""), "personal": old.get("personal_by") == "source"}
            row = _keep_marks(_keep_research(analyze(item, rule_list, str(old.get("origin", "user")), mark=old.get("marked"), team_match=mates), old), old)
            if row != old:
                rows[i] = row
                changed += 1
        if changed:
            _save(rows, origs)
    events.emit("work.feed.retriage", status="ok", count=seen, changed=changed)
    return {"events": seen, "changed": changed}


def _keep_research(row: dict[str, Any], old: dict[str, Any] | None) -> dict[str, Any]:
    """A new analysis keeps the research already done on the links it still has."""
    done = [r for r in (old or {}).get("research") or [] if r.get("url") in row["links"]]
    if done:
        row["research"] = done
    return row


ASK_MARKS = ("not-ask", "done")


def _keep_marks(row: dict[str, Any], old: dict[str, Any] | None) -> dict[str, Any]:
    """A new analysis keeps the owner's ask mark (not an ask, done): their call wins over the next pull."""
    if old and old.get("ask_mark"):
        row["ask_mark"] = old["ask_mark"]
        if old.get("ask_marked_at"):
            row["ask_marked_at"] = old["ask_marked_at"]
    return row


def mark_ask(event_id: str, value: str) -> dict[str, Any]:
    """The owner's call on an ask (issue #185): `not-ask` (it never asked them) or `done` (handled elsewhere) close it;
    `ask` opens it again. Not-ask marks from one channel or sender later propose a feed rule (`signals`)."""
    if value not in (*ASK_MARKS, "ask"):
        raise FeedError(f"an ask is marked {', '.join(ASK_MARKS)}, or ask to open it again")
    with _locked():
        rows = {r["id"]: r for r in load()}
        row = rows.get(event_id)
        if row is None:
            raise FeedError(f"no feed event {event_id} (see: careerbrew work feed show --show)")
        if value == "ask":
            row.pop("ask_mark", None)
            row.pop("ask_marked_at", None)
        else:
            row["ask_mark"] = value
            row["ask_marked_at"] = _utc(_now_iso())
        store.write_jsonl(events_path(), sorted(rows.values(), key=lambda r: (r["at"], r["id"])))
    if value == "not-ask":
        events.emit("work.feed.mark", status="ok", action="not-ask")
    elif value == "done":
        events.emit("work.feed.mark", status="ok", action="done")
    else:
        events.emit("work.feed.mark", status="ok", action="ask")
    return row


def _now_iso() -> str:
    from ..calendar import now

    return now().isoformat(timespec="seconds")


def needs_research(row: dict[str, Any]) -> list[str]:
    """The links of a kept event (not noise, not personal) with no research entry yet, or only a reason the
    live cycle stored (`by: cycle`, issue #266): a placeholder the agent may still replace with its own tools."""
    if row.get("triage") == "noise" or row.get("personal"):
        return []
    done = {r.get("url") for r in row.get("research") or [] if not (r.get("by") == "cycle" and r.get("opened") is False)}
    return [u for u in row.get("links") or [] if u not in done]


def cycle_research(event_id: str, entries: list[dict[str, Any]]) -> dict[str, Any]:
    """Store what the live cycle found for some links of one event (issue #266), each marked `by: cycle`. Unlike
    `research`, it need not cover every link; an entry the agent stored (no `by: cycle`) is kept over the cycle's."""
    with _locked():
        rows = {r["id"]: r for r in load()}
        row = rows.get(event_id)
        if row is None:
            raise FeedError(f"no feed event {event_id}")
        have = row.get("links") or []
        kept = {e["url"]: e for e in row.get("research") or []}
        for e in entries:
            if e.get("url") in have and (e["url"] not in kept or kept[e["url"]].get("by") == "cycle"):
                kept[e["url"]] = {**e, "by": "cycle"}
        row["research"] = [kept[u] for u in have if u in kept]
        store.write_jsonl(events_path(), sorted(rows.values(), key=lambda r: (r["at"], r["id"])))
    events.emit("work.feed.research", status="ok", count=len(entries), opened=sum(bool(e.get("opened")) for e in entries))
    return row


def research(event_id: str, entries: Any) -> dict[str, Any]:
    """Store what each link of one event says. Refused unless it covers every link: a summary, or opened: false and why."""
    if not isinstance(entries, list) or not entries:
        raise FeedError("research is a JSON list: one {url, summary} or {url, opened: false, reason} per link")
    with _locked():
        rows = {r["id"]: r for r in load()}
        row = rows.get(event_id)
        if row is None:
            raise FeedError(f"no feed event {event_id} (see: careerbrew work feed links)")
        if row.get("triage") == "noise" or row.get("personal"):
            raise FeedError(f"{event_id} is {'personal' if row.get('personal') else 'noise'}: only kept events are researched (see: careerbrew work feed links --pending)")
        have = row.get("links") or []
        out: list[dict[str, Any]] = []
        for n, e in enumerate(entries, 1):
            if not isinstance(e, dict) or str(e.get("url") or "") not in have:
                raise FeedError(f"entry {n}: url must be one of this event's links (see: careerbrew work feed links)")
            if e["url"] in {x["url"] for x in out}:
                raise FeedError(f"entry {n}: the same link twice; one entry per link")
            if e.get("opened", True) is False:
                reason = " ".join(str(e.get("reason") or "").split())
                if not reason or len(reason) > REASON_MAX:
                    raise FeedError(f"entry {n}: opened: false needs a reason of at most {REASON_MAX} characters (a sign-in page, say)")
                out.append({"url": e["url"], "opened": False, "reason": reason})
            else:
                summary = str(e.get("summary") or "")
                if not summary.strip() or "\n" in summary.strip() or len(summary) > SUMMARY_MAX:
                    raise FeedError(f"entry {n}: summary is one line of at most {SUMMARY_MAX} characters, or set opened: false with a reason")
                out.append({"url": e["url"], "opened": True, "summary": " ".join(summary.split())})
        missing = [n for n, u in enumerate(have, 1) if u not in {e["url"] for e in out}]
        if missing:  # positions, never the links: an error reaches an agent's stdout unfiltered
            raise FeedError(f"refused: {len(missing)} link(s) not researched (link {', '.join(map(str, missing))} of {len(have)}; see: careerbrew work feed links --pending) (fix: add a summary, or opened: false with a reason, for each)")
        row["research"] = out
        store.write_jsonl(events_path(), sorted(rows.values(), key=lambda r: (r["at"], r["id"])))
    events.emit("work.feed.research", status="ok", count=len(out), opened=sum(e["opened"] for e in out))
    return row


def mark(event_id: str, personal: bool) -> dict[str, Any]:
    """The owner's call on one event; it wins over the source and the rules. Personal drops it from evidence too."""
    from . import evidence

    with _locked():
        rows = {r["id"]: r for r in load()}
        row = rows.get(event_id)
        if row is None:
            raise FeedError(f"no feed event {event_id} (see: careerbrew work feed show --show)")
        row.update(marked=personal, personal=personal, personal_by="owner")
        row["evidence"] = bool(row.get("evidence_rule")) and row["triage"] == "mine" and not personal
        store.write_jsonl(events_path(), sorted(rows.values(), key=lambda r: (r["at"], r["id"])))
    dropped = evidence.forget({event_id, *aliases(event_id)}) if personal else 0
    memory_dropped = 0
    if personal:
        from . import memory_sync  # local import: memory_sync reads this module

        memory_dropped = memory_sync.forget({event_id, *aliases(event_id)})
    events.emit("work.feed.mark", status="ok", personal=personal, dropped=dropped)
    return {**row, "dropped": dropped, "memory_dropped": memory_dropped}


# ---------- events out ----------


_SLACK_ID = re.compile(r"^slack:[^:]+:(\d+\.\d+)$")


def aliases(event_id: str) -> set[str]:
    """The ids one event has elsewhere: a Slack message is `slack:<ts>` as recognition evidence."""
    m = _SLACK_ID.match(event_id)
    return {f"slack:{m.group(1)}"} if m else set()


def personal_ids() -> set[str]:
    """Ids of personal events and their aliases; the day log skips rows with these ids or refs."""
    out: set[str] = set()
    for r in load():
        if r.get("personal"):
            out |= {r["id"], *aliases(r["id"])}
    return out


def as_evidence(row: dict[str, Any]) -> dict[str, Any]:
    from .evidence import tags_for

    kind = "pagerduty" if row.get("source") == "pagerduty" else "feed"
    tags = sorted(set(row.get("tags") or []) | set(tags_for(str(row.get("title", "")))))
    return {"id": row["id"], "kind": kind, "title": row["title"], "at": row["at"], "url": row.get("url", ""), "tags": tags, "origin": row.get("origin", "user"), "feed_source": row["source"]}


def evidence_items() -> list[dict[str, Any]]:
    """Your own non-personal actions that a rule marks as evidence."""
    return [as_evidence(r) for r in load() if r.get("evidence") and not r.get("personal")]


def _marks() -> dict[str, bool]:
    return {r["id"]: r["marked"] for r in load() if "marked" in r}


def evidence_from(items: list[dict[str, Any]], origin: str) -> list[dict[str, Any]]:
    """Evidence from freshly read events without storing them (a connector's `work collect` path); same rules and marks."""
    from . import team

    rule_list, marks, mates = rules(), _marks(), team.matcher()
    rows = [analyze(it, rule_list, origin, mark=marks.get(it["id"]), team_match=mates) for it in (clean(x) for x in items)]
    return [as_evidence(r) for r in rows if r["evidence"]]


def personal_check(origin: str = "user") -> Callable[[dict[str, Any]], bool]:
    """A test for events read outside the store (Slack recognition): kept out as personal by the owner's mark, the source, or a rule?"""
    from . import team

    rule_list, marks, mates = rules(), _marks(), team.matcher()
    return lambda item: bool(analyze(clean(item), rule_list, origin, mark=marks.get(str(item.get("id"))), team_match=mates)["personal"])


def thread_key(row: dict[str, Any], es: list[dict[str, Any]] | None = None) -> str:
    """The event's chat thread; with none, its PR, merge request, issue, or ticket link (issue #228); else ""."""
    from . import project_priority

    if row.get("thread"):
        return str(row["thread"])
    url = str(row.get("url") or "")
    try:
        p = urlsplit(url)
    except ValueError:
        return ""
    if p.scheme.lower() not in ("http", "https") or not p.hostname:
        return ""
    host = p.hostname.lower()
    if m := _ITEM_PATH.match(p.path):
        return f"link:{host}{m.group(1)}"
    ticket = project_priority.ticket_pattern(es, [url])
    for part in p.path.split("/"):
        if ticket.fullmatch(part):  # a whole path part (/browse/PROJ-7), never a key inside a slug
            return f"ticket:{host}:{part}"
    return ""


def open_asks(rows: list[dict[str, Any]] | None = None, include_personal: bool = False) -> list[dict[str, Any]]:
    """Asks with a timestamp and no later event of yours in the same thread, PR, or ticket."""
    from . import project_priority

    rows = [row for row in (load() if rows is None else rows) if isinstance(row.get("at"), str)]
    es = project_priority.entries()
    keys = {r["id"]: thread_key(r, es) for r in rows}
    answered: dict[str, str] = {}
    for r in rows:
        if r.get("actor") == "me" and (k := keys[r["id"]]):
            answered[k] = max(answered.get(k, ""), r["at"])
    return [r for r in rows if r["triage"] == "act" and not r.get("ask_mark") and (include_personal or not r.get("personal")) and not (keys[r["id"]] and answered.get(keys[r["id"]], "") >= r["at"])]


def _closure_instant(value: Any) -> datetime | None:
    if not isinstance(value, str):
        return None
    try:
        instant = datetime.fromisoformat(value)
        return instant.astimezone(UTC) if instant.utcoffset() is not None else None
    except (ValueError, OverflowError):
        return None


def closed_today(rows: list[dict[str, Any]], at: datetime, timezone: tzinfo) -> int:
    """Count each ask on the local date of its first qualifying closure."""
    from . import project_priority

    cutoff = at.astimezone(UTC)
    today = at.astimezone(timezone).date()
    entries = project_priority.entries()
    replies: dict[str, list[tuple[datetime, str]]] = {}
    for row in rows:
        if row.get("actor") != "me" or row.get("personal"):
            continue
        instant, event_id = _closure_instant(row.get("at")), row.get("id")
        if instant is not None and instant <= cutoff and isinstance(event_id, str) and (key := thread_key(row, entries)):
            replies.setdefault(key, []).append((instant, event_id))
    for thread_replies in replies.values():
        thread_replies.sort()

    counted: set[str] = set()
    for row in rows:
        mark, event_id = row.get("ask_mark"), row.get("id")
        if row.get("triage") != "act" or row.get("personal") or mark == "not-ask" or not isinstance(event_id, str):
            continue
        ask_at = _closure_instant(row.get("at"))
        if ask_at is None or ask_at > cutoff:
            continue
        done = _closure_instant(row.get("ask_marked_at")) if mark == "done" else None
        first = done if done is not None and ask_at <= done <= cutoff else None
        thread_replies = replies.get(thread_key(row, entries), [])
        # Equal stored instants qualify, as in the open-list matcher.
        index = bisect_left(thread_replies, (ask_at, ""))
        while index < len(thread_replies):
            reply_at, reply_id = thread_replies[index]
            if reply_id != event_id:
                first = min(first, reply_at) if first is not None else reply_at
                break
            index += 1
        if first is not None:
            try:
                if first.astimezone(timezone).date() == today:
                    counted.add(event_id)
            except OverflowError:
                continue
    return len(counted)


def waiting_minutes(row: dict[str, Any], at: datetime) -> int:
    """Whole minutes from the event until `at` (pass calendar.now(), the pinned clock); never negative."""
    return max(0, int((at - datetime.fromisoformat(str(row["at"]).replace("Z", "+00:00"))).total_seconds() // 60))


WorkHours = tuple[time, time, frozenset[int], tzinfo]


def work_hours() -> WorkHours:
    """The workday from the settings: `work_start`, `work_end`, `work_days` (Monday=0), in the local time zone. Read it once per view."""
    from ..calendar import local_tz

    start, end = (time.fromisoformat(str(config.get(k))) for k in ("work_start", "work_end"))
    return start, end, frozenset(int(d) for d in config.get("work_days")), local_tz()


def work_minutes(row: dict[str, Any], at: datetime, hours: WorkHours) -> int:
    """Whole work minutes from the event until `at`: only time from work_start to work_end on work_days counts; never negative."""
    start, end, days, tz = hours
    lo, hi = datetime.fromisoformat(str(row["at"]).replace("Z", "+00:00")).astimezone(tz), at.astimezone(tz)
    seconds, day = 0.0, lo.date()
    while day <= hi.date():
        if day.weekday() in days:
            a, b = max(lo, datetime.combine(day, start, tz)), min(hi, datetime.combine(day, end, tz))
            seconds += max(0.0, b.timestamp() - a.timestamp())  # timestamps, so a daylight-saving change is counted right
        day += timedelta(days=1)
    return int(seconds // 60)


def summary(rows: list[dict[str, Any]] | None = None) -> dict[str, Any]:
    from ..calendar import now

    rows = load() if rows is None else rows
    work = [r for r in rows if not r.get("personal")]
    by_triage = {t: sum(1 for r in work if r["triage"] == t) for t in TRIAGES}
    by_source: dict[str, int] = {}
    for r in work:
        by_source[r["source"]] = by_source.get(r["source"], 0) + 1
    asks, hours, at, workday = open_asks(rows), int(config.get("feed_ask_overdue_hours")), now(), work_hours()
    overdue = sum(1 for r in asks if work_minutes(r, at, workday) > hours * 60)
    return {
        "events": len(rows),
        "personal": len(rows) - len(work),
        "triage": by_triage,
        "sources": by_source,
        "open_asks": len(asks),
        "open_asks_overdue": overdue,
        "overdue_hours": hours,
        "evidence": sum(1 for r in work if r.get("evidence")),
    }
