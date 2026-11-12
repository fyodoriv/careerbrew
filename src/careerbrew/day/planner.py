"""Daily plan from facts, and the proactive next-task suggestion.

Facts used:
- calendar events (Outlook / ICS) → busy blocks inside work hours → free blocks;
- open tasks (`work/tasks.yaml`) ranked by priorities.rank (deadline, impact,
  promotion gap, project priority);
- calibration: the median actual/estimate ratio from finished tasks
  (`estimate_ratio` in metrics.jsonl) inflates estimates, per task: its project's own ratio, then its
  source's, then everyone's (issue #185); focus factor 0.8
  leaves room for interruptions;
- carry-over: yesterday's planned tasks that are still open come first.

Placement is first-fit decreasing by score: the best task takes the first free
block it fits in. Sources for each rule: knowledge/strategy.yaml
(planning-fallacy, wsjf-prioritization, time-blocking).
"""

from __future__ import annotations

import math
import statistics
from dataclasses import dataclass
from datetime import UTC, date, datetime, time, timedelta
from typing import Any
from urllib.parse import urlsplit

from .. import calendar, events, goals, paths, store
from ..work import feed, priorities, projects, promo
from . import results

FOCUS_FACTOR = 0.8
MIN_BLOCK_MIN = 25
PREP_WINDOW_MIN = 10


