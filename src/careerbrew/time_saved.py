"""Time saved: the manual minutes an action replaces, minus the time careerbrew took.

A run is one CLI run or one Copy, Send, or Accept in the web app. Each CLI run of an
action that replaces manual work logs `cli.run` with `saves` (the id below) and `ms`
(wall time of the run: a lower bound of your time, so time saved is an upper bound).
The web app logs `web.timing` when a result opens and when you press Copy, Send, or
Accept (PM update, standup, inbox), with `seconds`: your own time from the open to that
press, while the tab was visible. A web run counts that time; each run says which time
it used (`time`: `web`, `cli`, or `web+cli` for a week with both). The baseline is the
manual time per action: one you measured by hand (`careerbrew time baseline <action>
<minutes>`, kept in settings), else the estimate below, always labeled `estimate`.
Nothing here reads text: only the event log's action ids, steps, seconds, and timestamps.
"""

from __future__ import annotations

import json
from collections import defaultdict
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any

from . import calendar, config, events, paths, store

ESTIMATE_SOURCE = "careerbrew default, no published source"

# action id -> (CLI commands that do it, estimated manual minutes, what the manual work is)
ACTIONS: dict[str, tuple[tuple[str, ...], int, str]] = {
    "pm_update": (("day.send",), 15, "write and post the PM update by hand"),
    "standup": (("work.standup",), 10, "gather what you did since the last standup"),
    "day_log": (("day.log",), 10, "write the day's done list and tomorrow's plan"),
    "day_plan": (("day.plan",), 10, "plan the day from calendar and tasks"),
    "evidence": (("work.collect",), 20, "collect the week's work evidence from git, GitHub, and tickets"),
    "promo_doc": (("work.promo",), 120, "draft a promotion doc against the ladder"),
    "job_scan": (("jobs.scan",), 30, "check job boards for new postings that fit"),
    "inbox": (("day.accept",), 3, "read one suggestion and apply it by hand (plan the task, track the ticket)"),
}
COMMAND_ACTION = {cmd: action for action, (cmds, _m, _w) in ACTIONS.items() for cmd in cmds}


# The web app's timed results (`POST /api/timing`): the action, the step, and the seconds from the open.
WEB_ACTIONS = ("pm_update", "standup", "inbox")
WEB_STEPS = ("open", "copy", "send", "accept")
DONE_STEPS = ("copy", "send", "accept")
MAX_WEB_SECONDS = 86400


def record_web(saves: Any, step: Any, seconds: Any) -> None:
    """Log one content-free `web.timing` event; anything else is refused with ValueError."""
    if saves not in WEB_ACTIONS or step not in WEB_STEPS:
        raise ValueError(f"send {{saves: {'|'.join(WEB_ACTIONS)}, step: {'|'.join(WEB_STEPS)}, seconds}}")
    if isinstance(seconds, bool) or not isinstance(seconds, int | float) or not 0 <= seconds <= MAX_WEB_SECONDS:  # NaN fails both comparisons
        raise ValueError(f"seconds must be a number from 0 to {MAX_WEB_SECONDS}")
    events.emit("web.timing", saves=saves, step=step, seconds=round(float(seconds), 1))


def setting_key(action: str) -> str:
    return f"time_baseline_{action}"


def baseline(action: str) -> tuple[float, str]:
    """Manual minutes for one run of the action, and the kind: `measured` or `estimate, <source>`."""
    measured = config.get(setting_key(action))
    if measured:
        return float(measured), "measured"
    return float(ACTIONS[action][1]), f"estimate, {ESTIMATE_SOURCE}"


def set_baseline(action: str, minutes: str) -> Any:
    if action not in ACTIONS:
        raise ValueError(f"unknown action {action}; known: {', '.join(ACTIONS)}")
    return config.set_value(setting_key(action), minutes)


def _runs(log: Path) -> list[dict[str, Any]]:
    """The timed rows: `cli.run` (ok, with `saves`) and `web.timing`, each with `minutes` and `via` (cli or web)."""
    if not log.is_file():
        return []
    rows = []
    for line in log.read_text(encoding="utf-8").splitlines():
        if '"cli.run"' not in line and '"web.timing"' not in line:  # the log holds every API read too: skip those unparsed
            continue
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if row.get("event") == "cli.run" and row.get("status") == "ok" and row.get("saves") in ACTIONS and isinstance(row.get("ms"), int | float):
            rows.append({**row, "via": "cli", "minutes": row["ms"] / 60000})
        elif row.get("event") == "web.timing" and row.get("saves") in WEB_ACTIONS and row.get("step") in WEB_STEPS and isinstance(row.get("seconds"), int | float):
            rows.append({**row, "via": "web", "minutes": row["seconds"] / 60})
    return rows


def _local(ts: str) -> datetime:
    return datetime.strptime(ts, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=calendar.now_utc().tzinfo).astimezone(calendar.local_tz())


def _week_of(day: datetime) -> str:
    year, week, _ = day.isocalendar()
    return f"{year}-W{week:02d}"


def _week(ts: str) -> str:
    return _week_of(_local(ts))


def compute(log: Path | None = None) -> dict[str, Any]:
    """Minutes saved per action per local ISO week, with counts. Numbers and ids only."""
    runs = _runs(log or events.log_path())
    weeks: dict[str, dict[str, dict[str, Any]]] = defaultdict(lambda: defaultdict(lambda: {"runs": 0, "runs_web": 0, "runs_cli": 0, "opened": 0, "minutes_saved": 0.0, "minutes_spent": 0.0}))
    starts: dict[str, str] = {}
    for row in runs:
        local = _local(row["ts"])
        week = _week_of(local)
        starts.setdefault(week, (local - timedelta(days=local.weekday())).strftime("%Y-%m-%d"))
        cell = weeks[week][row["saves"]]
        if row["via"] == "web" and row["step"] not in DONE_STEPS:
            cell["opened"] += 1  # an open is not a run: the run is the Copy, Send, or Accept after it
            continue
        minutes, _kind = baseline(row["saves"])
        cell["runs"] += 1
        cell[f"runs_{row['via']}"] += 1
        cell["minutes_spent"] += row["minutes"]
        cell["minutes_saved"] += minutes - row["minutes"]
    history = []
    for week in sorted(weeks):
        actions = {a: {**{k: round(v, 1) if isinstance(v, float) else v for k, v in c.items()}, "time": _time(c)} for a, c in sorted(weeks[week].items()) if c["runs"] or c["opened"]}
        history.append({"week": week, "start": starts[week], "runs": sum(c["runs"] for c in actions.values()), "minutes_saved": round(sum(c["minutes_saved"] for c in actions.values()), 1), "actions": actions})
    baselines = {a: {"minutes": baseline(a)[0], "kind": baseline(a)[1]} for a in ACTIONS}
    return {
        "computed": calendar.now().strftime("%Y-%m-%d"),
        "this_week": _week_of(calendar.now()),
        "measured_baselines": sum(b["kind"] == "measured" for b in baselines.values()),
        "baselines": baselines,
        "weeks": history,
        "minutes_saved_total": round(sum(w["minutes_saved"] for w in history), 1),
        "runs_total": sum(w["runs"] for w in history),
    }


def _time(cell: dict[str, Any]) -> str:
    """Which time a cell's runs used: `web` (your time up to copy, send, or accept), `cli` (run time), or both."""
    return "+".join(v for v in ("web", "cli") if cell[f"runs_{v}"]) or "none"


def out_path() -> Path:
    return paths.sub("out", "time-saved.json")


def write(data: dict[str, Any]) -> Path:
    path = out_path()
    store.write_json_if_changed(path, data)
    return path
