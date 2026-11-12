"""Bulk import of work projects from a sheet: a team's goal list or a quarter's cutline (issue #69).

Rows come from a CSV file, a JSON file (a list of objects, or {"projects": [...]}), or a Google
Sheets link. A link is read as its CSV export, so the sheet must be shared as "anyone with the
link"; careerbrew never signs in to Google, and the request obeys the plugin host policy.
Columns, in any order and any case: name (required), title, group, parent, links, trackers.
Other columns are ignored and named. `links` and `trackers` hold several values split by commas,
semicolons, `|`, or spaces. A name is lowered and its spaces become dashes ("Checkout Analytics"
-> checkout-analytics); parent and links are matched the same way.

All or nothing: every row is checked first, against the sheet and the record together (names,
labels, parents and links that exist, no parent loops), and one bad row writes nothing. A new
name adds a project; a known one takes each non-empty cell, and an empty cell keeps what is
there. Trackers become `tracker` sources. A source is never removed, so its `checked_at`
survives a re-import.
"""

from __future__ import annotations

import copy
import csv
import io
import json
import re
import urllib.error
from pathlib import Path
from typing import Any

from .. import connectors, events, paths, store
from . import projects
from .projects import ProjectError

COLUMNS = ("name", "title", "group", "parent", "links", "trackers")
MAX_ROWS = 2000
_SHEET = re.compile(r"^https://docs\.google\.com/spreadsheets/d/(e/)?([A-Za-z0-9_-]+)")
_MANY = re.compile(r"[,;|\s]+")
_FIX_SHARE = "fix: use File > Download > CSV and import the file (recommended); use a shared link only when the sheet's owner and your company policy allow it"


