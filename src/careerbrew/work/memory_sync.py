"""Two-way sync between work projects and their memory files.

A project's memory file is the notes path set with `work project memory <name> <path>` (or
`--memory` on add): usually an agent's notes file, sometimes a file under the data root.

In (memory file -> project). The watcher reads each memory file every cycle and hashes its text
(the decrypted text, so a re-encrypted file with the same content is no change, and an edit that
keeps the mtime still is one). Only on a new hash does it parse the file, with the same reader
`work project absorb` uses for a ref (`research.detect`):
  - every http(s) link becomes a source of its kind: a Jira `/browse/` link a tracker, a GitHub link
    a repo, a Google Docs link a doc, a Slack archive link a chat, anything else a url (a link that
    opens a sign-in page is a reference, as always);
  - `trackers: PROJ-7, PROJ-8` lines, and ticket keys whose prefix the project already tracks, become
    tracker sources; `channels: #eng-billing` lines become chat sources;
  - `links: other-project` lines link projects that exist (an unknown name is skipped);
  - the last `status: <one line>` becomes the status of the project's memory source, which is stamped
    checked (and added when the project has none).
More than MAX_NEW_SOURCES new refs in one change: the rest become proposals (`work project show`).
The lines careerbrew wrote (its notes below, absorb, checkup research) are never parsed back: every
write records their digests, also while the sync is off, so turning it on again never reads them back.
A file careerbrew has no record of yet (notes from before this sync): its section headers (`## <date>
<kind> <ref>: ...`, `## <date> work tools: ...`) and the bullets under them in the exact shape that
section's writer produces (SHAPES; a key line for a file or page) count as careerbrew's. Any other
line, a bullet the owner added under careerbrew's section included, is the owner's and is read.

Out (work tools -> memory file). After careerbrew collects (the watcher, `work collect`, `work feed
pull | add`), every work-feed event and evidence item from the last NOTE_DAYS days that matches a
project goes into that project's memory file as one dated line with its link, under a
`## <date> work tools: N new item(s)` header. A match is, strongest first: the exact tracker key; a
link source (repo, chat channel, doc, url) or a pattern of the project's `project_priority` entry;
a tracker key prefix. An item goes to the best-matching project(s). A project with no memory path uses
its nearest parent's (`projects.notes_path`, as absorb and checkup research do); its items go there under
their own header tagged `(project <name>)`, unless the file's own project matches the item too. Never written: personal events (a
private entry, a rule, the owner's mark), noise (bots, no-reply), Slack recognition that repeats a feed
event, and anything into a MEMORY.md index. Each item is written once per file: the ids written are
recorded, and a line already in the file is not added again. When an event becomes personal later, the
exact line careerbrew wrote for it is removed. That is the one rewrite: right before the swap careerbrew
reads the file again, carries over text appended since its first read, keeps the file's mode, and leaves
a file changed any other way alone until the next sync.
Business content goes into a memory file only when business reading is allowed: a terminal or the
owner's web app for a CLI run, or a plugin policy `agent_may_read_business` for the watcher (the same
rule as checkup research).

No loop: every careerbrew write (these notes, absorb, checkup research) records the file's new hash,
so the next read sees no change. A write first parses an edit careerbrew has not read yet, so a
careerbrew write never hides an edit. The setting `project_memory_sync` (default true) turns both
directions off; a write made while it is off records its lines but not the hash, so an edit made
meanwhile is read once it is on.

State: `work/memory-sync.json` (encrypted at rest): per memory file, the last content hash, digests of
the lines careerbrew wrote that are still in it, and per item written, a digest of its id -> [a digest
of its line, the item's date]. No copied content.
Lifecycle: nothing is held in module scope between calls. `_locked()` serializes the CLI and the watcher
with an fcntl lock on `work/.memory-sync.lock`, released by the OS when the process exits; functions
named with a leading underscore expect the caller to hold it.
"""

from __future__ import annotations

import contextlib
import fcntl
import os
import re
import stat
from collections.abc import Iterator
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any

from .. import calendar, config, events, paths, store
from . import evidence, feed, project_priority, projects, research

