"""The standup report: what you did on each row you report on, since the last standup (issue #110).

Window: from the last standup to now. The settings `standup_days` (weekdays, Monday=0),
`standup_time` (HH:MM), `standup_tz` (empty: the `timezone` setting), and `standup_minutes` say
when standups are. A standup is past once it has ended, so a run during the standup still counts
from the one before, and a Monday run counts from the last standup day of the week before.

Rows: a plugin or an agent passes the rows to report on with `work standup --rows <file|->`: a
JSON or YAML list, or `{rows: [...], groups: [...], ticket_projects: [...]}`. A row has a title
and may have status, owner, keys (ticket keys), project, repos (repo aliases), aliases (words),
url, and mine. Rows you own (`mine: true`, owner `me`, or an owner in `standup_me`) are marked yours.
A row shows only when it has an update in the window: your work matched it, an agent passed an action
for it, an agent wrote its talking point, or (with proofs) its live check passed (issue #271). A row with no update is hidden, yours too and
one already done before the window, and one count line says how many. The last rows passed are kept,
encrypted, in `work/standup.json`, so the web app and the next run use them. An empty list (`[]`) is
allowed: it clears the rows, and the page says no row passed the proof check.

Your work: evidence (commits, PRs, notes, tickets, plugin rows, handled pages) and your own
work-feed events (not chat, mail, or meetings) inside the window; never a personal one. An evidence
time with no offset is read in the standup zone, never dropped. Copies of one action (the same PR as
a PR, its squash commit, and a feed event) become one line that keeps the repo and link of any copy.
A shared title alone never merges two items from one source, or two items with different links.

Matching, first hit wins, level by level across all rows: project, ticket key, repo alias, then a
whole-word alias. Ticket keys are read only for configured projects (the prefixes of the rows' keys,
the groups' keys, and `ticket_projects`), so `UTF-8` is never a key. A group with `never_rows: true`
(your tooling repos) takes its work before any row can; the other groups take work no row matched;
the rest is "Other work".

Agent notes (issue #229): an agent passes talking points (`--talk`, `{row title: text}`, up to 600
characters each) and actions careerbrew does not collect (`--extra`, `[{title, at, kind, url?, row?}]`:
Slack threads, meetings, mail). A talking point replaces the row's generated line, and the row says it
was written by an agent. An extra action joins the evidence of the row it names (or is matched like other
work) and counts as new work. Both are kept, encrypted, in `work/standup-notes.json` for the current
standup window only: a report in a new window ignores them, and a run deletes them.

Proofs (issue #271): an agent passes `--proof` (`{row title: {verdict, proofs: [{source, url, checked_at,
quote}], reason}}`), the result of a live check made right before the build. With proofs passed, a row shows
only when its verdict is `pass`, it has at least one proof, every proof has a source, an http(s) URL, and a
quote, and every `checked_at` is after the window start, no older than `standup_proof_minutes` before the
build, and not in the future. The gate runs at every build, so a proof goes stale as time passes. A failed
row stays off the page and the view; the command prints its row number and reasons (its title and the
agent's reason only with `--show`). Proofs are kept with the agent notes, for the current window only.

Output: `out/standup.html`, one readable page per run (plaintext by design, like every report in
`out/`): per row a talking point first, then its status, then the evidence list. The CLI prints
counts and the page path; titles print only with `--show` to your terminal. The web app's Standup
view shows the same report (`GET /api/standup`).

Lifecycle: nothing is kept in process memory between calls; the rows file and the page are the state.
"""

from __future__ import annotations

import html
import json
import re
from collections.abc import Iterable
from datetime import datetime, time, timedelta
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo

import yaml

from .. import calendar, config, events, paths, store
from ..day import log as daylog
from . import evidence, feed, project_priority

OTHER = "Other work"
LEVELS = ("project", "key", "repo", "alias")  # the matching order: the first level with a hit wins
# Conversations and calendar entries are not actions you report; the work they lead to is.
FEED_SKIP_KINDS = frozenset({"dm", "mention", "message", "mail", "meeting"})
NOUN = {"pr": "PR", "github": "PR", "commit": "commit", "jira": "ticket", "note": "note", "pagerduty": "incident", "gdocs": "doc", "zoom": "meeting", "outlook": "meeting", "slack": "shout-out", "feed": "event"}
NOUN_ORDER = ("PR", "commit", "ticket", "note", "incident", "doc", "meeting", "thread", "email", "shout-out", "event", "action", "item")
# The kind of an action an agent passes (issue #229): a source careerbrew does not collect.
EXTRA_NOUN = {"slack": "thread", "chat": "thread", "thread": "thread", "message": "thread", "meeting": "meeting", "call": "meeting", "mail": "email", "email": "email", "doc": "doc"}
TALK_MAX = 200
MAX_TALK_BULLETS = 5
MAX_EXTRA = 200
MAX_PROOFS = 20  # proofs per row
FUTURE_SKEW = timedelta(minutes=5)  # a checked_at later than the build by more than this is not a real check
VERDICTS = ("pass", "fail")
_LEAD = {"pr": 0, "github": 0, "commit": 1}  # the copy whose title leads a merged line: a PR's title is the cleanest
MAX_ROWS = 300
_KEY = re.compile(r"^[A-Za-z][A-Za-z0-9]+-\d+$")
_PREFIX = re.compile(r"^[A-Za-z][A-Za-z0-9]+$")
_PR_ID = re.compile(r"(?:^|:)pr:[^#\s]+#(\d+)$")
_PR_URL = re.compile(r"/(?:pull|pulls|merge_requests)/(\d+)(?!\d)")
_PR_TITLE = re.compile(r"\(#(\d+)\)\s*$|\b(?:PR|pull request|MR)\s*#(\d+)\b", re.I)
FIX = 'fix: pass a JSON list of rows, each with a title, for example [{"title": "Order export", "status": "In review", "owner": "me", "keys": ["PROJ-12"]}]'


