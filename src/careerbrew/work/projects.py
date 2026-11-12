"""Work projects: where a long-lived project's state lives, and when each source was last read.

Record: `<data root>/work/projects.yaml` (encrypted at rest like every data file):
  projects:
    <name>:
      created_at: ISO UTC
      memory: path to the agent's notes (optional; empty: the nearest parent's, see `notes_path`)
      stale_after_days: 7
      sources: [{kind, ref, checked_at: ISO UTC | null, status: one line | "", reference: bool (optional)}]
      tasks: [work task ids]   # next steps; `work plan` and `work focus` rank them
      title: one line          # optional, like the fields below (issue #69)
      group: a label           # a team, a goal list, or a quarter's cutline
      parent: <name>           # the project this one belongs to
      links: [<name>]          # related projects
      proposed: [{kind, ref, title, parent, via}]  # sources research or the memory file found; the inbox asks (issue #147)
      archived_at: ISO UTC     # set by `work project archive`: not watched, never stale (issue #172)
      updates: [{at: ISO UTC, kind, text}]  # the last UPDATES_KEPT changes an agent saw, newest last (issue #172)
      kind: prep               # optional: it prepares the owner for an event; done only after its readiness gate (issue #276)
      event_on: YYYY-MM-DD     # a prep project's event day: that day's plan starts with its open gates
      gates: {tool: ISO UTC, questions: ISO UTC, links: {ref: ISO UTC}}  # when the owner marked each gate (each link) done
      quizzes: [{at: ISO UTC, asked, missed}]     # content-free quiz results, newest last (QUIZZES_KEPT)
      promise: {event, to, due, made_at, steps, closed}  # kind promise: a promise the owner made (issue #268, promises.py)

It holds pointers, dates, and those short labels, never copied content; the one exception is a promise's title,
the promised action read from the owner's own sent message (one line, issue #268). A source is stale
when it was never checked or its `checked_at` is older than `stale_after_days`, unless it is a
reference: careerbrew never reads a reference and it never goes stale (issue #70). A link that
opens a sign-in page (`SIGN_IN_HOSTS`) is a reference by default, and so is a url on a host
outside the plugin policy `allowed_hosts`, unless a plugin's `agent_reads: [url]` hands urls to the
agent (issue #101); `reference: true | false` on
the source is the user's choice and wins (`set_reference`). A parent or
link always names a project in the record, and parents never loop (`graph_problems`).
"""

from __future__ import annotations

import contextlib
import fcntl
import functools
import re
import threading
import urllib.parse
import uuid
from collections.abc import Callable, Iterator
from datetime import UTC, date, datetime, timedelta
from pathlib import Path
from typing import Any

from .. import calendar, events, paths, store
from . import priorities

KINDS = ("doc", "tracker", "repo", "chat", "memory", "url")
# Links to these hosts (and their subdomains) open a sign-in page, so careerbrew has no reader for them.
SIGN_IN_HOSTS = ("docs.google.com", "drive.google.com", "outlook.office.com", "outlook.office365.com", "outlook.live.com", "sharepoint.com", "teams.microsoft.com", "slack.com")
_NAME = re.compile(r"^[a-z0-9][a-z0-9-]{0,39}$")
DEFAULT_STALE_DAYS = 7
TITLE_MAX, GROUP_MAX = 200, 60
CLEAR = "none"  # `work project set <name> --parent none` clears a value
UPDATE_KINDS = ("company", "project", "team", "other", "approval")
UPDATES_KEPT, UPDATE_MAX = 20, 200
# A prep project's readiness gate (issue #276): done means ready, not "a prep pack exists".
PROJECT_KINDS = ("prep",)
GATES = ("tool", "links", "questions", "quiz")
MARKED_GATES = ("tool", "links", "questions")  # the owner marks these (links one ref at a time); the quiz follows from its results
GATE_TEXT = {
    "tool": "try the tool hands-on (the app, the coding pad, the deck)",
    "links": "read every source link in the pack",
    "questions": "write your own questions",
    "quiz": "pass the agent quiz on the format, rubric, questions, and logistics",
}
GATE_MINUTES = 15  # each open gate's block in the event day's plan
QUIZZES_KEPT, QUIZ_MAX = 20, 500


class ProjectError(ValueError):
    pass


def path():
    return paths.sub("work", "projects.yaml")


def _load() -> dict[str, Any]:
    return (store.read_yaml(path(), {}) or {}).get("projects", {}) or {}


def records() -> dict[str, Any]:
    """Every project's record as stored, for readers outside this module (promises.py). Change it only through a writer."""
    return _load()


def _save(projects: dict[str, Any]) -> None:
    store.write_yaml(path(), {"projects": projects})


# Process scope: how deep this thread is inside `locked()`, so a writer that calls another writer does not wait for itself.
_held = threading.local()


