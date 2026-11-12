"""Read-only acquisition health, separate from stable definitions and proof age.

Direct receipts have data-root lifecycle in work/feed/direct-cycle.json. The
live cycle lock owns completion; GETs never write or renew these receipts.
"""

from __future__ import annotations

import hashlib
import json
from datetime import UTC, datetime, timedelta
from typing import Any

from .. import calendar, config, paths, store
from . import live

DIRECT_COMMANDS = ("careerbrew work feed transcript <session.jsonl> --since 30m", "careerbrew work feed cycle")


def declared() -> tuple[list[dict[str, Any]], list[str], str]:
    sources = live.sources()
    tools = config.get("feed_tool_sources")
    required = sorted({item.split("=", 1)[1] for item in tools} | {s["name"] for s in sources if s["required"]})
    binding = {"tools": tools, "sources": sources, "required": required, "max_age_minutes": config.get("live_source_max_age_minutes")}
    identity = hashlib.sha256(json.dumps(binding, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    return sources, required, identity


def _time(value: Any) -> datetime | None:
    try:
        at = datetime.fromisoformat(value)
        return at.astimezone(UTC) if at.tzinfo else None
    except (ValueError, TypeError, OverflowError):
        return None


def _age(value: Any, at: datetime, highwater: Any = None) -> float | None:
    stamp, previous = _time(value), _time(highwater)
    if stamp is None or stamp > at or (highwater is not None and (previous is None or previous > at)):
        return None
    return (at - stamp).total_seconds()


def complete(required: list[str], batch: dict[str, Any]) -> None:
    """Finalize stored direct coverage while the caller holds the live cycle lock."""
    from ..day import results
    from . import feed

    root, at = paths.data_root(), calendar.now_utc()
    _sources, declared_required, identity = declared()
    path = root / "work/feed/direct-cycle.json"
    old = store.read_json(path, {}) or {}
    received = batch.get("acquired") or {}
    binding_matches = batch.get("identity") == identity
    if not binding_matches:
        received = {}
    stored = set(batch.get("stored") or [])
    missing = sorted(set(required) - stored)
    healthy = bool(declared_required) and set(declared_required).issubset(stored) and not missing and not batch.get("errors") and not set(batch.get("failed_sources", [])).intersection(required)
    clocks = [_time(received.get(name)) for name in declared_required]
    st = live._state()
    valid_clock = all(t is not None and t <= at for t in clocks) and (not old or _age(old.get("checked_at"), at) is not None) and (not st.get("checked_at") or _age(st["checked_at"], at) is not None)
    status = "unknown" if declared_required and not binding_matches else "failed" if not healthy else "unknown" if not valid_clock else "ok"
    previous = _time(old.get("checked_at"))
    checked_at = max(at, previous) if previous else at
    record = {
        "cycles": int(old.get("cycles", 0)) + 1,
        "identity": identity,
        "required": declared_required,
        "stored": sorted(stored),
        "status": status,
        "checked_at": checked_at.isoformat(),
        "acquired": {name: received.get(name) for name in declared_required},
        "errors": bool(batch.get("errors")),
        "failed_sources": batch.get("failed_sources", []),
    }
    store.write_json(path, record)
    # Intake remains useful even when another required source is missing. Only
    # eligible evidence reaches results; completing a batch never calls a collector.
    by_id = {row["id"]: row for row in feed.load()}
    completed_ids = set(batch.get("ids") or [])
    eligible = [{**item, "feed_source": by_id[item["id"]]["source"]} for item in feed.evidence_items() if item["id"] in completed_ids]
    days = {stamp.astimezone(calendar.local_tz()).date() for item in eligible if (stamp := _time(item.get("at"))) is not None}
    for day in days:
        results.append(results.evidence_rows(eligible, day), day)
    live._standup(st, [row for row in by_id.values() if row["id"] in completed_ids])
    fresh = live._state()
    st.update({key: fresh[key] for key in ("running", "started_at", "stopped_at") if key in fresh})
    live._save_state(st)


def snapshot() -> dict[str, Any]:
    try:
        return _snapshot()
    except live.LiveError:
        state = store.read_json(paths.data_root() / "work/live/state.json", {}) or {}
        return {
            "version": 1,
            "as_of": calendar.now_utc().isoformat(),
            "status": "error",
            "reason": "invalid-config",
            "running": bool(state.get("running")),
            "mode": "none",
            "required": 0,
            "healthy": 0,
            "sources": [],
            "next_expiry": None,
            "commands": ["careerbrew work live start --sources <file.yaml>"],
            "row_review": {"status": "unknown", "checked_at": None, "age_seconds": None, "max_age_minutes": 60},
        }


def _snapshot() -> dict[str, Any]:
    """Existing encrypted config/state only; no pull, rebuild, clock write or network."""
    root, at = paths.data_root(), calendar.now_utc()
    sources, required, identity = declared()
    state = store.read_json(root / "work/live/state.json", {}) or {}
    direct = store.read_json(root / "work/feed/direct-cycle.json", {}) or {}
    last = (store.read_jsonl(root / "work/live/cycles.jsonl") or [{}])[-1]
    rows, expiries = [], []
    direct_bound = bool(required) and direct.get("identity") == identity and direct.get("required") == required
    direct_rows = []
    by_name = {s["name"]: s for s in sources}
    for name in required:
        limit = by_name.get(name, {}).get("max_age_minutes", config.get("live_source_max_age_minutes"))
        stamp = (direct.get("acquired") or {}).get(name)
        age = _age(stamp, at, direct.get("checked_at")) if direct_bound else None
        health = "unknown" if age is None else "healthy" if age <= limit * 60 else "stale"
        if direct_bound and direct.get("status") != "ok":
            if direct.get("status") == "unknown":
                health = "unknown"
            elif name not in direct.get("stored", []) or name in direct.get("failed_sources", []) or direct.get("errors"):
                health = "error"
        acquisition, checked = _time(stamp), _time(direct.get("checked_at"))
        direct_rows.append(
            {
                "name": name,
                "required": True,
                "health": health,
                "acquired_at": acquisition.isoformat() if direct_bound and acquisition else None,
                "checked_at": checked.isoformat() if direct_bound and checked else None,
                "age_seconds": age,
                "max_age_minutes": limit,
            }
        )
    adapter_rows, assessed_failure = [], False
    for source in sources:
        receipt = (state.get("validated") or {}).get(source["name"], {})
        bound = receipt.get("identity") == live._identity(source)
        age = _age(receipt.get("acquired_at"), at, state.get("checked_at")) if bound else None
        limit = source.get("max_age_minutes", config.get("live_source_max_age_minutes"))
        prior = (last.get("sources") or {}).get(source["name"], {})
        health = "unknown" if age is None else "healthy" if age <= limit * 60 else "stale"
        if prior.get("source_identity") == live._identity(source):
            assessed_failure |= source["name"] in required and prior.get("health") != "healthy"
            if prior.get("health") in {"error", "missing"}:
                health = prior["health"]
        acquisition, checked = _time(receipt.get("acquired_at")), _time(state.get("checked_at"))
        adapter_rows.append(
            {
                "name": source["name"],
                "required": source["name"] in required,
                "health": health,
                "acquired_at": acquisition.isoformat() if bound and acquisition else None,
                "checked_at": checked.isoformat() if checked else None,
                "age_seconds": age,
                "max_age_minutes": limit,
            }
        )
    present = {r["name"] for r in adapter_rows}
    adapter_rows.extend(
        {"name": name, "required": True, "health": "unknown", "acquired_at": None, "checked_at": None, "age_seconds": None, "max_age_minutes": config.get("live_source_max_age_minutes")} for name in required if name not in present
    )
    running = bool(state.get("running"))
    adapter_at, direct_at = _time(last.get("at")), _time(direct.get("checked_at"))
    ordering_unknown = bool(last) and (adapter_at is None or adapter_at > at or _age(state.get("checked_at"), at) is None)
    if running and ordering_unknown:
        for row in adapter_rows:
            if row["health"] not in {"error", "missing"}:
                row["health"], row["age_seconds"] = "unknown", None
    adapter_complete = bool(required) and all(r["health"] == "healthy" for r in adapter_rows if r["required"])
    adapter_wins = running and direct_bound and (ordering_unknown or (adapter_at is not None and direct_at is not None and ((assessed_failure and adapter_at >= direct_at) or (adapter_complete and adapter_at > direct_at))))
    use_direct = direct_bound and not adapter_wins
    direct_ok = use_direct and direct.get("status") == "ok" and all(r["health"] == "healthy" for r in direct_rows)
    rows = [*direct_rows, *(r for r in adapter_rows if not r["required"])] if use_direct else adapter_rows
    status, reason, commands = "healthy", "current", []
    if not sources and not required:
        status, reason, commands = "missing", "no-sources", ["careerbrew work live start --sources <file.yaml>"]
    elif use_direct and not direct_ok:
        status = "error" if direct.get("status") == "failed" else "stale" if any(r["health"] == "stale" for r in rows) else "unknown"
        reason, commands = "direct-gap", list(DIRECT_COMMANDS)
    elif not sources and required and not direct_ok:
        status, reason, commands = "unknown", "direct-gap", list(DIRECT_COMMANDS)
    elif not direct_ok and not running:
        status, reason, commands = "stopped", "adapters-stopped", ["careerbrew work live start"]
    elif not direct_ok and any(r["required"] and r["health"] != "healthy" for r in rows):
        failures = {r["health"] for r in rows if r["required"]}
        status = next((h for h in ("error", "missing", "stale", "unknown") if h in failures), "unknown")
        reason, commands = "required-gap", ["careerbrew work live cycle"]
    elif any(not r["required"] and r["health"] != "healthy" for r in rows):
        reason, commands = "optional-gap", ["careerbrew work live cycle"]
    for row in rows:
        if row["health"] == "healthy" and (stamp := _time(row["acquired_at"])) is not None:
            expiries.append(stamp + timedelta(minutes=row["max_age_minutes"], seconds=1))
    saved = store.read_json(root / "work/standup.json", {}) or {}
    saved_at = _time(saved.get("saved_at"))
    age = _age(saved.get("saved_at"), at)
    review = {"status": "unknown" if age is None else "current" if age <= 3600 else "due", "checked_at": saved_at.isoformat() if saved_at is not None and age is not None else None, "age_seconds": age, "max_age_minutes": 60}
    if review["status"] == "current" and saved_at is not None:
        expiries.append(saved_at + timedelta(minutes=60, seconds=1))
    return {
        "version": 1,
        "as_of": at.isoformat(),
        "status": status,
        "reason": reason,
        "running": running,
        "mode": "direct" if use_direct else "adapter" if sources else "none",
        "required": len(required),
        "healthy": sum(r["required"] and r["health"] == "healthy" for r in rows),
        "sources": rows,
        "next_expiry": min(expiries).isoformat() if expiries else None,
        "commands": commands,
        "row_review": review,
    }


def result_health(row: dict[str, Any], health: dict[str, Any]) -> dict[str, Any]:
    """Match explicit canonical provenance; legacy ids and event dates prove no acquisition."""
    return next((source for source in health["sources"] if source["name"] == row.get("feed_source")), {"health": "unknown", "age_seconds": None, "acquired_at": None, "checked_at": None})