NOTE_DAYS = 7  # items older than this are not written as notes
KEEP_DAYS = 60  # how long the record of a written item is kept (it cannot qualify again after NOTE_DAYS)
MAX_NEW_SOURCES = 20  # new refs beyond this in one change become proposals
TICKET = project_priority.TICKET
HEADING = re.compile(r"^#{1,6}\s")
OWN_SECTION = re.compile(r"^##\s+\d{4}-\d{2}-\d{2}\s+(?:(doc|tracker|repo|chat|memory|url)\s+(\S+)|(work tools):)")
# The bullets careerbrew's writer for each section kind produces (research.read, note_line). A local doc
# file, a memory file, and a page give key lines (research._key_lines) instead.
SHAPES = {
    "work tools": re.compile(r"\d{4}-\d{2}-\d{2} \S+: .*(?:<https?://\S+>|\([^()]*\))"),
    "tracker": re.compile(r"status: .*; updated [\d-]*(?:; due .*)?|child [A-Z][A-Z0-9]+-\d+: .*"),
    "repo": re.compile(r"\d+ PR\(s\) updated, \d+ merged|PR #\d+ .*"),
    "gdoc": re.compile(r"modified [\d-]*; \d+ new comment\(s\), \d+ open|comment: .*"),
    "chat": re.compile(r"\d+ message\(s\) since the last check|https?://\S+"),
}
NOTES_SECTION = re.compile(r"^##\s+\d{4}-\d{2}-\d{2}\s+work tools:")
LINK = re.compile(r"https?://[^\s<>()\[\]{}\"'`]+")
KEY_LINE = re.compile(r"^\s*(?:[-*+]\s+)?[*_]*(status|trackers?|channels?|links|related)[*_]*\s*:[*_]*\s*(.*?)\s*$", re.I)
_WORDS = re.compile(r"[,;|\s]+")
_CHANNEL = re.compile(r"#?[a-z0-9][a-z0-9._-]{0,79}")
OFF_BUSINESS = "business reading is off here: no notes written (run it in your terminal, or a plugin sets agent_may_read_business)"


def enabled() -> bool:
    return bool(config.get("project_memory_sync"))


def state_path() -> Path:
    return paths.sub("work", "memory-sync.json")


@contextlib.contextmanager
def _locked() -> Iterator[None]:
    fh = paths.sub("work", ".memory-sync.lock").open("a+")
    try:
        fcntl.flock(fh, fcntl.LOCK_EX)
        yield
    finally:
        fcntl.flock(fh, fcntl.LOCK_UN)
        fh.close()


def _state() -> dict[str, Any]:
    st = store.read_json(state_path(), {}) or {}
    st.setdefault("files", {})
    return st


# ---------- files ----------


def _path(memory: str) -> Path:
    p = Path(memory).expanduser()
    return p if p.is_absolute() else paths.data_root() / p


def _key(memory: str) -> str:
    return str(_path(memory).resolve())


def _inside(p: Path) -> bool:
    return p.resolve().is_relative_to(paths.data_root().resolve())


def _read(p: Path) -> str | None:
    if not p.is_file():
        return None
    if _inside(p):
        return store.read_text(p)
    return p.read_text(encoding="utf-8", errors="replace")


def _append(p: Path, before: str, section: str) -> None:
    if _inside(p):
        store.write_text(p, before + section)  # one ciphertext: read, add, rewrite (under the lock)
        return
    p.parent.mkdir(parents=True, exist_ok=True)
    with p.open("a", encoding="utf-8") as fh:  # an agent may write too: append, never rewrite
        fh.write(section)


def _rewrite(p: Path, old: str, new: str) -> bool:
    """Swap `old` (the text careerbrew read) for `new`. Reads the file again first: text appended since is
    carried over; any other change returns False and leaves the file alone. Keeps the file's mode."""
    now = _read(p)
    if now is None or not now.startswith(old):
        return False
    text = new + now[len(old) :]
    if _inside(p):
        store.write_text(p, text)
        return True
    tmp = p.with_name(f".{p.name}.{os.getpid()}.tmp")
    tmp.unlink(missing_ok=True)
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        os.fchmod(fd, stat.S_IMODE(p.stat().st_mode))
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            fh.write(text)
        tmp.replace(p)
    except BaseException:
        tmp.unlink(missing_ok=True)
        raise
    return True


