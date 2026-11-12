"""The results-of-the-day list: one JSONL file per local day.

`<data root>/day/YYYY-MM-DD.jsonl`, one row per thing that happened:
{id, at, kind, title, source, origin}. Appends are idempotent by id, so a
watcher that is killed mid-cycle and resumed never writes a row twice. A row
seen again with a newer kind or a corrected title replaces its stored copy
(issue #73), so a fixed item never stays stale for the rest of the day.
Business data: titles print only where the CLI allows (see cli.py).
"""

from __future__ import annotations

from datetime import date, datetime
from pathlib import Path
from typing import Any

from .. import paths, store


def today() -> date:
    from ..calendar import now

    return now().date()


def path_for(day: date) -> Path:
    return paths.sub("day", f"{day.isoformat()}.jsonl")


def load(day: date | None = None) -> list[dict[str, Any]]:
    return store.read_jsonl(path_for(day or today()))


VERSION_FIELDS = ("kind", "title", "draft", "state", "merged_at", "feed_source")  # content, lifecycle and explicit provenance change a version; `at` alone does not


def append(rows: list[dict[str, Any]], day: date | None = None) -> int:
    """Add rows whose id is new for that day, and replace stored rows whose kind or title changed.
    Returns how many rows changed (added plus replaced), so the watcher rebuilds the log after a fix too.
    One write per batch, so a crash loses at most this batch."""
    added, replaced = store.upsert_jsonl(path_for(day or today()), rows, VERSION_FIELDS)
    return added + replaced


def evidence_rows(items: list[dict[str, Any]], day: date) -> list[dict[str, Any]]:
    """Convert already eligible evidence for its local day, without collecting sources."""
    from ..calendar import local_tz

    tz, rows = local_tz(), []
    for item in items:
        try:
            when = datetime.fromisoformat(str(item.get("at", "")).replace("Z", "+00:00"))
        except ValueError:
            continue
        if when.tzinfo is None:
            when = when.replace(tzinfo=tz)
        if when.astimezone(tz).date() == day:
            rows.append(
                {
                    "id": item["id"],
                    "at": item["at"],
                    "kind": item.get("kind", "other"),
                    "title": item.get("title", ""),
                    "source": "evidence",
                    "origin": item.get("origin", "user"),
                    **{key: item[key] for key in ("draft", "state", "merged_at", "feed_source") if key in item},
                }
            )
    return rows


def summary(day: date | None = None) -> dict[str, int]:
    out: dict[str, int] = {}
    for r in load(day):
        out[r.get("kind", "other")] = out.get(r.get("kind", "other"), 0) + 1
    return out
