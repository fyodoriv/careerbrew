"""Project priority: one ranked setting that says which project matters most (issue #68).

The setting `project_priority` (`careerbrew config`, or `careerbrew work priority`) lists
projects in rank order, rank 1 first. Each entry is `name=glob|glob`; a bare `name` is its
own pattern. A work item belongs to the first ranked project with a pattern that matches one
of its keys (shell-style globs, case-insensitive):
  - its repo, as `owner/repo` and `repo` (from PR and commit ids, or a GitHub link);
  - ticket keys in its title or link (`PROJ-12`) and their prefix (`PROJ`);
  - the work project that links the task (`work project next`);
  - its title (so `*invoice*` works).
Ticket keys (`ticket_pattern`) are keys of the ticket projects a pattern names (an upper-case
`PROJ-*` or `PROJ-12`, and a bare upper-case `PROJ` next to one) or a tracker link names
(`.../browse/OPS-3`). When no pattern names one (a repo glob such as `api-*` names none), any
`ABC-123` is a key except a standard such as `UTF-8` or `ISO-8601`.
Three places read it: the PM update (one line per project, in rank order), the planner
(`priorities.rank` adds `bonus()` to a task's score), and Next step (its why names the project).
"""

from __future__ import annotations

import fnmatch
import functools
import re
from collections.abc import Iterable
from typing import Any

from .. import config, events
from . import priorities

BONUS = 3.0  # the top project's score bonus: the same size as the largest promotion bonus
TICKET = re.compile(r"\b([A-Z][A-Z0-9]+)-\d+\b")  # any key-shaped word, UTF-8 too: read keys with ticket_pattern()
# Standards and versions shaped like ticket keys: words, never keys, when no pattern names a ticket project.
STANDARDS = ("AES", "ANSI", "CVE", "CWE", "ECMA", "IEC", "IEEE", "ISO", "PEP", "RFC", "RSA", "SHA", "TLS", "UCS", "UTF")
_NAMED = re.compile(r"([A-Z][A-Z0-9]+)-[\d*?!\[\]-]*")  # PROJ-* or PROJ-12, upper case: a repo glob such as api-* is not one
_BARE = re.compile(r"[A-Z][A-Z0-9]+")  # a bare upper-case PROJ: a ticket project only next to a PROJ-* one
_TRACKER = re.compile(r"/browse/([A-Z][A-Z0-9]+)-\d+\b")
Entry = dict[str, Any]


def entries() -> list[Entry]:
    """The ranked projects: [{name, rank, match: [globs]}], rank 1 first."""
    out = []
    for i, raw in enumerate(config.get("project_priority") or [], 1):
        name, _, pats = str(raw).partition("=")
        out.append({"name": name, "rank": i, "match": [p for p in pats.split("|") if p] or [name]})
    return out


def repo_of(rid: str = "", url: str = "") -> str:
    """`owner/repo` (or a local repo's folder name) from a PR or commit id, or from a GitHub link."""
    if m := re.search(r"(?:^|:)pr:([\w.-]+/[\w.-]+)#\d+$", rid or ""):
        return m.group(1)
    if m := re.search(r"(?:^|:)git:([\w.-]+):[0-9a-f]+$", rid or ""):
        return m.group(1)
    if m := re.search(r"https?://[^/\s]*github[^/\s]*/([\w.-]+/[\w.-]+)", url or ""):
        return m.group(1)
    return ""


def ticket_projects(es: list[Entry] | None = None) -> set[str]:
    """The ticket projects (key prefixes) the ranked projects' patterns name: an upper-case `PROJ-*` or `PROJ-12`, and a
    bare upper-case `PROJ` next to one. A repo glob (`api-*`, `web-2`) or a lone `SRE` names none, so the generic strip stays."""
    pats = [p for e in (entries() if es is None else es) for p in e["match"]]
    named = {m.group(1) for p in pats if (m := _NAMED.fullmatch(p))}
    return named | {p for p in pats if _BARE.fullmatch(p)} if named else set()


def ticket_pattern(es: list[Entry] | None = None, links: Iterable[str] = ()) -> re.Pattern[str]:
    """Ticket keys, group 1 the prefix: of named ticket projects and tracker links only; with none named, any key but a standard."""
    linked = frozenset(m.group(1) for link in links for m in _TRACKER.finditer(link or ""))
    return _pattern(frozenset(ticket_projects(es)), linked)