def _record(p: Path, expected: str, rec: dict[str, Any]) -> None:
    """Remember the hash careerbrew wrote, so the next read skips it. Another writer in between: keep the old hash (it is parsed next)."""
    if _read(p) == expected:
        rec["hash"] = store.digest(expected)


def _files() -> dict[str, tuple[Path, list[tuple[str, str]]]]:
    """Memory file key -> (path, [(project, memory as stored)]) for every project with a memory path."""
    out: dict[str, tuple[Path, list[tuple[str, str]]]] = {}
    for name, p in projects._load().items():
        mem = str(p.get("memory") or "")
        if mem and not p.get("archived_at"):  # an archived project is not watched (issue #173)
            out.setdefault(_key(mem), (_path(mem), []))[1].append((name, mem))
    return out


# ---------- in: memory file -> project ----------


def _line_id(line: str) -> str:
    return store.digest(line.strip())


def _writes(kind: str, note: str) -> bool:
    """Whether careerbrew's writer for a section of this kind writes this note (a bullet without its `- `)."""
    if shape := SHAPES.get(kind):
        return bool(shape.fullmatch(note))
    return len(note) <= 200 and note == note.strip(" -*#\t") and bool(research.KEY_LINE.search(note))


def _seed_own(text: str) -> set[str]:
    """A file careerbrew has no record of: careerbrew's section headers, and the bullets under one (up to
    the first other line) in the shape that section's writer produces, count as careerbrew's."""
    own: set[str] = set()
    kind = ""  # the kind of careerbrew's section this line is in; "" outside one
    for line in text.splitlines():
        if HEADING.match(line):
            kind = ""
            if m := OWN_SECTION.match(line):
                gdoc = m.group(1) == "doc" and "/" not in m.group(2)  # a Google Docs id; a local doc's ref is its path
                kind = "gdoc" if gdoc else m.group(1) or m.group(3)
                own.add(_line_id(line))
        elif line.strip() and not line.startswith("- "):
            kind = ""
        elif kind and line.strip() and _writes(kind, line[2:].strip()):
            own.add(_line_id(line))
    return own


def _own(rec: dict[str, Any], text: str) -> set[str]:
    """The lines careerbrew wrote into this file, by digest (recorded on every write)."""
    return set(rec["own"]) if "own" in rec else _seed_own(text)


def _wrote(rec: dict[str, Any], before: str, section: str) -> None:
    rec["own"] = sorted(_own(rec, before) | {_line_id(ln) for ln in section.splitlines() if ln.strip()})


def parse(text: str, tracked: set[str], own: set[str] | None = None) -> dict[str, Any]:
    """{"refs": [(kind, ref)], "links": [project], "status": str} from the lines careerbrew did not write
    (`own`: their digests). `tracked`: tracker key prefixes the project has."""
    refs: list[tuple[str, str]] = []
    links: list[str] = []
    bare: list[str] = []
    status = ""
    own = _seed_own(text) if own is None else own
    for line in (ln for ln in text.splitlines() if _line_id(ln) not in own):
        for raw in LINK.findall(line):
            refs.append(research.detect(raw.rstrip(".,;:!?*_")))
        rest = LINK.sub(" ", line)
        if m := KEY_LINE.match(line):
            key, value = m.group(1).lower(), m.group(2)
            words = [w for w in _WORDS.split(LINK.sub(" ", value)) if w]
            if key == "status":
                status = " ".join(value.split())[:200] or status
            elif key.startswith("tracker"):
                refs += [("tracker", w.upper()) for w in words if TICKET.fullmatch(w.upper())]
            elif key.startswith("channel"):
                refs += [("chat", "#" + w.lower().lstrip("#")) for w in words if _CHANNEL.fullmatch(w.lower())]
            else:
                links += [s for s in (re.sub(r"[^a-z0-9]+", "-", w.lower()).strip("-") for w in words) if s]
        bare += [t.group(0) for t in TICKET.finditer(rest)]
    tracked = tracked | {r.split("-")[0] for k, r in refs if k == "tracker"}
    refs += [("tracker", t) for t in bare if t.split("-")[0] in tracked]
    return {"refs": list(dict.fromkeys(refs)), "links": list(dict.fromkeys(links)), "status": status}


