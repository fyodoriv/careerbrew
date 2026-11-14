"""`careerbrew serve`: the local JSON API behind the web app (web/).

Security model (the browser user is you, on this machine):
- binds to 127.0.0.1 only;
- one random token, kept encrypted in `day/web-token.json` and reused across
  restarts until `careerbrew web rotate-token` or `web_token_max_days` (issue #141);
  every /api request needs header `X-Careerbrew-Token` (the link `careerbrew open`
  gives carries it once as `#token=`; the app keeps it in sessionStorage and the
  URL fragment never reaches the server);
- a tab that sends the token gets an HttpOnly, SameSite=Strict, host-only cookie
  for 127.0.0.1 (`POST /api/session`); only `GET /api/session` reads it, and only
  for this app's own pages (`Sec-Fetch-Site: same-origin` or absent), to give a
  reload, a new tab, or a bookmark the token back; no other route accepts it;
- the Host header must be 127.0.0.1:<port> or localhost:<port> (blocks DNS
  rebinding); cross-origin requests get no CORS headers, and every response is
  `Cross-Origin-Resource-Policy: same-origin`;
- job-hunt endpoints answer 403 on a corporate-role machine;
- text that leaves careerbrew still passes privacy.redact for sensitive terms
  only where it is public-facing (LinkedIn); the local UI shows your own titles.
Built files are served from web/dist.
"""

from __future__ import annotations

import contextlib
import fcntl
import json
import math
import mimetypes
import os
import re
import secrets as pysecrets
import signal
import subprocess
import threading
import time
from collections.abc import Callable, Iterator
from datetime import datetime, timedelta
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from time import monotonic
from typing import Any
from urllib.parse import SplitResult, parse_qs, unquote, urlsplit

from . import config, events, machine, paths, plugins, store

DIST = paths.REPO_ROOT / "web" / "dist"
JOB_PATHS = ("/api/jobs", "/api/interviews", "/api/cards", "/api/library", "/api/explanations", "/api/exercises", "/api/drill")


class ApiError(Exception):
    def __init__(self, status: int, message: str):
        super().__init__(message)
        self.status = status


# ---------- data builders (pure functions over the data root) ----------


def overview() -> dict[str, Any]:
    from .day import watch

    st = watch.status()
    return {
        "role": machine.role(),
        "plugins": [{"name": p["name"], "version": p["version"], "extends": p.get("extends")} for p in plugins.installed()],
        "watcher": {"state": st.get("state"), "running": st["running"], "reloads": st.get("reloads", 0), "today": st["today"]},
    }


def _label_plan(plan: dict[str, Any], of: dict[str, str], titles: dict[str, str], context: dict[str, dict[str, Any]]) -> dict[str, Any]:
    """Readers see task titles, projects, links, and the task's own why; raw task ids stay in `task` for details and actions."""
    for s in plan.get("schedule", []):
        if s["task"] in of:
            s["project"] = of[s["task"]]
        s.update(context.get(s["task"], {}))
    plan["unplaced_titles"] = [titles.get(t, t) for t in plan.get("unplaced", [])]
    for big in plan.get("too_big", []):
        big["title"] = titles.get(big["task"], big["task"])
    return plan


def _task_labels() -> tuple[dict[str, str], dict[str, str], dict[str, dict[str, Any]]]:
    from .work import priorities, projects

    tasks = priorities.load()
    return projects.task_projects(), {t["id"]: t["title"] for t in tasks}, priorities.context(tasks)


def today() -> dict[str, Any]:
    from .day import planner, results
    from .work import health

    plan = planner.build()
    nxt = planner.next_step()
    labels = _task_labels()
    nxt.update(labels[2].get(nxt.get("task") or "", {}))
    snapshot = health.snapshot()
    return {"plan": _label_plan(plan, *labels), "next": nxt, "results": [{**row, "source_health": health.result_health(row, snapshot)} for row in results.load()], "summary": results.summary(), "feed_health": snapshot}


def week(days: int = 7) -> dict[str, Any]:
    from .day import planner

    labels = _task_labels()
    from .calendar import now

    t = now()
    return {"days": [_label_plan(d, *labels) for d in planner.plan_week(t.date(), max(1, min(days, 14)), t)]}


def jobs() -> dict[str, Any]:
    from . import interview
    from .jobs import criteria, sources, want

    plan = store.read_json(paths.sub("jobs", "strategy.json"), {}) or {}
    prof, crit = want.profile(), criteria.load()
    kept, rejected = criteria.apply(sources.load(), crit)
    rated = want.ratings()
    rows = []
    for p in kept:
        s, f = want.score(p, want.weights(), prof, crit)
        rows.append(
            {
                "id": p.id,
                "company": p.company,
                "title": p.title,
                "location": p.location,
                "region": p.region,
                "want": s,
                "features": f,
                "rating": rated.get(p.id),
                "url": p.url,
                "source": p.source_name,
                "has_loop": bool(interview.find_company(p.company)),
            }
        )
    rows.sort(key=lambda r: -r["want"])
    from . import notion_import

    board = [{k: j.get(k) for k in ("id", "company", "role", "stage", "want", "salary", "location", "remote", "url", "reason_no", "last_their_contact", "created")} for j in notion_import.jobs_board()]
    return {"strategy": plan, "postings": rows[:200], "rejected": dict(rejected), "board": board}


def teach_view() -> dict[str, Any]:
    from . import teacher

    return {"step": teacher.next_step(), "summary": teacher.summary(), "rules_version": teacher.rules_version()}


VOICE_MAX_BYTES = 12_000_000  # about 5 minutes of browser audio, base64-encoded


def _drill_voice(body: dict[str, Any]) -> dict[str, Any]:
    """Grade a spoken answer (base64 audio, transcribed on this machine) or a typed one."""
    import base64
    import binascii
    import tempfile

    from .tutor import voice

    item, text, audio = str(body.get("item", "")), body.get("text"), body.get("audio")
    try:
        if isinstance(audio, str) and audio:
            ext = {"audio/wav": ".wav", "audio/ogg": ".ogg", "audio/mp4": ".m4a"}.get(str(body.get("mime", "")).split(";")[0], ".webm")
            with tempfile.TemporaryDirectory(prefix="careerbrew-voice-") as tmp:
                f = Path(tmp) / f"answer{ext}"
                f.write_bytes(base64.b64decode(audio, validate=True))
                return voice.answer(item, audio=f)
        return voice.answer(item, text=str(text or ""))
    except (voice.VoiceError, binascii.Error) as exc:
        raise ApiError(HTTPStatus.BAD_REQUEST, str(exc)) from None
    except KeyError as exc:
        raise ApiError(HTTPStatus.NOT_FOUND, str(exc).strip("'\"")) from None


def drill_view() -> dict[str, Any]:
    """The next drill item for voice practice, its rubric, and whether local speech-to-text is set up."""
    from .jobs import strategy
    from .tutor import engine, voice

    it = engine.next_item(strategy.CORE_TOPICS)
    rubric = engine.rubrics().get(it["rubric"], {}) if it else {}
    return {"item": it, "criteria": [{"id": c["id"], "text": c["text"]} for c in rubric.get("criteria", [])], "stt": bool(voice.stt_command())}


def goals_view() -> dict[str, Any]:
    from . import goals

    g = goals.load()
    return {**g, "focus": goals.focus(g["upcoming"]), "days_left": goals.days_left(g["upcoming"]), "kinds": list(goals.KINDS)}


def improvements_view() -> dict[str, Any]:
    from . import teacher

    return {"improvements": teacher.improvements()}


def mistakes_view() -> dict[str, Any]:
    from .tutor import mistakes

    return mistakes.summary()


def exercises_view() -> dict[str, Any]:
    from .tutor import exercises

    st = exercises.status()
    return {"exercises": [{k: e[k] for k in ("id", "order", "pattern", "difficulty", "title")} | st.get(e["id"], {}) for e in exercises.load()]}


def cards_view() -> dict[str, Any]:
    from .tutor import cards

    return {"queue": cards.queue(limit=30), "stats": cards.stats()}


def library_view() -> dict[str, Any]:
    from . import notion_import
    from .tutor import public

    kb = [{k: e[k] for k in ("id", "title", "section")} for e in public.library_entries()]
    return {"pages": kb + notion_import.library()}


