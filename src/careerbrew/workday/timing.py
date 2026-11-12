"""Per-step timing for the work day (issue #263): each row's end and duration, and a history across days.

- A row's `started` (written when the step starts, #259) and `ended` (written when its status changes
  to done, blocked, or failed) give `seconds`, the step's duration. A new start clears both.
- The history file, `<data root>/day/work-history.json` (encrypted at rest through `store`), keeps one
  entry per step per day: `{seconds, status}`, the day's final status. It holds step names, durations,
  and status words only, never row text. It keeps the last `KEEP_DAYS` days.
- A finished step is `slow` when it took more than `work_slow_percent` percent of its `median`: the
  median of its done runs on up to `WINDOW` earlier days, once there are `MIN_SAMPLES` of them. A failed
  run's time is not the step's normal time, so it never counts in the median.
- A step that went from blocked to done keeps its first `started`: its time counts the wait on the owner,
  because the row's own time (`started` to `ended`) is what the page shows.
- `summary` answers "how did the work day go this week?" and "which step fails most?" for `careerbrew day history`.

Lifecycle: nothing is kept in process memory; each call reads the history file again, and each write
reads, changes, and writes it under its lock (`.work-history.json.lock`). A file it cannot read (corrupt,
or encrypted with another key) is moved aside to `work-history.json.unreadable-<UTC stamp>`, never overwritten, and never over an earlier copy.
"""

from __future__ import annotations

import fcntl
import statistics
from datetime import date, datetime, timedelta
from pathlib import Path
from typing import Any

from .. import paths, store, vault

ENDS = ("done", "blocked", "failed")
KEEP_DAYS = 120
WINDOW = 10
MIN_SAMPLES = 3


def history_path() -> Path:
    return paths.sub("day", "work-history.json")


def _read(strict: bool) -> Any:
    try:
        return store.read_json(history_path(), {}) or {}
    except (vault.VaultError, ValueError):
        if strict:
            raise
        return {}


def history(strict: bool = False) -> dict[str, Any]:
    """The history, with every day a mapping; a file nobody can read or decode (corrupt, or a wrong key) reads as
    empty, so the work day never breaks on it. `strict` raises instead, for the writer."""
    data = _read(strict)
    days = data.get("days") if isinstance(data, dict) else None
    return {"days": {d: v for d, v in days.items() if isinstance(v, dict)} if isinstance(days, dict) else {}}


def _seconds(started: str, ended: str) -> int | None:
    try:
        return max(0, int((datetime.fromisoformat(ended) - datetime.fromisoformat(started)).total_seconds()))
    except (TypeError, ValueError):
        return None  # no start (the step never ran) or a stamp nobody can read: no duration


def close(row: dict[str, Any], prev: str, stamp: str) -> None:
    """Stamp a row's end when its status changes: a new start clears the end, an end status sets it and the duration."""
    if row["status"] == prev:
        return
    if row["status"] == "running":
        row.update(ended="", seconds=None)
    elif row["status"] in ENDS:
        row.update(ended=stamp, seconds=_seconds(row.get("started", ""), stamp))


def _aside(path: Path) -> Path:
    """Move an unreadable history to `work-history.json.unreadable-<UTC stamp>[-n]`: a second one never overwrites the first."""
    from ..calendar import now_utc

    base = path.with_name(f"{path.name}.unreadable-{now_utc().strftime('%Y%m%dT%H%M%S')}")
    dest, n = base, 1
    while dest.exists():
        n += 1
        dest = base.with_name(f"{base.name}-{n}")
    path.replace(dest)
    return dest