def _apply(every: dict[str, Any], name: str, mem: str, found: dict[str, Any], stamp: str) -> dict[str, int]:
    p = every[name]
    have = {(s["kind"], s["ref"]) for s in p["sources"]}
    new = [r for r in found["refs"] if r not in have]
    add, extra = new[:MAX_NEW_SOURCES], new[MAX_NEW_SOURCES:]
    for kind, ref in add:
        p["sources"].append({"kind": kind, "ref": ref, "checked_at": None, "status": ""})
    if add and p.get("proposed"):
        p["proposed"] = [x for x in p["proposed"] if (x["kind"], x["ref"]) not in set(add)]
    known = {(x["kind"], x["ref"]) for x in p.get("proposed", [])}
    proposed = [{"kind": k, "ref": r, "via": "memory"} for k, r in extra if (k, r) not in known]  # the inbox says where it came from (issue #147)
    if proposed:
        p.setdefault("proposed", []).extend(proposed)
    links = [x for x in found["links"] if x in every and x != name and x not in (p.get("links") or [])]
    if links:
        p["links"] = list(p.get("links") or []) + links
    src = next((s for s in p["sources"] if s["kind"] == "memory" and s["ref"] == mem), None)
    if src is None:
        src = {"kind": "memory", "ref": mem, "checked_at": None, "status": ""}
        p["sources"].append(src)
    src["checked_at"] = stamp
    src.pop("stale_reason", None)
    changed = bool(found["status"]) and src.get("status") != found["status"]
    if found["status"]:
        src["status"] = found["status"]
    return {"sources": len(add), "proposed": len(proposed), "links": len(links), "status": int(changed)}


def _parse_into(key: str, users: list[tuple[str, str]], text: str, st: dict[str, Any]) -> dict[str, int]:
    """Parse one memory file into every project that uses it, and record its hash and careerbrew's lines still in it."""
    rec = st["files"].setdefault(key, {})
    own = _own(rec, text)
    every = projects._load()
    stamp = projects._now().isoformat(timespec="seconds")
    got = {"sources": 0, "proposed": 0, "links": 0, "status": 0}
    for name, mem in users:
        if name not in every:
            continue
        tracked = {str(s["ref"]).upper().split("-")[0] for s in every[name]["sources"] if s["kind"] == "tracker"}
        for k, v in _apply(every, name, mem, parse(text, tracked, own), stamp).items():
            got[k] += v
    projects._save(every)
    rec["hash"] = store.digest(text)
    rec["own"] = sorted(own & {_line_id(ln) for ln in text.splitlines()})  # lines the owner deleted are forgotten
    return got


def _catch_up(key: str, text: str | None, st: dict[str, Any]) -> None:
    """Before careerbrew writes a memory file, parse an edit it has not read yet, so the write never hides it."""
    if text is None or not enabled() or (st["files"].get(key) or {}).get("hash") == store.digest(text):
        return
    users = _files().get(key)
    if users:
        _parse_into(key, users[1], text, st)


def sync_in() -> dict[str, int]:
    """Parse every memory file whose content changed since careerbrew last read or wrote it."""
    out = {"files": 0, "sources": 0, "proposed": 0, "links": 0, "status": 0, "errors": 0}
    if not enabled():
        return out
    with _locked():
        st = _state()
        before = store.digest(st)
        for key, (path, users) in _files().items():
            try:
                text = _read(path)
            except (OSError, ValueError, RuntimeError):  # unreadable, or a vault error
                out["errors"] += 1
                continue
            if text is None or (st["files"].get(key) or {}).get("hash") == store.digest(text):
                continue
            out["files"] += 1
            for k, v in _parse_into(key, users, text, st).items():
                out[k] += v
        if store.digest(st) != before:
            store.write_json(state_path(), st)
    if out["files"] or out["errors"]:
        events.emit("work.memory.in", status="ok" if not out["errors"] else "error", files=out["files"], sources=out["sources"], proposed=out["proposed"], links=out["links"], count=out["status"], errors=out["errors"])
    return out