def library_page(page_id: str) -> dict[str, Any]:
    from . import notion_import
    from .tutor import public

    kb = next((e for e in public.library_entries() if e["id"] == page_id), None)
    if kb is not None:
        return kb
    p = notion_import.page(page_id)
    if p is None:
        raise ApiError(HTTPStatus.NOT_FOUND, "no such library page")
    return p


def interviews() -> dict[str, Any]:
    from . import interview, prep

    comps = []
    for cid, c in sorted(interview.companies().items()):
        hist = store.read_json(prep.history_path(c.get("name", cid)), {}) or {}
        runs = hist.get("runs") or []
        comps.append(
            {"id": cid, "name": c.get("name", cid), "steps": [s["step"] for s in c["loop"]], "ai": (c.get("ai_policy") or {}).get("allowed", "unknown"), "checked": c.get("checked"), "prep_score": runs[-1]["score"] if runs else None}
        )
    return {"companies": comps, "patterns": interview.patterns()}


def interview_detail(company: str) -> dict[str, Any]:
    from . import interview, prep

    info = interview.loop_for(company)
    src = interview.sources()
    ids = {i for s in info["loop"] for i in s.get("company_sources", []) + s.get("sources", [])} | set(info.get("sources", []))
    ids |= {i for p in info.get("ai_shared", []) for i in p.get("sources", [])}
    # By title, then id: `ids` is a set, whose order changed with every server start (US-75 visual review).
    info["source_list"] = {k: src[k] for k in sorted((i for i in ids if i in src), key=lambda i: (str(src[i].get("title", "")).casefold(), i))}
    info["stale"] = prep.stale_mark(info["company"])
    return info


def _ranked_first(rows: list[dict[str, Any]], rank: dict[str, int]) -> list[dict[str, Any]]:
    """Issue #198: each ranked project leads, in rank order, with its whole subtree (depths rebased to 0).

    A ranked project under a ranked ancestor stays nested in that ancestor's subtree with its badge. The rest keep tree order.
    """
    rows = [{**r, "rank": rank.get(r["name"])} for r in rows]
    parent = {r["name"]: r.get("parent") or "" for r in rows}

    def ranked_above(n: str) -> bool:
        seen: set[str] = set()
        while (n := parent.get(n, "")) and n not in seen:
            seen.add(n)
            if n in rank:
                return True
        return False

    blocks: list[tuple[int, list[dict[str, Any]]]] = []
    taken: set[int] = set()
    for i, r in enumerate(rows):
        if r["rank"] is None or ranked_above(r["name"]):
            continue
        j = i + 1
        while j < len(rows) and rows[j]["depth"] > r["depth"]:
            j += 1
        blocks.append((r["rank"], [{**x, "depth": x["depth"] - r["depth"]} for x in rows[i:j]]))
        taken.update(range(i, j))
    return [x for _, b in sorted(blocks, key=lambda t: t[0]) for x in b] + [r for i, r in enumerate(rows) if i not in taken]


def work() -> dict[str, Any]:
    from .work import evidence, priorities, project_priority, projects, promo

    s = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
    try:
        asm = promo.assess(s.get("ladder", "generic-swe"), s.get("target_level", "senior"))
        gaps = promo.gaps(asm)
    except promo.LadderError as exc:
        asm, gaps = {"error": str(exc), "competencies": []}, []
    for c in asm.get("competencies", []):
        c.pop("evidence", None)
    gp = {g["id"]: g["priority"] for g in gaps}
    ev = sorted(evidence.load(), key=lambda e: str(e.get("at", "")), reverse=True)[:50]
    of = projects.task_projects()
    tasks = [{**t, "project": of.get(t["id"])} for t in priorities.rank(gp)]
    rank = {e["name"]: e["rank"] for e in project_priority.entries()}
    rows = _ranked_first(projects.tree(), rank)
    return {
        "promo": asm,
        "gaps": gaps,
        "tasks": tasks,
        "projects": rows,
        "watched": projects.watch_list(active=True, business=True),
        "evidence": [{k: e.get(k) for k in ("id", "kind", "title", "at", "tags", "origin")} for e in ev],
    }


def priority_view() -> dict[str, Any]:
    """Active now (issue #198): the ranked projects, rank 1 first, with the counts `work priority show` prints."""
    from .work import project_priority

    return {"projects": project_priority.summary()}


def agents_view() -> dict[str, Any]:
    """Agent activity on Work (issue #272): background agents' runs by project, each with its last event."""
    from .work import agents

    return agents.view()


ASKS_MAX = 50  # rows in one answer by default; `?limit=` asks for more, up to ASKS_LIMIT (issue #275)
ASKS_LIMIT = 1000


def feed_asks(count: int = ASKS_MAX) -> dict[str, Any]:
    """Open asks from the work feed for Today: newest first (issue #275), personal and answered ones left out.

    The answer holds the `count` newest asks (1 to ASKS_LIMIT), the `total`, and `limit` (ASKS_LIMIT, the one
    place the web app reads the cap from), so a long list never hides the newest ones. Rows come from the analysis only (never the original text). A link that is not http(s) is dropped,
    so a plugin's event cannot put a `javascript:` link on the page. An ask is overdue after
    `feed_ask_overdue_hours` work hours (`feed.work_minutes`), the same count as `work feed show`.
    """
    from .calendar import now
    from .work import feed

    rows = feed.load()
    s = feed.summary(rows)
    at, limit, hours = now(), s["overdue_hours"] * 60, feed.work_hours()
    # Stored times are UTC ("...Z"), so the text order is the time order.
    open_rows = sorted(feed.open_asks(rows), key=lambda r: (r["at"], r["id"]), reverse=True)
    asks = []
    for r in open_rows[: max(1, min(count, ASKS_LIMIT))]:
        url, worked = str(r.get("url") or ""), feed.work_minutes(r, at, hours)
        asks.append(
            {**{k: r[k] for k in ("id", "source", "kind", "at", "title")}, "url": url if url.startswith(("https://", "http://")) else "", "waiting_minutes": feed.waiting_minutes(r, at), "work_minutes": worked, "overdue": worked > limit}
        )
    return {
        "asks": asks,
        "total": len(open_rows),
        "limit": ASKS_LIMIT,
        "events": len(rows),
        "overdue_hours": s["overdue_hours"],
        "closed_today": feed.closed_today(rows, at, hours[3]),
    }


def focus() -> dict[str, Any]:
    from .work import focus as focus_mod

    return focus_mod.build()


# Each Dashboard block the page asks for on its own (issue #142), with the clock resolution of its answer.
# The same table as `work.focus.BLOCKS` (test: `test_the_server_and_focus_list_the_same_blocks`); it is
# copied here so that importing the server does not import the planner.
FOCUS_BLOCKS = {"now": "minute", "today": "minute", "week": "minute", "month": "minute", "quarter": "day", "year": "day", "company": "day", "tomorrow": "minute", "stale": "day"}


def focus_block(block: str, cache: ResponseCache | None = None) -> dict[str, Any]:
    """One Dashboard block. Blocks asked for in the same minute, on the same data, share one `focus.Build`."""
    from .calendar import now
    from .work import focus as focus_mod

    if block not in FOCUS_BLOCKS:
        raise ApiError(HTTPStatus.NOT_FOUND, f"no such Dashboard block (one of {', '.join(FOCUS_BLOCKS)})")
    t = now()
    if cache is None:
        return focus_mod.Build(t).block(block)
    shared = cache.shared("focus", store.digest(t.isoformat(timespec="minutes"), files_version()), lambda: focus_mod.Build(t))
    return shared.block(block)


def inbox_view() -> dict[str, Any]:
    """careerbrew's open suggestions and questions: the fast part of the day log, shown on its own (issue #142),
    with the groups of alike suggestions and the ones hidden as done elsewhere (issue #147)."""
    from .day import inbox

    return inbox.view()


def checklist_view() -> dict[str, Any]:
    """Getting started on Dashboard (issue #147): each first step, done or not, until all are done or it is hidden."""
    from . import checklist

    return checklist.build()


