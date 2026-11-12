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

ALLOWED_STRING_KEYS = {"event", "story", "command", "status", "route", "action", "source_kind", "tier", "error", "saves", "step"}
_ID = re.compile(r"^[a-z0-9_.:-]{1,48}$")


def log_path() -> Path:
    return paths.sub("logs", "events.jsonl")


def clean(fields: dict[str, Any]) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for key, value in fields.items():
        if isinstance(value, bool) or isinstance(value, int | float) or isinstance(value, str) and key in ALLOWED_STRING_KEYS and _ID.match(value):
            out[key] = value
        # Anything else (free text, lists, dicts) is dropped on purpose.
    return out


def emit(event: str, **fields: Any) -> None:
    if os.environ.get("CAREERBREW_EVENTS", "on") == "off":
        return
    record = {"ts": datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ"), **clean({"event": event, **fields})}
    with log_path().open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(record, sort_keys=True) + "\n")