# ---------- out: work tools -> memory file ----------


def _when(at: Any) -> datetime | None:
    try:
        dt = datetime.fromisoformat(str(at or "").strip().replace("Z", "+00:00"))
    except ValueError:
        return None
    tz = calendar.local_tz()
    return (dt if dt.tzinfo else dt.replace(tzinfo=tz)).astimezone(tz)


def candidates() -> list[dict[str, Any]]:
    """Work items from the last NOTE_DAYS days: feed events (not personal, not noise), then evidence the feed does not already hold."""
    cut = calendar.now() - timedelta(days=NOTE_DAYS)
    rows = feed.load()
    skip = feed.personal_ids()
    for r in rows:
        skip |= {r["id"], *feed.aliases(r["id"])}  # Slack recognition repeats a feed event under its alias
    out: list[dict[str, Any]] = []
    for r in rows:
        if not r.get("personal") and r.get("triage") != "noise":
            out.append({"id": r["id"], "source": r.get("source") or "feed", "title": r.get("title") or "", "at": r.get("at"), "url": r.get("url") or "", "channel": r.get("channel") or "", "team": list(r.get("team") or [])})
    for e in evidence.load():
        if e.get("id") not in skip and not e.get("personal"):
            skip.add(e["id"])
            out.append({"id": e["id"], "source": e.get("kind") or "evidence", "title": e.get("title") or "", "at": e.get("at"), "url": e.get("url") or "", "repo": e.get("repo") or ""})
    keep = []
    for it in out:
        when = _when(it["at"])
        if when is not None and when >= cut:
            keep.append({**it, "when": when})
    return sorted(keep, key=lambda it: (it["when"], it["id"]))


def _facts(it: dict[str, Any]) -> dict[str, Any]:
    url, title, rid = str(it.get("url") or ""), str(it.get("title") or ""), str(it.get("id") or "")
    repo = project_priority.repo_of(rid, url) or str(it.get("repo") or "")
    return {
        "id": rid,
        "url": url,
        "title": title.lower(),
        "repo": repo.lower(),
        "channel": str(it.get("channel") or "").lower(),
        "tickets": {m.group(0) for m in TICKET.finditer(f"{title} {url}")},
        "keys": project_priority.keys(title, repo, url),
    }


def _score(f: dict[str, Any], p: dict[str, Any], globs: list[str] | None) -> int:
    """3 an exact tracker key; 2 a repo, channel, doc, url, or priority pattern; 1 a tracker key prefix; 0 no match."""
    best = 0
    prefixes = {t.split("-")[0] for t in f["tickets"]}
    for s in p.get("sources") or []:
        kind, ref = s.get("kind"), str(s.get("ref") or "")
        if not ref:
            continue
        if kind == "tracker":
            r = ref.upper()
            best = max(best, 3 if r in f["tickets"] else 1 if r.split("-")[0] in prefixes else 0)
        elif kind == "repo" and f["repo"]:
            r = ref.lower()
            if f["repo"] == r or ("/" not in f["repo"] and f["repo"] == r.rsplit("/", 1)[-1]):
                best = max(best, 2)
        elif kind == "chat":
            if ref.startswith("#"):
                name = ref[1:].lower()
                if f["channel"] == name or re.search(rf"(?<![\w-])#{re.escape(name)}(?![\w-])", f["title"]):
                    best = max(best, 2)
            elif re.search(rf"/archives/{re.escape(ref)}(?:[/?]|$)", f["url"]) or f["id"].startswith(f"slack:{ref}:"):
                best = max(best, 2)
        elif kind == "doc" and "/" not in ref and ref in f["url"]:
            best = max(best, 2)
        elif kind == "url" and f["url"] and f["url"].rstrip("/").startswith(ref.rstrip("/")):
            # A PR/issue source names one item; numeric prefixes are other items.
            if re.fullmatch(r"https?://github(?:\.com|\.[^/]+)/[^/]+/[^/]+/(?:pull|issues)/\d+/?", ref, re.I):
                suffix = f["url"][len(ref.rstrip("/")) :]
                if suffix and suffix[0] not in "/?#":
                    continue
            best = max(best, 2)
    if globs and project_priority.match(f["keys"], [{"name": "", "rank": 0, "match": globs}]):
        best = max(best, 2)
    return best