def daylog_view(day: str | None = None) -> dict[str, Any]:
    """Today's log (rebuilt), the PM draft, open proposals and questions, and the kept days."""
    from datetime import date as _date

    from .day import inbox
    from .day import log as daylog

    if day:
        try:
            d = _date.fromisoformat(day)
        except ValueError:
            raise ApiError(HTTPStatus.BAD_REQUEST, "use a date like 2026-10-20") from None
        text = daylog.read(d)
        if text is None:
            raise ApiError(HTTPStatus.NOT_FOUND, f"no log kept for {day}")
        return {"day": day, "markdown": text}
    built = daylog.save()
    return {"log": built, "markdown": daylog.render(built), "pm": daylog.pm_report(built), **inbox.view(), "history": daylog.history()}


def standup_view() -> dict[str, Any]:
    """The standup report since the last standup (issue #110): the same data as `work standup` and its page.
    Rows that failed the proof check (issue #271) stay off the view: only their count is sent."""
    from .work import health, standup

    r = standup.build()
    r.pop("failed", None)
    r["feed_health"] = health.snapshot()
    return r


def feed_health_view() -> dict[str, Any]:
    from .work import health

    return health.snapshot()


def time_saved_view() -> dict[str, Any]:
    """Dashboard's Time saved block: minutes saved per week, numbers and action ids only (`metrics time-saved`)."""
    from . import time_saved

    return time_saved.compute()


def promises_view() -> dict[str, Any]:
    """Dashboard's Commitments block (issue #268): open promises you made, overdue first, each with its link and next step.
    A link that is not http(s) is dropped, so a plugin's event cannot put a `javascript:` link on the page."""
    from .work import promises

    rows = [{**r, "url": r["url"] if r["url"].startswith(("https://", "http://")) else ""} for r in promises.open_list(business=True)]
    return {"promises": rows, "steps": promises.STEP_TEXT}


def actions_view() -> dict[str, Any]:
    from . import actions

    return {"actions": actions.catalog()}


def notifications_view() -> dict[str, Any]:
    from . import notify

    return notify.view()


def config_view() -> dict[str, Any]:
    from . import config

    return {"settings": config.effective()}


def review() -> dict[str, Any]:
    from . import review as rv

    return {"metrics": rv.run()}


def reports_view() -> dict[str, Any]:
    """Every kept run page's content-free summary, newest first (the Reports view)."""
    from . import report

    return {"reports": report.all_summaries()[:50]}


def report_page(ref: str) -> dict[str, Any]:
    """One run page (by id, `latest`, or a run name) and its summary; the page shows in a sandboxed frame."""
    from . import report

    found = report.page_html(ref)
    if found is None:
        raise ApiError(HTTPStatus.NOT_FOUND, f"no run page {ref} (run scripts/checkup.sh, careerbrew, or careerbrew review to make one)")
    summary, page = found
    return {"summary": summary, "html": page}


# Folders whose writes change what Today, Week, and Dashboard show: evidence, tasks, inbox, day log, results.
CHANGE_DIRS = ("work", "day")
# Locks, the server's own state (its pid and port, and its sign-in token), and the watcher's bookkeeping (its
# state with the last cycle's CPU time, and each source's checkpoint time), which every cycle rewrites, and the work
# feed's live-feed cycle record (`feed-cycle.json`: the sources stored this cycle, issue #254). No cached
# or live view reads them, so they are no input change (test: `test_no_cached_or_live_view_reads_a_file_the_versions_skip`).
CHANGE_SKIP = {"web.json", "web-token.json", "web.lock", "inbox.lock", "watch.lock", "watch.json", "cursors.json", "telemetry.json", "feed-cycle.json", "loops.json"}
# The PM draft the Day log view showed (`day/pm-shown-<day>.json`, issue #185): the view writes it, and only a save
# reads it back, so it is no input change.
CHANGE_SKIP_PREFIXES = ("pm-shown-",)


def ping() -> dict[str, Any]:
    """`start_background` asks this with the tracked token: an answer means a link with that token works."""
    return {"ok": True}


def changes_view() -> dict[str, Any]:
    """A version string that moves whenever evidence, the inbox, or the day log is written.

    It hashes file names, inodes, sizes, and mtimes (never content), so writes by any process
    (the watcher, a plugin `command` source, the CLI) count. The page polls it while visible.
    """
    return {"version": files_version(CHANGE_DIRS)}


# The event log grows on every request and is no view's input; the response cache's version skips it.
VERSION_SKIP_DIRS = ("logs",)


def files_version(dirs: tuple[str, ...] | None = None) -> str:
    """The `/api/changes` signal: a hash of file names, inodes, sizes, and mtimes under `dirs` of the data root.

    With no `dirs` it covers the whole data root except `VERSION_SKIP_DIRS`: a superset of the
    `/api/changes` folders that also holds settings, the machine role, plugins, jobs, and study
    data, so a response cached under it is a miss whenever `/api/changes` moves. It uses
    `os.scandir` (one stat per file): about 20 ms for 4,000 files, where `Path.rglob` took 150 ms.
    """
    import hashlib

    root = paths.data_root()
    rows: list[str] = []
    if dirs is None:
        with os.scandir(root) as it:
            for e in it:
                if e.is_dir(follow_symlinks=False):
                    if e.name not in VERSION_SKIP_DIRS:
                        _scan(e.path, f"{e.name}/", rows)
                elif e.is_file():
                    _row(e, "", rows)
    else:
        for d in dirs:
            if (root / d).is_dir():
                _scan(str(root / d), f"{d}/", rows)
    return hashlib.sha256("".join(sorted(rows)).encode()).hexdigest()[:16]


def _scan(base: str, prefix: str, rows: list[str]) -> None:
    with os.scandir(base) as it:
        for e in it:
            if e.is_dir(follow_symlinks=False):
                _scan(e.path, f"{prefix}{e.name}/", rows)
            elif e.is_file():
                _row(e, prefix, rows)


def _row(e: os.DirEntry[str], prefix: str, rows: list[str]) -> None:
    if e.name in CHANGE_SKIP or e.name.startswith(CHANGE_SKIP_PREFIXES) or e.name.endswith(".tmp"):
        return
    try:
        st = e.stat()
    except FileNotFoundError:  # removed while we scanned (a reset PM draft): it counts as gone
        return
    if prefix == "day/" and st.st_size == 0 and (e.name == ".pm-edits.jsonl.lock" or re.fullmatch(r"\.pm-sent-[0-9]{4}-[0-9]{2}-[0-9]{2}\.lock", e.name)):
        return  # an empty PM history or sent-day lock contains no view input
    if prefix == "work/feed/" and e.name == ".lock" and st.st_size == 0:
        return  # the feed's empty lock contains no view input
    rows.append(f"{prefix}{e.name}:{st.st_ino}:{st.st_size}:{st.st_mtime_ns}\n")  # a rewrite is a new inode, even within one mtime tick


# ---------- response cache (issue #117) ----------

# Views whose response depends only on the data root, the clock, and calendar feeds, with the clock
# resolution each one needs: "minute" for views that count minutes (the next step, how long an ask has
# waited, and the plans that start from the free time left today: Week, the day log's plan, Tomorrow),
# "day" for the rest. Views with other inputs are not cached: the watcher process (overview),
# keychain secrets (setup), the teacher's step timer (teach), review history writes (review), settings
# (instant after a change), and `/api/changes` itself.
CACHED_VIEWS = {
    "/api/today": "minute",
    "/api/focus": "minute",
    "/api/feed/asks": "minute",
    "/api/week": "minute",
    "/api/daylog": "minute",
    "/api/work": "day",
    "/api/priority": "minute",
    "/api/agents": "minute",
    "/api/jobs": "day",
    "/api/actions": "day",
    **{f"/api/focus/{b}": resolution for b, resolution in FOCUS_BLOCKS.items()},
}


