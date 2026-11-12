"""Content-free host goal/cron warning (#339).

The caller owns live hook state. CAREERBREW_GOAL_ACTIVE carries only that
boolean for this invocation. Its absence means unknown; no host files or
transcripts are scanned. Loop state remains in the existing encrypted store.
"""

from __future__ import annotations

from typing import Any

from . import watch


def check(env: dict[str, str]) -> list[dict[str, Any]]:
    active = env.get("CAREERBREW_GOAL_ACTIVE", "").lower()
    known = active in ("0", "1", "true", "false")
    goal = active in ("1", "true")
    cron = sum(1 for row in watch._load()["loops"] if row.get("kind") == "cron" and not row.get("stopped"))
    state = "active" if goal else "inactive" if known else "unknown"
    return [
        {
            "check": "goal and cron",
            "ok": not cron or (known and not goal),
            "value": f"goal hook {state}; {cron} active session cron(s)",
            "safe": "session cron jobs fire only while idle: run careerbrew day loop catch-up at each turn start; the host supplies CAREERBREW_GOAL_ACTIVE=1 only while its goal hook is active",
        }
    ]
