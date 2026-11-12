"""The most efficient work action per horizon, each with the numbers behind it.

Horizons: now, today, week, month, quarter, year, company. No new model: each
horizon reuses an existing ranking, so every "why" is checkable.
  now     planner.next_step (meetings, free window, the day plan)
  today   the day plan (tasks ranked by priorities.rank, placed in free blocks)
  week    the week plan, minus what today already holds
  month   ranked tasks due within 31 days, or too big for one day (>= 4h)
  quarter promotion gaps by priority = weight x (1 - coverage) / effort
  year    competencies furthest below the bar for the target level
  company the active ladder's highest-weight competencies (a plugin ladder is
          the company's own rubric)
A horizon with no data returns `empty`: the action that would create it.
Business data: served only by the local web app, never logged.

Issue #142: the Dashboard asks for each block on its own (`Build.block`), so a block shows as soon as
its own inputs are ready. A `Build` computes each shared input (the ranked tasks, the day and week
plans, the ladder assessment) once, on first use, even when several blocks ask for it at once.
"""

from __future__ import annotations

import threading
from collections.abc import Callable
from datetime import date, datetime
from typing import Any

from .. import calendar, paths, store
from ..day import planner
from . import priorities, promo

Item = dict[str, Any]
BIG_TASK_H = 4.0


def _due_text(days: int) -> str:
    if days < 0:
        return f"overdue by {-days} day{'' if days == -1 else 's'}"
    return "due today" if days == 0 else f"due in {days} day{'' if days == 1 else 's'}"


def _task_why(t: dict[str, Any], pos: dict[str, int], total: int) -> str:
    """Quote the numbers rank() used (priorities.rank `parts`), and the task's real rank."""
    p = t["parts"]
    u = f"urgency {p['urgency']:g}"
    if p["due_days"] is not None and p["due_days"] <= 2:
        u = f"urgency {p['urgency'] - 3:g} + 3 because it is {_due_text(p['due_days'])}"
    bonus = f" + promotion bonus {p['bonus']:g} for {', '.join(t.get('competencies', []))}" if p["bonus"] else ""
    if p.get("project_bonus"):
        bonus += f" + project bonus {p['project_bonus']:g} for {p['project']} (priority {p['project_rank']} of {p['projects']})"
    return f"score {t['score']} = (impact {p['impact']:g} + {u}{bonus}) / sqrt({p['size_h']:g}h); rank {pos[t['id']]} of {total} open tasks by value per hour"


def _horizon(hid: str, label: str, items: list[Item], empty: str) -> dict[str, Any]:
    return {"id": hid, "label": label, "items": items, "empty": "" if items else empty}


def _with_projects(h: dict[str, Any], of: dict[str, str], context: dict[str, dict[str, Any]]) -> dict[str, Any]:
    """Name the project each task item serves, with the task's links and own why; the raw task id stays only in `ref`."""
    for it in h["items"]:
        if it.get("ref") in of:
            it["project"] = of[it["ref"]]
        it.update(context.get(it.get("ref") or "", {}))
    return h


def _tomorrow(now: datetime, week: list[dict[str, Any]] | None = None) -> dict[str, Any]:
    """The proposed plan for the next workday: work tasks only, with project and time (issue #64).

    It is that day of the week plan from `now` (`week`, when the caller has it): what today's plan
    still holds is not proposed again, and what no longer fits today moves here (US-75 pictures).
    """
    from datetime import timedelta

    from . import projects

    today = now.date()
    day = today + timedelta(days=1)
    while not planner.is_workday(day):
        day += timedelta(days=1)
    ahead = (day - today).days
    days = week if week is not None and len(week) > ahead else planner.plan_week(today, ahead + 1, now)
    plan = days[ahead]
    of, context = projects.task_projects(), priorities.context()

    def work(d: dict[str, Any]) -> list[dict[str, Any]]:
        return [s for s in d["schedule"] if not str(s["task"]).startswith(("goal:", "gate:"))]

    items = [{"title": s["title"], "project": of.get(s["task"]), "start": s["start"][11:16], "end": s["end"][11:16], "ref": s["task"], **context.get(s["task"], {})} for s in work(plan)]
    empty = "Today's plan holds every open task: add a task with hours (Week view) to plan more." if any(work(d) for d in days[:ahead]) else "Add a task with hours (Week view) and tomorrow gets a proposed plan."
    return {"day": day.isoformat(), "items": items, "empty": "" if items else empty}