class ResponseCache:
    """The last response body of each cached view, in memory.

    Lifecycle: server-instance scope. `make_server` makes one per server; it goes away with that
    server and is never shared between servers. Each view (path plus query) keeps one entry:
    (key, expiry, body), at most `MAX_ENTRIES` in all (the oldest goes first). The key hashes the view,
    the clock at the view's resolution, and `files_version()`, so a write by any process, a new
    minute, or a new day is a miss. The expiry (`web_cache_seconds`) bounds inputs no file shows,
    such as a calendar feed read over the network. Any POST clears every entry.
    """

    MAX_ENTRIES = 64  # 17 views; Week has one entry per `days` value

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._entries: dict[str, tuple[str, float, bytes]] = {}
        self._path_locks = {path: threading.Lock() for path in CACHED_VIEWS}
        self._shared: dict[str, tuple[str, Any]] = {}
        # What the warmer (issue #142) last warmed: the data version, and whether a warm runs now (`/api/ping`).
        self.warmed_version: str | None = None
        self.warming = False

    def path_lock(self, path: str) -> threading.Lock:
        """One rebuild at a time per view: a second tab waits for the first rebuild instead of running its own."""
        return self._path_locks[path]

    def get(self, view: str, key: str) -> bytes | None:
        with self._lock:
            entry = self._entries.get(view)
        if entry and entry[0] == key and monotonic() < entry[1]:
            return entry[2]
        return None

    def put(self, view: str, key: str, ttl: float, body: bytes) -> None:
        with self._lock:
            self._entries.pop(view, None)
            self._entries[view] = (key, monotonic() + ttl, body)
            while len(self._entries) > self.MAX_ENTRIES:
                del self._entries[next(iter(self._entries))]

    def clear(self) -> None:
        with self._lock:
            self._entries.clear()
            self._shared.clear()

    def shared(self, name: str, key: str, make: Callable[[], Any]) -> Any:
        """One object per `name` for the current `key`: the Dashboard blocks of one page share one focus build."""
        with self._lock:
            cur = self._shared.get(name)
            if cur is None or cur[0] != key:
                cur = (key, make())
                self._shared[name] = cur
            return cur[1]

    def warm_state(self) -> str:
        """`off` with no warmer, `busy` while a warm runs or new data waits for one, `done` when every warm view is built."""
        if self.warmed_version is None and not self.warming:
            return "off"
        return "busy" if self.warming or self.warmed_version != files_version() else "done"


# Views the warmer keeps built (issue #142): the work views a page opens first. A rebuild of the day log or of
# Today can write its file once (new results), which moves the version every other key holds: they go first.
WARM_WRITERS = ("/api/daylog", "/api/today")
WARM_READERS = ("/api/feed/asks", "/api/week?days=7", "/api/work", *(f"/api/focus/{b}" for b in FOCUS_BLOCKS))
WARM_VIEWS = (*WARM_WRITERS, *WARM_READERS)
WARM_POLL_SECONDS = 2.0


def warm(cache: ResponseCache) -> tuple[str, int]:
    """Build each warm view that `cache` does not hold for the current inputs.

    Returns the data version the views were built on and how many it rebuilt. A view that fails is left
    out; its next request shows the error. With `web_cache_seconds` 0 the cache is off, so it does nothing.
    """
    if _cache_seconds() <= 0:
        return files_version(), 0
    rebuilt = 0

    def one(view: str) -> int:
        try:
            return int(respond(cache, urlsplit(view))[1] == "miss")
        except Exception:  # noqa: BLE001 (a broken view must not stop the others; its request shows the error)
            return 0

    for _ in range(3):  # until a writer's rebuild writes nothing more
        version = files_version()
        rebuilt += sum(one(v) for v in WARM_WRITERS)
        if files_version() == version:
            break
    rebuilt += sum(one(v) for v in WARM_READERS)
    return version, rebuilt


class Warmer:
    """Keeps the warm views built, so the first open after a start or after new data answers from memory.

    Lifecycle: server-instance scope. `run()` starts one daemon thread per `careerbrew serve`. Every
    `poll` seconds it compares the data version (`files_version`) with the one it last warmed; at start,
    and when a watcher cycle, the CLI, or a plugin wrote data, it warms. It records what it warmed on the
    cache, for `/api/ping`. It ends with the process, or on `stop()` (tests).
    """

    def __init__(self, cache: ResponseCache, poll: float = WARM_POLL_SECONDS) -> None:
        self.cache, self.poll = cache, poll
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._loop, name="careerbrew-warm", daemon=True)

    def start(self) -> Warmer:
        self._thread.start()
        return self

    def stop(self) -> None:
        self._stop.set()
        self._thread.join(timeout=30)

    def _loop(self) -> None:
        while not self._stop.is_set():
            try:
                if files_version() != self.cache.warmed_version:
                    self.once()
            except Exception:  # noqa: BLE001 (the warmer must outlive a bad cycle; requests still build views)
                events.emit("web.warm", status="error")
            self._stop.wait(self.poll)

    def once(self) -> None:
        t0 = monotonic()
        self.cache.warming = True
        try:
            version, rebuilt = warm(self.cache)
        finally:
            self.cache.warming = False
        self.cache.warmed_version = version
        events.emit("web.warm", status="ok", views=len(WARM_VIEWS), rebuilt=rebuilt, ms=int((monotonic() - t0) * 1000))


def _cache_seconds() -> int:
    """`web_cache_seconds`; a hand-edited value that is not a number falls back to the default."""
    try:
        return int(config.get("web_cache_seconds"))
    except (TypeError, ValueError):
        return int(config.DEFAULTS["web_cache_seconds"][0])


def _clock(resolution: str) -> str:
    from .calendar import now

    t = now()
    return t.isoformat(timespec="minutes") if resolution == "minute" else t.date().isoformat()


def _route(path: str) -> str:
    """The route name for the event log. A path parameter (a company or page name) is content and never logged."""
    for prefix, name in (("/api/library/", "library.page"), ("/api/daylog/", "daylog.day"), ("/api/interviews/", "interviews.company"), ("/api/reports/", "reports.page")):
        if path.startswith(prefix):
            return name
    return path.removeprefix("/api/").replace("/", ".")[:48] or "api"


def _encode(data: Any) -> bytes:
    return json.dumps(_finite(data), default=str, allow_nan=False).encode()


def respond(cache: ResponseCache, url: SplitResult) -> tuple[bytes, str | None]:
    """A GET view's body, and `hit` or `miss` for a cached view (None for a view that is never cached).

    A cached view's body comes from memory when its inputs did not change, else it is rebuilt and kept
    (issue #117). Requests and the warmer both come here, so two of them wait for one rebuild.
    """
    if url.path == "/api/ping":
        return _encode({"ok": True, "warm": cache.warm_state()}), None
    resolution = CACHED_VIEWS.get(url.path)
    if resolution is None:
        return _encode(view(url, cache)), None
    ttl = _cache_seconds()
    if ttl <= 0:
        return _encode(view(url, cache)), "miss"
    name = url.path + (f"?{url.query}" if url.query else "")
    with cache.path_lock(url.path):
        key = store.digest(name, _clock(resolution), files_version())
        body = cache.get(name, key)
        if body is not None:
            return body, "hit"
        body = _encode(view(url, cache))
        cache.put(name, key, ttl, body)
        return body, "miss"


def view(url: SplitResult, cache: ResponseCache | None = None) -> Any:
    """The data of one GET view, built fresh."""
    q = parse_qs(url.query)
    routes = {
        "/api/overview": overview,
        "/api/today": today,
        "/api/jobs": jobs,
        "/api/interviews": interviews,
        "/api/work": work,
        "/api/priority": priority_view,
        "/api/agents": agents_view,
        "/api/focus": focus,
        "/api/feed/asks": feed_asks,
        "/api/feed/health": feed_health_view,
        "/api/daylog": daylog_view,
        "/api/inbox": inbox_view,
        "/api/checklist": checklist_view,
        "/api/standup": standup_view,
        "/api/config": config_view,
        "/api/notifications": notifications_view,
        "/api/actions": actions_view,
        "/api/review": review,
        "/api/setup": setup,
        "/api/cards": cards_view,
        "/api/library": library_view,
        "/api/teach": teach_view,
        "/api/drill": drill_view,
        "/api/goals": goals_view,
        "/api/teach/improvements": improvements_view,
        "/api/mistakes": mistakes_view,
        "/api/exercises": exercises_view,
        "/api/changes": changes_view,
        "/api/ping": ping,
        "/api/reports": reports_view,
        "/api/time-saved": time_saved_view,
        "/api/promises": promises_view,
    }
    if url.path == "/api/week":
        return week(int(q.get("days", ["7"])[0]))
    if url.path == "/api/feed/asks" and (count := q.get("limit", [""])[0]).isdigit():
        return feed_asks(int(count))
    if url.path.startswith("/api/focus/"):
        return focus_block(unquote(url.path.split("/api/focus/", 1)[1]), cache)
    if url.path.startswith("/api/library/"):
        return library_page(unquote(url.path.split("/api/library/", 1)[1]))
    if url.path.startswith("/api/daylog/"):
        return daylog_view(unquote(url.path.split("/api/daylog/", 1)[1]))
    if url.path.startswith("/api/interviews/"):
        return interview_detail(unquote(url.path.split("/api/interviews/", 1)[1]))
    if url.path.startswith("/api/reports/"):
        return report_page(unquote(url.path.split("/api/reports/", 1)[1]))
    if url.path in routes:
        return routes[url.path]()
    raise ApiError(HTTPStatus.NOT_FOUND, "no such endpoint")