def record(day: date, row: dict[str, Any]) -> None:
    """Keep the day's final status and duration for this step; a row that has not ended changes nothing."""
    if row.get("status") not in ENDS:
        return
    path = history_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.with_name(f".{path.name}.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        try:
            days = history(strict=True)["days"]
        except (vault.VaultError, ValueError):  # never overwrite what we cannot read (a wrong key would lose every day)
            _aside(path)
            days = {}
        days.setdefault(day.isoformat(), {})[row["step"]] = {"seconds": row.get("seconds"), "status": row["status"]}
        keep = sorted(days)[-KEEP_DAYS:]
        store.write_json_if_changed(path, {"days": {d: days[d] for d in keep}})  # a summary-only update changes nothing here


def median(step: str, before: date, days: dict[str, Any] | None = None) -> int | None:
    """The median duration of the step's done runs on up to `WINDOW` days before `before`, or None under `MIN_SAMPLES`."""
    have: dict[str, Any] = history()["days"] if days is None else days
    got = [e["seconds"] for d in sorted(have) if d < before.isoformat() and isinstance(e := have[d].get(step), dict) and e.get("status") == "done" and isinstance(e.get("seconds"), int)]
    got = got[-WINDOW:]
    return int(statistics.median(got)) if len(got) >= MIN_SAMPLES else None


def flags(rows: list[dict[str, Any]], day: date) -> dict[str, dict[str, Any]]:
    """{step: {median, slow}} for the day's rows: slow when a finished step took more than the setting's percent of its median."""
    from .. import config

    percent = int(config.get("work_slow_percent"))
    days = history()["days"]
    out: dict[str, dict[str, Any]] = {}
    for r in rows:
        mid = median(r["step"], day, days)
        took = r.get("seconds")
        out[r["step"]] = {"median": mid, "slow": bool(mid is not None and isinstance(took, int) and r.get("status") in ENDS and took * 100 > mid * percent)}
    return out


def shown(seconds: int | None) -> str:
    """A duration in words: `45 s`, `18 min`, `1 h 5 min`; empty when there is none."""
    if seconds is None:
        return ""
    if seconds < 60:
        return f"{seconds} s"
    minutes = round(seconds / 60)
    return f"{minutes} min" if minutes < 60 else f"{minutes // 60} h {minutes % 60} min"


def order(step: dict[str, Any]) -> tuple[int, str]:
    """The sort key of `summary` steps: most failed first, then by name."""
    return (-step["failed"], step["step"])


def summary(today: date, n_days: int = 7) -> dict[str, Any]:
    """The last `n_days` days up to `today`: per step its runs, done, blocked, failed, median, and total time; per day its totals.

    Steps come most failed first, then by name; days newest first. Step names only, never row text."""
    since = (today - timedelta(days=n_days - 1)).isoformat()
    days = {d: v for d, v in history()["days"].items() if since <= d <= today.isoformat() and isinstance(v, dict)}
    per: dict[str, dict[str, Any]] = {}
    out_days = []
    for d in sorted(days, reverse=True):
        entries = {s: e for s, e in days[d].items() if isinstance(e, dict)}
        took = [e["seconds"] for e in entries.values() if isinstance(e.get("seconds"), int)]
        out_days.append({"day": d, "steps": len(entries), **{s: sum(1 for e in entries.values() if e.get("status") == s) for s in ENDS}, "seconds": sum(took)})
        for step, e in entries.items():
            p = per.setdefault(step, {"step": step, "runs": 0, **dict.fromkeys(ENDS, 0), "seconds": 0, "_times": []})
            p["runs"] += 1
            if e.get("status") in ENDS:
                p[e["status"]] += 1
            if isinstance(e.get("seconds"), int):
                p["seconds"] += e["seconds"]
                if e.get("status") == "done":
                    p["_times"].append(e["seconds"])
    steps_ = []
    for p in per.values():
        times = p.pop("_times")
        steps_.append({**p, "median": int(statistics.median(times)) if times else None})
    steps_.sort(key=order)
    return {"since": since, "until": today.isoformat(), "days": out_days, "steps": steps_}


def labels(declared: list[dict[str, Any]], names: list[str]) -> dict[str, str]:
    """{step: what an agent may see}: careerbrew's and the `work_steps` setting's steps by name (`careerbrew config`
    already shows the setting), a plugin's step as `plugin:<name> step <n>`, since its name may name the company.
    A step no longer declared is `other step <n>`: nothing says where it came from."""
    out: dict[str, str] = {}
    seen: dict[str, int] = {}
    for s in declared:
        if not s["from"].startswith("plugin:"):
            out[s["step"]] = s["step"]
        else:
            seen[s["from"]] = seen.get(s["from"], 0) + 1
            out[s["step"]] = f"{s['from']} step {seen[s['from']]}"
    n = 0
    for name in names:
        if name not in out:
            n += 1
            out[name] = f"other step {n}"
    return out
