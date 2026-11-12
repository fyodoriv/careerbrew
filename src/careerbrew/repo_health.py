"""Public repo health: an allowlisted, content-free export of how the careerbrew repo itself is doing.

`careerbrew report public` builds it from repo facts only: the gate's `check.step` events in
`.metrics/events.jsonl` (step name, seconds, ok), the last e2e run in `.metrics/e2e-results.json`
(counted, never named), and the newest repo checkup's summary (status, counts, and blocker ids).
It never reads `private/` data beyond that summary, and never a page, a log, a label, or a title.

`validate()` is the allowlist: the exact keys below, finite numbers, and strings that match a step
name, a day, a time, or a status. Anything else fails the export before it is printed or written,
and so does any of your sensitive terms. This is the only careerbrew data an agent may publish as a
shared page (the repo health Dashboard: `/careerbrew-checkup`, `checkup_artifact_url`).
"""

from __future__ import annotations

import json
import math
import os
import re
from collections import defaultdict
from datetime import UTC, date, datetime, timedelta
from pathlib import Path
from typing import Any

from . import __version__, calendar, events, paths, privacy

SCHEMA = 1
DAY_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
TIME_RE = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:[+-]\d{2}:\d{2}|Z)?$")
STATUSES = {"ok", "blocked", "none"}
KEYS = {"schema", "version", "generated_at", "window_days", "summary", "steps", "daily"}
SUMMARY_KEYS = {"gate_runs", "failed_steps", "step_runs", "failure_rate", "gate_seconds_avg", "e2e_passed", "e2e_total", "checkup_status", "checkup_at", "checkup_blockers", "checkup_next", "open_prs"}
STEP_KEYS = {"step", "runs", "failed", "failure_rate", "seconds_avg"}
DAY_KEYS = {"day", "gate_runs", "step_runs", "failed_steps"}


class PublicError(RuntimeError):
    pass


GATE_STEP = re.compile(r"(?m)(?:^|;\s*then|&&)\s*step ([a-z][a-z0-9-]{0,30}) ")


def _repo() -> Path:
    """The checkout to read: CAREERBREW_REPO_DIR (as `checkup` uses it), else this one."""
    return Path(os.environ.get("CAREERBREW_REPO_DIR") or paths.REPO_ROOT)


def gate_steps() -> set[str]:
    """The step names `scripts/check.sh` defines: the only step names the export may carry."""
    try:
        return set(GATE_STEP.findall((_repo() / "scripts" / "check.sh").read_text(encoding="utf-8")))
    except OSError:
        return set()


def _metrics(name: str) -> Path:
    return _repo() / ".metrics" / name


def _day(ts: object) -> str:
    """A stamp's day in the owner's time zone (as the weekly metrics count it), or ""."""
    try:
        dt = datetime.fromisoformat(str(ts).replace("Z", "+00:00"))
    except ValueError:
        return ""
    return (dt.astimezone(calendar.local_tz()) if dt.tzinfo else dt).date().isoformat()


def _rate(failed: int, runs: int) -> float:
    return round(failed / runs, 4) if runs else 0.0


def _e2e() -> tuple[int, int]:
    try:
        results = json.loads(_metrics("e2e-results.json").read_text(encoding="utf-8")).get("results") or {}
    except (OSError, ValueError, AttributeError):
        return 0, 0
    return sum(1 for v in results.values() if v == "ok"), len(results)


def _checkup() -> dict[str, Any]:
    from . import report

    s = report.latest("repo-checkup") or {}
    pr = re.compile(r"pr-\d+")
    at = str(s.get("at", ""))
    raw = s.get("next")
    nxt: list[Any] = raw if isinstance(raw, list) else []
    return {
        "checkup_status": s.get("status") if s.get("status") in STATUSES else "none",
        "checkup_at": at if _is_time(at) else "",
        "checkup_blockers": len(s["blockers"]) if isinstance(s.get("blockers"), list) else 0,
        "checkup_next": len(nxt),
        "open_prs": sum(1 for x in nxt if isinstance(x, dict) and pr.fullmatch(str(x.get("id", "")))),
    }