def targets(it: dict[str, Any], every: dict[str, Any], globs: dict[str, list[str]]) -> list[str]:
    """The best-matching project(s) for one item; none when nothing matches."""
    f = _facts(it)
    scores = {n: _score(f, p, globs.get(n)) for n, p in every.items()}
    best = max(scores.values(), default=0)
    return sorted(n for n, s in scores.items() if s and s == best)


def note_line(it: dict[str, Any]) -> str:
    """`<date> <source>: <title> <link>`: the item's own local date, its title, and its link (or its id when it has none)."""
    when = it.get("when") or _when(it.get("at"))
    day = when.date().isoformat() if when else ""
    title = " ".join(str(it.get("title") or "").split())[:160]
    if it.get("team"):  # a teammate's event says whose it is (issue #180)
        title = f"{', '.join(it['team'])}: {title}"
    link = f"<{it['url']}>" if it.get("url") else f"({it['id']})"
    return f"{day} {it['source']}: {title} {link}"


def _bullet(line: str) -> str:
    return f"- {line}"


def _section(header: str, lines: list[str]) -> str:
    return f"\n## {header}\n\n" + "".join(_bullet(n) + "\n" for n in lines)


def _for(names: set[str]) -> str:
    """The header tag for an item in a parent's file: none when the file's own project matches it ("" in
    `names`), else the child project(s) it is for, as `research.write_memory` heads a child's notes."""
    if "" in names:
        return ""
    kids = sorted(names)
    return f" (project{'s' if len(kids) > 1 else ''} {', '.join(kids)})"


def _prune(notes: dict[str, Any]) -> None:
    cut = (calendar.now() - timedelta(days=KEEP_DAYS)).date().isoformat()
    for d in [d for d, (_line, day) in notes.items() if day and day < cut]:
        del notes[d]


def sync_out(allowed: bool) -> dict[str, Any]:
    """Write matching work items as dated notes; first drop the notes of events that became personal."""
    out: dict[str, Any] = {"notes": 0, "projects": 0, "removed": forget(feed.personal_ids()), "errors": 0, "reason": ""}
    if not enabled():
        out["reason"] = "project_memory_sync is off (turn it on: careerbrew config set project_memory_sync true)"
        return out
    if not allowed:
        out["reason"] = OFF_BUSINESS
        return out
    every = projects._load()
    globs = {e["name"]: e["match"] for e in project_priority.entries()}
    plan: dict[str, dict[str, Any]] = {}
    for it in candidates():
        for name in targets(it, every, globs):
            if every[name].get("archived_at"):
                continue
            mem = projects.notes_path(name, every)  # its own, else the nearest parent's (as absorb writes)
            if not mem or projects.is_memory_index(mem):
                continue
            w = plan.setdefault(_key(mem), {"path": _path(mem), "projects": set(), "items": {}, "for": {}})
            w["projects"].add(name)
            w["items"].setdefault(it["id"], it)
            w["for"].setdefault(it["id"], set()).add("" if every[name].get("memory") else name)  # "": the file's own project
    if not plan:
        return out
    header = f"{calendar.now().date().isoformat()} work tools"
    written: set[str] = set()
    with _locked():
        st = _state()
        for key, w in plan.items():
            rec = st["files"].setdefault(key, {})
            notes = rec.setdefault("notes", {})
            try:
                text = _read(w["path"])
                _catch_up(key, text, st)
                present = {ln.strip() for ln in (text or "").splitlines()}
                groups: dict[str, list[str]] = {}  # header tag -> lines
                pending = {}
                for iid, it in w["items"].items():
                    d = store.digest(it["id"])
                    if d in notes:
                        continue
                    line = note_line(it)
                    pending[d] = [store.digest(_bullet(line)), it["when"].date().isoformat()]
                    if _bullet(line) not in present:
                        groups.setdefault(_for(w["for"][iid]), []).append(line)
                if groups:
                    section = "".join(_section(f"{header}: {len(lines)} new item(s){tag}", lines) for tag, lines in sorted(groups.items()))
                    _append(w["path"], text or "", section)
                    _record(w["path"], (text or "") + section, rec)
                    _wrote(rec, text or "", section)
                    out["notes"] += sum(len(lines) for lines in groups.values())
                    written |= w["projects"]
                notes.update(pending)  # only after the write went through
            except (OSError, ValueError, RuntimeError):
                out["errors"] += 1
            _prune(notes)
        store.write_json_if_changed(state_path(), st)  # the watcher runs this every cycle: nothing new is no change (US-64)
    out["projects"] = len(written)
    if out["notes"] or out["errors"]:
        events.emit("work.memory.out", status="ok" if not out["errors"] else "error", notes=out["notes"], projects=out["projects"], errors=out["errors"])
    return out


