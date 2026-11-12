"""Grade a PM or daily update against the owner's next ladder level (issue #216).

Each line gets the ladder dimensions it shows, by the same signals `work promo` uses. The owner sees a coverage table
(strong / some / none) and one reword hint per weak dimension. The hints quote only the owner's own lines; with no
fitting line they name the work that would create one. Tags and table never go into the sent message.
`record` keeps each tagged line as evidence (tag `dim:<id>`), so `work promo` and `work gaps` count it.
"""

from __future__ import annotations

from datetime import datetime
from typing import Any

from .. import calendar, events
from . import evidence, promo

STRONG = 2  # lines that show a dimension before its coverage is strong


def target() -> tuple[str, str]:
    from .. import paths, store

    settings = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
    return str(settings.get("ladder", "generic-swe")), str(settings.get("target_level", "senior"))


def levels(ladder_id: str, level: str) -> list[dict[str, Any]]:
    """The level's dimensions: id, name, the level's description, and its looks-like / doesn't-look-like examples."""
    lads = promo.ladders()
    if ladder_id not in lads:
        raise promo.LadderError(f"unknown ladder {ladder_id}; have {sorted(lads)}")
    lad = lads[ladder_id]
    if level not in lad["levels"]:
        raise promo.LadderError(f"level {level} not in {lad['levels']}")
    out = []
    for c in lad["competencies"]:
        ex = (c.get("examples") or {}).get(level) or {}
        out.append(
            {
                "id": c["id"],
                "name": c["name"],
                "description": (c.get("descriptions") or {}).get(level, ""),
                "looks_like": list(ex.get("looks_like") or []),
                "doesnt_look_like": list(ex.get("doesnt_look_like") or []),
                "how_to_close": c.get("how_to_close", ""),
            }
        )
    return out


def dims_of(title: str, ladder_id: str | None = None) -> list[str]:
    """The ladder dimensions a task or line title shows, by the ladder's own signals; [] for an unknown ladder."""
    lad = promo.ladders().get(ladder_id or target()[0])
    return [c["id"] for c in lad["competencies"] if promo._matches({"title": title}, c)] if lad else []


def grade(lines: list[str], ladder_id: str, level: str) -> dict[str, Any]:
    comps = {c["id"]: c for c in promo.ladders()[ladder_id]["competencies"]} if ladder_id in promo.ladders() else {}
    dims = levels(ladder_id, level)
    tagged = [{"line": ln, "dims": [d["id"] for d in dims if promo._matches({"title": ln}, comps[d["id"]])]} for ln in lines if ln.strip()]
    table, hints = [], []
    for d in dims:
        shown = [t["line"] for t in tagged if d["id"] in t["dims"]]
        coverage = "strong" if len(shown) >= STRONG else "some" if shown else "none"
        table.append({"id": d["id"], "name": d["name"], "lines": shown, "coverage": coverage})
        if coverage == "some":
            bar = d["description"] or d["name"].lower()
            hints.append({"id": d["id"], "line": shown[0], "hint": f"say the {level}-level part of this line with true facts only (the bar: {bar})"})
        elif coverage == "none":
            hints.append({"id": d["id"], "line": "", "hint": f"no line shows it, and no true fact fits; work that would create one: {d['how_to_close'] or d['name']}"})
    events.emit("work.ladder_grade", status="ok", lines=len(tagged), strong=sum(r["coverage"] == "strong" for r in table), none=sum(r["coverage"] == "none" for r in table))
    return {"ladder": ladder_id, "level": level, "lines": tagged, "coverage": table, "suggestions": hints}


def record(graded: dict[str, Any], day: str) -> int:
    """Keep each tagged line as evidence for the day, once: a regrade of the same line adds nothing."""
    have = {it.get("id") for it in evidence.load()}
    added = 0
    for t in graded["lines"]:
        if not t["dims"]:
            continue
        at = datetime.fromisoformat(f"{day}T18:00:00").replace(tzinfo=calendar.local_tz()).isoformat()
        if "note:" + _digest(t["line"], at) in have:
            continue
        evidence.note(t["line"], [f"dim:{d}" for d in t["dims"]], at=at)
        added += 1
    return added


def _digest(title: str, at: str) -> str:
    from .. import store

    return store.digest(title, at)