def _events() -> list[dict[str, Any]]:
    """The gate's event rows; a torn line (the gate appends while this reads) is skipped, never a crash."""
    try:
        text = _metrics("events.jsonl").read_text(encoding="utf-8")
    except OSError:
        return []
    rows = []
    for line in text.splitlines():
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if isinstance(row, dict):
            rows.append(row)
    return rows


def collect(days: int = 28) -> dict[str, Any]:
    """The export for the last `days` days, ending now. A step `scripts/check.sh` does not define is dropped, never renamed."""
    if not 1 <= days <= 366:
        raise PublicError("--days must be 1 to 366")
    now = calendar.now()
    start = (now.date() - timedelta(days=days - 1)).isoformat()
    per_step: dict[str, list[float]] = defaultdict(list)
    fails: dict[str, int] = defaultdict(int)
    daily: dict[str, dict[str, int]] = {}
    known = gate_steps()
    for e in _events():
        name, secs, day = e.get("name"), e.get("seconds"), _day(e.get("ts"))
        # a cached step logs 0 s and ok without running: counting it would shrink every time and failure rate
        if e.get("event") != "check.step" or e.get("cached") or not isinstance(name, str) or name not in known or day < start:
            continue
        if isinstance(secs, bool) or not isinstance(secs, int | float) or not math.isfinite(secs):
            continue
        per_step[name].append(float(secs))
        d = daily.setdefault(day, {"day": day, "gate_runs": 0, "step_runs": 0, "failed_steps": 0})  # type: ignore[dict-item]
        d["step_runs"] += 1
        d["gate_runs"] += name == "docs"  # every gate run has exactly one docs step
        if e.get("ok") is False:
            fails[name] += 1
            d["failed_steps"] += 1
    steps = [{"step": n, "runs": len(v), "failed": fails[n], "failure_rate": _rate(fails[n], len(v)), "seconds_avg": round(sum(v) / len(v), 1)} for n, v in sorted(per_step.items(), key=lambda kv: (-_rate(fails[kv[0]], len(kv[1])), kv[0]))]
    gate_runs = len(per_step.get("docs", []))
    step_runs = sum(len(v) for v in per_step.values())
    failed = sum(fails.values())
    e2e_ok, e2e_total = _e2e()
    summary = {
        "gate_runs": gate_runs,
        "step_runs": step_runs,
        "failed_steps": failed,
        "failure_rate": _rate(failed, step_runs),
        "gate_seconds_avg": round(sum(sum(v) for v in per_step.values()) / gate_runs, 1) if gate_runs else 0.0,
        "e2e_passed": e2e_ok,
        "e2e_total": e2e_total,
        **_checkup(),
    }
    payload = {
        "schema": SCHEMA,
        "version": __version__,
        "generated_at": now.astimezone(UTC).isoformat(timespec="seconds"),
        "window_days": days,
        "summary": summary,
        "steps": steps,
        "daily": [daily[k] for k in sorted(daily)],
    }
    validate(payload)
    events.emit("report.public", status="ok", steps=len(steps), days=len(payload["daily"]))
    return payload


def _is_time(v: object) -> bool:
    if not isinstance(v, str) or not TIME_RE.fullmatch(v):
        return False
    try:
        datetime.fromisoformat(v.replace("Z", "+00:00"))
    except ValueError:
        return False
    return True


def _is_day(v: object) -> bool:
    if not isinstance(v, str) or not DAY_RE.fullmatch(v):
        return False
    try:
        date.fromisoformat(v)
    except ValueError:
        return False
    return True


def _count(v: object) -> bool:
    return isinstance(v, int) and not isinstance(v, bool) and v >= 0


def _number(v: object) -> bool:
    return isinstance(v, int | float) and not isinstance(v, bool) and math.isfinite(v) and v >= 0