@contextlib.contextmanager
def locked() -> Iterator[None]:
    """Hold projects.yaml's file lock (the `.<name>.lock` that `store.append_jsonl` uses) across a load, change, and save,
    so two sessions that update different projects never drop each other's change (issue #230)."""
    if getattr(_held, "depth", 0):
        _held.depth += 1
        try:
            yield
        finally:
            _held.depth -= 1
        return
    file = path()
    file.parent.mkdir(parents=True, exist_ok=True)
    with file.with_name(f".{file.name}.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        _held.depth = 1
        try:
            yield
        finally:
            _held.depth = 0


def _writes[F: Callable[..., Any]](fn: F) -> F:
    """Run a writer under `locked()`: its `_load()` then reads the file after any other writer is done."""

    @functools.wraps(fn)
    def run(*args: Any, **kwargs: Any) -> Any:
        with locked():
            return fn(*args, **kwargs)

    return run  # type: ignore[return-value]


def _now() -> datetime:
    return calendar.now().astimezone(UTC)


def parse_days(text: str | int) -> int:
    """`7`, `7d`, or `2w` → days."""
    m = re.fullmatch(r"\s*(\d+)\s*([dw]?)\s*", str(text))
    if not m or int(m.group(1)) < 1:
        raise ProjectError(f"stale-after must look like 7d or 2w, not {text!r}")
    return int(m.group(1)) * (7 if m.group(2) == "w" else 1)


def _get(projects: dict[str, Any], name: str) -> dict[str, Any]:
    if name not in projects:
        have = ", ".join(sorted(projects)) or "none yet"
        raise ProjectError(f"unknown project {name} (fix: `careerbrew work project add {name}`; have: {have})")
    return projects[name]


def memory_path(name: str, memory: str) -> str:
    """Where a project's notes go. A folder gets `<project>-research-log.md` inside it; an agent memory
    index (`MEMORY.md`, loaded into every session one line per entry) is refused (issue #39)."""
    if not memory:
        return ""
    p = Path(memory).expanduser()
    if p.name == "MEMORY.md":
        log = p.parent / f"{name}-research-log.md"
        raise ProjectError(f"{memory} is an agent memory index; notes would bloat it (fix: use {log}, and add this one line to the index: - [{name} research log]({log.name}) — dated notes from careerbrew)")
    if memory.endswith("/") or p.is_dir():
        log = p / f"{name}-research-log.md"
        if not log.exists():
            log.parent.mkdir(parents=True, exist_ok=True)
            store.write_text(log, f"# {name} research log\n\nDated notes written by careerbrew (work project absorb and checkup).\n")
        return str(log)
    return memory


def is_memory_index(memory: str) -> bool:
    return Path(memory or "").name == "MEMORY.md"


@_writes
def set_memory(name: str, memory: str) -> None:
    projects = _load()
    _get(projects, name)["memory"] = memory_path(name, memory)
    _save(projects)


@_writes
def set_rules(name: str, text: str) -> None:
    """Replace one project's bounded standing rules under the project-store lock."""
    projects = _load()
    _get(projects, name)["standing_rules"] = label(text, "standing rules", 1000)
    _save(projects)
    events.emit("work.project", status="ok", action="rules")


def rules(name: str) -> str:
    """Read the project's current standing rules from the encrypted store."""
    return _get(_load(), name).get("standing_rules", "")


def root(name: str, every: dict[str, Any] | None = None) -> str:
    """The top of a project's parent chain (itself when it has no parent). A parent loop, which only a
    hand edit makes, ends at its first name in sort order, so every project in the loop has one root."""
    every = _load() if every is None else every
    chain, cur = [name], (every.get(name) or {}).get("parent") or ""
    while cur in every and cur not in chain:
        chain.append(cur)
        cur = every[cur].get("parent") or ""
    return min(chain[chain.index(cur) :]) if cur in chain else chain[-1]


def notes_path(name: str, every: dict[str, Any] | None = None) -> str:
    """Where a project's notes go: its own memory path, else the nearest one up its parent chain, so one
    answer covers a whole imported tree. Empty when no project in the chain has one."""
    every = _load() if every is None else every
    seen: set[str] = set()
    while name in every and name not in seen:
        if every[name].get("memory"):
            return every[name]["memory"]
        seen.add(name)
        name = every[name].get("parent") or ""
    return ""


def valid_name(name: str) -> bool:
    return bool(_NAME.match(name))


def label(value: str, what: str, limit: int) -> str:
    """A title or group: one line, at most `limit` characters (the text itself is never echoed)."""
    value = (value or "").strip()
    if "\n" in value or len(value) > limit:
        raise ProjectError(f"{what} is one line of at most {limit} characters")
    return value


def new_record(stale_after: str | int = DEFAULT_STALE_DAYS) -> dict[str, Any]:
    return {"incarnation_id": uuid.uuid4().hex, "created_at": _now().isoformat(timespec="seconds"), "memory": "", "stale_after_days": parse_days(stale_after), "sources": [], "tasks": []}


def session_record(name: str) -> tuple[dict[str, Any], dict[str, Any], bool]:
    """Load under the project lock and stage an identity for an older project."""
    projects = _load()
    record = _get(projects, name)
    migrated = "incarnation_id" not in record
    if migrated:
        record["incarnation_id"] = uuid.uuid4().hex
    if not isinstance(record["incarnation_id"], str) or not re.fullmatch(r"[0-9a-f]{32}", record["incarnation_id"]):
        raise ProjectError("saved project identity is invalid; repair the project record before opening a session")
    return record, projects, migrated


def _put(p: dict[str, Any], key: str, value: Any) -> None:
    """Optional fields stay out of the record while empty."""
    if value:
        p[key] = value
    else:
        p.pop(key, None)


def graph_problems(projects: dict[str, Any]) -> list[tuple[str, str]]:
    """(project, problem) for each parent or link that names no project or itself, and each parent loop."""
    out: list[tuple[str, str]] = []
    for name, p in sorted(projects.items()):
        parent = p.get("parent") or ""
        if parent == name:
            out.append((name, "a project cannot be its own parent"))
        elif parent and parent not in projects:
            out.append((name, f"parent {parent} is not a project (fix: add {parent} first, or leave parent empty)"))
        for link in p.get("links") or []:
            if link == name:
                out.append((name, "a project cannot link to itself"))
            elif link not in projects:
                out.append((name, f"link {link} is not a project (fix: add {link} first, or drop the link)"))
    loops: set[frozenset[str]] = set()
    for name in sorted(projects):
        chain, cur = [name], projects[name].get("parent") or ""
        while cur in projects and cur not in chain:
            chain.append(cur)
            cur = projects[cur].get("parent") or ""
        if cur in chain and cur != chain[-1] and frozenset(loop := chain[chain.index(cur) :]) not in loops:
            loops.add(frozenset(loop))
            out.append((min(loop), f"parents loop: {' -> '.join([*loop, cur])} (fix: clear one of these parents)"))
    return out


def _refuse_bad_graph(projects: dict[str, Any]) -> None:
    if problems := graph_problems(projects):
        raise ProjectError("; ".join(f"{n}: {msg}" for n, msg in problems))


def _relate(p: dict[str, Any], title: str | None, group: str | None, parent: str | None, link: list[str], unlink: list[str]) -> None:
    if title is not None:
        _put(p, "title", "" if title == CLEAR else label(title, "title", TITLE_MAX))
    if group is not None:
        _put(p, "group", "" if group == CLEAR else label(group, "group", GROUP_MAX))
    if parent is not None:
        _put(p, "parent", "" if parent == CLEAR else parent.strip())
    links = list(p.get("links") or [])
    for x in unlink:
        if x not in links:
            raise ProjectError(f"the project does not link to {x} (have: {', '.join(links) or 'no links'})")
        links.remove(x)
    _put(p, "links", links + [x for x in dict.fromkeys(link) if x not in links])


def _set_prep(p: dict[str, Any], kind: str | None, event_on: str | None) -> None:
    """Set or clear (`none`) a project's kind and event day. Only a prep project has an event day."""
    if kind is not None:
        if kind not in (*PROJECT_KINDS, CLEAR):
            raise ProjectError(f"unknown project kind {kind}; use {', '.join(PROJECT_KINDS)} (or none to clear it)")
        _put(p, "kind", "" if kind == CLEAR else kind)
    if event_on is not None:
        if event_on != CLEAR:
            try:
                event_on = date.fromisoformat(event_on.strip()).isoformat()
            except ValueError:
                raise ProjectError(f"the event day looks like 2026-11-03, not {event_on!r}") from None
        _put(p, "event_on", "" if event_on == CLEAR else event_on)
    if p.get("event_on") and p.get("kind") != "prep":
        raise ProjectError("only a prep project has an event day (fix: add --kind prep)")


@_writes
def add(name: str, memory: str = "", stale_after: str | int = DEFAULT_STALE_DAYS, title: str = "", group: str = "", parent: str = "", links: list[str] | None = None, kind: str = "", event_on: str = "") -> dict[str, Any]:
    if not valid_name(name):
        raise ProjectError("a project name uses lowercase letters, digits, and dashes (max 40)")
    projects = _load()
    if name in projects:
        raise ProjectError(f"project {name} already exists (see `careerbrew work project show {name}`)")
    record = new_record(stale_after)
    _relate(record, title or None, group or None, parent or None, list(links or []), [])
    _set_prep(record, kind or None, event_on or None)
    projects[name] = record
    _refuse_bad_graph(projects)
    memory = memory_path(name, memory)
    record["memory"] = memory
    if memory:
        record["sources"].append({"kind": "memory", "ref": memory, "checked_at": None, "status": ""})
    _save(projects)
    events.emit("work.project", status="ok", action="add")
    return projects[name]


@_writes
def edit(
    name: str,
    title: str | None = None,
    group: str | None = None,
    parent: str | None = None,
    link: list[str] | None = None,
    unlink: list[str] | None = None,
    kind: str | None = None,
    event_on: str | None = None,
    quiet_until: str | None = None,
) -> dict[str, Any]:
    """Change a project's title, group, parent, links, kind, or event day; `none` clears a value. A dangling name or a loop is refused."""
    projects = _load()
    _relate(_get(projects, name), title, group, parent, list(link or []), list(unlink or []))
    _set_prep(projects[name], kind, event_on)
    if quiet_until is not None:
        if quiet_until != CLEAR:
            try:
                if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", quiet_until):
                    raise ValueError
                quiet_until = date.fromisoformat(quiet_until).isoformat()
            except ValueError:
                raise ProjectError("quiet until must be a date like 2026-11-03, or none") from None
        if quiet_until != CLEAR and not _quiet_record(projects[name], calendar.now().date()):
            projects[name]["quiet_generation"] = uuid.uuid4().hex
        if quiet_until == CLEAR and projects[name].get("quiet_until"):
            projects[name]["quiet_resumed"] = True
        elif quiet_until != CLEAR:
            projects[name].pop("quiet_resumed", None)
        _put(projects[name], "quiet_until", "" if quiet_until == CLEAR else quiet_until)
    _refuse_bad_graph(projects)
    _save(projects)
    events.emit("work.project", status="ok", action="set")
    return projects[name]


def _quiet_record(project: dict[str, Any], today: date) -> bool:
    value = project.get("quiet_until", "")
    if not value:
        return False
    try:
        return today < date.fromisoformat(value)
    except (TypeError, ValueError):
        raise ProjectError("saved quiet date is invalid; set it again with --quiet-until") from None


def quiet(name: str, today: date | None = None, *, records: dict[str, Any] | None = None) -> bool:
    """Whether a saved quiet period still applies on the local calendar day."""
    return _quiet_record(_get(records if records is not None else _load(), name), today or calendar.now().date())


def _source_ref(kind: str, ref: str) -> str:
    """Normalize within the caller's chosen kind, without widening its scope."""
    from . import research

    ref = ref.strip()
    if kind == "chat" and re.fullmatch(r"#[CGD][A-Z0-9]+", ref):
        return ref[1:]
    if kind == "memory":
        return ref
    try:
        detected, normalized = research.detect(ref)
    except research.ReadError:
        return ref  # bare document/channel ids are valid only with an explicit kind
    if detected == kind:
        return normalized
    if "://" in ref and kind != "url":
        raise ProjectError(f"source link has kind {detected}, not {kind}; use work project source <name> {detected} <ref>")
    return ref


def _source_matches(source: dict[str, Any], ref: str) -> bool:
    """Compare a selector in the saved source's kind without widening read scope."""
    if source["ref"] == ref:
        return True
    if source["kind"] == "url" and source.get("reference") is True:
        return False
    try:
        if source["kind"] == "doc" and "://" not in source["ref"] and "://" not in ref:
            saved = Path(source["ref"]).expanduser()
            if saved.is_absolute() and saved.resolve(strict=False) == Path(ref.strip()).expanduser().resolve(strict=False):
                return True
        return _source_ref(source["kind"], source["ref"]) == _source_ref(source["kind"], ref)
    except (ValueError, OSError, RuntimeError):
        return False


def select_sources(sources: list[dict[str, Any]], ref: str | None) -> list[dict[str, Any]]:
    """Prefer literal selectors; refuse aliases that cross saved source kinds."""
    if ref is None:
        return sources
    literal = [s for s in sources if s["ref"] == ref]
    if literal:
        return literal
    hits = [s for s in sources if _source_matches(s, ref)]
    if len({s["kind"] for s in hits}) > 1:
        raise ProjectError("source alias matches multiple source kinds; use the exact saved ref from work project show")
    return hits


@_writes
def normalize_sources() -> None:
    """Repair old source refs during refresh/sync; preserve explicit reference choices."""
    every = _load()
    before = store.digest(every)
    for p in every.values():
        sources: dict[tuple[str, str], dict[str, Any]] = {}
        for source in p["sources"]:
            try:
                ref = source["ref"] if source.get("reference") is True and source["kind"] != "tracker" else _source_ref(source["kind"], source["ref"])
            except ValueError:
                ref = source["ref"]  # a malformed link or mismatched kind needs the owner's correction
            if ref != source["ref"]:
                source.update(ref=ref, checked_at=None, status="")
                source.pop("stale_reason", None)
            key = source["kind"], ref
            if key not in sources:
                sources[key] = source
            elif source.get("reference") is True:
                sources[key]["reference"] = True
        p["sources"] = list(sources.values())
    if store.digest(every) != before:
        _save(every)


@_writes
def add_source(name: str, kind: str, ref: str) -> dict[str, Any]:
    if kind not in KINDS:
        raise ProjectError(f"unknown source kind {kind}; use one of {', '.join(KINDS)}")
    ref = _source_ref(kind, ref)
    normalize_sources()
    projects = _load()
    p = _get(projects, name)
    if any(s["kind"] == kind and s["ref"] == ref for s in p["sources"]):
        raise ProjectError(f"{kind} {ref} is already a source of {name}")
    p["sources"].append({"kind": kind, "ref": ref, "checked_at": None, "status": ""})
    p["proposed"] = [x for x in p.get("proposed", []) if (x["kind"], x["ref"]) != (kind, ref)]
    _save(projects)
    events.emit("work.project", status="ok", action="source")
    return p


@_writes
def check(name: str, ref: str | None = None, status: str = "") -> int:
    """Stamp `checked_at` = now (UTC) on one source, or on all of them. Returns how many were stamped."""
    if "\n" in status or len(status) > 200:
        raise ProjectError("status is one line of at most 200 characters")
    projects = _load()
    p = _get(projects, name)
    hits = select_sources(p["sources"], ref)
    if not hits:
        raise ProjectError(f"{name} has no source {ref} (see `careerbrew work project show {name}`)" if ref else f"{name} has no sources yet (fix: `careerbrew work project source {name} <kind> <ref>`)")
    stamp = _now().isoformat(timespec="seconds")
    for s in hits:
        s["checked_at"] = stamp
        s.pop("stale_reason", None)
        if status:
            s["status"] = status
    _save(projects)
    events.emit("work.project", status="ok", action="check", count=len(hits))
    return len(hits)


def no_reader(ref: str, kind: str = "") -> str:
    """Why careerbrew cannot read this ref at all, or "" when it can try: a link that opens a sign-in page,
    or a url on a host outside the plugin policy `allowed_hosts` that the agent does not read (issue #101)."""
    from .. import connectors

    if "://" not in ref:
        return ""
    host = (urllib.parse.urlsplit(ref).hostname or "").lower()
    if any(host == h or host.endswith("." + h) for h in SIGN_IN_HOSTS):
        return f"careerbrew has no reader for {host} links (they open a sign-in page)"
    if kind == "url" and not connectors.host_allowed(host) and not agent_reads(kind):
        return f"careerbrew may not read {host} links (not in the plugin policy allowed_hosts; a plugin with agent_reads: [url] hands them to the agent)"
    return ""


def reference_reason(s: dict[str, Any]) -> str:
    """Why a source is a reference (never read, never stale), or "" when it is tracked. The user's choice wins."""
    if s.get("reference") is True:
        return "marked as a reference"
    return "" if s.get("reference") is False else no_reader(s["ref"], s["kind"])


@_writes
def set_reference(name: str, ref: str, on: bool = True) -> int:
    """Mark a source reference-only, or track it again (`on=False`). Returns how many sources changed."""
    projects = _load()
    hits = select_sources(_get(projects, name)["sources"], ref)
    if not hits:
        raise ProjectError(f"{name} has no source {ref} (see `careerbrew work project show {name}`)")
    for s in hits:
        s["reference"] = on
        s.pop("stale_reason", None)
    _save(projects)
    events.emit("work.project", status="ok", action="reference", count=len(hits))
    return len(hits)


@_writes
def mark_unreadable(name: str, ref: str, reason: str) -> None:
    """Keep the source stale and record why it could not be read (shown by `show`)."""
    projects = _load()
    for s in select_sources(_get(projects, name)["sources"], ref):
        s["stale_reason"] = reason[:200]
    _save(projects)


@_writes
def add_proposals(found: list[dict[str, str]]) -> None:
    """Related sources a refresh found; the user accepts one with `work project source`.

    A proposal may carry the ticket's `title` and the `parent` source that led to it (a child issue), so the
    inbox can say why it belongs (issue #147); a proposal already kept gets them when it had none.
    """
    projects = _load()
    for f in found:
        p = projects.get(f["project"])
        if p is None:
            continue
        extra = {k: " ".join(str(f[k]).split())[:TITLE_MAX] for k in ("title", "parent") if f.get(k)}
        old = next((x for x in p.get("proposed", []) if (x["kind"], x["ref"]) == (f["kind"], f["ref"])), None)
        if old is not None:
            old.update({k: v for k, v in extra.items() if not old.get(k)})
        elif not any((x["kind"], x["ref"]) == (f["kind"], f["ref"]) for x in p["sources"]):
            p.setdefault("proposed", []).append({"kind": f["kind"], "ref": f["ref"], **extra})
    _save(projects)


@_writes
def link_task(name: str, task_id: str) -> dict[str, Any]:
    if task_id not in {t["id"] for t in priorities.load()}:
        raise ProjectError(f'unknown task {task_id} (fix: `careerbrew work task "<title>" --hours <h>` first)')
    projects = _load()
    p = _get(projects, name)
    if task_id not in p["tasks"]:
        p["tasks"].append(task_id)
    _save(projects)
    return p


def task_projects() -> dict[str, str]:
    """Task id -> the first project that links it, so views can say which project a task serves."""
    out: dict[str, str] = {}
    for name, p in _load().items():
        for tid in p.get("tasks", []):
            out.setdefault(tid, name)
    return out


def _age_days(s: dict[str, Any], now: datetime) -> float | None:
    if not s.get("checked_at"):
        return None
    return (now - datetime.fromisoformat(s["checked_at"])).total_seconds() / 86400


def _is_stale(s: dict[str, Any], limit: int, now: datetime) -> bool:
    if reference_reason(s):
        return False
    age = _age_days(s, now)
    return age is None or age > limit


def show(name: str) -> dict[str, Any]:
    now = _now()
    every = _load()
    p = _get(every, name)
    limit = int(p.get("stale_after_days", DEFAULT_STALE_DAYS))
    rows = [{**s, "age_days": None if (a := _age_days(s, now)) is None else round(a, 1), "stale": _is_stale(s, limit, now), "reference": bool(why := reference_reason(s)), "reference_reason": why} for s in p["sources"]]
    rows.sort(key=lambda r: (r["reference"], r["age_days"] is not None, -(r["age_days"] or 0)))  # never checked first, then oldest; references last
    open_ids = {t["id"]: t for t in priorities.load() if priorities.is_open(t)}
    related = {
        "title": p.get("title", ""),
        "group": p.get("group", ""),
        "parent": p.get("parent", ""),
        "children": sorted(n for n, q in every.items() if q.get("parent") == name),
        "links": list(p.get("links") or []),
        "linked_from": sorted(n for n, q in every.items() if name in (q.get("links") or [])),
    }
    return {"name": name, "memory": p.get("memory", ""), "stale_after_days": limit, "sources": rows, "proposed": p.get("proposed", []), "tasks": [open_ids[t] for t in p["tasks"] if t in open_ids], "prep": readiness(name, p), **related}


def list_rows() -> list[dict[str, Any]]:
    now = _now()
    local_day = calendar.now().date()
    out = []
    for name, p in _load().items():
        limit = int(p.get("stale_after_days", DEFAULT_STALE_DAYS))
        tracked = [s for s in p["sources"] if not reference_reason(s)]
        stamps = [s["checked_at"] for s in tracked]
        oldest = None if (None in stamps or not stamps) else min(stamps)
        out.append(
            {
                "name": name,
                "sources": len(p["sources"]),
                "references": len(p["sources"]) - len(tracked),
                "stale": 0 if p.get("archived_at") else sum(_is_stale(s, limit, now) for s in tracked),
                "oldest_checked_at": oldest,
                "group": p.get("group", ""),
                "parent": p.get("parent", ""),
                "links": list(p.get("links") or []),
                "archived": bool(p.get("archived_at")),
                "quiet_until": p.get("quiet_until", ""),
                "quiet": _quiet_record(p, local_day),
                "prep": readiness(name, p),
            }
        )
    return sorted(out, key=lambda r: (-r["stale"], r["oldest_checked_at"] or "", r["name"]))


def tree() -> list[dict[str, Any]]:
    """Every project once, by group, parents before their children (`depth` 0 is a root): the Work view.
    Promises (issue #268) are left out: Dashboard's Commitments lists them."""
    every = {n: p for n, p in _load().items() if p.get("kind") != "promise"}
    counts = {r["name"]: r for r in list_rows()}
    kids: dict[str, list[str]] = {}
    for n, p in every.items():
        parent = p.get("parent") or ""
        kids.setdefault(parent if parent in every and parent != n else "", []).append(n)

    def order(n: str) -> tuple[str, str]:
        return (every[n].get("group") or "\uffff", n)  # projects with no group come last

    out: list[dict[str, Any]] = []
    seen: set[str] = set()

    def walk(n: str, depth: int) -> None:
        if n in seen:
            return
        seen.add(n)
        p = every[n]
        out.append(
            {
                "name": n,
                "title": p.get("title", ""),
                "group": p.get("group", ""),
                "parent": p.get("parent", ""),
                "links": list(p.get("links") or []),
                "children": len(kids.get(n, [])),
                "depth": depth,
                "sources": counts[n]["sources"],
                "stale": counts[n]["stale"],
                "archived": counts[n]["archived"],
            }
        )
        for c in sorted(kids.get(n, []), key=order):
            walk(c, depth + 1)

    for n in sorted(kids.get("", []), key=order):
        walk(n, 0)
    for n in sorted(every, key=order):  # a loop made by a hand edit still shows, as a root
        walk(n, 0)
    return out


SOURCE_CONNECTOR = {"tracker": "jira", "repo": "github", "doc": "gdocs", "chat": "slack"}
AGENT_READ_KIND = {**SOURCE_CONNECTOR, "url": "url"}  # source kind -> the `agent_reads` entry that covers it


def agent_reads(kind: str) -> bool:
    """True when an installed plugin's `agent_reads` covers this source kind: its connector (`slack` for chat), or `url`."""
    from .. import plugins

    agent = {k for p in plugins.installed() for k in (p["manifest"].get("agent_reads") or [])}
    return kind in AGENT_READ_KIND and AGENT_READ_KIND[kind] in agent


def local_file(kind: str, ref: str) -> bool:
    """A `doc` source that is a file on this machine: careerbrew reads it itself, whatever `agent_reads` says."""
    if kind != "doc" or not ref:
        return False
    p = Path(ref).expanduser()
    return (p if p.is_absolute() else paths.data_root() / p).is_file()


def reader(kind: str, ref: str = "") -> str:
    """Who reads this source: careerbrew (a connector), the agent (a plugin's agent_reads), or you (no reader for the ref)."""
    if no_reader(ref, kind):
        return "you"
    if agent_reads(kind) and not local_file(kind, ref):
        return "agent"
    if kind in ("memory", "url") or kind in SOURCE_CONNECTOR:
        return "careerbrew"
    return "you"


def stale_sources() -> list[dict[str, Any]]:
    """Every stale source across projects, stalest first: for checkup, day, and focus. References are never stale."""
    now = _now()
    out = []
    for name, p in _load().items():
        limit = int(p.get("stale_after_days", DEFAULT_STALE_DAYS))
        for s in [] if p.get("archived_at") else p["sources"]:
            if _is_stale(s, limit, now):
                age = _age_days(s, now)
                out.append({"project": name, "kind": s["kind"], "ref": s["ref"], "age_days": None if age is None else round(age, 1), "limit": limit, "reader": reader(s["kind"], s["ref"])})
    return sorted(out, key=lambda r: (r["age_days"] is not None, -(r["age_days"] or 0)))


@_writes
def archive(name: str, on: bool = True, force: bool = False) -> None:
    """Archive a project: it leaves the watch list, the watcher's memory sync, and the stale sources. `on=False` watches it again.

    A prep project is done only when its readiness gate passes (issue #276): archiving one with open gates is refused,
    unless `force` (the event was cancelled)."""
    projects = _load()
    r = readiness(name, _get(projects, name))
    if on and r and not r["ready"] and not force:
        todo = [g for g in r["gates"] if not g["done"]]
        raise ProjectError(f"{name} is not ready: {len(todo)} of {len(GATES)} gate(s) open ({', '.join(g['gate'] for g in todo)}); fix: {todo[0]['fix']} (the event was cancelled? archive it with --force)")
    _put(_get(projects, name), "archived_at", _now().isoformat(timespec="seconds") if on else "")
    if projects[name].get("kind") == "promise":  # archived is closed (done, unless it said how); opened again is open
        pr = projects[name].setdefault("promise", {})
        if on:
            pr.setdefault("closed", "done")
        else:
            pr.pop("closed", None)
    _save(projects)
    if on:
        events.emit("work.project", status="ok", action="archive")
    else:
        events.emit("work.project", status="ok", action="unarchive")


@_writes
def remove(name: str) -> dict[str, Any]:
    """Delete a project's record; its memory file and linked tasks stay. Refused while another project names it."""
    projects = _load()
    p = _get(projects, name)
    users = sorted(n for n, q in projects.items() if n != name and (q.get("parent") == name or name in (q.get("links") or [])))
    if users:
        raise ProjectError(f"{', '.join(users)} name {name} as a parent or link (fix: `careerbrew work project set <name> --parent none --unlink {name}` first, or archive it)")
    del projects[name]
    _save(projects)
    events.emit("work.project", status="ok", action="remove")
    return p


@_writes
def record_update(name: str, kind: str, text: str) -> dict[str, Any]:
    """Note one change or approval on a project: kept newest last, at most UPDATES_KEPT."""
    if kind not in UPDATE_KINDS:
        raise ProjectError(f"unknown update kind {kind}; use one of {', '.join(UPDATE_KINDS)}")
    text = label(text, "an update", UPDATE_MAX)
    if not text:
        raise ProjectError("an update needs one line of text")
    projects = _load()
    p = _get(projects, name)
    row = {"at": _now().isoformat(timespec="seconds"), "kind": kind, "text": text}
    p["updates"] = [*(p.get("updates") or []), row][-UPDATES_KEPT:]
    _save(projects)
    events.emit("work.project", status="ok", action="update")
    return row


@_writes
def add_promise(name: str, title: str, parent: str, link: str, source_kind: str, promise: dict[str, Any]) -> None:
    """A promise the owner made, as a project of kind `promise` (issue #268). Its link is a reference: never read, never stale."""
    title = " ".join((title or "").split())
    if not title:
        raise ProjectError("a promise needs one line of text: what you promised")
    projects = _load()
    if name in projects:
        raise ProjectError(f"project {name} already exists (see `careerbrew work promise list`)")
    if parent:
        _get(projects, parent)
    record = new_record()
    record.update(kind="promise", title=label(title[:TITLE_MAX], "a promise", TITLE_MAX), promise=promise)
    _put(record, "parent", parent)
    if link:
        record["sources"].append({"kind": source_kind, "ref": _source_ref(source_kind, link) if source_kind == "tracker" else link, "checked_at": None, "status": "", "reference": True})
    projects[name] = record
    _save(projects)


@_writes
def sync_pm_promises(day: str, rows: list[dict[str, Any]], observations: dict[str, dict[str, str]] | None = None) -> bool:
    """Replace one sent day's promise set under the project-file lock (#331)."""
    every = _load()
    names = {r["name"] for r in rows}
    changed = False
    for name, observation in (observations or {}).items():
        pr = every[name]["promise"]
        current = pr.get("pm_observation") or {}
        decision = pr.get("pm_decision") or {}
        if max(str(current.get("day") or ""), str(decision.get("day") or "")) > observation["day"]:
            continue
        if pr.get("pm_observation") != observation:
            pr["pm_observation"] = observation
            changed = True
    for name, p in every.items():
        pr = p.get("promise") or {}
        if pr.get("source") == "pm-update" and pr.get("pm_day") == day and name not in names and not p.get("archived_at"):
            p["promise"] = {**pr, "closed": "dropped", "pm_removed": True}
            p["archived_at"] = _now().isoformat(timespec="seconds")
            changed = True
    for row in rows:
        name = row["name"]
        if name in every:
            existing = every[name]
            pr = existing.get("promise") or {}
            if pr.get("source") != "pm-update" or pr.get("pm_day") != day:
                raise ProjectError("a PM promise id collides with another project; preserve it and repair the promise identity")
            if pr.get("pm_removed") and pr.get("closed") == "dropped":
                existing["promise"] = {k: v for k, v in pr.items() if k not in ("closed", "pm_removed")}
                existing.pop("archived_at", None)
                changed = True
            continue
        record = new_record()
        record.update(kind="promise", title=label(row["title"][:TITLE_MAX], "a promise", TITLE_MAX), promise=row["promise"])
        if row["parent"]:
            _get(every, row["parent"])
            record["parent"] = row["parent"]
        if row["link"]:
            record["sources"].append({"kind": "url", "ref": row["link"], "checked_at": None, "status": "", "reference": True})
        every[name] = record
        changed = True
    if changed:
        _save(every)
    return changed


def pm_proposal_id(name: str, promise: dict[str, Any]) -> str:
    return "pm-outcome-" + store.digest(name, promise)


def pm_identity_records(every: dict[str, Any], identity: str) -> dict[str, Any]:
    return {name: row for name, row in every.items() if (row.get("promise") or {}).get("source") == "pm-update" and (row.get("promise") or {}).get("identity_digest") == identity}


def pm_candidate_valid(every: dict[str, Any], candidate: dict[str, Any]) -> bool:
    row = every.get(candidate["name"])
    if row is None:
        return True
    promise = row.get("promise") or {}
    expected = candidate["promise"]
    return bool(
        row.get("kind") == "promise" and not row.get("archived_at") and not promise.get("closed") and not promise.get("pm_removed") and all(promise.get(key) == expected.get(key) for key in ("source", "pm_day", "identity_digest", "due"))
    )


def pm_resolution_id(name: str, project: dict[str, Any], revision: str, choice: str, context: dict[str, Any] | None = None) -> str:
    return "pm-resolution-" + store.digest(name, project, revision, choice, context)


@_writes
def resolve_pm_outcome(name: str, proposal_id: str, revision: str, choice: str, context: dict[str, Any] | None = None) -> None:
    every = _load()
    project = _get(every, name)
    promise = project.get("promise") or {}
    decision = promise.get("pm_decision") or {}
    closed = promise.get("closed")
    if (
        choice not in {"keep", "reopen", "drop"}
        or project.get("kind") != "promise"
        or decision.get("state") not in ("done", "moved")
        or closed not in (None, "", "done", "dropped")
        or bool(closed) != bool(project.get("archived_at"))
        or not revision
        or not decision.get("revision")
        or decision["revision"] == revision
        or pm_resolution_id(name, project, revision, choice, context) != proposal_id
        or (context is not None and pm_identity_records(every, promise.get("identity_digest", "")) != context.get("matches"))
        or (context is not None and context.get("candidate") is not None and not pm_candidate_valid(every, context["candidate"]))
    ):
        raise ProjectError("PM promise proposal is no longer current")
    try:
        dates = [promise.get("due"), promise.get("original_due", promise.get("due"))]
        if decision.get("state") == "moved":
            dates.append(decision.get("due"))
        if any(not isinstance(day, str) or date.fromisoformat(day).isoformat() != day for day in dates):
            raise ValueError
    except ValueError:
        raise ProjectError("PM promise proposal is no longer current") from None
    if choice == "keep":
        try:
            date.fromisoformat(str(decision["day"]))
            if decision["state"] == "done":
                datetime.fromisoformat(str(decision["accepted_at"]))
            elif decision["state"] == "moved":
                date.fromisoformat(str(decision["due"]))
            else:
                raise ValueError
        except (KeyError, ValueError):
            raise ProjectError("PM promise proposal is no longer current") from None
    candidate = context.get("candidate") if context else None
    if candidate and candidate["name"] not in every:
        record = new_record()
        record.update(kind="promise", title=label(candidate["title"][:TITLE_MAX], "a promise", TITLE_MAX), promise=candidate["promise"])
        if candidate["parent"]:
            _get(every, candidate["parent"])
            record["parent"] = candidate["parent"]
        if candidate["link"]:
            record["sources"].append({"kind": "url", "ref": candidate["link"], "checked_at": None, "status": "", "reference": True})
        every[candidate["name"]] = record
    promise["pm_decision_history"] = [*(promise.get("pm_decision_history") or []), {**decision, "resolution": choice, "resolved_at": _now().isoformat(timespec="seconds")}]
    promise.pop("pm_decision", None)
    promise.pop("pm_observation", None)
    if choice == "keep" and decision.get("state") == "done":
        promise["manual_kept_day"] = decision["day"]
        promise["closed"] = "done"
        project["archived_at"] = decision["accepted_at"]
    elif choice == "keep" and decision.get("state") == "moved":
        promise["due"] = decision["due"]
        promise["pm_moved"] = True
        promise.pop("closed", None)
        promise.pop("manual_kept_day", None)
        project.pop("archived_at", None)
    elif choice == "reopen":
        promise["due"] = promise.get("original_due", promise.get("due", ""))
        promise.pop("closed", None)
        promise.pop("pm_removed", None)
        promise.pop("pm_moved", None)
        promise.pop("manual_kept_day", None)
        project.pop("archived_at", None)
    elif choice == "drop":
        promise["closed"] = "dropped"
        promise.pop("pm_moved", None)
        promise.pop("manual_kept_day", None)
        project["archived_at"] = _now().isoformat(timespec="seconds")
    _save(every)


@_writes
def accept_pm_outcome(name: str, proposal_id: str, revision: str) -> None:
    every = _load()
    project = _get(every, name)
    promise = project["promise"]
    observed = promise.get("pm_observation") or {}
    if project.get("archived_at") or observed.get("revision") != revision or observed.get("state") not in {"done", "moved"} or pm_proposal_id(name, promise) != proposal_id or (promise.get("pm_decision") or {}).get("revision") == revision:
        raise ProjectError("PM promise proposal is no longer current")
    action = observed["state"]
    accepted_at = _now().isoformat(timespec="seconds")
    promise["pm_decision"] = {"revision": revision, "state": action, "day": observed["day"], "due": observed["due"], "accepted_at": accepted_at}
    if action == "moved":
        promise.setdefault("original_due", promise["due"])
        promise["due"] = observed["due"]
        promise["pm_moved"] = True
    else:
        promise["closed"] = "done"
        project["archived_at"] = accepted_at
    _save(every)


@_writes
def close_promise(name: str, how: str) -> None:
    """Archive a promise and say how it ended (done, or dropped: it was not a promise)."""
    projects = _load()
    p = _get(projects, name)
    p["promise"] = {**{k: v for k, v in (p.get("promise") or {}).items() if k not in ("pm_removed", "manual_kept_day")}, "closed": how}
    p["archived_at"] = _now().isoformat(timespec="seconds")
    _save(projects)


@_writes
def promise_step(name: str, which: str, text: str) -> dict[str, Any]:
    """Stamp one step of the work on a promise (research, draft, status) and note it as the project's latest update."""
    text = label(text, "a step note", UPDATE_MAX)
    if not text:
        raise ProjectError("a step needs one line of text: what was done")
    projects = _load()
    p = _get(projects, name)
    at = _now().isoformat(timespec="seconds")
    pr = p.get("promise") or {}
    p["promise"] = {**pr, "steps": {**(pr.get("steps") or {}), which: at}}
    row = {"at": at, "kind": "project", "text": f"{which}: {text}"}
    p["updates"] = [*(p.get("updates") or []), row][-UPDATES_KEPT:]
    _save(projects)
    return row


def watch_list(active: bool = False, business: bool = False) -> list[dict[str, Any]]:
    """Each project with its sources, last check, and latest update, for an agent's routine and the Work view.

    `active` leaves out archived projects. Without `business`, titles, statuses, and update text stay out:
    what is left is pointers, dates, and kinds."""
    now = _now()
    local_day = calendar.now().date()
    out = []
    for name, p in sorted(_load().items()):
        if active and p.get("archived_at") or p.get("kind") == "promise":  # promises have their own list (`work promise list`, issue #268)
            continue
        limit = int(p.get("stale_after_days", DEFAULT_STALE_DAYS))
        sources = []
        for s in p["sources"]:
            row = {"kind": s["kind"], "ref": s["ref"], "checked_at": s.get("checked_at"), "stale": not p.get("archived_at") and _is_stale(s, limit, now), "reference": bool(reference_reason(s)), "reader": reader(s["kind"], s["ref"])}
            if business:
                row["status"] = s.get("status", "")
            sources.append(row)
        stamps = [s["checked_at"] for s in sources if s["checked_at"]]
        latest = dict((p.get("updates") or [None])[-1] or {}) or None
        if latest and not business:
            latest.pop("text")
        out.append(
            {
                "name": name,
                **({"title": p.get("title", "")} if business else {}),
                "group": p.get("group", ""),
                "parent": p.get("parent", ""),
                "archived": bool(p.get("archived_at")),
                "quiet_until": p.get("quiet_until", ""),
                "quiet": _quiet_record(p, local_day),
                "stale_after_days": limit,
                "stale": sum(s["stale"] for s in sources),
                "last_checked_at": max(stamps) if stamps else None,
                "latest_update": latest,
                "sources": sources,
                **({"prep": r} if (r := readiness(name, p)) else {}),
            }
        )
    return out


def readiness(name: str, p: dict[str, Any]) -> dict[str, Any] | None:
    """A prep project's four gates (issue #276), content-free: which are done, and the command that closes each open one.

    tool and questions: the owner marked them (`gate`). links: the owner marked every source but the memory file read
    (`gate <name> links <ref>`); a `check` stamp does not count, since the watcher stamps what careerbrew read. quiz: the latest quiz had no miss, so a quiz with misses stays open until a re-quiz of
    every miss comes back clean. None for a project that is not a prep project."""
    if p.get("kind") != "prep":
        return None
    marks = p.get("gates") or {}
    links = [s for s in p.get("sources") or [] if s["kind"] != "memory"]
    read = marks.get("links") or {}
    unread = [s for s in links if s["ref"] not in read]
    quizzes = p.get("quizzes") or []
    last = quizzes[-1] if quizzes else None
    rows = []
    for g in GATES:
        if g == "links":
            done = bool(links) and not unread
            at = max((read[s["ref"]] for s in links), default=None) if done else None
            note = f"{len(links) - len(unread)} of {len(links)} read by you" if links else "no links in the pack yet"
            fix = f"careerbrew work project gate {name} links {unread[0]['ref']}" if unread else f"careerbrew work project source {name} url <link>"
        elif g in MARKED_GATES:
            done, at, note, fix = bool(marks.get(g)), marks.get(g), "", f"careerbrew work project gate {name} {g}"
        else:
            done = bool(last) and not last["missed"]
            at = last["at"] if done and last else None
            note = "not taken yet" if not last else f"last: {last['asked'] - last['missed']} of {last['asked']}, {last['missed']} miss(es)"
            fix = f"careerbrew work project quiz {name} --asked <n> --missed <n>" + (f" (re-quiz the {last['missed']} miss(es))" if last and last["missed"] else "")
        rows.append({"gate": g, "text": GATE_TEXT[g], "done": done, "at": at, "note": note, "fix": "" if done else fix})
    n = sum(r["done"] for r in rows)
    quiz = {"at": last["at"], "asked": last["asked"], "missed": last["missed"], "score": round((last["asked"] - last["missed"]) / last["asked"], 2)} if last else None
    return {"event_on": p.get("event_on", ""), "done": n, "total": len(GATES), "ready": n == len(GATES), "gates": rows, "quiz": quiz, "quizzes": len(quizzes)}


@_writes
def gate(name: str, which: str, undo: bool = False, refs: list[str] | None = None) -> dict[str, Any]:
    """Mark a prep project's tool or questions gate done, or each link in `refs` read by the owner (`undo` opens it
    again). Returns its readiness."""
    projects = _load()
    p = _get(projects, name)
    if p.get("kind") != "prep":
        raise ProjectError(f"{name} is not a prep project (fix: careerbrew work project set {name} --kind prep)")
    if which not in MARKED_GATES:
        how = f"careerbrew work project quiz {name} --asked <n> --missed <n>"
        raise ProjectError(f"the {which} gate is not marked by hand: {how}" if which in GATES else f"unknown gate {which}; mark one of {', '.join(MARKED_GATES)}")
    refs = list(refs or [])
    if (which == "links") != bool(refs):
        raise ProjectError(f"name each link you read: careerbrew work project gate {name} links <ref> [<ref> ...]" if which == "links" else f"only the links gate takes refs (fix: careerbrew work project gate {name} {which})")
    marks = dict(p.get("gates") or {})
    now = _now().isoformat(timespec="seconds")
    if which == "links":
        have = {s["ref"] for s in p["sources"] if s["kind"] != "memory"}
        if missing := [r for r in refs if r not in have]:
            raise ProjectError(f"{name} has no link {missing[0]} (add it first: careerbrew work project source {name} url {missing[0]})")
        read = dict(marks.get("links") or {})
        for r in refs:
            _put(read, r, "" if undo else now)
        _put(marks, "links", read)
    else:
        _put(marks, which, "" if undo else now)
    _put(p, "gates", marks)
    _save(projects)
    events.emit("work.prep", status="ok", action="gate", step=which, undo=undo)
    return readiness(name, p) or {}


@_writes
def quiz(name: str, asked: int, missed: int) -> dict[str, Any]:
    """Store one quiz result, counts only (never a question or an answer). A re-quiz after misses asks each miss again."""
    projects = _load()
    p = _get(projects, name)
    if p.get("kind") != "prep":
        raise ProjectError(f"{name} is not a prep project (fix: careerbrew work project set {name} --kind prep)")
    if not 1 <= asked <= QUIZ_MAX or not 0 <= missed <= asked:
        raise ProjectError(f"a quiz asks 1 to {QUIZ_MAX} questions and misses 0 to all of them (got --asked {asked} --missed {missed})")
    quizzes = list(p.get("quizzes") or [])
    if quizzes and quizzes[-1]["missed"] > asked:
        raise ProjectError(f"the last quiz had {quizzes[-1]['missed']} miss(es): a re-quiz asks each of them again, so --asked is at least {quizzes[-1]['missed']}")
    quizzes.append({"at": _now().isoformat(timespec="seconds"), "asked": asked, "missed": missed})
    p["quizzes"] = quizzes[-QUIZZES_KEPT:]
    _save(projects)
    events.emit("work.prep", status="ok" if not missed else "fail", action="quiz", asked=asked, missed=missed)
    return readiness(name, p) or {}


def gate_blocks(day: date, until: date | None = None) -> list[dict[str, Any]]:
    """The open gates of each active prep project whose event is on `day` up to `until` (default: the day after):
    those days' plans start with them (issue #276), so an early event still gets its gates the workday before.
    The reason names no ref."""
    out = []
    last = until or day + timedelta(days=1)
    for name, p in sorted(_load().items()):
        try:
            on = date.fromisoformat(p.get("event_on") or "")
        except ValueError:
            continue
        when = "today" if on == day else "tomorrow" if on == day + timedelta(days=1) else f"on {on:%a %b} {on.day}"
        if p.get("archived_at") or not day <= on <= last or not (r := readiness(name, p)):
            continue
        for g in r["gates"]:
            if not g["done"]:
                reason = f"the event is {when} and this gate is open (see: careerbrew work project show {name})"
                out.append({"id": f"gate:{name}:{g['gate']}", "title": f"Prep gate: {g['text']}", "minutes": GATE_MINUTES, "gate": True, "project": name, "reason": reason})
    return out