def _stale() -> list[dict[str, Any]]:
    from . import projects

    return projects.stale_sources()


HORIZONS = ("now", "today", "week", "month", "quarter", "year", "company")
LADDER_HORIZONS = ("quarter", "year", "company")
# Every Dashboard block the web app asks for on its own (`/api/focus/<block>`), with the clock resolution its
# answer depends on (the server's response cache key). The first four and Tomorrow start from the free time left
# today, so they change by the minute; the rest change by the day. server.FOCUS_BLOCKS keeps the same table.
BLOCKS = {"now": "minute", "today": "minute", "week": "minute", "month": "minute", "quarter": "day", "year": "day", "company": "day", "tomorrow": "minute", "stale": "day"}

_NEXT_WHY = {
    "meeting": "You are in a meeting until it ends; the best use is to capture decisions while fresh.",
    "prepare": "A meeting starts soon; a 5-minute prep beats starting deep work you must drop.",
    "done": "Your work hours are over; reviewing the day keeps tomorrow's plan honest.",
    "log": "Shipped work with no note is evidence your promotion case will miss.",
    "small": "No planned task fits the free window; small tasks use it without breaking focus later.",
    "gate": "An event is near and its prep is not done: an open readiness gate comes before other work.",
}


class Build:
    """One Dashboard build at `now`: each shared input and each horizon is computed once, on first use.

    Lifecycle: a plain object, owned by its caller. `build()` makes one per call; the web server keeps one
    per minute and data version (server-instance scope), so the blocks a page asks for at once share it.
    Thread-safe: two threads that ask for one part at once compute it once (one lock per part).
    """

    def __init__(self, now: datetime) -> None:
        self.now = now
        self.today = now.date()
        self._lock = threading.Lock()
        self._locks: dict[str, threading.Lock] = {}
        self._values: dict[str, Any] = {}

    def _once(self, name: str, make: Callable[[], Any]) -> Any:
        with self._lock:
            lock = self._locks.setdefault(name, threading.Lock())
        with lock:
            if name not in self._values:
                self._values[name] = make()
            return self._values[name]

    # ---------- shared inputs ----------

    def ranked(self) -> list[dict[str, Any]]:
        return self._once("ranked", lambda: priorities.rank(planner._gap_priorities(), today=self.today))

    def _why(self, t: dict[str, Any]) -> str:
        ranked = self.ranked()
        pos = self._once("pos", lambda: {r["id"]: i for i, r in enumerate(ranked, 1)})
        return _task_why(t, pos, len(ranked))

    def week_plan(self) -> list[dict[str, Any]]:
        """The next 7 days planned from `now`: This week and Tomorrow read the same plan."""
        return self._once("week_plan", lambda: planner.plan_week(self.today, 7, self.now))

    def tomorrow(self) -> dict[str, Any]:
        return self._once("tomorrow", lambda: _tomorrow(self.now, self.week_plan()))

    def _by_id(self) -> dict[str, dict[str, Any]]:
        return self._once("by_id", lambda: {t["id"]: t for t in self.ranked()})

    def _projects(self) -> dict[str, str]:
        from . import projects

        return self._once("projects", projects.task_projects)

    def _context(self) -> dict[str, dict[str, Any]]:
        """Each task's links and own why (issue #143), read once per build."""
        return self._once("context", priorities.context)

    def assessment(self) -> dict[str, Any]:
        """The ladder assessment, or `error` when the ladder cannot load."""

        def make() -> dict[str, Any]:
            settings = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
            try:
                asm = promo.assess(settings.get("ladder", "generic-swe"), settings.get("target_level", "senior"), at=self.now)
            except promo.LadderError as e:
                return {"error": f"Fix the ladder first: {e}"}
            lad = promo.ladders()[asm["ladder"]]
            return {"asm": asm, "lad": lad, "comps": {c["id"]: c for c in lad["competencies"]}, "company": str(asm["ladder_origin"]).startswith("plugin:")}

        return self._once("assessment", make)

    # ---------- horizons (each with the task or competency ids it took, so later ones skip them) ----------

    def _now(self) -> dict[str, Any]:
        def make() -> dict[str, Any]:
            nxt = planner.next_step(self.now)
            why = nxt.get("reason") or _NEXT_WHY.get(nxt.get("kind", ""), "")
            if nxt.get("task") in self._by_id():
                why = self._why(self._by_id()[nxt["task"]])
            items = [{"title": nxt.get("plain", nxt["text"]).rstrip(".") + (f": {nxt['title']}" if nxt.get("title") else ""), "why": why or "Next step from your plan and calendar.", "ref": nxt.get("task")}]
            return _with_projects(_horizon("now", "Right now", items, ""), self._projects(), self._context())

        return self._once("now", make)

    def _today(self) -> tuple[dict[str, Any], set[str]]:
        def make() -> tuple[dict[str, Any], set[str]]:
            plan = planner.build(self.today, self.now, persist=False)
            used: set[str] = set()
            items = []
            for s in plan["schedule"][:3]:
                t = self._by_id().get(s["task"])
                used.add(s["task"])
                items.append({"title": f"{s['title']} ({s['start'][11:16]}–{s['end'][11:16]})", "why": self._why(t) if t else s["reason"], "ref": s["task"]})
            return _with_projects(_horizon("today", "Today", items, "Add a task with hours (Week view) and today gets a time-blocked plan."), self._projects(), self._context()), used

        return self._once("today", make)

    def _week(self) -> tuple[dict[str, Any], set[str]]:
        def make() -> tuple[dict[str, Any], set[str]]:
            used = set(self._today()[1])
            items = []
            for d in self.week_plan():
                for s in d["schedule"]:
                    if s["task"] in used or len(items) >= 3:
                        continue
                    used.add(s["task"])
                    t = self._by_id().get(s["task"])
                    why = f"planned for {d['weekday']} {d['day']}, the first workday with room; " + (self._why(t) if t else s["reason"])
                    items.append({"title": s["title"], "why": why, "ref": s["task"]})
            return _with_projects(_horizon("week", "This week", items, "Nothing beyond today fits this week: add tasks with due dates."), self._projects(), self._context()), used

        return self._once("week", make)

    def _month(self) -> dict[str, Any]:
        def make() -> dict[str, Any]:
            used = set(self._week()[1])
            items = []
            for t in self.ranked():
                if t["id"] in used or len(items) >= 3:
                    continue
                due_days = None
                if t.get("due"):
                    try:
                        due_days = (date.fromisoformat(str(t["due"])) - self.today).days
                    except ValueError:
                        pass
                big = priorities.estimate(t) >= BIG_TASK_H
                if (due_days is not None and due_days <= 31) or big:
                    used.add(t["id"])
                    reason = f"{_due_text(due_days)} ({t['due']})" if due_days is not None and due_days <= 31 else f"{priorities.estimate(t):g}h is too big for one day: split it and book focus blocks"
                    items.append({"title": t["title"], "why": f"{reason}; {self._why(t)}", "ref": t["id"]})
            return _with_projects(_horizon("month", "This month", items, "No task is due within 31 days or bigger than a day."), self._projects(), self._context())

        return self._once("month", make)

    def _quarter(self) -> tuple[dict[str, Any], set[str]]:
        def make() -> tuple[dict[str, Any], set[str]]:
            a = self.assessment()
            if "error" in a:
                return _horizon("quarter", "This quarter", [], a["error"]), set()
            asm, comps = a["asm"], a["comps"]
            items = []
            for g in promo.gaps(asm)[:2]:
                effort = comps[g["id"]].get("effort", 2)
                items.append(
                    {
                        "title": f"{g['name']}: {g['how_to_close'] or 'add evidence for this competency'}",
                        "why": f"priority {g['priority']} = weight {g['weight']} x (1 - coverage {g['coverage']:.0%}) / effort {effort}: the most promotion value per unit of effort",
                        "ref": g["id"],
                    }
                )
            h = _horizon("quarter", "This quarter", items, f"Every competency meets the {asm['level']} bar: ask your manager to start the promotion process.")
            return _with_projects(h, self._projects(), self._context()), {it["ref"] for it in items}

        return self._once("quarter", make)

    def _year(self) -> dict[str, Any]:
        def make() -> dict[str, Any]:
            a = self.assessment()
            if "error" in a:
                return _horizon("year", "This year", [], a["error"])
            asm, lad, level = a["asm"], a["lad"], a["asm"]["level"]
            in_quarter = self._quarter()[1]
            short = sorted((r for r in asm["competencies"] if r["coverage"] < 1 and r["id"] not in in_quarter), key=lambda r: (-(r["need"] - r["credit"]) * r["weight"], r["id"]))
            window = int(lad.get("window_months", 12))
            items = [
                {
                    "title": f"Reach the {level} bar in {r['name']}",
                    "why": f"{r['credit']:g} of {r['need']:g} pieces of evidence ({r['coverage']:.0%}); weight {r['weight']}. A promotion needs every competency at the bar within {window} months; after this quarter's gaps, this is the largest weighted shortfall: start collecting evidence now.",
                    "ref": r["id"],
                }
                for r in short[:2]
            ]
            h = _horizon("year", "This year", items, f"Beyond this quarter's gaps you meet the {level} bar: pick the next level and set target_level with `careerbrew data edit settings.yaml`.")
            return _with_projects(h, self._projects(), self._context())

        return self._once("year", make)

    def _company(self) -> dict[str, Any]:
        def make() -> dict[str, Any]:
            a = self.assessment()
            if "error" in a:
                return _horizon("company", "This company", [], a["error"])
            asm, level, is_company = a["asm"], a["asm"]["level"], a["company"]
            top_w = max((r["weight"] for r in asm["competencies"]), default=0)
            tied = sum(1 for r in asm["competencies"] if r["weight"] == top_w)
            tie_note = f", tied with {tied - 1} other{'' if tied == 2 else 's'}; lowest coverage first" if tied > 1 else ""
            items = [
                {
                    "title": f"Be known for {r['name']} ({r['coverage']:.0%} of the {level} bar)",
                    "why": (
                        f"Your company's ladder gives {r['name']} the top weight ({r['weight']}{tie_note})"
                        if is_company
                        else f"The generic ladder gives {r['name']} the top weight ({r['weight']}{tie_note}); install your company plugin for its real rubric"
                    )
                    + (": the bar is met, so keep the evidence fresh." if r["coverage"] >= 1 else f": the highest-weighted competencies count most toward overall coverage ({asm['overall']:.0%} now)."),
                    "ref": r["id"],
                }
                for r in sorted(asm["competencies"], key=lambda r: (-r["weight"], r["coverage"], r["id"]))
                if r["weight"] == top_w and top_w > 0
            ][:3]
            empty = "Your company ladder has no weighted competencies: add them to the plugin's ladder, then `careerbrew plugin refresh`." if is_company else "Install your company plugin: `careerbrew plugin new <company>`."
            return _with_projects(_horizon("company", "This company", items, empty), self._projects(), self._context())

        return self._once("company", make)

    def horizon(self, hid: str) -> dict[str, Any]:
        part = {"now": self._now, "today": self._today, "week": self._week, "month": self._month, "quarter": self._quarter, "year": self._year, "company": self._company}[hid]()
        return part[0] if isinstance(part, tuple) else part

    def ladder(self) -> dict[str, Any]:
        """The ladder line of the Dashboard: empty when the ladder cannot load."""
        a = self.assessment()
        return {} if "error" in a else {"ladder": a["asm"]["ladder"], "company_ladder": a["company"]}

    def block(self, bid: str) -> dict[str, Any]:
        """One Dashboard block, the answer of `/api/focus/<bid>` (issue #142)."""
        if bid not in BLOCKS:
            raise KeyError(f"no Dashboard block {bid}")
        out: dict[str, Any] = {"id": bid, "at": self.now.isoformat(timespec="minutes")}
        if bid == "tomorrow":
            out["tomorrow"] = self.tomorrow()
        elif bid == "stale":
            out["stale"] = self._once("stale", _stale)
        else:
            out["horizon"] = self.horizon(bid)
            if bid in LADDER_HORIZONS:
                out |= self.ladder()
        return out


def build(now: datetime | None = None) -> dict[str, Any]:
    b = Build(now or calendar.now())
    horizons = [b.horizon(h) for h in HORIZONS]
    return {"at": b.now.isoformat(timespec="minutes"), **b.ladder(), "stale": _stale(), "horizons": horizons, "tomorrow": b.tomorrow()}