@functools.lru_cache(maxsize=64)
def _pattern(named: frozenset[str], linked: frozenset[str]) -> re.Pattern[str]:
    alt = "|".join(sorted((re.escape(p) for p in named | linked), key=len, reverse=True))
    if named:
        return re.compile(rf"\b({alt})-\d+\b")
    generic = rf"(?!(?:{'|'.join(STANDARDS)})-)[A-Z][A-Z0-9]+"
    return re.compile(rf"\b({generic}{'|' + alt if alt else ''})-\d+\b")


def keys(title: str = "", repo: str = "", link: str = "", project: str = "", es: list[Entry] | None = None) -> list[str]:
    out = [repo, repo.rsplit("/", 1)[-1]] if repo else []
    ticket = ticket_pattern(es, [link])
    for text in (title, link):
        for m in ticket.finditer(text or ""):
            out += [m.group(0), m.group(1)]
    out += [x for x in (project, title) if x]
    return [k.lower() for k in out]


def match(ks: list[str], es: list[Entry]) -> Entry | None:
    """The first ranked project with a pattern that matches one of the keys."""
    for e in es:
        if any(fnmatch.fnmatchcase(k, p.lower()) for p in e["match"] for k in ks):
            return e
    return None


def of_task(t: dict[str, Any], linked: dict[str, str], es: list[Entry]) -> Entry | None:
    urls = [x["url"] for x in priorities.links(t)]  # every link of the task (issue #143), an old single `link` included
    repo = next((r for u in urls if (r := repo_of(url=u))), "")
    return match(keys(str(t.get("title") or ""), repo, " ".join(urls), linked.get(str(t.get("id")), ""), es=es), es) if es else None


def bonus(rank: int, n: int) -> float:
    """Rank 1 of n gets BONUS, the last gets BONUS / n: the same step between neighbours."""
    return round(BONUS * (n - rank + 1) / n, 2) if n and 1 <= rank <= n else 0.0


def _text(e: Entry) -> str:
    return e["name"] if e["match"] == [e["name"]] else f"{e['name']}={'|'.join(e['match'])}"


def set_project(name: str, rank: int | None = None, match_globs: list[str] | None = None) -> list[Entry]:
    """Add a project or move it to `rank`; `match_globs` replaces its patterns (else they stay)."""
    if rank is not None and rank < 1:
        raise ValueError("rank starts at 1 (the top project)")
    es = entries()
    cur = next((e for e in es if e["name"] == name), None)
    others = [e for e in es if e["name"] != name]
    pos = rank - 1 if rank is not None else (cur["rank"] - 1 if cur else len(others))
    new = {"name": name, "match": list(match_globs) if match_globs else (cur["match"] if cur else [name])}
    ordered = [*others[:pos], new, *others[pos:]]
    config.set_value("project_priority", ", ".join(_text(e) for e in ordered))
    events.emit("work.priority", status="ok", action="set", projects=len(ordered))
    return entries()


def remove(name: str) -> list[Entry]:
    es = entries()
    if name not in {e["name"] for e in es}:
        raise ValueError(f"unknown project {name} in project_priority (have: {', '.join(e['name'] for e in es) or 'none yet'})")
    rest = [e for e in es if e["name"] != name]
    if rest:
        config.set_value("project_priority", ", ".join(_text(e) for e in rest))
    else:
        config.unset("project_priority")
    events.emit("work.priority", status="ok", action="remove", projects=len(rest))
    return entries()


def summary() -> list[dict[str, Any]]:
    """Each ranked project with what it matches now: open tasks, and today's work rows (counts only)."""
    from ..day import results
    from . import evidence, priorities, projects

    es = entries()
    linked = projects.task_projects()
    tasks = {e["name"]: 0 for e in es}
    for t in priorities.load():
        if priorities.is_open(t) and (hit := of_task(t, linked, es)):
            tasks[hit["name"]] += 1
    try:
        urls = {str(e["id"]): str(e.get("url") or "") for e in evidence.load()}
    except Exception:  # noqa: BLE001 (links are optional; counts still work from ids)
        urls = {}
    today = {e["name"]: 0 for e in es}
    for r in results.load():
        rid = str(r.get("id", ""))
        if hit := match(keys(str(r.get("title") or ""), repo_of(rid, urls.get(rid, "")), urls.get(rid, ""), es=es), es):
            today[hit["name"]] += 1
    return [{**e, "open_tasks": tasks[e["name"]], "today": today[e["name"]]} for e in es]