def app_url(port: int, view: str | None = None) -> str:
    """The link that opens the running web app (started once if needed), with its token in the fragment."""
    url = start_background(port)
    if view:
        base, _, frag = url.partition("#")
        url = f"{base}#/{view.strip('/')}&{frag}"
    return url


def setup() -> dict[str, Any]:

    from . import connectors
    from .work import evidence

    conns = [c for c in evidence.sources_list() if c.get("kind") in connectors.KINDS]
    return {
        "role": machine.role(),
        "plugins": [{"name": p["name"], "version": p["version"], "extends": p.get("extends"), "exec": p["exec_allowed"], "provides": sorted((p["manifest"].get("provides") or {}).keys())} for p in plugins.installed()],
        "policy": plugins.effective_policy(),
        "connectors": [connectors.status(c) for c in conns],
    }


# ---------- the web token (issue #141) ----------

SIGN_IN = "fix: run `careerbrew open` in a terminal"


def _now() -> datetime:
    from .calendar import now

    return now()


def web_token_path() -> Path:
    return paths.sub("day", "web-token.json")


def _token_days() -> int:
    """`web_token_max_days`; a hand-edited value that is not a number falls back to the default."""
    try:
        return max(1, int(config.get("web_token_max_days")))
    except (TypeError, ValueError):
        return int(config.DEFAULTS["web_token_max_days"][0])


def _stored_token() -> tuple[str, datetime] | None:
    """(token, created) from `day/web-token.json`; None when it is missing or unreadable."""
    try:
        st = store.read_json(web_token_path(), {}) or {}
        tok, created = str(st.get("token") or ""), datetime.fromisoformat(str(st["created"]))
    except (KeyError, ValueError, TypeError, AttributeError):
        return None
    if created.tzinfo is None or not re.fullmatch(r"[A-Za-z0-9_-]{20,}", tok):
        return None
    return tok, created


def _expires(created: datetime) -> datetime:
    return created + timedelta(days=_token_days())


def _write_token(tok: str, reason: str) -> str:
    store.write_json(web_token_path(), {"token": tok, "created": _now().isoformat(timespec="seconds")})
    events.emit("web.token", status=reason)  # first, request, age, or adopted; never the token
    return tok


def _token(rotate: bool = False) -> str:
    """The stable web token; call it under `_web_lock`.

    Reused by every start, so open tabs and bookmarks survive a restart. A new one only when there is
    none, when `rotate` asks (`careerbrew web rotate-token`), or after `web_token_max_days`. The first
    start after the upgrade adopts the per-run token that `day/web.json` held, so an open tab keeps working.
    """
    if rotate:
        return _write_token(pysecrets.token_urlsafe(32), "request")
    st = _stored_token()
    if st is not None:
        return st[0] if _now() < _expires(st[1]) else _write_token(pysecrets.token_urlsafe(32), "age")
    state = store.read_json(web_state_path(), {}) or {}
    old = str(state.pop("token", "") or "")
    if re.fullmatch(r"[A-Za-z0-9_-]{20,}", old):
        store.write_json(web_state_path(), state)  # web.json keeps the pid and port only
        return _write_token(old, "adopted")
    return _write_token(pysecrets.token_urlsafe(32), "first")


def web_token() -> str:
    """The stable web token (made when missing or expired). Never print it: it goes only into the one-time open link."""
    with _web_lock():
        return _token()


def rotate_token() -> dict[str, Any]:
    """A new web token: every open tab, bookmark, and sign-in cookie stops working at once (servers re-read the file)."""
    with _web_lock():
        _token(rotate=True)
    return {"days": _token_days()}


class TokenSource:
    """The token a server accepts, and when it expires.

    Lifecycle: server-instance scope; `make_server` makes one per server, and it goes away with that
    server. A pinned token (`CAREERBREW_SERVE_TOKEN`, for tests and e2e runs) never changes or expires.
    Otherwise it reads `day/web-token.json` and reads it again whenever the file changes (inode, size,
    or mtime), so `careerbrew web rotate-token` signs out open tabs without a restart. The expiry comes
    from the file's `created` and the current `web_token_max_days`.
    """

    def __init__(self, pinned: str | None = None) -> None:
        self.pinned = pinned or ""
        self._lock = threading.Lock()
        self._sig: tuple[int, int, int] | None = None
        self._stored: tuple[str, datetime] | None = None

    def current(self) -> tuple[str, datetime | None]:
        if self.pinned:
            return self.pinned, None
        try:
            s = web_token_path().stat()
        except FileNotFoundError:
            return "", None
        with self._lock:
            sig = (s.st_ino, s.st_size, s.st_mtime_ns)
            if sig != self._sig:
                self._stored, self._sig = _stored_token(), sig
            stored = self._stored
        return (stored[0], _expires(stored[1])) if stored else ("", None)


# ---------- HTTP ----------


