"""Fresh public information: static page fetch with timestamps, and staleness of sources.

- `page(url)`: fetches a public page (no JavaScript), keeps its text, a SHA-256
  of the text, and `fetched_at` in `<data root>/research/pages/`. Pages that
  need JavaScript or a login are for the browser-research skill (the agent's
  browser), which writes findings into knowledge/research/*.yaml.
- `stale(days)`: sources in knowledge/sources.yaml (and plugin companies)
  whose `checked` date is older than `days`, companies in your pipeline first.
"""

from __future__ import annotations

import hashlib
from datetime import date
from pathlib import Path
from typing import Any

from . import calendar, connectors, events, paths, store
from .jobs.sources import strip_html


def _page_path(url: str) -> Path:
    return paths.sub("research", "pages", hashlib.sha256(url.encode()).hexdigest()[:24] + ".json")


def page(url: str) -> dict[str, Any]:
    if not url.startswith("https://"):
        raise ValueError("research page needs an https URL")
    raw = connectors.http_get(url).decode("utf-8", "replace")
    text = strip_html(raw)[:20000]
    rec = {"url": url, "fetched_at": calendar.now_utc().isoformat(timespec="seconds"), "sha256": hashlib.sha256(text.encode()).hexdigest(), "chars": len(text), "text": text}
    prev = store.read_json(_page_path(url), {}) or {}
    rec["changed"] = bool(prev) and prev.get("sha256") != rec["sha256"]
    store.write_json(_page_path(url), rec)
    events.emit("research.page", status="changed" if rec["changed"] else "ok")
    return rec


def last_page(url: str) -> dict[str, Any] | None:
    return store.read_json(_page_path(url), None)


def stale(days: int = 180, today: date | None = None) -> list[dict[str, Any]]:
    from . import interview

    today = today or calendar.now().date()
    srcs = interview.sources()
    used_by: dict[str, set[str]] = {}
    for cid, c in interview.companies().items():
        ids = set(c.get("sources", [])) | set((c.get("ai_policy") or {}).get("sources", []))
        for s in c.get("loop", []):
            ids |= set(s.get("sources", []))
        for i in ids:
            used_by.setdefault(i, set()).add(c.get("name", cid))
    plan = store.read_json(paths.sub("jobs", "strategy.json"), {}) or {}
    pipeline = {r["company"].casefold() for rows in (plan.get("tiers") or {}).values() for r in rows}
    out = []
    for sid, s in srcs.items():
        checked = str(s.get("checked", "1970-01-01"))
        age = (today - date.fromisoformat(checked)).days
        if age > days:
            comps = sorted(used_by.get(sid, set()))
            out.append({"id": sid, "url": s["url"], "checked": checked, "age_days": age, "companies": comps, "in_pipeline": any(c.casefold() in pipeline for c in comps)})
    return sorted(out, key=lambda r: (not r["in_pipeline"], -r["age_days"]))
