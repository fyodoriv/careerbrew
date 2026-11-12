"""Owner-set goals: long-term goals and a queue of upcoming goals; the nearest one drives the schedule and the teacher.

Only the owner sets goals. careerbrew never writes this file on its own: the web app
(which only the owner's browser can reach, with the web token) and the CLI on an
interactive terminal can change it; agents may suggest, never set (see the `teacher` skill).

Upcoming goals form a queue the owner edits freely (add, change, remove, reorder by date).
The current one is the nearest dated goal still ahead (else the first undated one); when
its date passes, the next takes over. The current goal decides the focus (which exercise patterns, card topics, and lessons
come first) and a daily study budget. Changing it re-plans today at once, so the day
plan and the teacher queue follow the new goal in the same response.
"""

from __future__ import annotations

import re
from datetime import date
from pathlib import Path
from typing import Any

from . import calendar, events, paths, store

KINDS = ("interview", "learn", "job-search", "promotion", "custom")
JOB_HUNT_KINDS = ("interview", "job-search")  # job-hunt data: never saved on a corporate machine
DEFAULT_MINUTES = 90
CODING_STEPS = {"technical-phone-screen", "coding-onsite", "online-assessment", "pair-programming", "take-home"}


class GoalError(ValueError):
    pass


def _path() -> Path:
    return paths.sub("goals.yaml")


def _current(queue: list[dict[str, Any]], today: date | None = None) -> dict[str, Any] | None:
    """The goal that drives the schedule: the nearest dated goal still ahead, else the first undated one."""
    t = (today or calendar.now().date()).isoformat()
    ahead = sorted((g for g in queue if g.get("date") and g["date"] >= t), key=lambda g: g["date"])
    return ahead[0] if ahead else next((g for g in queue if not g.get("date")), None)


def load() -> dict[str, Any]:
    data = store.read_yaml(_path(), {}) or {}
    queue = data.get("queue")
    if queue is None:  # files written before the queue existed hold one "upcoming" goal
        queue = [data["upcoming"]] if data.get("upcoming") else []
    return {"long_term": data.get("long_term", []), "queue": queue, "upcoming": _current(queue), "history": data.get("history", [])}


def job_hunt_goals(data: Any) -> int:
    """How many saved goals (upcoming or past) are job-hunt data. A file that is not a mapping counts as one: refuse, do not guess."""
    if data in (None, {}):
        return 0
    if not isinstance(data, dict):
        return 1
    rows = [*(data.get("queue") or []), *(data.get("history") or []), *([data["upcoming"]] if data.get("upcoming") else [])]
    return sum(1 for g in rows if not isinstance(g, dict) or g.get("kind") in JOB_HUNT_KINDS)


def _save(data: dict[str, Any]) -> None:
    store.write_yaml(_path(), {"long_term": data["long_term"], "queue": data["queue"], "history": data["history"][-20:]})