@dataclass
class Block:
    start: datetime
    end: datetime

    @property
    def minutes(self) -> int:
        return int((self.end - self.start).total_seconds() // 60)


def settings() -> dict[str, Any]:
    s = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
    return {"work_start": s.get("work_start", "09:00"), "work_end": s.get("work_end", "17:30"), **s}


def _at(day: date, hhmm: str) -> datetime:
    h, m = (int(x) for x in hhmm.split(":"))
    return datetime.combine(day, time(h, m), calendar.local_tz())


def free_blocks(day: date, evs: list[calendar.Event], now: datetime | None = None) -> list[Block]:
    s = settings()
    start, end = _at(day, s["work_start"]), _at(day, s["work_end"])
    if now and now > start:
        start = min(max(start, now), end)
    blocks, cur = [], start
    for e in sorted((e for e in evs if e.busy), key=lambda e: e.start):
        if e.end <= cur or e.start >= end:
            continue
        if e.start > cur:
            blocks.append(Block(cur, min(e.start, end)))
        cur = max(cur, e.end)
    if cur < end:
        blocks.append(Block(cur, end))
    return [b for b in blocks if b.minutes >= 1]


CALIBRATE_MIN = 3  # finished tasks of one project (or source) before its own ratio replaces the global one (issue #185)


def _ratio_rows() -> list[dict[str, Any]]:
    rows = [r for r in store.read_jsonl(paths.sub("metrics.jsonl")) if r.get("metric") == "estimate_ratio"]
    return [r for r in rows if isinstance(v := r.get("value"), int | float) and math.isfinite(v) and v > 0]


def _median(values: list[float]) -> float:
    return min(priorities.MAX_RATIO, max(1.0, round(statistics.median(values), 2))) if values else 1.0


def calibration(t: dict[str, Any] | None = None, rows: list[dict[str, Any]] | None = None, of: dict[str, str] | None = None) -> float:
    """The actual/estimate ratio to plan with: for a task, its project's own (`CALIBRATE_MIN` finished tasks), else
    its source's (an agent's tasks may run long, yours short), else everyone's; with no task, everyone's."""
    rows = _ratio_rows() if rows is None else rows
    if t is not None:
        of = projects.task_projects() if of is None else of
        if (proj := of.get(t["id"])) and len(mine := [r["value"] for r in rows if of.get(str(r.get("task"))) == proj]) >= CALIBRATE_MIN:
            return _median(mine)
        if len(same := [r["value"] for r in rows if r.get("source") == str(t.get("source") or "user")]) >= CALIBRATE_MIN:
            return _median(same)
    return _median([r["value"] for r in rows])


def _gap_priorities() -> dict[str, float]:
    s = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
    try:
        asm = promo.assess(s.get("ladder", "generic-swe"), s.get("target_level", "senior"))
    except promo.LadderError:
        return {}
    return {g["id"]: g["priority"] for g in promo.gaps(asm)}


def plan_path(day: date):
    return paths.sub("day", f"plan-{day.isoformat()}.json")


def _carry(day: date) -> set[str]:
    """The tasks the day before planned: those still open come first on `day`."""
    yesterday = store.read_json(plan_path(day - timedelta(days=1)), {}) or {}
    return {s["task"] for s in yesterday.get("schedule", [])}


def _block_kind(g: dict[str, Any]) -> dict[str, Any]:
    """A prep gate block names its project; a study block is a goal's."""
    return {"gate": True, "project": g["project"]} if g.get("gate") else {"goal": True}


def build(day: date | None = None, now: datetime | None = None, persist: bool = True) -> dict[str, Any]:
    day = day or results.today()
    evs, cal_errors = calendar.events_for(day)
    blocks = free_blocks(day, evs, now)
    ratio = calibration()
    cal_rows, of = _ratio_rows(), projects.task_projects()
    gp = _gap_priorities()
    ranked = priorities.rank(gp, today=day)
    carry = _carry(day)
    ranked.sort(key=lambda t: (t["id"] not in carry, -t["score"]))
    remaining = [Block(b.start, b.end) for b in blocks]
    budget = int(sum(b.minutes for b in blocks) * FOCUS_FACTOR)
    schedule, unplaced = [], []
    for g in [*_gate_blocks(day), *goals.study_blocks(day)]:  # an event's open prep gates come first (issue #276), then the owner's upcoming goal
        slot = next((b for b in remaining if b.minutes >= g["minutes"]), None)
        if slot is None or g["minutes"] > budget:
            unplaced.append(g["id"])
            continue
        budget -= g["minutes"]
        s, e = slot.start, slot.start + timedelta(minutes=g["minutes"])
        schedule.append({"task": g["id"], "title": g["title"], "start": s.isoformat(timespec="minutes"), "end": e.isoformat(timespec="minutes"), "carry_over": False, "reason": g["reason"], **_block_kind(g)})
        slot.start = e + timedelta(minutes=5)
        remaining = [b for b in remaining if b.minutes >= MIN_BLOCK_MIN]
    for t in ranked:
        need = int(priorities.estimate(t) * 60 * calibration(t, cal_rows, of))
        slot = next((b for b in remaining if b.minutes >= need), None)
        if slot is None or need > budget:
            unplaced.append(t["id"])
            continue
        budget -= need
        s, e = slot.start, slot.start + timedelta(minutes=need)
        schedule.append({"task": t["id"], "title": t["title"], "start": s.isoformat(timespec="minutes"), "end": e.isoformat(timespec="minutes"), "carry_over": t["id"] in carry, "reason": _reason(t, gp)})
        slot.start = e + timedelta(minutes=5)
        remaining = [b for b in remaining if b.minutes >= MIN_BLOCK_MIN]
    meetings = [{"start": e.start.isoformat(timespec="minutes"), "end": e.end.isoformat(timespec="minutes"), "title": e.title} for e in evs if e.busy]
    free_min = sum(b.minutes for b in blocks)
    plan = {
        "day": day.isoformat(),
        "meetings": meetings,
        "free_minutes": free_min,
        "focus_minutes": int(free_min * FOCUS_FACTOR),
        "calibration": ratio,
        "schedule": schedule,
        "unplaced": unplaced,
        "calendar_errors": cal_errors,
        "calendar_connected": bool(calendar.configured()),
    }
    if persist:
        store.write_json_if_changed(plan_path(day), plan)  # a read must not look like new data (issue #117)
    events.emit("day.plan", status="ok", tasks=len(schedule), meetings=len(meetings))
    return plan


def is_workday(day: date) -> bool:
    s = settings()
    days = s.get("work_days", [0, 1, 2, 3, 4])  # Monday=0
    return day.weekday() in days


def _gate_blocks(day: date) -> list[dict[str, Any]]:
    """Prep gates for `day`: events up to the next workday, so a Monday event's gates show on Friday (issue #276)."""
    nxt = next((day + timedelta(days=n) for n in range(1, 8) if is_workday(day + timedelta(days=n))), day + timedelta(days=1))
    return projects.gate_blocks(day, nxt)


def plan_week(start: date | None = None, days: int = 7, now: datetime | None = None) -> list[dict[str, Any]]:
    """Plan ahead: each open task is placed once, on the earliest workday with room, in rank order.

    Days are planned in memory only; nothing is written. Weekends (or any day not in
    settings.work_days) get meetings but no tasks. The first day puts the tasks the day before
    planned first, as `build` does. With `now`, the day of `now` has only the free time left after
    it: so the first day is `build(start, now)`, a task today's plan holds is not planned again on
    a later day, and what no longer fits today moves on to the next workday.

    The last day lists what was left: `unplaced` (the days had no room) and `too_big` (more minutes
    than the focus time of any whole day in the window, `day_focus_minutes`: it fits only when split).
    """
    start = start or results.today()
    cal_rows, of = _ratio_rows(), projects.task_projects()
    gp = _gap_priorities()
    remaining = priorities.rank(gp, today=start)
    carry = _carry(start)
    out = []
    connected = bool(calendar.configured())
    day_focus = 0  # the most focus minutes one whole workday in the window has, in one free block
    for i in range(days):
        d = start + timedelta(days=i)
        evs, _ = calendar.events_for(d)
        meetings = [{"start": e.start.isoformat(timespec="minutes"), "end": e.end.isoformat(timespec="minutes"), "title": e.title} for e in evs if e.busy]
        workday = is_workday(d)
        if workday:
            whole = free_blocks(d, evs)
            day_focus = max(day_focus, min(int(sum(b.minutes for b in whole) * FOCUS_FACTOR), max((b.minutes for b in whole), default=0)))
        blocks = free_blocks(d, evs, now if now and now.date() == d else None) if workday else []
        free_min = sum(b.minutes for b in blocks)
        budget = int(free_min * FOCUS_FACTOR)
        schedule, left = [], []
        for g in [*_gate_blocks(d), *goals.study_blocks(d)] if workday else []:
            slot = next((b for b in blocks if b.minutes >= g["minutes"]), None)
            if slot is None or g["minutes"] > budget:
                continue
            budget -= g["minutes"]
            s, e = slot.start, slot.start + timedelta(minutes=g["minutes"])
            schedule.append({"task": g["id"], "title": g["title"], "start": s.isoformat(timespec="minutes"), "end": e.isoformat(timespec="minutes"), "carry_over": False, "reason": g["reason"], **_block_kind(g)})
            slot.start = e + timedelta(minutes=5)
            blocks = [b for b in blocks if b.minutes >= MIN_BLOCK_MIN]
        first = i == 0 and bool(carry)
        for t in sorted(remaining, key=lambda t: t["id"] not in carry) if first else remaining:  # stable: rank order otherwise
            need = int(priorities.estimate(t) * 60 * calibration(t, cal_rows, of))
            slot = next((b for b in blocks if b.minutes >= need), None)
            if slot is None or need > budget:
                left.append(t)
                continue
            budget -= need
            s, e = slot.start, slot.start + timedelta(minutes=need)
            schedule.append({"task": t["id"], "title": t["title"], "start": s.isoformat(timespec="minutes"), "end": e.isoformat(timespec="minutes"), "carry_over": first and t["id"] in carry, "reason": _reason(t, gp)})
            slot.start = e + timedelta(minutes=5)
            blocks = [b for b in blocks if b.minutes >= MIN_BLOCK_MIN]
        waiting = {t["id"] for t in left}
        remaining = [t for t in remaining if t["id"] in waiting]  # back in rank order after the carry-over day
        out.append(
            {
                "day": d.isoformat(),
                "weekday": d.strftime("%a"),
                "workday": workday,
                "meetings": meetings,
                "schedule": schedule,
                "free_minutes": free_min,
                "unplaced": [],
                "calendar_connected": connected,
            }
        )
    if out:
        need = {t["id"]: int(priorities.estimate(t) * 60 * calibration(t, cal_rows, of)) for t in remaining}
        big = {i for i, n in need.items() if day_focus and n > day_focus}  # with no workday in the window, nothing is "too big"
        out[-1]["unplaced"] = [t["id"] for t in remaining if t["id"] not in big]
        out[-1]["too_big"] = [{"task": t["id"], "minutes": need[t["id"]]} for t in remaining if t["id"] in big]
        out[-1]["day_focus_minutes"] = day_focus
    return out


def _reason(t: dict[str, Any], gp: dict[str, float]) -> str:
    bits = []
    if t.get("due"):
        bits.append(f"due {t['due']}")
    if (p := t.get("parts") or {}).get("project"):  # project_priority (issue #68)
        bits.append(f"project {p['project']} (priority {p['project_rank']} of {p['projects']})")
    if t.get("promo_bonus"):
        gaps = [c for c in t.get("competencies", []) if c in gp]
        bits.append("closes promotion gap " + ",".join(gaps))
    bits.append(f"impact {t.get('impact', 3)}")
    return "; ".join(bits)


def log_suggestion(nxt: dict[str, Any]) -> None:
    if nxt.get("kind") == "task":
        store.append_jsonl(paths.sub("day", "suggestions.jsonl"), [{"at": calendar.now().isoformat(timespec="minutes"), "task": nxt["task"]}])


def _feed_next(now: datetime, window: int) -> dict[str, Any] | None:
    """Choose an open ask that fits the remaining work window."""
    if window < 5:
        return None
    cutoff = now.astimezone(UTC)
    best_key = None
    best = None
    for row in feed.open_asks():
        review = row.get("kind") in ("review", "review-request", "review_request")
        minutes = 10 if review else 5
        title, url, at, event_id = (row.get(key) for key in ("title", "url", "at", "id"))
        if minutes > window or not isinstance(title, str) or not title.strip() or not isinstance(event_id, str) or not event_id:
            continue
        if not isinstance(url, str) or "\\" in url or any(char.isspace() or not char.isprintable() for char in url) or not priorities.is_web(url):
            continue
        if not isinstance(at, str):
            continue
        try:
            address = urlsplit(url)
            if address.username is not None or address.password is not None:
                continue
            # Reading the port also rejects malformed authorities.
            _ = address.port
            instant = datetime.fromisoformat(at)
            if instant.utcoffset() is None:
                continue
            instant = instant.astimezone(UTC)
        except (OverflowError, ValueError):
            continue
        key = (instant, event_id)
        if instant > cutoff or (best_key is not None and key >= best_key):
            continue
        best_key = key
        instruction = "Start reviewing this request" if review else "Read and respond to this ask"
        best = {"kind": "feed", "title": title, "text": f"{instruction} ({minutes} min).", "links": [{"label": "Open source", "url": url}]}
    return best


def next_step(now: datetime | None = None) -> dict[str, Any]:
    """What to do right now, and why."""
    now = now or calendar.now()
    day = now.date()
    evs, _ = calendar.events_for(day)
    upcoming = [e for e in evs if e.busy and e.end > now]
    current = next((e for e in upcoming if e.start <= now < e.end), None)
    if current:
        return {"kind": "meeting", "text": "You are in a meeting. Afterwards, log any decision with `careerbrew work note`.", "until": current.end.isoformat(timespec="minutes")}
    nxt = next((e for e in upcoming if e.start > now), None)
    if nxt and (nxt.start - now) <= timedelta(minutes=PREP_WINDOW_MIN):
        return {"kind": "prepare", "text": f"Meeting in {int((nxt.start - now).total_seconds() // 60)} min: skim its agenda and write one question.", "meeting": nxt.title}
    window = int(((nxt.start if nxt else _at(day, settings()["work_end"])) - now).total_seconds() // 60)
    if window <= 0:
        return {"kind": "done", "text": "Work hours are over. Run `careerbrew day results` to review the day."}
    plan = store.read_json(plan_path(day), {}) or build(day, now)
    open_ids = {t["id"] for t in priorities.load() if priorities.is_open(t)}
    cal_rows, of = _ratio_rows(), projects.task_projects()
    ranked = {t["id"]: t for t in priorities.rank(_gap_priorities(), today=day)}
    gates = {b["id"]: b["minutes"] for b in _gate_blocks(day)}  # a prep gate still open (issue #276) comes before any task
    for item in plan.get("schedule", []):
        if item["task"] in gates and gates[item["task"]] <= window:
            return {"kind": "gate", "task": item["task"], "title": item["title"], "text": f"Close this prep gate now ({window} min free).", "reason": item["reason"]}
        t = ranked.get(item["task"])
        if t and t["id"] in open_ids and priorities.estimate(t) * 60 * calibration(t, cal_rows, of) <= window:
            return {"kind": "task", "task": t["id"], "title": t["title"], "text": f"Work on {t['id']} now ({window} min free).", "plain": f"Work on it now ({window} min free).", "reason": item["reason"]}
    small = [t for t in ranked.values() if priorities.estimate(t) * 60 * calibration(t, cal_rows, of) <= window]
    if small:
        t = small[0]
        return {"kind": "task", "task": t["id"], "title": t["title"], "text": f"{window} min free: {t['id']} fits.", "plain": f"{window} min free: this task fits.", "reason": _reason(t, {})}
    done_today = [r for r in results.load(day) if r.get("kind") in ("commit", "pr", "jira", "github")]
    if done_today and not any(r.get("kind") == "note" for r in results.load(day)):
        return {"kind": "log", "text": "You shipped work today with no note yet: record the impact with `careerbrew work note`."}
    # A meeting beyond work end must not extend the feed's selection window.
    feed_window = min(window, int((_at(day, settings()["work_end"]) - now).total_seconds() // 60)) if nxt else window
    if recommendation := _feed_next(now, feed_window):
        return recommendation
    return {"kind": "small", "text": f"{window} min free and nothing fits: review a PR or clear your inbox."}
