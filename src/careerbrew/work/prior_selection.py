"""Classify supplied session snapshots without inferring completion (#432)."""

import re
from collections import Counter
from datetime import date, datetime, timedelta
from typing import Any

_ID = re.compile(r"[0-9a-f]{32}\Z", re.ASCII)
_DAY = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}\Z", re.ASCII)
_LIMIT = 10_000


def _id(value: Any) -> bool:
    return type(value) is str and _ID.fullmatch(value) is not None


def _day(value: Any) -> date | None:
    if type(value) is not str or _DAY.fullmatch(value) is None:
        return None
    try:
        parsed = date.fromisoformat(value)
    except ValueError:
        return None
    return parsed if parsed.isoformat() == value else None


def select(today: date, rows: list, projects: list) -> list[dict[str, Any]]:
    """Return invocation-local classifications; never interpret a status as done."""
    if type(today) is not date or isinstance(today, datetime) or today == date.min or type(rows) is not list or type(projects) is not list:
        raise ValueError("invalid selection input")
    if len(rows) > _LIMIT or len(projects) > _LIMIT or any(not _id(value) for value in projects):
        raise ValueError("invalid selection input")
    yesterday = today - timedelta(days=1)
    project_counts = Counter(projects)
    record_counts = Counter(row.get("record_id") for row in rows if isinstance(row, dict) and _id(row.get("record_id")))
    out = []
    for index, row in enumerate(rows):
        outcome, reason = "attention", "invalid_row"
        if isinstance(row, dict):
            day = _day(row.get("day"))
            record = row.get("record_id")
            project = row.get("project_incarnation")
            if day is None:
                reason = "invalid_day"
            elif not _id(record):
                reason = "invalid_record_id"
            elif record_counts[record] != 1:
                reason = "duplicate_record_id"
            elif not _id(project):
                reason = "invalid_project_id"
            elif project_counts[project] != 1:
                reason = "unknown_project_id"
            elif day != yesterday:
                outcome, reason = "out_of_day", "outside_day"
            else:
                outcome, reason = "candidate", "prior_day"
        out.append({"index": index, "outcome": outcome, "reason": reason})
    return out