def _slug(text: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", text.casefold()).strip("-")[:40] or "goal"


def _date(value: Any, field: str) -> str | None:
    if value in (None, ""):
        return None
    try:
        return date.fromisoformat(str(value)).isoformat()
    except ValueError as e:
        raise GoalError(f"{field} must be a date like 2026-11-05") from e


def set_long_term(items: list[dict[str, Any]]) -> dict[str, Any]:
    clean = []
    for it in items:
        text = " ".join(str(it.get("text", "")).split())
        if not text:
            continue
        clean.append({"id": it.get("id") or _slug(text), "text": text[:300], "by": _date(it.get("by"), "by")})
    data = load()
    data["long_term"] = clean
    _save(data)
    events.emit("goals.update", status="long_term", items=len(clean))
    return data


def _clean(goal: dict[str, Any]) -> dict[str, Any]:
    kind = str(goal.get("kind") or "custom")
    if kind not in KINDS:
        raise GoalError(f"kind must be one of {', '.join(KINDS)}")
    title = " ".join(str(goal.get("title", "")).split())
    if not title:
        raise GoalError("give the goal a title")
    minutes = goal.get("minutes_per_day", DEFAULT_MINUTES)
    try:
        minutes = int(minutes)
    except (TypeError, ValueError) as e:
        raise GoalError("minutes_per_day must be a whole number") from e
    if not 10 <= minutes <= 600:
        raise GoalError("minutes_per_day must be between 10 and 600")
    return {
        "id": str(goal.get("id") or "") or _slug(title),
        "kind": kind,
        "title": title[:200],
        "date": _date(goal.get("date"), "date"),
        "company": (str(goal.get("company") or "").strip() or None),
        "topics": [str(t).casefold().strip() for t in goal.get("topics") or [] if str(t).strip()],
        "minutes_per_day": minutes,
        "set_at": goal.get("set_at") or calendar.now_utc().isoformat(timespec="seconds"),
    }


def set_queue(queue: list[dict[str, Any]]) -> dict[str, Any]:
    """Replace every upcoming goal at once (add, edit, remove, reorder) and re-plan."""
    clean = [_clean(g) for g in queue]
    ids = [g["id"] for g in clean]
    if len(ids) != len(set(ids)):
        raise GoalError("two upcoming goals have the same title; make them different")
    from . import machine

    if machine.role() == "corporate" and any(g["kind"] in JOB_HUNT_KINDS for g in clean):
        raise GoalError("interview and job-search goals are personal-only and this machine's role is corporate. Set them on your personal machine (job-hunt data never goes on employer hardware).")
    data = load()
    before = data["upcoming"]
    data["queue"] = clean
    after = _current(clean)
    if before and (not after or after["id"] != before["id"]):
        data["history"] = [*data["history"], {**before, "replaced_at": calendar.now_utc().isoformat(timespec="seconds")}]
    _save(data)
    events.emit("goals.update", status="upcoming", items=len(clean))
    return {**load(), "plan": replan()}


def set_upcoming(goal: dict[str, Any] | None) -> dict[str, Any]:
    """Set the goal that drives the schedule now: replace the current one (None removes it) and re-plan."""
    data = load()
    current = data["upcoming"]
    queue = [g for g in data["queue"] if not current or g["id"] != current["id"]]
    if goal is not None:
        queue.insert(0, {**goal, "id": None})
    return set_queue(queue)


def remove(goal_id: str) -> dict[str, Any]:
    return set_queue([g for g in load()["queue"] if g["id"] != goal_id])


def replan() -> dict[str, Any]:
    """Rebuild today's plan so it follows the current goal (study blocks first, then work)."""
    from .day import planner

    return planner.build(persist=True)


def days_left(goal: dict[str, Any] | None = None, today: date | None = None) -> int | None:
    goal = goal if goal is not None else load()["upcoming"]
    if not goal or not goal.get("date"):
        return None
    return max(0, (date.fromisoformat(goal["date"]) - (today or calendar.now().date())).days)


def focus(goal: dict[str, Any] | None = None) -> dict[str, Any]:
    """What the goal means for study: exercise patterns, card topics, and the activities in order."""
    from .tutor import exercises

    goal = goal if goal is not None else load()["upcoming"]
    all_patterns = list(exercises.PATTERNS)
    if not goal:
        return {"patterns": all_patterns, "card_topics": [], "activities": ["exercise", "cards", "lesson"], "why": "the default order (no upcoming goal yet)"}
    topics = goal.get("topics") or []
    if goal["kind"] == "interview":
        from . import interview

        steps = [s["id"] for s in interview.loop_for(goal.get("company") or "")["loop"]] if goal.get("company") else []
        activities = []
        if CODING_STEPS & set(steps) or not steps:
            activities.append("exercise")
        if "system-design" in steps:
            activities.append("system-design")
        if {"behavioral", "hiring-manager", "bar-raiser"} & set(steps) or not steps:
            activities.append("behavioural")
        activities += ["cards", "mistakes"]
        card_topics = ["system design"] * ("system-design" in steps) + ["behavioural", "leadership"]
        return {"patterns": [p for p in all_patterns if p in topics] or all_patterns, "card_topics": card_topics + topics, "activities": activities, "loop": steps, "why": f"{goal.get('company') or 'the'} interview loop"}
    if goal["kind"] == "learn":
        pats = [p for p in all_patterns if p in topics]
        return {"patterns": pats or all_patterns, "card_topics": [t for t in topics if t not in pats], "activities": (["exercise"] if pats else []) + ["lesson", "cards", "mistakes"], "why": "your learning topics"}
    if goal["kind"] == "job-search":
        return {"patterns": all_patterns, "card_topics": topics, "activities": ["applications", "exercise", "cards"], "why": "the jobs you chose"}
    if goal["kind"] == "promotion":
        return {"patterns": [], "card_topics": topics, "activities": ["promotion", "cards"], "why": "your promotion case"}
    return {"patterns": all_patterns, "card_topics": topics, "activities": ["exercise", "cards", "lesson"], "why": "your goal"}


def study_blocks(today: date | None = None) -> list[dict[str, Any]]:
    """Today's study blocks for the upcoming goal, sized to its daily budget and the days left."""
    from . import machine
    from .tutor import exercises

    goal = _current(load()["queue"], today)
    if not goal or (machine.role() == "corporate" and goal["kind"] in ("interview", "job-search")):
        return []  # job-hunt study never lands on a corporate machine
    f = focus(goal)
    minutes = int(goal.get("minutes_per_day") or DEFAULT_MINUTES)
    left = days_left(goal, today)
    if left == 0:
        return [{"id": "goal:review", "title": f"Goal day: {goal['title']}. Light review only, then rest.", "minutes": min(minutes, 30), "goal": True, "reason": "the goal is today"}]
    blocks: list[dict[str, Any]] = []
    acts = f["activities"]
    share = {a: 1 for a in acts}
    if "exercise" in share:
        share["exercise"] = 2
    total = sum(share.values())
    for a in acts:
        m = max(10, round(minutes * share[a] / total / 5) * 5)
        if a == "exercise":
            todo = [e for e in exercises.load() if e["pattern"] in f["patterns"] and not exercises.status().get(e["id"], {}).get("solved")]
            per_day = max(1, -(-len(todo) // left)) if left else 2
            title = f"Coding: {min(per_day, max(1, m // 20))} exercise(s) in {todo[0]['pattern']}" if todo else "Coding: review solved exercises"
            blocks.append({"id": "goal:exercise", "title": title, "minutes": m, "goal": True, "reason": f.get("why", "")})
        elif a == "cards":
            blocks.append({"id": "goal:cards", "title": "Flashcards: due reviews, then new cards" + (f" ({', '.join(f['card_topics'][:2])})" if f["card_topics"] else ""), "minutes": m, "goal": True, "reason": f.get("why", "")})
        elif a == "mistakes":
            blocks.append({"id": "goal:mistakes", "title": "Mistake review: your prevention cards", "minutes": max(10, m // 2), "goal": True, "reason": "so mistakes do not repeat"})
        else:
            label = {
                "system-design": "System design: one practice design",
                "behavioural": "Behavioural: one STAR story out loud",
                "lesson": "Lesson: learn and confirm one topic",
                "applications": "Applications: prepare one chosen job",
                "promotion": "Promotion: close one gap",
            }.get(a, a)
            blocks.append({"id": f"goal:{a}", "title": label, "minutes": m, "goal": True, "reason": f.get("why", "")})
    return blocks