class Handler(BaseHTTPRequestHandler):
    port = 0
    tokens: TokenSource  # set per server by make_server
    cache: ResponseCache  # set per server by make_server
    # The key the page encrypts its per-tab copy of each view with (issue #142). Lifecycle: server-instance
    # scope, like the token: random per server, held only in this process's memory, never written to disk.
    tab_key = ""

    def log_message(self, *_args: Any) -> None:  # keep stdout content-free
        return

    def _send(self, status: int, body: bytes, ctype: str = "application/json", cache: str | None = None, extra: tuple[tuple[str, str], ...] = ()) -> None:
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        if cache:
            self.send_header("X-Careerbrew-Cache", cache)  # hit: answered from the server's memory; miss: rebuilt
        for name, value in extra:
            self.send_header(name, value)
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("Content-Security-Policy", "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'")
        self.send_header("Cross-Origin-Resource-Policy", "same-origin")  # no other site may load a response, the token's included
        self.send_header("Cache-Control", "no-store")  # personal data never lands in the browser's disk cache
        self.end_headers()
        self.wfile.write(body)

    def _json(self, status: int, data: Any, extra: tuple[tuple[str, str], ...] = ()) -> None:
        self._send(status, _encode(data), extra=extra)

    def _host_ok(self) -> bool:
        return self.headers.get("Host", "") in (f"127.0.0.1:{self.port}", f"localhost:{self.port}")

    def _check_token(self, given: str) -> datetime | None:
        """Raise 401 unless `given` is this server's token and it has not expired; return when it expires (None: never)."""
        tok, expires = self.tokens.current()
        if not tok or not pysecrets.compare_digest(given.encode("utf-8", "replace"), tok.encode()):
            raise ApiError(HTTPStatus.UNAUTHORIZED, f"missing or wrong token ({SIGN_IN})")
        if expires is not None and _now() >= expires:
            raise ApiError(HTTPStatus.UNAUTHORIZED, f"the sign-in expired after web_token_max_days ({SIGN_IN})")
        return expires

    def _guard(self, path: str) -> None:
        if not self._host_ok():
            raise ApiError(HTTPStatus.FORBIDDEN, "bad host")
        self._check_token(self.headers.get("X-Careerbrew-Token", ""))
        if machine.role() == "corporate" and path.startswith(JOB_PATHS):
            raise ApiError(HTTPStatus.FORBIDDEN, "job-hunt views are personal-only; this is a corporate machine")

    # ---------- sign-in (issue #141) ----------

    def _own_page(self, required: bool) -> bool:
        """True when the browser says this app's own page sent the request (`Sec-Fetch-Site: same-origin`).

        Browsers set the header and pages cannot forge it. `required=False` also lets a client that sends
        none through (an older browser, a test): the same-origin policy still keeps the answer from other sites.
        """
        site = self.headers.get("Sec-Fetch-Site")
        return site == "same-origin" or (site is None and not required)

    def _cookie_name(self) -> str:
        return f"careerbrew-{self.port}"  # cookies ignore the port: two careerbrew apps on 127.0.0.1 keep one each

    def _cookie_token(self) -> str:
        for part in self.headers.get("Cookie", "").split(";"):
            name, _, value = part.strip().partition("=")
            if name == self._cookie_name():
                return value.strip()
        return ""

    def _session(self, method: str, path: str) -> None:
        """`/api/session`: POST sets the sign-in cookie for a tab with the token, GET gives a tab the token from it;
        POST `/api/session/open` opens a signed-in tab in the default browser (the sign-in page's button)."""
        action = {("POST", "/api/session"): "set", ("GET", "/api/session"): "restore", ("POST", "/api/session/open"): "open"}.get((method, path))
        if action is None:
            return self._json(HTTPStatus.NOT_FOUND, {"error": "no such endpoint"})
        try:
            if not self._host_ok():
                raise ApiError(HTTPStatus.FORBIDDEN, "bad host")
            if not self._own_page(required=action == "open"):
                raise ApiError(HTTPStatus.FORBIDDEN, f"only careerbrew's own pages may sign in ({SIGN_IN})")
            if action == "set":
                expires = self._check_token(self.headers.get("X-Careerbrew-Token", ""))
                status, body, extra = self._set_cookie(expires)
            elif action == "restore":
                self._check_token(self._cookie_token())
                status, body, extra = HTTPStatus.OK, {"token": self.tokens.current()[0]}, ()
            else:
                status, body, extra = self._open_tab(), {"opened": True}, ()
        except ApiError as exc:
            events.emit("web.session", route=action, status="refused", code=exc.status, cookie=bool(self._cookie_token()))
            return self._json(exc.status, {"error": str(exc)})
        events.emit("web.session", route=action, status="ok", cookie=bool(self._cookie_token()))  # cookie: the request carried one
        self._json(status, body, extra)

    def _set_cookie(self, expires: datetime | None) -> tuple[int, dict[str, Any], tuple[tuple[str, str], ...]]:
        left = int((expires - _now()).total_seconds()) if expires is not None else _token_days() * 86400
        body: dict[str, Any] = {"signed_in": True, "days_left": left // 86400}
        if self.headers.get("Host") != f"127.0.0.1:{self.port}":  # bound to 127.0.0.1, the host every careerbrew link uses
            return HTTPStatus.OK, {**body, "cookie": False}, ()
        # Host-only (no Domain), HttpOnly (no script reads it), SameSite=Strict (never sent from another site).
        cookie = f"{self._cookie_name()}={self.tokens.current()[0]}; Max-Age={max(left, 0)}; Path=/; HttpOnly; SameSite=Strict"
        return HTTPStatus.OK, {**body, "cookie": True}, (("Set-Cookie", cookie),)

    def _open_tab(self) -> int:
        """Open the one-time sign-in link in the default browser, the way `careerbrew open` does; the reply never holds the token."""
        import webbrowser

        length = int(self.headers.get("Content-Length", "0") or 0)
        if not 0 <= length <= 1000:
            raise ApiError(HTTPStatus.BAD_REQUEST, "Content-Length must be 0 to 1000")
        body = json.loads(self.rfile.read(length) or b"{}")
        view = str(body.get("view", "")) if isinstance(body, dict) else ""
        view = view if re.fullmatch(r"[a-z]{1,20}", view) else "today"
        token = self.tokens.pinned or web_token()  # a new token when the old one expired: the button still works
        if not webbrowser.open(f"http://127.0.0.1:{self.port}/#/{view}&token={token}"):
            raise ApiError(HTTPStatus.CONFLICT, f"could not open a browser from the web app here ({SIGN_IN})")
        return HTTPStatus.OK

    def do_GET(self) -> None:  # noqa: N802
        url = urlsplit(self.path)
        if url.path.startswith("/api/session"):
            return self._session("GET", url.path)
        if not url.path.startswith("/api/"):
            return self._static(url.path)
        t0 = monotonic()
        try:
            self._guard(url.path)
            if url.path == "/api/tab-key":
                body, cache = _encode({"key": self.tab_key if config.get("web_tab_cache") is not False else None}), None
            else:
                body, cache = respond(self.cache, url)
            events.emit("web.api", status="ok", route=_route(url.path), cached=cache == "hit", ms=int((monotonic() - t0) * 1000))
            self._send(HTTPStatus.OK, body, cache=cache)
        except ApiError as exc:
            self._json(exc.status, {"error": str(exc)})
        except BAD_INPUT as exc:
            self._json(HTTPStatus.BAD_REQUEST, {"error": f"bad request: {exc}"[:300]})

    def do_POST(self) -> None:  # noqa: N802
        url = urlsplit(self.path)
        if url.path.startswith("/api/session"):
            try:
                return self._session("POST", url.path)
            except BAD_INPUT as exc:
                return self._json(HTTPStatus.BAD_REQUEST, {"error": f"bad request: {exc}"[:300]})
        try:
            self._guard(url.path)
            length = int(self.headers.get("Content-Length", "0") or 0)
            limit = VOICE_MAX_BYTES if url.path == "/api/drill/voice" else 64_000
            if not 0 <= length <= limit:
                raise ApiError(HTTPStatus.BAD_REQUEST, f"Content-Length must be 0 to {limit}")
            body = json.loads(self.rfile.read(length) or b"{}")
            if not isinstance(body, dict):
                raise ApiError(HTTPStatus.BAD_REQUEST, "the request body must be a JSON object")
            try:
                result = post(url.path, body)
            finally:
                if url.path not in KEEPS_CACHE:
                    self.cache.clear()  # the owner acted: every view rebuilds on its next read, even one that runs before this reply arrives
            self._json(HTTPStatus.OK, result)
        except ApiError as exc:
            self._json(exc.status, {"error": str(exc)})
        except BAD_INPUT as exc:
            self._json(HTTPStatus.BAD_REQUEST, {"error": f"bad request: {exc}"[:300]})

    def _static(self, path: str) -> None:
        rel = "index.html" if path in ("", "/") else path.lstrip("/")
        f = (DIST / rel).resolve()
        if not f.is_relative_to(DIST.resolve()) or not f.is_file():
            f = DIST / "index.html"
        if not f.exists():
            return self._send(HTTPStatus.NOT_FOUND, b"web app not built: cd web && npm ci && npm run build", "text/plain")
        self._send(HTTPStatus.OK, f.read_bytes(), mimetypes.guess_type(str(f))[0] or "application/octet-stream")


# POSTs that change no data a view reads, so every cached view stays: a timing writes only the event log.
KEEPS_CACHE = {"/api/timing"}

# Errors that mean "the request was bad"; RecursionError covers deeply nested JSON.
BAD_INPUT = (ValueError, TypeError, KeyError, AttributeError, OverflowError, RecursionError)


def _finite(data: Any) -> Any:
    """Strict JSON has no NaN or Infinity; browsers reject them, so send null."""
    if isinstance(data, float) and not math.isfinite(data):
        return None
    if isinstance(data, dict):
        return {k: _finite(v) for k, v in data.items()}
    if isinstance(data, list | tuple):
        return [_finite(v) for v in data]
    return data


def post(path: str, body: dict[str, Any]) -> dict[str, Any]:
    from .day import watch
    from .work import priorities

    if path.startswith("/api/inbox/") and path.rsplit("/", 1)[-1] in ("accept", "dismiss", "answer", "reopen"):
        from .day import inbox
        from .day import log as daylog

        item_id, action = unquote(path.split("/")[3]), path.rsplit("/", 1)[-1]
        try:
            if action == "accept":
                item = inbox.accept(item_id, from_ui=True)  # the owner's browser: goal proposals may apply here
            elif action == "dismiss":
                item = inbox.dismiss(item_id)
            elif action == "reopen":
                item = inbox.reopen(item_id)
            else:
                item = inbox.answer(item_id, str(body.get("text", "")))
        except inbox.InboxError as exc:
            raise ApiError(HTTPStatus.BAD_REQUEST, str(exc)) from None
        daylog.save()
        return {"item": item, **daylog_view()}
    if path.startswith("/api/inbox-group/") and path.rsplit("/", 1)[-1] in ("accept", "dismiss"):
        from .day import inbox
        from .day import log as daylog

        group_id, action = unquote(path.split("/")[3]), path.rsplit("/", 1)[-1]
        if body.get("yes") is not True:  # Accept all and Dismiss all are hard to undo: the page asks first, then sends yes
            raise ApiError(HTTPStatus.BAD_REQUEST, f"{action} all decides every suggestion of the group: confirm it (send yes: true)")
        ids = body.get("ids")
        if not isinstance(ids, list) or not ids or not all(isinstance(x, str) for x in ids):  # rows that joined the group after the confirm stay open
            raise ApiError(HTTPStatus.BAD_REQUEST, f"{action} all decides the suggestions you confirmed: send their ids (the group's ids)")
        try:
            result = inbox.decide_group(group_id, action, ids=ids, from_ui=True)
        except inbox.InboxError as exc:
            raise ApiError(HTTPStatus.BAD_REQUEST, str(exc)) from None
        daylog.save()
        return {"result": result, **daylog_view()}
    if path.startswith("/api/notifications/") and path.rsplit("/", 1)[-1] in ("read", "accept", "dismiss", "snooze"):
        from . import notify

        try:
            notify.act(unquote(path.split("/")[3]), path.rsplit("/", 1)[-1], via="ui")  # the owner's browser: a goal may be approved here
        except notify.NotifyError as exc:
            raise ApiError(HTTPStatus.BAD_REQUEST, str(exc)) from None
        return notify.view()
    if path.startswith("/api/promises/") and path.rsplit("/", 1)[-1] in ("done", "dropped"):
        from .work import projects, promises

        try:
            promises.close(unquote(path.split("/")[3]), path.rsplit("/", 1)[-1])
        except projects.ProjectError as exc:
            raise ApiError(HTTPStatus.BAD_REQUEST, str(exc)) from None
        return promises_view()
    if path.startswith("/api/actions/"):
        from . import actions

        values = body.get("values") or {}
        if not isinstance(values, dict):
            raise ApiError(HTTPStatus.BAD_REQUEST, "send {values: {...}, confirm: true|false}")
        try:
            return actions.run(unquote(path.split("/api/actions/", 1)[1]), values, confirm=bool(body.get("confirm")))
        except actions.ActionError as exc:
            raise ApiError(HTTPStatus.BAD_REQUEST, str(exc)) from None
    if path == "/api/timing":
        from . import time_saved

        try:
            time_saved.record_web(body.get("saves"), body.get("step"), body.get("seconds"))
        except ValueError as exc:
            raise ApiError(HTTPStatus.BAD_REQUEST, str(exc)) from None
        return {"logged": True}
    if path == "/api/pm/draft":
        from datetime import date as _date

        from .day import log as daylog

        day = str(body.get("day", ""))
        try:
            _date.fromisoformat(day)
        except ValueError:
            raise ApiError(HTTPStatus.BAD_REQUEST, "send {day: YYYY-MM-DD, text: ...}") from None
        text = str(body.get("text", ""))
        try:
            daylog.save_pm_draft(day, text)
        except daylog.UnlinkedRefError as exc:
            raise ApiError(HTTPStatus.BAD_REQUEST, str(exc)) from None
        return {"saved": True, "edited": bool(text.strip())}
    if path == "/api/pm/send":
        from .day import log as daylog

        try:
            return {"sent": daylog.send_pm(str(body.get("text", "")))}
        except (daylog.SendError, daylog.UnlinkedRefError) as exc:
            raise ApiError(HTTPStatus.BAD_REQUEST, str(exc)) from None
    if path == "/api/config":
        from . import config

        try:
            config.set_value(str(body.get("key", "")), str(body.get("value", "")))
        except KeyError as exc:
            raise ApiError(HTTPStatus.BAD_REQUEST, str(exc).strip("'\"")) from None
        return {"settings": config.effective()}

    if path == "/api/drill/voice":
        return _drill_voice(body)
    if path == "/api/teach/complete":
        from . import teacher

        step, result = body.get("step"), body.get("result")
        if not isinstance(step, dict) or not isinstance(result, dict) or "kind" not in step:
            raise ApiError(HTTPStatus.BAD_REQUEST, "send {step, result}")
        return teacher.complete(step, result)
    if path == "/api/teach/draft":
        from . import teacher

        try:
            teacher.save_draft(str(body.get("id", "")), str(body.get("code", "")))
        except ValueError as exc:
            raise ApiError(HTTPStatus.BAD_REQUEST, str(exc)) from None
        return {"saved": True}
    if path == "/api/teach/feedback":
        from . import teacher

        rating = body.get("rating")
        return {"feedback": teacher.feedback(str(body.get("text", "")), str(body.get("step", "")), int(rating) if rating is not None else None)}
    if path == "/api/goals":
        from . import goals

        out: dict[str, Any] = {}
        if "long_term" in body:
            out = goals.set_long_term(list(body["long_term"]))
        if "upcoming" in body:
            out = goals.set_upcoming(body["upcoming"])
        if "queue" in body:
            out = goals.set_queue(list(body["queue"]))
        return out or goals.load()
    if path in ("/api/jobs/choose", "/api/jobs/unchoose"):
        from .jobs import strategy

        try:
            picks = strategy.choose(str(body["posting_id"]), str(body["tier"])) if path.endswith("/choose") else strategy.unchoose(str(body["posting_id"]))
        except KeyError as exc:
            raise ApiError(HTTPStatus.NOT_FOUND, str(exc).strip("'\"")) from None
        strategy.build()
        return {"chosen": picks}
    if path.startswith("/api/approvals/") and path.endswith("/decide"):
        from . import approvals

        try:
            return {"approval": approvals.decide(unquote(path.split("/")[3]), str(body.get("decision", "")), str(body.get("note", "")))}
        except KeyError as exc:
            raise ApiError(HTTPStatus.NOT_FOUND, str(exc).strip("'\"")) from None
    if path == "/api/tasks":
        t = priorities.add(str(body["title"])[:200], int(body.get("impact", 3)), int(body.get("urgency", 3)), float(body.get("hours", 1)), [str(c) for c in body.get("competencies", [])][:10], body.get("due"))
        return {"task": t}
    if path.startswith("/api/tasks/") and path.endswith("/done"):
        h = priorities.hours(body["hours"])
        try:
            return {"task": priorities.done(path.split("/")[3], h)}
        except KeyError as exc:
            raise ApiError(HTTPStatus.NOT_FOUND, str(exc).strip("'\"")) from None
    if path.startswith("/api/cards/") and path.endswith("/review"):
        from .tutor import cards

        seconds = body.get("seconds")
        try:
            return {"review": cards.review(unquote(path.split("/")[3]), str(body.get("rating", "")), seconds=int(seconds) if seconds is not None else None)}
        except KeyError as exc:
            raise ApiError(HTTPStatus.NOT_FOUND, str(exc).strip("'\"")) from None
    if path.startswith("/api/explanations/") and path.endswith("/flag"):
        from .tutor import public

        try:
            return {"flag": public.flag(unquote(path.split("/")[3]), str(body.get("note", "")))}
        except KeyError as exc:
            raise ApiError(HTTPStatus.NOT_FOUND, str(exc).strip("'\"")) from None
    if path.startswith("/api/watch/"):
        action = path.rsplit("/", 1)[1]
        fn = {"on": watch.start, "off": watch.stop, "pause": watch.pause, "resume": watch.resume, "reload": watch.reload}.get(action)
        if fn is None:
            raise ApiError(HTTPStatus.NOT_FOUND, "unknown watcher action")
        fn()
        return {"watcher": {k: v for k, v in watch.status().items() if k in ("state", "running", "reloads")}}
    raise ApiError(HTTPStatus.NOT_FOUND, "no such endpoint")


class Server(ThreadingHTTPServer):
    # The stdlib listen backlog is 5: a page that asks for its blocks at once (the Dashboard's 9, issue #142)
    # beside its own reads overflowed it on a busy machine, and the kernel reset the extra connections.
    request_queue_size = 128

    def server_bind(self) -> None:
        # The stdlib bind asks the resolver for the host's full name (`socket.getfqdn`). With no resolver
        # answer that takes many seconds, and a start missed its wait (hosted macOS run, 2026-11-13).
        # The server listens on a literal address, so its name is that address.
        import socketserver

        socketserver.TCPServer.server_bind(self)
        self.server_name, self.server_port = str(self.server_address[0]), int(self.server_address[1])


def make_server(port: int = 8765, token: str | None = None) -> ThreadingHTTPServer:
    """A server on 127.0.0.1:`port`. `token` pins its token (tests); None: the stable token in `day/web-token.json`."""
    import base64

    tab_key = base64.b64encode(pysecrets.token_bytes(32)).decode()
    handler = type("BoundHandler", (Handler,), {"tokens": TokenSource(token), "cache": ResponseCache(), "tab_key": tab_key})
    srv = Server(("127.0.0.1", port), handler)
    handler.port = srv.server_address[1]
    return srv


def serve_in_thread(port: int, token: str | None = None) -> tuple[ThreadingHTTPServer, threading.Thread]:
    srv = make_server(port, token)
    t = threading.Thread(target=srv.serve_forever, daemon=True)
    t.start()
    return srv, t


def run(port: int = 8765) -> None:
    import sys

    pinned = os.environ.get("CAREERBREW_SERVE_TOKEN") or None  # e2e runs pin it; otherwise the stable token
    stored = _stored_token()
    # start_background writes the token before it starts this process and holds the lock while it waits: read, never wait.
    token = pinned or (stored[0] if stored and _now() < _expires(stored[1]) else web_token())
    srv = make_server(port, pinned)
    base = f"http://127.0.0.1:{srv.server_address[1]}/"
    if sys.stdout.isatty():  # the sign-in link with its token: only in your own terminal, never to an agent or a log
        print(f"careerbrew web app: {base}#token={token}")
    else:
        print(f"careerbrew web app: {base} (sign in with `careerbrew open`; the link with its token prints only in a terminal)")
    print("local only (127.0.0.1); the token stays until `careerbrew web rotate-token` or web_token_max_days. Ctrl-C stops it.", flush=True)  # a background launch shows the URL at once
    Warmer(srv.RequestHandlerClass.cache).start()  # type: ignore[attr-defined]  # builds the work views now and after new data
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        srv.server_close()


def web_state_path() -> Path:
    return paths.sub("day", "web.json")


@contextlib.contextmanager
def _web_lock() -> Iterator[None]:
    """Serialize starts: two callers at once (`careerbrew open`, a notification click, `careerbrew`) never start two servers.

    Lifecycle: an fcntl lock on `day/web.lock`, held only while one caller checks, stops, or starts the server;
    the OS releases it if that process dies.
    """
    fh = paths.sub("day", "web.lock").open("a")
    try:
        fcntl.flock(fh, fcntl.LOCK_EX)
        yield
    finally:
        fcntl.flock(fh, fcntl.LOCK_UN)
        fh.close()


def _ping(port: int, token: str, timeout: float = 2) -> bool:
    """True when the server on `port` is a careerbrew server that accepts `token` (a link to it works)."""
    import urllib.request

    req = urllib.request.Request(f"http://127.0.0.1:{port}/api/ping", headers={"X-Careerbrew-Token": token, "Host": f"127.0.0.1:{port}"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:  # noqa: S310 (local)
            return bool(json.loads(r.read() or b"{}").get("ok"))
    except (OSError, ValueError, AttributeError):
        return False


def listening_pids(port: int) -> list[int]:
    """Pids that listen on TCP `port` (lsof); empty when lsof is missing or finds none."""
    try:
        out = subprocess.run(["lsof", "-nP", "-t", f"-iTCP:{port}", "-sTCP:LISTEN"], capture_output=True, text=True, timeout=10, check=False).stdout  # noqa: S607
    except (OSError, subprocess.SubprocessError):
        return []
    return sorted({int(x) for x in out.split() if x.isdigit()})


_SERVE_CMD = re.compile(r"(?:^|[\s/])careerbrew\s+serve(?:\s|$)")


def is_careerbrew_server(pid: int) -> bool:
    """True only when the process runs `careerbrew serve` (from `ps`). Anything else, or no answer, is never stopped."""
    try:
        cmd = subprocess.run(["ps", "-o", "command=", "-p", str(pid)], capture_output=True, text=True, timeout=10, check=False).stdout  # noqa: S607
    except (OSError, subprocess.SubprocessError):
        return False
    return bool(_SERVE_CMD.search(cmd.strip()))


def _stop(pids: list[int], port: int) -> None:
    """SIGTERM, then SIGKILL; return once none of `pids` listens on `port` (a stopped child may stay a zombie)."""
    for sig in (signal.SIGTERM, signal.SIGKILL):
        for pid in pids:
            with contextlib.suppress(ProcessLookupError, PermissionError):
                os.kill(pid, sig)
        deadline = time.monotonic() + 3
        while set(pids) & set(listening_pids(port)):
            if time.monotonic() > deadline:
                break
            time.sleep(0.1)
        else:
            return


def stop_untracked(port: int, only: set[int] | None = None) -> int:
    """Stop careerbrew servers on `port` that the web state does not track (issue #118); return how many.

    A program that is not a careerbrew server is never stopped: unless `only` names the pids to look at,
    it raises RuntimeError with that program's pid and the fix.
    """
    pids = [p for p in listening_pids(port) if p != os.getpid() and (only is None or p in only)]
    ours = [p for p in pids if is_careerbrew_server(p)]
    others = [p for p in pids if p not in ours]
    if others and only is None:
        raise RuntimeError(f"the web app cannot start on port {port}: pid {', '.join(map(str, others))} uses it and is not a careerbrew server (fix: careerbrew config set serve_port <free port>, or stop that program)")
    if ours:
        _stop(ours, port)
        events.emit("web.untracked", status="stopped", count=len(ours))
    return len(ours)


# How long a start waits for a new server to answer. A cold start took 1.5 to 2.4 s alone at load 84 and passed
# the old 5 s wait in a parallel test run at load 125 (2026-10-25); a server that cannot bind its port exits at once.
START_WAIT_SECONDS = 20


def start_background(port: int) -> str:
    """Start the web app as a detached process (once) and return its URL with the token.

    The token is the stable one in `day/web-token.json` (encrypted at rest, issue #141), so a restart
    keeps open tabs and bookmarks working; `day/web.json` tracks the pid and port, so a second
    `careerbrew` reuses the running app instead of starting another one. Under `day/web.lock`, a
    start reuses the tracked server only when it answers to the token, stops a careerbrew server
    that nothing tracks (a crash, a test run, another data folder) instead of failing, and writes
    the state before it waits, so no server it starts is left untracked.
    """
    import sys

    with _web_lock():
        token = _token()
        st = store.read_json(web_state_path(), {}) or {}
        if st.get("port"):
            sport = int(st["port"])
            # A busy machine can make the tracked server slow: when its pid still listens, wait longer before replacing it (open tabs keep their token).
            if _ping(sport, token) or (st.get("pid") and int(st["pid"]) in listening_pids(sport) and _ping(sport, token, timeout=10)):
                return f"http://127.0.0.1:{sport}/#token={token}"
        if st.get("pid") and st.get("port") and int(st["port"]) != port:  # the port setting changed: stop the old one too
            stop_untracked(int(st["port"]), only={int(st["pid"])})
        stop_untracked(port)
        log = paths.sub("logs", "web.log").open("a")
        # The server reads the token from the encrypted day/web-token.json, never from its environment (`ps` shows that);
        # a test run's pinned token never reaches it. Its stdout (the plain URL) goes nowhere.
        env = {k: v for k, v in os.environ.items() if k != "CAREERBREW_SERVE_TOKEN"}
        proc = subprocess.Popen([sys.executable, "-m", "careerbrew", "serve", "--port", str(port)], stdout=subprocess.DEVNULL, stderr=log, stdin=subprocess.DEVNULL, start_new_session=True, env=env)  # noqa: S603
        store.write_json(web_state_path(), {"pid": proc.pid, "port": port})
        deadline = time.monotonic() + START_WAIT_SECONDS
        while time.monotonic() < deadline:
            if proc.poll() is not None:  # it could not bind the port (another program has it) or crashed: no wait
                break
            if _ping(port, token, timeout=1):  # our server, with our token: another program on the port would answer 401 or 404
                return f"http://127.0.0.1:{port}/#token={token}"
            time.sleep(0.1)
        proc.kill()
        store.write_json(web_state_path(), {})
    raise RuntimeError(f"the web app did not start on port {port}: another program may use it (fix: careerbrew config set serve_port <free port>, or stop the other one)")


def dist_exists() -> bool:
    return Path(DIST / "index.html").exists()
