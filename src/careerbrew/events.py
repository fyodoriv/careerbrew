"""Content-free JSONL events (see docs/observability.md).

String fields pass only when their key is allow-listed and the value is a
short identifier. Free text can never reach the log, so the log cannot leak
job titles, company names, or work content. `CAREERBREW_EVENTS=off` turns
logging off (tests do this unless they check the log).
"""

from __future__ import annotations

import json
import os
import re
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from . import paths

ALLOWED_STRING_KEYS = {"event", "story", "command", "status", "route", "action", "source_kind", "tier", "error", "saves", "step", "step_kind", "from_version", "error_class", "via", "signin", "source", "recheck"}
_ID = re.compile(r"^[a-z0-9_.:-]{1,48}$")


def log_path() -> Path:
    return paths.sub("logs", "events.jsonl")


def clean(fields: dict[str, Any]) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for key, value in ((k, v.lower() if k == "error" and isinstance(v, str) else v) for k, v in fields.items()):  # an error is a lowercase class
        if isinstance(value, bool) or isinstance(value, int | float) or isinstance(value, str) and key in ALLOWED_STRING_KEYS and _ID.match(value):
            out[key] = value
        # Anything else (free text, lists, dicts) is dropped on purpose.
    return out


def utc_now() -> datetime:
    """The wall clock in UTC: an event's `ts` records when the code really ran (telemetry compares stamps with it)."""
    return datetime.now(UTC)


def stamp() -> str:
    return utc_now().strftime("%Y-%m-%dT%H:%M:%SZ")


def emit(event: str, **fields: Any) -> None:
    if os.environ.get("CAREERBREW_EVENTS", "on") == "off":
        return
    write_row(record(event, **fields))


def record(event: str, **fields: Any) -> dict[str, Any]:
    """rust-05a: the row `emit` writes: the stamp, the version, then only the clean fields."""
    return {"ts": stamp(), "v": _version(), **clean({"event": event, **fields})}


def write_row(row: dict[str, Any]) -> None:
    """rust-05a: append one row to the event log as one sorted JSON line (the IO half of `emit`)."""
    with log_path().open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(row, sort_keys=True) + "\n")


def _version() -> str:
    """The version that wrote the line: a line sent the next day, after an upgrade, keeps its own version."""
    from . import __version__

    return __version__
