"""Your team roster: the people whose work-feed events are worth a look (issue #180).

Record: `<data root>/work/team.yaml` (encrypted at rest like every data file):
  team:
    - name: Sam Lee          # how the person shows in a tool; matched in event text as a whole word
      chat: U0SAM            # chat user id (Slack); matched as the actor and as a <@U0SAM> mention
      email: sam@example.com
      login: samlee          # code-host login; matched as the actor and as @samlee

Lifecycle: this module owns the file; nothing is kept in process memory between calls. A row
lives until `work team remove <name>`. `_locked()` serializes writers with an fcntl lock on
`work/.team.lock`. Names and ids are business data: the CLI prints them only when business reading
is on, and errors never name a teammate.
"""

from __future__ import annotations

import contextlib
import fcntl
import re
from collections.abc import Iterator
from pathlib import Path
from typing import Any

from .. import events, paths, store

FIELDS = ("chat", "email", "login")
NAME_MAX, FIELD_MAX = 80, 120


class TeamError(ValueError):
    pass


@contextlib.contextmanager
def _locked() -> Iterator[None]:
    """Serialize read-modify-write of the roster (CLI, web app, an agent) with an fcntl lock the OS frees on exit."""
    lock = paths.sub("work", ".team.lock")
    lock.parent.mkdir(parents=True, exist_ok=True)
    with open(lock, "a") as fh:
        fcntl.flock(fh, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(fh, fcntl.LOCK_UN)


def path() -> Path:
    return paths.sub("work", "team.yaml")


def load() -> list[dict[str, str]]:
    rows = (store.read_yaml(path(), {}) or {}).get("team") or []
    return [{k: str(r.get(k) or "") for k in ("name", *FIELDS)} for r in rows if isinstance(r, dict) and r.get("name")]


def _save(rows: list[dict[str, str]]) -> None:
    store.write_yaml(path(), {"team": [{k: v for k, v in r.items() if v} for r in rows]})


def _one_line(value: str, what: str, limit: int) -> str:
    value = " ".join((value or "").split())
    if len(value) > limit:
        raise TeamError(f"{what} is at most {limit} characters")
    return value


def add(name: str, chat: str = "", email: str = "", login: str = "") -> dict[str, Any]:
    """Add a teammate, or fill in the ids of one already on the roster (matched by name, any case). `changed` says whether the roster moved."""
    name = _one_line(name, "a name", NAME_MAX)
    if not name:
        raise TeamError('a teammate needs a name (fix: careerbrew work team add "Sam Lee" --chat U0SAM)')
    given = {"chat": _one_line(chat, "--chat", FIELD_MAX), "email": _one_line(email, "--email", FIELD_MAX).lower(), "login": _one_line(login, "--login", FIELD_MAX).lstrip("@")}
    if given["chat"] and not re.fullmatch(r"[UW][A-Z0-9]{2,30}", given["chat"]):
        raise TeamError("--chat is a Slack user id such as U0SAM (Slack: the person's profile, More, Copy member ID)")
    if given["email"] and "@" not in given["email"]:
        raise TeamError(f"--email {given['email']!r} is not an email address")
    with _locked():
        rows = load()
        row = next((r for r in rows if r["name"].lower() == name.lower()), None)
        added = row is None
        if row is None:
            row = {"name": name, **dict.fromkeys(FIELDS, "")}
            rows.append(row)
        before = dict(row)
        row.update({k: v for k, v in given.items() if v})
        changed = added or row != before
        if changed:
            _save(rows)
    if added:
        events.emit("work.team", status="ok", action="add", count=len(rows))
    else:
        events.emit("work.team", status="ok", action="set", count=len(rows))
    return {**row, "changed": changed}


def remove(name: str) -> None:
    name = " ".join(name.split()).lower()
    with _locked():
        rows = load()
        kept = [r for r in rows if r["name"].lower() != name]
        if len(kept) == len(rows):  # no names here: an error reaches an agent's stdout unfiltered
            raise TeamError(f"that name is not on your team ({len(rows)} teammate(s); see: careerbrew work team list)")
        _save(kept)
    events.emit("work.team", status="ok", action="remove", count=len(kept))


def roster() -> list[dict[str, str]]:
    """Your roster plus each installed plugin's `live.team` file (issue #267); a name on yours wins.
    A plugin's rows are read only: `add` and `remove` change your file alone."""
    from .live import plugin_files

    rows = load()
    names = {r["name"].lower() for r in rows}
    import yaml

    for _origin, file in plugin_files("team"):
        try:
            data = store.read_yaml(file, {}) or {}
        except yaml.YAMLError:  # a broken plugin roster must not stop every feed ingest: skip it and say so in the log
            events.emit("work.team", status="error", action="plugin-roster", count=len(rows))
            continue
        for r in (data.get("team") if isinstance(data, dict) else None) or []:
            if isinstance(r, dict) and r.get("name") and str(r["name"]).lower() not in names:
                rows.append({k: str(r.get(k) or "") for k in ("name", *FIELDS)})
                names.add(str(r["name"]).lower())
    return rows


def matcher(rows: list[dict[str, str]] | None = None):
    """A function from (actor id, from, text) to the names of the teammates an event is by or about."""
    rows = roster() if rows is None else rows
    pats: list[tuple[str, re.Pattern[str], re.Pattern[str] | None, set[str]]] = []
    for r in rows:
        ids = {v.lower() for v in (r["name"], r["chat"], r["email"], r["login"]) if v}
        # The name matches as written (a capital "Max", never "max retries"); ids in any case.
        ids_in_text = [re.escape(f"<@{r['chat']}>")] * bool(r["chat"]) + [re.escape(f"@{r['login']}")] * bool(r["login"])
        by_name = re.compile(rf"(?<![\w@]){re.escape(r['name'])}(?![\w])")
        by_id = re.compile(rf"(?<![\w@])(?:{'|'.join(ids_in_text)})(?![\w])", re.I) if ids_in_text else None
        pats.append((r["name"], by_name, by_id, ids))

    def match(actor_id: str, sender: str, text: str) -> list[str]:
        who = {str(actor_id or "").lower(), str(sender or "").lower()} - {""}
        return sorted({name for name, by_name, by_id, ids in pats if (who & ids) or by_name.search(text or "") or (by_id and by_id.search(text or ""))})

    return match


def feed(days: int = 7) -> list[dict[str, Any]]:
    """Kept teammate events from the last `days` days, newest first, each with the project(s) it matches
    (`projects` is empty for none: a project's memory note gets the rest) and the research on its links."""
    from datetime import timedelta

    from .. import calendar
    from . import feed as work_feed
    from . import memory_sync, project_priority, projects

    cut = (calendar.now_utc() - timedelta(days=days)).strftime("%Y-%m-%dT%H:%M:%SZ")
    every = projects._load()
    globs = {e["name"]: e["match"] for e in project_priority.entries()}
    out = []
    for r in work_feed.load():
        if not r.get("team") or r.get("triage") == "noise" or r.get("personal") or str(r.get("at", "")) < cut:
            continue
        it = {"id": r["id"], "source": r.get("source") or "feed", "title": r.get("title") or "", "url": r.get("url") or "", "channel": r.get("channel") or ""}
        names = [n for n in memory_sync.targets(it, every, globs) if not every[n].get("archived_at")]
        out.append({**r, "projects": names, "to_research": work_feed.needs_research(r)})
    return sorted(out, key=lambda r: r["at"], reverse=True)
