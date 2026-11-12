"""Pivot when the world changes: promotion ladders, plugins, company loops, source pages.

`check()` hashes what careerbrew's advice depends on and compares it with the
last snapshot (`<data root>/changes/snapshot.json`):
- ladders (base, user, plugins)  → re-assess the promotion case and re-plan today;
- plugin trees                   → same, plus the merged policy may have changed;
- company knowledge files        → rebuild the job strategy; prep for that company is marked stale;
- fetched source pages (`fetch=True`, pipeline companies' sources only)
                                 → same as a company change, and the source is due a re-check.
Each change becomes a pivot event in `<data root>/changes/events.jsonl` with
the action taken, and a short notification (ids only, no content).
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

from . import calendar, events, paths, plugins, store


def _h(obj: Any) -> str:
    return hashlib.sha256(json.dumps(obj, sort_keys=True, default=str).encode()).hexdigest()[:16]


def current(fetch: bool = False) -> dict[str, dict[str, str]]:
    from . import interview, research
    from .work import promo

    snap: dict[str, dict[str, str]] = {"ladders": {}, "plugins": {}, "companies": {}, "pages": {}}
    for lid, lad in promo.ladders().items():
        snap["ladders"][lid] = _h({k: v for k, v in lad.items() if k != "origin"})
    for p in plugins.installed():
        snap["plugins"][p["name"]] = plugins.tree_hash(Path(p["root"]))[:16]
    for cid, c in interview.companies().items():
        snap["companies"][cid] = _h({k: c.get(k) for k in ("loop", "remote_policy", "ai_policy", "company_tips")})
    if fetch:
        plan = store.read_json(paths.sub("jobs", "strategy.json"), {}) or {}
        pipeline = {r["company"].casefold() for rows in (plan.get("tiers") or {}).values() for r in rows}
        srcs = interview.sources()
        for cid, c in interview.companies().items():
            if c.get("name", "").casefold() not in pipeline and cid not in pipeline:
                continue
            for sid in c.get("sources", []):
                url = (srcs.get(sid) or {}).get("url")
                if url:
                    try:
                        snap["pages"][sid] = research.page(url)["sha256"][:16]
                    except (OSError, ValueError, RuntimeError):
                        continue
    return snap


def snapshot_path() -> Path:
    return paths.sub("changes", "snapshot.json")


def check(fetch: bool = False, act: bool = True) -> list[dict[str, Any]]:
    now = current(fetch)
    prev = store.read_json(snapshot_path(), None)
    store.write_json_if_changed(snapshot_path(), now)  # the watcher checks hourly: the same snapshot is no change (US-64)
    if prev is None:
        return []  # first run: baseline only
    changes = []
    for kind, items in now.items():
        old = prev.get(kind, {})
        for key, val in items.items():
            if key in old and old[key] != val:
                changes.append({"kind": kind[:-1], "id": key, "change": "changed"})
            elif key not in old and kind != "pages":
                changes.append({"kind": kind[:-1], "id": key, "change": "added"})
        for key in old:
            if key not in items:
                changes.append({"kind": kind[:-1], "id": key, "change": "removed"})
    if act and changes:
        for c in changes:
            c["action"] = pivot(c)
        store.append_jsonl(paths.sub("changes", "events.jsonl"), [{**c, "at": calendar.now_utc().isoformat(timespec="seconds")} for c in changes])
        events.emit("changes.pivot", items=len(changes))
    return changes


def companies_for_source(source_id: str) -> list[str]:
    from . import interview

    out = []
    for cid, c in interview.companies().items():
        ids = set(c.get("sources", []))
        for key in ("ai_policy", "remote_policy", "local_presence"):
            ids |= set((c.get(key) or {}).get("sources", []))
        for s in [*c.get("loop", []), *c.get("company_tips", [])]:
            ids |= set(s.get("sources", []))
        if source_id in ids:
            out.append(c.get("name", cid))
    return sorted(out)


def pivot(change: dict[str, Any]) -> str:
    """Re-run whatever depends on the changed thing. Returns a short action note."""
    from .day import planner
    from .jobs import sources, strategy
    from .work import promo

    kind = change["kind"]
    if kind in ("ladder", "plugin"):
        s = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
        try:
            asm = promo.assess(s.get("ladder", "generic-swe"), s.get("target_level", "senior"))
            promo.write_doc(asm)
            gaps = promo.gaps(asm)
            planner.build()
            top = gaps[0]["id"] if gaps else "none"
            return f"promotion case re-assessed; top gap now {top}; today's plan rebuilt"
        except promo.LadderError as exc:
            return f"ladder needs attention: {exc}"
    if kind in ("company", "page"):
        from . import interview, prep

        if sources.load():
            strategy.build()
        if kind == "company":
            c = interview.companies().get(change["id"].casefold())
            names = [c.get("name", change["id"])] if c else []  # a removed company has no prep to re-read
        else:
            names = companies_for_source(change["id"])
        for name in names:
            prep.mark_stale(name, f"{kind} {change['change']}")
        return f"job strategy rebuilt; prep marked stale for {', '.join(names) or 'no company'}; re-check sources"
    return "noted"