def _drop_empty_note_sections(text: str) -> str:
    """A `work tools` section left with nothing under its header after a removal goes too."""
    blocks = re.split(r"(?m)^(?=#{1,6}\s)", text)
    keep = [b for b in blocks if not (NOTES_SECTION.match(b) and not b.partition("\n")[2].strip())]
    return "".join(keep)


def forget(ids: set[str]) -> int:
    """Remove the notes careerbrew wrote for these event ids (an event that became personal). Returns lines removed."""
    if not ids:
        return 0
    want = {store.digest(i) for i in ids}
    removed = 0
    with _locked():
        st = _state()
        before = store.digest(st)
        for key, rec in st["files"].items():
            notes = rec.get("notes") or {}
            hit = [d for d in notes if d in want]
            if not hit:
                continue
            drop = {notes[d][0] for d in hit}
            p = Path(key)
            try:
                text = _read(p)
                if text is not None:
                    _catch_up(key, text, st)
                    lines = text.splitlines(keepends=True)
                    kept = [ln for ln in lines if store.digest(ln.strip()) not in drop]
                    if len(kept) < len(lines):  # else the owner edited or removed those lines: leave them be
                        new = _drop_empty_note_sections("".join(kept))
                        if not _rewrite(p, text, new):
                            continue  # the file changed while careerbrew read it: the next sync tries again
                        if enabled():
                            _record(p, new, rec)  # text appended meanwhile: no match, so the next read parses it
                        removed += len(lines) - len(kept)
            except (OSError, ValueError, RuntimeError):
                continue  # unreadable now: the next sync tries again
            for d in hit:
                del notes[d]
        if store.digest(st) != before:
            store.write_json(state_path(), st)
    if removed:
        events.emit("work.memory.forget", status="ok", dropped=removed)
    return removed


def append(memory: str, header: str, notes: list[str]) -> bool:
    """Append a dated section to a memory file and record the hash (absorb and checkup research write through here)."""
    if not memory or not notes:
        return False
    p, key = _path(memory), _key(memory)
    section = _section(header, notes)
    with _locked():
        st = _state()
        text = _read(p)
        if enabled():
            _catch_up(key, text, st)
        _append(p, text or "", section)
        rec = st["files"].setdefault(key, {})
        if enabled():
            _record(p, (text or "") + section, rec)  # sync off: no hash, so an edit made meanwhile is read once it is on
        _wrote(rec, text or "", section)  # always: turning the sync on again never reads these lines back
        store.write_json(state_path(), st)
    return True


def run(allowed: bool) -> dict[str, Any]:
    """Both directions: memory files into projects, then matching work items into memory files."""
    projects.normalize_sources()
    return {"enabled": enabled(), "in": sync_in(), "out": sync_out(allowed)}