class StandupError(ValueError):
    pass


def rows_path() -> Path:
    return paths.sub("work", "standup.json")


def notes_path() -> Path:
    return paths.sub("work", "standup-notes.json")


def page_path() -> Path:
    return paths.sub("out", "standup.html")


# ---------- the window ----------


def zone() -> ZoneInfo:
    tz = str(config.get("standup_tz") or "")
    return ZoneInfo(tz) if tz else calendar.local_tz()


def label(dt: datetime) -> str:
    return dt.strftime("%a %b %d, %H:%M")


def window(now: datetime | None = None) -> dict[str, Any]:
    """From the start of the standup to compare against, to now, in the standup zone.

    standup_window previous-day (the default, issue #200): the previous standup day's standup, never today's own.
    since-last: the last standup that has ended, today's too.
    """
    tz = zone()
    end = (now or calendar.now()).astimezone(tz)
    days = {int(d) for d in config.get("standup_days") or []}
    hh, mm = (int(x) for x in str(config.get("standup_time")).split(":"))
    length = timedelta(minutes=int(config.get("standup_minutes")))
    mode = str(config.get("standup_window") or "previous-day")
    day = end.date() - timedelta(days=1) if mode == "previous-day" else end.date()
    for _ in range(15):
        if day.weekday() in days:
            start = datetime.combine(day, time(hh, mm), tz)
            if start + length <= end:
                return {
                    "start": start,
                    "end": end,
                    "tz": tz.key,
                    "since": label(start),
                    "until": label(end),
                    "days": sorted(days),
                    "time": f"{hh:02d}:{mm:02d}",
                    "minutes": int(length.total_seconds() // 60),
                    "mode": mode,
                    "compares": start.strftime("%a %b %d"),
                }
        day -= timedelta(days=1)
    raise StandupError("no standup day is set (fix: careerbrew config set standup_days 0,1,2,3,4)")


def parse_at(value: Any, tz: ZoneInfo) -> datetime | None:
    """An evidence time; one with no offset is in the standup zone (issue #110). An unreadable time is None."""
    text = str(value or "").strip().replace("Z", "+00:00")
    if not text:
        return None
    try:
        dt = datetime.fromisoformat(text)
    except ValueError:
        return None
    return dt if dt.tzinfo else dt.replace(tzinfo=tz)


# ---------- ticket keys ----------


def key_pattern(prefixes: Iterable[str]) -> re.Pattern[str] | None:
    """Ticket keys of the configured projects only; a generic `[A-Z]+-\\d+` would read `UTF-8` as a key."""
    alt = "|".join(sorted({re.escape(p.upper()) for p in prefixes if p}, key=len, reverse=True))
    return re.compile(rf"(?<![A-Za-z0-9])(?:{alt})-\d+(?![A-Za-z0-9])", re.I) if alt else None


def ticket_keys(text: str, pattern: re.Pattern[str] | None) -> list[str]:
    return [] if pattern is None else list(dict.fromkeys(m.group(0).upper() for m in pattern.finditer(text or "")))


def prefixes(saved: dict[str, Any]) -> set[str]:
    out = set(saved.get("ticket_projects") or [])
    for t in [*(saved.get("rows") or []), *(saved.get("groups") or [])]:
        out |= {k.split("-", 1)[0] for k in t.get("keys", [])}
    return out


# ---------- rows in ----------


def _text(value: Any, limit: int) -> str:
    return "" if value is None else " ".join(str(value).split())[:limit]


def _words(value: Any, where: str, what: str) -> list[str]:
    if value is None:
        return []
    if isinstance(value, str):
        value = [value]
    if not isinstance(value, list) or not all(isinstance(x, str) for x in value):
        raise StandupError(f"{where}: {what} is a word or a list of words")
    return list(dict.fromkeys(" ".join(x.split())[:120] for x in value if x.strip()))


def _keys(r: dict[str, Any], where: str) -> list[str]:
    keys = _words(r.get("keys"), where, "keys") + _words(r.get("key"), where, "key")
    bad = [k for k in keys if not _KEY.match(k)]
    if bad:
        raise StandupError(f"{where}: key {bad[0][:40]!r} is not a ticket key such as PROJ-12")
    return list(dict.fromkeys(k.upper() for k in keys))


def _matchers(r: dict[str, Any], where: str) -> dict[str, Any]:
    return {"project": feed.project_label(r.get("project")), "keys": _keys(r, where), "repos": _words(r.get("repos"), where, "repos") + _words(r.get("repo"), where, "repo"), "aliases": _words(r.get("aliases"), where, "aliases")}


def _row(r: Any, where: str) -> dict[str, Any]:
    if not isinstance(r, dict):
        raise StandupError(f"{where}: a row is a mapping with a title (and status, owner, keys, project, repos, aliases)")
    title = _text(r.get("title"), 200)
    if not title:
        raise StandupError(f"{where}: missing title (fix: every row needs a title)")
    mine = r.get("mine", False)
    if not isinstance(mine, bool):
        raise StandupError(f"{where}: mine is true or false")
    url = _text(r.get("url"), 500)
    return {"title": title, "status": _text(r.get("status"), 60), "owner": _text(r.get("owner"), 120), "mine": mine, "url": url if url.startswith(("http://", "https://")) else "", **_matchers(r, where)}


def _group(g: Any, where: str, seen: set[str]) -> dict[str, Any]:
    if not isinstance(g, dict):
        raise StandupError(f"{where}: a group is a mapping with a label and at least one of project, keys, repos, aliases")
    name = _text(g.get("label"), 60)
    if not name:
        raise StandupError(f"{where}: missing label (fix: every group needs a label, such as Tooling)")
    if name.lower() in seen:
        raise StandupError(f"{where}: the label {name} is taken ({OTHER} is kept for work that nothing matched)")
    seen.add(name.lower())
    never = g.get("never_rows", False)
    if not isinstance(never, bool):
        raise StandupError(f"{where}: never_rows is true or false")
    m = _matchers(g, where)
    if not any(m.values()):
        raise StandupError(f"{where}: a group needs at least one of project, keys, repos, aliases (fix: for example {{label: Tooling, repos: [agentbrew], never_rows: true}})")
    return {"label": name, "never_rows": never, **m}


def parse_rows(text: str) -> dict[str, Any]:
    """Check the rows (and groups, ticket_projects) a plugin or agent passes; the error names the row and the fix."""
    try:
        data = yaml.safe_load(text) if text and text.strip() else None
    except yaml.YAMLError:
        raise StandupError(f"the rows are not JSON or YAML ({FIX})") from None
    if isinstance(data, list):
        data = {"rows": data}
    if not isinstance(data, dict) or data.get("rows") is None:
        raise StandupError(f"no rows ({FIX}; an empty list [] clears them)")
    if not isinstance(data["rows"], list) or len(data["rows"]) > MAX_ROWS:
        raise StandupError(f"rows is a list of at most {MAX_ROWS} rows")
    groups = data.get("groups") or []
    if not isinstance(groups, list):
        raise StandupError("groups is a list of {label, repos, keys, aliases, project, never_rows}")
    projects = _words(data.get("ticket_projects"), "ticket_projects", "ticket_projects")
    if bad := [p for p in projects if not _PREFIX.match(p)]:
        raise StandupError(f"ticket_projects: {bad[0][:40]!r} is not a project key such as PROJ")
    seen = {OTHER.lower()}
    return {"rows": [_row(r, f"row {i}") for i, r in enumerate(data["rows"], 1)], "groups": [_group(g, f"group {i}", seen) for i, g in enumerate(groups, 1)], "ticket_projects": [p.upper() for p in projects]}


def save_rows(text: str) -> dict[str, Any]:
    """Keep the rows for this and later runs (encrypted at rest). A bad input writes nothing."""
    parsed = parse_rows(text)
    store.write_json(rows_path(), {**parsed, "saved_at": calendar.now().isoformat(timespec="seconds")})
    events.emit("work.standup.rows", status="ok", rows=len(parsed["rows"]), groups=len(parsed["groups"]))
    return parsed


def load_rows() -> dict[str, Any]:
    """The last rows passed; {} when none were ever passed. An empty list passed on purpose keeps `rows: []`."""
    data = store.read_json(rows_path(), {}) or {}
    if not isinstance(data, dict) or not isinstance(data.get("rows"), list):
        return {}
    return {**parse_rows(json.dumps({k: data[k] for k in ("rows", "groups", "ticket_projects") if k in data})), "saved_at": str(data.get("saved_at") or "")}


# ---------- agent notes: talking points and extra actions (issue #229) ----------


def _load(text: str, what: str) -> Any:
    try:
        return yaml.safe_load(text) if text and text.strip() else None
    except yaml.YAMLError:
        raise StandupError(f"the {what} are not JSON or YAML") from None


def _known(title: str, titles: set[str], where: str) -> str:
    if title not in titles:
        raise StandupError(f"{where}: {title[:60]!r} is not a row you report on (fix: use a row title passed with --rows)")
    return title


def parse_talk(text: str, titles: set[str]) -> dict[str, str | list[str]]:
    """Each known row takes one string or 1–5 speaking bullets, at most TALK_MAX characters each."""
    data = _load(text, "talking points")
    if not isinstance(data, dict):
        raise StandupError('talk is a mapping of row title to talking point (fix: for example {"Order export": "Shipped the API. Next: the CSV format."})')
    out = {}
    for title, said in data.items():
        title = _known(_text(title, 200), titles, "talk")
        points = [said] if isinstance(said, str) else said
        if not isinstance(points, list) or not 1 <= len(points) <= MAX_TALK_BULLETS:
            raise StandupError(f"talk: the talking points for {title[:60]!r} need 1 to {MAX_TALK_BULLETS} bullets (fix: a string or a list of 1 to {MAX_TALK_BULLETS} strings)")
        folded = []
        for point in points:
            if not isinstance(point, str) or not point.strip():
                raise StandupError(f"talk: a bullet for {title[:60]!r} is empty (fix: use a nonempty string, or leave the row out)")
            point = " ".join(point.split())
            if len(point) > TALK_MAX:
                raise StandupError(f"talk: a bullet for {title[:60]!r} has {len(point)} characters (fix: at most {TALK_MAX} characters per bullet)")
            folded.append(point)
        out[title] = folded[0] if isinstance(said, str) else folded
    return out


def parse_extra(text: str, titles: set[str], w: dict[str, Any]) -> list[dict[str, Any]]:
    """[{title, at, kind, url?, row?}]: actions careerbrew does not collect, each inside the standup window."""
    data = _load(text, "extra actions")
    if isinstance(data, dict) and "extra" in data:
        data = data["extra"]
    if not isinstance(data, list) or len(data) > MAX_EXTRA:
        raise StandupError(f'extra is a list of at most {MAX_EXTRA} actions (fix: for example [{{"title": "Agreed the schema", "at": "2026-10-16T14:00", "kind": "meeting", "row": "Order export"}}])')
    tz, out = ZoneInfo(w["tz"]), []
    for i, x in enumerate(data, 1):
        where = f"extra {i}"
        if not isinstance(x, dict):
            raise StandupError(f"{where}: an action is a mapping with title, at, and kind (and url, row)")
        title, kind = _text(x.get("title"), 200), _text(x.get("kind"), 30).lower()
        if not title:
            raise StandupError(f"{where}: missing title (fix: every action needs a title)")
        if not kind:
            raise StandupError(f"{where}: missing kind (fix: slack, meeting, mail, or another word)")
        at = parse_at(x.get("at"), tz)
        if at is None:
            raise StandupError(f"{where}: at is not an ISO time such as 2026-10-16T14:00")
        if not w["start"] <= at <= w["end"]:
            raise StandupError(f"{where}: at is outside the standup window (fix: since {w['since']} until {w['until']})")
        url = _text(x.get("url"), 500)
        row = _known(_text(x.get("row"), 200), titles, where) if x.get("row") else ""
        out.append({"title": title, "at": at.isoformat(), "kind": kind, "url": url if url.startswith(("http://", "https://")) else "", "row": row})
    return out


PROOF_FIX = (
    'fix: for example {"Order export": {"verdict": "pass", "proofs": [{"source": "github", "url": "https://github.example.com/acme/shop/pull/12", "checked_at": "2026-10-19T09:40", "quote": "Merged"}], "reason": "the PR merged on Friday"}}'
)


def parse_proof(text: str, titles: set[str]) -> dict[str, dict[str, Any]]:
    """{row title: {verdict, proofs, reason}} from a live check (issue #271). The shape is checked here and a bad
    one writes nothing; whether a row passes (a quote, a link, a fresh time) is decided at each build by `gate`."""
    data = _load(text, "proofs")
    if not isinstance(data, dict):
        raise StandupError(f"proof is a mapping of row title to its check ({PROOF_FIX})")
    out = {}
    for title, check in data.items():
        title = _known(_text(title, 200), titles, "proof")
        where = f"proof for {title[:60]!r}"
        if not isinstance(check, dict):
            raise StandupError(f"{where}: a check is a mapping with verdict, proofs, and reason ({PROOF_FIX})")
        verdict = _text(check.get("verdict"), 10).lower()
        if verdict not in VERDICTS:
            raise StandupError(f"{where}: verdict is pass or fail")
        proofs = check.get("proofs") or []
        if not isinstance(proofs, list) or len(proofs) > MAX_PROOFS or not all(isinstance(p, dict) for p in proofs):
            raise StandupError(f"{where}: proofs is a list of at most {MAX_PROOFS} mappings with source, url, checked_at, and quote")
        out[title] = {
            "verdict": verdict,
            "proofs": [{"source": _text(p.get("source"), 60), "url": _text(p.get("url"), 500), "checked_at": _text(p.get("checked_at"), 40), "quote": _text(p.get("quote"), 500)} for p in proofs],
            "reason": _text(check.get("reason"), 300),
        }
    return out


def gate(check: dict[str, Any], w: dict[str, Any], built: datetime, minutes: int) -> list[str]:
    """Why a row's proof check fails, in content-free words ([] when it passes): the verdict is pass, there is at
    least one proof, and each proof has a source, an http(s) URL, a quote, and a checked_at after the window
    start, no older than `minutes` before the build, and not in the future."""
    reasons = [] if check.get("verdict") == "pass" else ["verdict is fail"]
    proofs = check.get("proofs") or []
    if not proofs:
        reasons.append("no proof")
    tz, oldest = ZoneInfo(w["tz"]), built - timedelta(minutes=minutes)
    for i, p in enumerate(proofs, 1):
        if not str(p.get("url") or "").startswith(("http://", "https://")):
            reasons.append(f"proof {i} has no http(s) url")
        if not p.get("source"):
            reasons.append(f"proof {i} has no source")
        if not p.get("quote"):
            reasons.append(f"proof {i} has no quote")
        at = parse_at(p.get("checked_at"), tz)
        if at is None:
            reasons.append(f"proof {i} checked_at is not an ISO time")
        elif at <= w["start"]:
            reasons.append(f"proof {i} was checked before the window started")
        elif at < oldest:
            reasons.append(f"proof {i} is older than {minutes} minutes")
        elif at > built + FUTURE_SKEW:
            reasons.append(f"proof {i} checked_at is in the future")
    return reasons


def load_notes(w: dict[str, Any] | None = None) -> dict[str, Any]:
    """The talking points, extra actions, and proofs of the current window; notes from an earlier window are cleared.
    `proof` is None when no proofs were passed in this window: then no row is gated."""
    key = (w or window())["start"].isoformat()
    data = store.read_json(notes_path(), {}) or {}
    if not isinstance(data, dict) or data.get("window") != key:
        return {"talk": {}, "extra": [], "proof": None}
    proof = data.get("proof")
    return {"talk": dict(data.get("talk") or {}), "extra": list(data.get("extra") or []), "proof": dict(proof) if isinstance(proof, dict) else None}


def save_notes(talk: str | None = None, extra: str | None = None, now: datetime | None = None, proof: str | None = None) -> dict[str, Any]:
    """Keep the talking points, extra actions, and proofs for this window (encrypted at rest). A bad input writes
    nothing. An input not passed keeps its value from this window."""
    w = window(now)
    saved = load_rows()
    titles = {r["title"] for r in saved.get("rows", [])}
    if not saved:
        raise StandupError("no rows to report on yet (fix: pass them with --rows first)")
    cur = load_notes(w)
    notes = {
        "talk": cur["talk"] if talk is None else parse_talk(talk, titles),
        "extra": cur["extra"] if extra is None else parse_extra(extra, titles, w),
        "proof": cur["proof"] if proof is None else parse_proof(proof, titles),
    }
    store.write_json(notes_path(), {"window": w["start"].isoformat(), **notes, "saved_at": calendar.now().isoformat(timespec="seconds")})
    events.emit("work.standup.notes", status="ok", talk=len(notes["talk"]), extra=len(notes["extra"]), gated=notes["proof"] is not None, proofs=len(notes["proof"] or {}))
    return notes


def _extra_action(x: dict[str, Any], tz: ZoneInfo, pattern: re.Pattern[str] | None) -> dict[str, Any] | None:
    at = parse_at(x.get("at"), tz)
    if at is None:
        return None
    title, url, kind = str(x.get("title") or ""), str(x.get("url") or ""), str(x.get("kind") or "action")
    return {"title": title, "at": at, "kinds": [kind], "ids": [], "repo": "", "url": url, "project": "", "keys": ticket_keys(f"{title} {url}", pattern), "extra": True, "row": str(x.get("row") or "")}


# ---------- your work in the window ----------


def _pr_number(rid: str, url: str, title: str) -> str:
    if m := _PR_ID.search(rid):
        return m.group(1)
    if m := _PR_URL.search(url):
        return m.group(1)
    if m := _PR_TITLE.search(title):
        return m.group(1) or m.group(2)
    return ""


def _copy(row: dict[str, Any], kind: str, at: datetime, pattern: re.Pattern[str] | None, ranked: list[dict[str, Any]]) -> dict[str, Any]:
    """One copy of an action, with the keys that tell copies of the same action apart from others."""
    rid = str(row.get("id") or "")
    title = _text(row.get("title"), 200)
    url = str(row.get("url") or "")
    repo = str(row.get("repo") or "") or project_priority.repo_of(rid, url)
    project = feed.project_label(row.get("project"))
    if not project and ranked and (hit := project_priority.match(project_priority.keys(title, repo, url), ranked)):
        project = hit["name"]  # careerbrew's own project ranking names it
    short = repo.rsplit("/", 1)[-1].lower()
    number = _pr_number(rid, url, title)
    pr = f"pr:{short}#{number}" if short and number else ""
    same = {k for k in (f"id:{rid}" if rid else "", pr, f"t:{short}:{daylog.normalize(title)}" if title else "") if k}
    return {"ids": [rid], "kind": kind, "title": title, "at": at, "repo": repo, "url": url, "project": project, "keys": ticket_keys(" ".join((title, url, rid)), pattern), "same": same, "pr": pr}


def copies(start: datetime, end: datetime, tz: ZoneInfo, pattern: re.Pattern[str] | None) -> list[dict[str, Any]]:
    """Evidence and your own feed events inside the window; never a personal one, chat, or mail."""
    ranked = project_priority.entries()
    personal = feed.personal_ids()
    out = []
    for e in evidence.load():
        if e.get("personal") or e.get("id") in personal:
            continue
        if (at := parse_at(e.get("at"), tz)) and start <= at <= end:
            out.append(_copy(e, str(e.get("kind") or "item"), at, pattern, ranked))
    for r in feed.load():
        if r.get("actor") != "me" or r.get("personal") or r.get("kind") in FEED_SKIP_KINDS:
            continue
        if (at := parse_at(r.get("at"), tz)) and start <= at <= end:
            ev = feed.as_evidence(r)
            out.append(_copy({**ev, "project": r.get("project")}, ev["kind"], at, pattern, ranked))
    return out


def _merge(group: list[dict[str, Any]]) -> dict[str, Any]:
    group = sorted(group, key=lambda c: (_LEAD.get(c["kind"], 2), c["at"]))
    repo = next((c["repo"] for c in group if "/" in c["repo"]), "") or next((c["repo"] for c in group if c["repo"]), "")
    return {
        "title": group[0]["title"],
        "at": max(c["at"] for c in group),
        "kinds": list(dict.fromkeys(c["kind"] for c in group)),
        "ids": [i for c in group for i in c["ids"]],
        "repo": repo,
        "url": next((c["url"] for c in group if c["url"].startswith(("http://", "https://"))), ""),
        "project": next((c["project"] for c in group if c["project"]), ""),
        "keys": list(dict.fromkeys(k for c in group for k in c["keys"])),
    }


def _apart(c: dict[str, Any], group: list[dict[str, Any]]) -> bool:
    """Copies that share only a title are two actions when they come from one source (a source keeps one
    row per action: two commits "fix: lint", two notes, two feed events) or link to two different pages."""
    url = c["url"] if c["url"].startswith(("http://", "https://")) else ""
    return any(o["kind"] == c["kind"] or (url and o["url"].startswith(("http://", "https://")) and o["url"] != url) for o in group)


def dedupe(found: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """One line per action: copies that share an id, a PR (repo and number), or a title in one repo are merged.
    Two PRs with different numbers stay two lines, even with the same title; so do two copies from one
    source, or with two different links, that share only a title."""
    groups: list[list[dict[str, Any]]] = []
    prs: list[set[str]] = []
    where: dict[str, int] = {}
    for c in sorted(found, key=lambda c: c["at"]):
        hits, seen = [], {c["pr"]} - {""}
        for g in sorted({where[k] for k in c["same"] if k in where}):
            if seen and prs[g] and not prs[g] & seen:
                continue  # the same title, another PR: another action
            if not any(where.get(k) == g for k in c["same"] if not k.startswith("t:")) and _apart(c, groups[g]):
                continue  # only the title is shared: another action
            hits.append(g)
            seen |= prs[g]
        i = hits[0] if hits else len(groups)
        if not hits:
            groups.append([])
            prs.append(set())
        for j in hits[1:]:  # this copy joins two lines that were apart: they are one action
            groups[i] += groups[j]
            prs[i] |= prs[j]
            groups[j], prs[j] = [], set()
            for k, v in where.items():
                if v == j:
                    where[k] = i
        groups[i].append(c)
        prs[i] |= {c["pr"]} - {""}
        for k in c["same"]:
            where[k] = i
    return [_merge(g) for g in groups if g]


# ---------- matching ----------


def _target(t: dict[str, Any], pattern: re.Pattern[str] | None) -> dict[str, Any]:
    repos: set[str] = set()
    for r in t["repos"]:
        repos |= {r.lower(), r.lower().rsplit("/", 1)[-1]}
    keys = set(t["keys"]) | set(ticket_keys(t.get("title", ""), pattern))
    words = [(w, re.compile(rf"(?<!\w){re.escape(w)}(?!\w)", re.I)) for w in t["aliases"]]
    return {"src": t, "project": t["project"].lower(), "keys": keys, "repos": repos, "words": words, "acts": []}


def _hit(level: str, a: dict[str, Any], t: dict[str, Any]) -> str:
    """Why the action matches the row or group at this level ("ticket PROJ-12"), or "" when it does not."""
    if level == "project":
        return f"project {a['project']}" if a["project"] and t["project"] and a["project"].lower() == t["project"] else ""
    if level == "key":
        key = next((k for k in a["keys"] if k in t["keys"]), "")
        return f"ticket {key}" if key else ""
    if level == "repo":
        forms = {a["repo"].lower(), a["repo"].lower().rsplit("/", 1)[-1]} - {""}
        return f"repo {a['repo']}" if forms & t["repos"] else ""
    word = next((w for w, rx in t["words"] if rx.search(a["title"])), "")
    return f'word "{word}"' if word else ""


def _first(a: dict[str, Any], targets: list[dict[str, Any]]) -> tuple[dict[str, Any] | None, str]:
    for level in LEVELS:
        for t in targets:
            if via := _hit(level, a, t):
                return t, via
    return None, ""


# ---------- live republish (issue #315) ----------

FRESH_MINUTES = 60  # a live cycle republishes rows older than this
LEAD_MINUTES = 20  # about one live cycle: the cycle that starts this close to the standup republishes


def due(new_rows: list[dict[str, Any]], saved: dict[str, Any], last_at: str, now: datetime | None = None) -> str:
    """Why a live cycle should republish the standup rows ("match", "stale", "standup next"), or "" when they are fresh."""
    now = now or calendar.now()
    pattern = key_pattern(prefixes(saved))
    targets = [_target(_matchers(t, "row"), pattern) for t in saved.get("rows") or []]
    for ev in new_rows:
        title = str(ev.get("title") or "")
        act = {"project": feed.project_label(ev.get("project")), "keys": ticket_keys(title, pattern), "repo": "", "title": title}
        if _first(act, targets)[0] is not None:
            return "match"
    last = parse_at(last_at, zone()) if last_at else None
    if last is None or now - last > timedelta(minutes=FRESH_MINUTES):
        return "stale"
    local = now.astimezone(zone())
    if local.weekday() in {int(d) for d in config.get("standup_days") or []}:
        hh, mm = (int(x) for x in str(config.get("standup_time")).split(":"))
        start = datetime.combine(local.date(), time(hh, mm), zone())
        if timedelta(0) < start - local <= timedelta(minutes=LEAD_MINUTES):
            return "standup next"
    return ""


# ---------- the report ----------


def _out(a: dict[str, Any], via: str, tz: ZoneInfo) -> dict[str, Any]:
    at = a["at"].astimezone(tz)
    extra = bool(a.get("extra"))
    nouns = [EXTRA_NOUN.get(k, "action") if extra else NOUN.get(k, "item") for k in a["kinds"]]
    return {**{k: v for k, v in a.items() if k != "row"}, "at": at.isoformat(), "when": label(at), "nouns": nouns, "via": via, "extra": extra}


def plain(title: str, pattern: re.Pattern[str] | None, limit: int = 70) -> str:
    """A title in plain words: no commit prefix, PR number, or configured ticket key (`pattern`, the
    standup rows' projects), so "decode UTF-8 names" keeps UTF-8."""
    t = daylog._CC.sub("", title.strip())
    if pattern is not None:
        t = pattern.sub(" ", t)
    t = re.sub(r"[(\[][\s,:;]*[)\]]", " ", re.sub(r"#\d+\b", " ", t))  # brackets the refs left empty
    t = " ".join(t.split()).strip(" :-,;.")
    while daylog._TAIL.search(t):
        t = daylog._TAIL.sub("", t).strip(" :-,;.")
    if len(t) > limit:
        t = t[:limit].rsplit(" ", 1)[0].rstrip(" :-,;.") + "…"
    return t[:1].lower() + t[1:] if t[:1].isupper() and not t[1:2].isupper() else t


def talking_point(acts: list[dict[str, Any]], pattern: re.Pattern[str] | None = None) -> str:
    """What to say first: the actions in plain words, latest first, and how many of each kind."""
    if not acts:
        return ""
    nouns = [a["nouns"][0] for a in acts]
    counts = ", ".join(daylog._n(nouns.count(n), n) for n in NOUN_ORDER if n in nouns)
    named = [p for a in acts if (p := plain(a["title"], pattern))]
    if not named:
        return counts[:1].upper() + counts[1:] + "."
    lead = daylog._few(named)
    return f"{lead[:1].upper()}{lead[1:]} ({counts})."


def talking_points(acts: list[dict[str, Any]], pattern: re.Pattern[str] | None = None) -> list[str]:
    """The latest three evidence items as separate speaking bullets."""
    return [point for a in acts[:3] if (point := talking_point([a], pattern))]


def build(now: datetime | None = None) -> dict[str, Any]:
    """The report as data: the window, the rows shown (yours, or matched by your work), the groups, and counts."""
    w = window(now)
    tz = ZoneInfo(w["tz"])
    saved = load_rows()
    pattern = key_pattern(prefixes(saved))
    rows = [_target(r, pattern) for r in saved.get("rows", [])]
    groups = [_target(g, pattern) for g in saved.get("groups", [])]
    first, rest = [g for g in groups if g["src"]["never_rows"]], [g for g in groups if not g["src"]["never_rows"]]
    other = _target({"label": OTHER, "never_rows": False, "project": "", "keys": [], "repos": [], "aliases": []}, None)
    notes = load_notes(w)
    talk = notes["talk"]
    extras = [a for x in notes["extra"] if (a := _extra_action(x, tz, pattern)) and w["start"] <= a["at"] <= w["end"]]
    by_title = {t["src"]["title"]: t for t in rows}
    acts = dedupe(copies(w["start"], w["end"], tz, pattern))
    for a in extras:
        if t := by_title.get(a["row"]):
            t["acts"].append(_out(a, "an agent", tz))
        else:
            acts.append(a)  # no row named: matched like other work
    for a in acts:
        t, via = _first(a, first)
        if t is None:
            t, via = _first(a, rows)
        if t is None:
            t, via = _first(a, rest)
        (t or other)["acts"].append(_out(a, via, tz))
    me = {"me"} | {str(m).lower() for m in config.get("standup_me") or []}
    proof, minutes = notes["proof"], int(config.get("standup_proof_minutes"))
    shown, failed, hidden = [], [], 0
    for i, t in enumerate(rows, 1):
        r, done = t["src"], sorted(t["acts"], key=lambda x: x["at"], reverse=True)
        mine = r["mine"] or r["owner"].lower() in me
        said = talk.get(r["title"], "")
        check = proof.get(r["title"]) if proof is not None else None
        if check is None and not done and not said:
            hidden += 1  # no update in the window (issue #271): yours too, and a row done before it
            continue
        proofs: list[dict[str, Any]] = []
        if proof is not None:
            check = check or {}
            reasons = gate(check, w, w["end"], minutes) if check else ["no proof for this row"]
            if reasons:
                failed.append({"row": i, "title": r["title"], "reasons": reasons, "reason": check.get("reason", "")})
                continue
            proofs = [{**p, "when": label(at.astimezone(tz)) if (at := parse_at(p["checked_at"], tz)) else ""} for p in check["proofs"]]
        points = ([said] if isinstance(said, str) else said) if said else talking_points(done, pattern)
        points = points or [f"Checked live in {len(proofs)} source{'' if len(proofs) == 1 else 's'}: see the proof below."]
        point = " ".join(points) if said else talking_point(done, pattern) or points[0]
        shown.append({**{k: r[k] for k in ("title", "status", "owner", "keys", "url", "project")}, "mine": mine, "talking_point": point, "talking_points": points, "agent_written": bool(said), "actions": done, "proofs": proofs})
    out_groups = []
    for t in [*groups, other]:
        if done := sorted(t["acts"], key=lambda x: x["at"], reverse=True):
            out_groups.append({"label": t["src"]["label"], "never_rows": t["src"]["never_rows"], "talking_point": talking_point(done, pattern), "talking_points": talking_points(done, pattern), "actions": done})
    counts = {
        "actions": len(acts) + sum(1 for a in extras if a["row"] in by_title),
        "rows": len(rows),
        "shown": len(shown),
        "yours": sum(1 for r in shown if r["mine"]),
        "matched": sum(1 for r in shown if not r["mine"]),
        "hidden": hidden,
        "on_rows": sum(len(r["actions"]) for r in shown),
        "by_group": [[g["label"], len(g["actions"])] for g in out_groups],
        "talked": sum(1 for r in shown if r["agent_written"]),
        "extra": len(extras),
        "gated": proof is not None,
        "passed": len(shown) if proof is not None else 0,
        "failed": len(failed),
    }
    win = {k: (v.isoformat() if isinstance(v, datetime) else v) for k, v in w.items()}
    return {
        "window": win,
        "rows": shown,
        "groups": out_groups,
        "hidden": hidden,
        "has_rows": bool(rows),
        "cleared": "rows" in saved and not rows,  # a plugin passed [] on purpose: no row passed its check
        "saved_at": saved.get("saved_at", ""),
        "updated": updated(saved.get("saved_at", ""), w),
        "counts": counts,
        "failed": failed,  # titles and the agent's reasons: for the command's output only, never the page or the view
        "page": str(page_path()),
    }


def updated(saved_at: str, w: dict[str, Any]) -> str:
    """When the rows were last passed, in the standup zone: "updated 09:40", or with the day when it was not today."""
    tz = ZoneInfo(w["tz"])
    at = parse_at(saved_at, calendar.local_tz())
    if at is None:
        return ""
    at = at.astimezone(tz)
    return f"updated {at:%H:%M}" if at.date() == w["end"].astimezone(tz).date() else f"updated {label(at)}"


# ---------- the page ----------

CSS = """
:root { color-scheme: light dark; --fg: #1d1d1f; --muted: #6e6e73; --bg: #ffffff; --line: #e3e3e8; --accent: #2f6fde; --mine: #f2f6ff; }
@media (prefers-color-scheme: dark) { :root { --fg: #f2f2f7; --muted: #a1a1a6; --bg: #1c1c1e; --line: #3a3a3c; --accent: #6ea8ff; --mine: #22283a; } }
* { box-sizing: border-box; overflow-wrap: anywhere; }
body { margin: 0; background: var(--bg); color: var(--fg); font: 16px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif; }
main { max-width: 760px; margin: 0 auto; padding: 24px 16px 48px; }
h1 { font-size: 1.6rem; margin: 0 0 4px; }
h2 { font-size: 1.15rem; margin: 0 0 6px; }
section { border-top: 1px solid var(--line); padding: 16px 12px; }
section.mine { background: var(--mine); border-radius: 8px; }
.talk { font-size: 1.05rem; margin: 0 0 6px; }
.by { font-size: 0.875rem; border: 1px solid var(--line); border-radius: 4px; padding: 0 4px; color: var(--muted); white-space: nowrap; }
.window, .status, .note, footer, time, .kinds, .repo, .via { color: var(--muted); }
.status { margin: 0 0 8px; font-size: 0.95rem; }
ul.evidence { margin: 0; padding-left: 18px; }
li.action { margin: 4px 0; }
.key { font: 0.8rem ui-monospace, SFMono-Regular, Menlo, monospace; border: 1px solid var(--line); border-radius: 4px; padding: 0 4px; margin-left: 4px; }
.kinds, .repo, .via, time { font-size: 0.85rem; }
ul.proofs { margin: 8px 0 0; padding-left: 18px; font-size: 0.9rem; }
.proofs .source { color: var(--muted); }
a { color: var(--accent); }
footer { margin-top: 32px; font-size: 0.85rem; }
"""


def _e(text: Any) -> str:
    return html.escape(str(text or ""), quote=True)


def _keys_html(keys: list[str]) -> str:
    return "".join(f'<span class="key">{_e(k)}</span>' for k in keys)


def _actions_html(acts: list[dict[str, Any]], empty: str) -> str:
    if not acts:
        return f'<p class="evidence note">{_e(empty)}</p>'
    items = []
    for a in acts:
        title = f'<a href="{_e(a["url"])}" rel="noreferrer noopener">{_e(a["title"])}</a>' if a["url"] else f'<span class="title">{_e(a["title"])}</span>'
        repo = f' <span class="repo">{_e(a["repo"])}</span>' if a["repo"] else ""
        via = f' <span class="via">via {_e(a["via"])}</span>' if a["via"] else ""
        items.append(
            f'<li class="action" data-kinds="{_e(" ".join(a["kinds"]))}"><time datetime="{_e(a["at"])}">{_e(a["when"])}</time> <span class="kinds">{_e(" · ".join(dict.fromkeys(a["nouns"])))}</span> {title}{repo}{_keys_html(a["keys"])}{via}</li>'
        )
    return '<ul class="evidence">' + "".join(items) + "</ul>"


def _proofs_html(proofs: list[dict[str, Any]]) -> str:
    """Each proof link under its row (issue #271): when and where it was checked, and the quote, linked."""
    if not proofs:
        return ""
    items = [f'<li class="proof"><span class="source">Checked {_e(p["when"])} in {_e(p["source"])}:</span> <a href="{_e(p["url"])}" rel="noreferrer noopener">{_e(p["quote"])}</a></li>' for p in proofs]
    return '<ul class="proofs">' + "".join(items) + "</ul>"


def no_row_passed(r: dict[str, Any]) -> bool:
    """The rows were cleared on purpose, or proofs were passed and every row with an update failed them."""
    return bool(r["cleared"] or (r["counts"]["gated"] and not r["rows"] and r["counts"]["failed"]))


def _talk_html(row: dict[str, Any]) -> str:
    points = row.get("talking_points") or [row["talking_point"]]
    by = ' <span class="by">written by an agent</span>' if row.get("agent_written") else ""
    if len(points) == 1:
        return f'<p class="talk">{_e(points[0])}{by}</p>'
    return '<ul class="talk">' + "".join(f"<li>{_e(point)}</li>" for point in points) + f"</ul>{by}"


def render(r: dict[str, Any]) -> str:
    w, c = r["window"], r["counts"]
    parts = [
        "<!doctype html>",
        '<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">',
        f"<title>Standup since {_e(w['since'])}</title><style>{CSS}</style></head><body><main>",
        "<h1>Standup</h1>",
        f'<p class="window">Compares against the {_e(w["compares"])} standup: since {_e(w["since"])} until {_e(w["until"])} ({_e(w["tz"])}) · {c["actions"]} action{"" if c["actions"] == 1 else "s"} of yours{f" · {_e(r['updated'])}" if r["updated"] else ""}</p>',
    ]
    if no_row_passed(r):
        parts.append('<p class="note">No row passed the proof check.</p>')
    elif not r["has_rows"]:
        parts.append('<p class="note">No rows to report on yet: a plugin or an agent passes them with <code>careerbrew work standup --rows &lt;file.json|-&gt;</code>.</p>')
    for row in r["rows"]:
        owner = "you" if row["mine"] else row["owner"] or "no owner"
        heading = f'<a href="{_e(row["url"])}" rel="noreferrer noopener">{_e(row["title"])}</a>' if row["url"] else _e(row["title"])
        parts.append(
            f'<section class="row{" mine" if row["mine"] else ""}"><h2>{heading}{_keys_html(row["keys"])}</h2>'
            f"{_talk_html(row)}"
            f'<p class="status"><b>Status:</b> {_e(row["status"] or "not given")} · <b>Owner:</b> {_e(owner)}</p>'
            f"{_actions_html(row['actions'], 'No actions of yours since the last standup.') if row['actions'] or not row.get('proofs') else ''}"
            f"{_proofs_html(row.get('proofs') or [])}</section>"
        )
    for g in r["groups"]:
        parts.append(f'<section class="group"><h2>{_e(g["label"])}</h2>{_talk_html(g)}{_actions_html(g["actions"], "")}</section>')
    if r["hidden"]:
        parts.append(f'<p class="note">{hidden_line(r["hidden"])}</p>')
    parts.append("<footer>Made by careerbrew from your local evidence and work feed. It holds business data: keep it on this machine, and delete it when you are done.</footer></main></body></html>")
    return "\n".join(parts) + "\n"


def hidden_line(n: int) -> str:
    return f"{n} row{'' if n == 1 else 's'} with no update hidden."


def run(now: datetime | None = None) -> dict[str, Any]:
    """Build the report and write the page; the event holds counts only."""
    r = build(now)
    if notes_path().exists() and load_notes(window(now)) == {"talk": {}, "extra": [], "proof": None}:
        notes_path().unlink()  # notes from an earlier standup window: cleared
    page_path().write_text(render(r), encoding="utf-8")
    c = r["counts"]
    events.emit("work.standup", status="ok", rows=c["shown"], hidden=c["hidden"], actions=c["actions"], on_rows=c["on_rows"], gated=c["gated"], passed=c["passed"], failed=c["failed"])
    return r