def slug(text: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", text.strip().lower()).strip("-")


def sheet_csv_url(url: str) -> str:
    """A Google Sheets link (edit, share, export, or published) -> its CSV export, same tab (gid)."""
    m = _SHEET.match(url.strip())
    if not m:
        raise ProjectError("import reads a .csv or .json file, or a Google Sheets link (https://docs.google.com/spreadsheets/d/<id>/...); for another site, download the sheet as CSV and import the file")
    gid = re.search(r"[#&?]gid=(\d+)", url)
    tab = f"&gid={gid.group(1)}" if gid else ""
    if m.group(1):  # published to the web: /d/e/<id>/pub
        return f"https://docs.google.com/spreadsheets/d/e/{m.group(2)}/pub?output=csv{tab}"
    return f"https://docs.google.com/spreadsheets/d/{m.group(2)}/export?format=csv{tab}"


def _fetch(url: str) -> str:
    try:
        raw = connectors.http_get(sheet_csv_url(url))
    except urllib.error.HTTPError as e:
        raise ProjectError(f"the sheet answered HTTP {e.code} ({_FIX_SHARE})") from None
    except (urllib.error.URLError, TimeoutError) as e:  # offline, DNS, proxy
        raise ProjectError(f"could not reach docs.google.com ({type(e).__name__}); download the sheet as CSV and import the file") from None
    text = raw.decode("utf-8-sig", errors="replace")
    if text.lstrip().startswith("<"):
        raise ProjectError(f"the link returned a sign-in page, not CSV: the sheet is private ({_FIX_SHARE})")
    return text


def _read(source: str) -> tuple[str, str]:
    """(text, "csv" | "json") from a file or a Google Sheets link."""
    if re.match(r"^[a-z]+://", source, re.I):
        return _fetch(source), "csv"
    path = Path(source).expanduser()
    if not path.is_file():
        raise ProjectError(f"no file {source} (fix: give a .csv or .json file, or a Google Sheets link)")
    inside = path.resolve().is_relative_to(paths.data_root().resolve())
    try:
        text = (store.read_text(path) if inside else path.read_text(encoding="utf-8-sig")) or ""
    except UnicodeDecodeError:
        raise ProjectError(f"{path.name} is not a text file (fix: save the sheet as CSV, File > Download > CSV, and import that)") from None
    return text.lstrip("\ufeff"), "json" if path.suffix.lower() == ".json" else "csv"


def _rows(text: str, fmt: str) -> tuple[list[tuple[str, dict[str, Any]]], list[str]]:
    """[(where, {column: cell})] for each non-empty row, and the ignored column names."""
    if fmt == "json":
        try:
            data = json.loads(text)
        except json.JSONDecodeError as e:
            raise ProjectError(f'not JSON ({e.msg}, line {e.lineno}); use a list of objects, or {{"projects": [...]}}') from None
        if isinstance(data, dict):
            data = data.get("projects")
        if not isinstance(data, list) or not all(isinstance(r, dict) for r in data):
            raise ProjectError('a JSON import is a list of objects, or {"projects": [...]}')
        found = [(f"item {i}", {str(k).strip().lower(): v for k, v in r.items()}) for i, r in enumerate(data, 1)]
        ignored = sorted({k for _, r in found for k in r} - set(COLUMNS))
        rows = [(w, {k: v for k, v in r.items() if k in COLUMNS}) for w, r in found]
    else:
        reader = csv.reader(io.StringIO(text))
        header = [h.strip().lower() for h in next(reader, [])]
        if "name" not in header:
            raise ProjectError(f"the sheet has no name column (fix: make the first row a header: {', '.join(COLUMNS)})")
        ignored = sorted({h for h in header if h and h not in COLUMNS})
        rows = [(f"row {i}", {h: c for h, c in zip(header, cells, strict=False) if h in COLUMNS}) for i, cells in enumerate(reader, 2) if any(c.strip() for c in cells)]
    if len(rows) > MAX_ROWS:
        raise ProjectError(f"the sheet has {len(rows)} rows; import at most {MAX_ROWS} at a time")
    return rows, ignored


def _text(v: Any) -> str:
    return "" if v is None else str(v).strip()


def _many(v: Any) -> list[str]:
    items = v if isinstance(v, list) else _MANY.split(_text(v))
    return list(dict.fromkeys(x for x in (_text(i) for i in items) if x))


def _parse(rows: list[tuple[str, dict[str, Any]]]) -> tuple[list[dict[str, Any]], list[str]]:
    out: list[dict[str, Any]] = []
    problems: list[str] = []
    seen: dict[str, str] = {}
    for where, r in rows:
        name = slug(_text(r.get("name")))
        if not name:
            problems.append(f"{where}: no name (fix: fill the name column, or delete the row)")
            continue
        if not projects.valid_name(name):
            problems.append(f"{where}: the name is longer than 40 characters (fix: shorten it)")
            continue
        if name in seen:
            problems.append(f"{where}: {name} is also {seen[name]} (fix: keep one row per project)")
            continue
        seen[name] = where
        try:
            title = projects.label(_text(r.get("title")), "title", projects.TITLE_MAX)
            group = projects.label(_text(r.get("group")), "group", projects.GROUP_MAX)
        except ProjectError as e:
            problems.append(f"{where} ({name}): {e}")
            continue
        trackers = _many(r.get("trackers"))
        if any(len(t) > 200 for t in trackers):
            problems.append(f"{where} ({name}): a tracker is at most 200 characters")
            continue
        links = list(dict.fromkeys(x for x in (slug(v) for v in _many(r.get("links"))) if x))
        out.append({"where": where, "name": name, "title": title, "group": group, "parent": slug(_text(r.get("parent"))), "links": links, "trackers": trackers})
    return out, problems


@projects._writes
def run(source: str, dry_run: bool = False) -> dict[str, Any]:
    """Check every row, then add or update the projects in one write (none on any problem)."""
    text, fmt = _read(source)
    rows, ignored = _rows(text, fmt)
    parsed, problems = _parse(rows)
    merged = copy.deepcopy(projects._load())
    added: list[str] = []
    updated: list[str] = []
    unchanged: list[str] = []
    sources = 0
    for r in parsed:
        new = r["name"] not in merged
        p = merged.setdefault(r["name"], projects.new_record())
        before = json.dumps(p, sort_keys=True)
        for key in ("title", "group", "parent", "links"):
            if r[key]:
                p[key] = r[key]
        for ref in r["trackers"]:
            if not any(s["kind"] == "tracker" and s["ref"] == ref for s in p["sources"]):
                p["sources"].append({"kind": "tracker", "ref": ref, "checked_at": None, "status": ""})
                sources += 1
        (added if new else updated if json.dumps(p, sort_keys=True) != before else unchanged).append(r["name"])
    where = {r["name"]: r["where"] for r in parsed}
    problems += [f"{where[n]} ({n}): {msg}" if n in where else f"{n}: {msg}" for n, msg in projects.graph_problems(merged)]
    if problems:
        problems.sort(key=lambda x: int(m.group(1)) if (m := re.match(r"(?:row|item) (\d+)", x)) else 0)  # in sheet order
        raise ProjectError(f"{len(problems)} problem(s), nothing imported:\n  " + "\n  ".join(problems))
    if not parsed:
        raise ProjectError(f"no project rows (fix: one row per project under a header: {', '.join(COLUMNS)})")
    if not dry_run:
        projects._save(merged)
        events.emit("work.project", status="ok", action="import", count=len(parsed), added=len(added), updated=len(updated))
    return {"rows": len(parsed), "added": added, "updated": updated, "unchanged": unchanged, "sources": sources, "ignored": ignored, "dry_run": dry_run}