# The allowlist: one check per field. A value passes only its own field's check, never another field's.
FIELDS: dict[str, Any] = {
    "schema": lambda v: v == SCHEMA,
    "version": lambda v: v == __version__,
    "generated_at": _is_time,
    "window_days": lambda v: _count(v) and 1 <= v <= 366,
    "status": lambda v: v in STATUSES,
    "time_or_empty": lambda v: v == "" or _is_time(v),
    "day": _is_day,
    "count": _count,
    "number": _number,
}
SUMMARY_FIELDS = {**dict.fromkeys(SUMMARY_KEYS, "count"), "failure_rate": "number", "gate_seconds_avg": "number", "checkup_status": "status", "checkup_at": "time_or_empty"}
STEP_FIELDS = {"step": "step", "runs": "count", "failed": "count", "failure_rate": "number", "seconds_avg": "number"}
DAY_FIELDS = {"day": "day", "gate_runs": "count", "step_runs": "count", "failed_steps": "count"}


def _check(where: str, kind: str, v: object, steps: set[str]) -> None:
    ok = (isinstance(v, str) and v in steps) if kind == "step" else FIELDS[kind](v)
    if not ok:
        raise PublicError(f"{where} is not an allowed {kind.replace('_', ' ')}; only numbers and allowlisted words may be published")


def _check_rows(name: str, rows: object, fields: dict[str, str], steps: set[str]) -> None:
    if not isinstance(rows, list):
        raise PublicError(f"{name} must be a list of rows")
    for n, row in enumerate(rows):
        if not isinstance(row, dict) or set(row) != set(fields):
            raise PublicError(f"{name}[{n}] keys must be exactly {sorted(fields)}")
        for k, v in row.items():
            _check(f"{name}[{n}].{k}", fields[k], v, steps)


def validate(payload: dict[str, Any]) -> None:
    """The allowlist. Raises PublicError on any key, type, or value its field does not allow, or on a sensitive term."""
    if not isinstance(payload, dict) or set(payload) != KEYS:
        raise PublicError(f"export keys must be exactly {sorted(KEYS)}")
    steps = gate_steps()
    for k in ("schema", "version", "generated_at", "window_days"):
        _check(k, k, payload[k], steps)
    if not isinstance(payload["summary"], dict) or set(payload["summary"]) != SUMMARY_KEYS:
        raise PublicError(f"summary keys must be exactly {sorted(SUMMARY_KEYS)}")
    for k, v in payload["summary"].items():
        _check(f"summary.{k}", SUMMARY_FIELDS[k], v, steps)
    _check_rows("steps", payload["steps"], STEP_FIELDS, steps)
    _check_rows("daily", payload["daily"], DAY_FIELDS, steps)
    if privacy.find_leaks(json.dumps(payload)):
        raise PublicError("the export holds one of your sensitive terms; nothing was written")


def write_rows(payload: dict[str, Any], out: Path) -> list[Path]:
    """The three row files the repo health Dashboard reads: summary.json (one row), steps.json, daily.json."""
    validate(payload)
    target = out.expanduser().resolve()
    for private in (paths.data_root().resolve(), (paths.REPO_ROOT / "private").resolve(), (_repo() / "private").resolve()):
        if target.is_relative_to(private):
            raise PublicError(f"--out must be outside {private}: these files are meant to be published")
    try:
        target.mkdir(parents=True, exist_ok=True)
    except OSError as exc:
        raise PublicError(f"--out {out} is not a folder careerbrew can write ({type(exc).__name__})") from exc
    out = target
    files = {"summary": [payload["summary"] | {"generated_at": payload["generated_at"], "window_days": payload["window_days"]}], "steps": payload["steps"], "daily": payload["daily"]}
    written = []
    for name, rows in files.items():
        p = out / f"{name}.json"
        try:
            p.unlink(missing_ok=True)  # a link left in a reused folder is replaced, never followed into another file
            p.write_text(json.dumps({"rows": rows}, indent=1, sort_keys=True) + "\n", encoding="utf-8")
        except OSError as exc:
            raise PublicError(f"could not write {p.name} in --out ({type(exc).__name__})") from exc
        written.append(p)
    return written
