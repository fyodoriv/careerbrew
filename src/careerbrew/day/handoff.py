"""The next-day handoff (issue #210): what a new session needs first thing tomorrow.

`<data root>/day/handoff-YYYY-MM-DD.json`, one file per local day, encrypted like
other work data. Each change rewrites the whole file, so a session that stops
early still leaves it current. Items hold ids, links, and short text only.
The first session of the next day reads the latest earlier file and treats
every item as "verify live before acting".

End of day (issue #265, `workday/end.py`) owns the items it writes (`owner: end-of-day`): each open
PR (section `prs`, with a `recheck` command that fails once the PR merged or closed) and each open
question record (section `questions`, its record's id and recheck). Writing them again replaces
only those; items an agent added stay. The next `careerbrew day start` runs each recheck and drops
the closed items.

Lifecycle: no process memory. Every load-change-save runs under the day file's lock
(`.handoff-<day>.json.lock`, `_locked`), so two sessions that write one handoff keep both changes.
"""

from __future__ import annotations

import contextlib
import fcntl
import hashlib
from collections.abc import Iterator
from datetime import date
from pathlib import Path
from typing import Any

from .. import paths, store

SECTIONS = ("calendar", "prs", "waiting", "questions", "promises", "unverified", "lessons", "gotchas", "projects")
TITLES = {
    "calendar": "Tomorrow's calendar",
    "prs": "Open PRs",
    "waiting": "Waiting on others",
    "questions": "Open questions",
    "promises": "Promises you made",
    "unverified": "Claims not verified yet",
    "lessons": "Rules and lessons learned",
    "gotchas": "Gotchas",
    "projects": "Project sessions",  # one item per project, written by its session (issue #274)
}
MAX_TEXT = 200


def today() -> date:
    from ..calendar import now

    return now().date()


def path_for(day: date) -> Path:
    return paths.sub("day", f"handoff-{day.isoformat()}.json")


def load(day: date | None = None) -> list[dict[str, Any]]:
    return list((store.read_json(path_for(day or today()), {}) or {}).get("items", []))


def _save(items: list[dict[str, Any]], day: date) -> None:
    store.write_json_if_changed(path_for(day), {"day": day.isoformat(), "items": items})


@contextlib.contextmanager
def _locked(day: date) -> Iterator[None]:
    """Hold the day's handoff lock for one load-change-save; another writer waits."""
    path = path_for(day)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.with_name(f".{path.name}.lock").open("a") as fh:
        fcntl.flock(fh, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(fh, fcntl.LOCK_UN)


def item(section: str, text: str, link: str = "", **extra: Any) -> dict[str, Any]:
    """A checked item (not saved): `extra` holds fields such as `recheck` or a fixed `id`."""
    if section not in SECTIONS:
        raise ValueError(f"unknown section {section!r}; use one of: {', '.join(SECTIONS)}")
    text = " ".join(text.split())
    if not text:
        raise ValueError("the item text is empty")
    if len(text) > MAX_TEXT:
        raise ValueError(f"keep an item short: {len(text)} characters, at most {MAX_TEXT} (put the detail behind a link)")
    out = {"id": "h-" + hashlib.sha256(f"{section}\n{text}".encode()).hexdigest()[:8], "section": section, "text": text}
    if link:
        out["link"] = link
    return {**out, **{k: v for k, v in extra.items() if v}}


def add(section: str, text: str, link: str = "", day: date | None = None) -> dict[str, Any]:
    """Add an item to the day's handoff (an item with the same section and text is kept once)."""
    new = item(section, text, link)
    day = day or today()
    with _locked(day):
        items = [i for i in load(day) if i["id"] != new["id"]] + [new]
        _save(items, day)
    return new


def replace_owned(owner: str, items: list[dict[str, Any]], day: date | None = None) -> None:
    """Replace the day's items that `owner` wrote with `items`; every other item stays (issue #265)."""
    day = day or today()
    ids = {i["id"] for i in items}
    with _locked(day):
        kept = [i for i in load(day) if i.get("owner") != owner and i["id"] not in ids]
        _save(kept + [{**i, "owner": owner} for i in items], day)


def done(item_id: str, day: date | None = None) -> bool:
    """Drop a finished item. Returns False when no item has that id."""
    day = day or today()
    with _locked(day):
        items = load(day)
        kept = [i for i in items if i["id"] != item_id]
        if len(kept) == len(items):
            return False
        _save(kept, day)
    return True


def with_promises(items: list[dict[str, Any]], business: bool = True) -> list[dict[str, Any]]:
    """The handoff as shown: its items, plus every open promise (issue #268) live in "promises". A hand-added
    promises item with a tracked promise's link is that promise, so it shows once and goes when the promise closes."""
    from ..work import promises

    tracked = promises.links()
    kept = [i for i in items if not (i["section"] == "promises" and i.get("link") and i["link"] in tracked)]
    return kept + promises.handoff_items(business)


def latest_before(day: date | None = None) -> tuple[date | None, list[dict[str, Any]]]:
    """The newest handoff written on a day before `day`: what the first session of `day` shows."""
    day = day or today()
    folder = path_for(day).parent
    found = sorted(
        (date.fromisoformat(p.stem.removeprefix("handoff-")) for p in folder.glob("handoff-*.json")),
        reverse=True,
    )
    for d in found:
        if d < day:
            items = load(d)
            if items:
                return d, items
    return None, []
