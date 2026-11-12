"""Opt-in weekly aggregate metrics, in the shared provider format (agentbrew issue 346).

`~/.local/share/careerbrew/metrics/weekly-<YYYY-Www>.json` = {tool, version, week, machine_id, metrics}.
`metrics` maps a name to a number: no titles, names, paths, or ids. agentbrew's `metrics share`
collects the file and sends it to the sink the user configured; careerbrew itself sends nothing.

Off unless the user opts in (`careerbrew config set metrics_share true`); a plugin policy
`metrics_share: false` forbids it on that machine. `validate()` refuses any value that is not a
number, so a later metric cannot leak text. The watcher writes the file once per ISO week.
Lifecycle: the random machine id lives in `<data root>/metrics-share.json` (encrypted at rest).
"""

from __future__ import annotations

import json
import math
import os
import secrets
import tempfile
from datetime import date, datetime, timedelta
from pathlib import Path
from typing import Any

from . import __version__, calendar, config, events, paths, plugins, store, vault

KEYS = {"tool", "version", "week", "machine_id", "metrics"}


class ShareError(RuntimeError):
    pass


def out_dir() -> Path:
    env = os.environ.get("CAREERBREW_METRICS_DIR")
    return Path(env).expanduser() if env else Path.home() / ".local" / "share" / "careerbrew" / "metrics"


def allowed() -> tuple[bool, str]:
    if any((p["manifest"].get("policy") or {}).get("metrics_share") is False for p in plugins.installed()):
        return False, "a plugin policy forbids sharing metrics on this machine"
    if not config.get("metrics_share"):
        return False, "off (opt in: careerbrew config set metrics_share true)"
    return True, "on"


def week_of(day: date) -> str:
    y, w, _ = day.isocalendar()
    return f"{y}-W{w:02d}"


def _machine_id() -> str:
    p = paths.sub("metrics-share.json")
    data = store.read_json(p, {}) or {}
    if not data.get("machine_id"):
        data["machine_id"] = secrets.token_hex(8)
        store.write_json(p, data)
    return str(data["machine_id"])


def _local_day(ts: str) -> date | None:
    """A stamp's date in the owner's time zone (day plans are named by it), or None."""
    try:
        dt = datetime.fromisoformat(str(ts).replace("Z", "+00:00"))
    except ValueError:
        return None
    return (dt.astimezone(calendar.local_tz()) if dt.tzinfo else dt).date()


def _in_week(ts: str, start: date, end: date) -> bool:
    d = _local_day(ts)
    return d is not None and start <= d < end


def collect(day: date | None = None) -> dict[str, Any]:
    """The payload for the ISO week that contains `day` (default: today)."""
    from .day import watch
    from .work import evidence, priorities

    day = day or calendar.now().date()
    start = day - timedelta(days=day.weekday())
    end = start + timedelta(days=7)
    m: dict[str, float] = {}
    evs = store.read_jsonl(paths.sub("logs", "events.jsonl"))
    m["sessions"] = len({_local_day(e.get("ts", "")) for e in evs if _in_week(e.get("ts", ""), start, end)})
    try:
        from . import review

        rows = [r for r in review.run() if r.get("value") is not None]
        if rows:
            m["keep_share"] = round(sum(r["status"] == "KEEP" for r in rows) / len(rows), 3)
    except Exception as exc:  # noqa: BLE001 (a metric that cannot be measured is left out, never guessed)
        events.emit("metrics.weekly.skip", source_kind="review", error=type(exc).__name__)
    cov = [float(v) for r in store.read_jsonl(paths.sub("metrics.jsonl")) if r.get("metric") == "promo_coverage" and isinstance(v := r.get("value"), int | float)]
    if cov:
        m["promo_coverage"] = cov[-1]
    m["evidence_items"] = sum(1 for e in evidence.load() if _in_week(e.get("at", ""), start, end))
    plans = [p for p in paths.sub("day", ".keep").parent.glob("plan-*.json") if start.isoformat() <= p.stem[5:] < end.isoformat()]
    m["day_plans"] = len(plans)
    planned = {s["task"] for p in plans for s in (store.read_json(p, {}) or {}).get("schedule", []) if not str(s.get("task", "")).startswith(("goal:", "gate:"))}
    if planned:
        done = {t["id"] for t in priorities.load() if t.get("status") == "done"}
        m["planned_done_share"] = round(len(planned & done) / len(planned), 3)
    cpu = [float(x) for x in (watch.read_state().get("cpu_history") or []) if isinstance(x, int | float)]
    if cpu:
        m["watcher_cpu_p95_ms"] = sorted(cpu)[max(0, math.ceil(0.95 * len(cpu)) - 1)]
    steps = [e for e in store.read_jsonl(paths.REPO_ROOT / ".metrics" / "events.jsonl") if e.get("event") == "check.step" and isinstance(e.get("seconds"), int | float) and _in_week(e.get("ts", ""), start, end)]
    runs = sum(1 for e in steps if e.get("name") == "docs")  # every gate run has exactly one docs step
    if runs:
        m["gate_seconds_avg"] = round(sum(float(e["seconds"]) for e in steps) / runs, 1)
    try:
        from . import machine

        if machine.role() == "corporate":
            rows = machine.readiness()
            m["readiness_passed"], m["readiness_total"] = sum(r["ok"] for r in rows), len(rows)
    except Exception as exc:  # noqa: BLE001
        events.emit("metrics.weekly.skip", source_kind="readiness", error=type(exc).__name__)
    payload = {"tool": "careerbrew", "version": __version__, "week": week_of(day), "machine_id": _machine_id(), "metrics": m}
    validate(payload)
    return payload


def validate(payload: dict[str, Any]) -> None:
    if set(payload) != KEYS:
        raise ShareError(f"weekly metrics keys must be exactly {sorted(KEYS)}")
    for name, value in payload["metrics"].items():
        if isinstance(value, bool) or not isinstance(value, int | float) or not math.isfinite(value):
            raise ShareError(f"metric {name} is not a number; only numbers may leave the machine")


def write(day: date | None = None) -> Path:
    ok, why = allowed()
    if not ok:
        raise ShareError(f"metrics sharing is {why}")
    day = day or calendar.now().date()
    d = out_dir()
    path = d / f"weekly-{week_of(day)}.json"
    if vault.sealable(path):
        raise ShareError("metrics output cannot be in a protected data root folder (fix: choose a folder outside the data root)")
    payload = collect(day)
    d.mkdir(parents=True, exist_ok=True)
    parent = d.resolve()
    target = parent / path.name
    if vault.sealable(target):
        raise ShareError("metrics output cannot be in a protected data root folder (fix: choose a folder outside the data root)")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=parent, prefix=".metrics-", suffix=".tmp", delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(json.dumps(payload, indent=1, sort_keys=True) + "\n")
        os.replace(temporary, target)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    events.emit("metrics.weekly", status="ok", items=len(payload["metrics"]))
    return path


def maybe_write() -> Path | None:
    """For the watcher: once per ISO week, only when allowed."""
    ok, _ = allowed()
    if not ok:
        return None
    path = out_dir() / f"weekly-{week_of(calendar.now().date())}.json"
    return None if path.exists() else write()
