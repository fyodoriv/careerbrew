"""Independent mode (US-112): each personal repo is an income bet.

`/indie` runs the personal work day across the owner's own repos. This module holds what `/work` has no
need for: the income goal, one bet per repo (hypothesis, metric, success, pivot), the ranking that gives
agent sessions to the bets that pay back, the outcome record, and the weekly review.

Data lives encrypted under `<data root>/indie/`. Printed output holds ids, counts and percentages only:
never an income amount, never a hypothesis (the owner reads those with `careerbrew data show`).
"""

from __future__ import annotations

import json
import math
import os
import random
import re
import shlex
import subprocess
from datetime import date, datetime, timedelta
from pathlib import Path
from typing import Any

from . import calendar, config, events, paths, store

STORY = "US-112"
KINDS = ("product", "sponsors", "audience", "consulting", "enabler")
STATES = ("proposed", "active", "paused", "pivot", "killed")
DECISIONS = {"activate": "active", "persevere": "active", "pause": "paused", "pivot": "pivot", "kill": "killed"}
OUTCOME = re.compile(r"^(feat|fix|perf)(\([^)]*\))?!?:")  # a merged change users can notice; docs, chore, test and refactor are upkeep
EXPLORE = 0.3  # UCB exploration constant; in 200 seeded 8-week runs it cut agent-hours per outcome by 38% on average against round-robin (`simulation_gain`)
ENABLER_SLOTS = 1  # tools that serve the other bets get at most one session a day: they earn nothing by themselves
MAX_SESSION_HOURS = 12.0
_ID = re.compile(r"[^a-z0-9]+")
_TASK = re.compile(r"^- \[ \] (?P<title>.+)$")
_PRIORITY = re.compile(r"^## (P[0-9])\b")


class IndieError(ValueError):
    pass


def _file(name: str) -> Path:
    return paths.sub("indie", name)


def _stamp(at: datetime | None = None) -> str:
    return (at or calendar.now()).isoformat(timespec="seconds")


def _local(value: str) -> datetime:
    return datetime.fromisoformat(value).astimezone(calendar.local_tz())


# ---------- goal and income ----------
def goal() -> dict[str, Any]:
    return store.read_yaml(_file("goal.yaml"), {}) or {}


def set_goal(monthly: float, by: str = "") -> dict[str, Any]:
    if not math.isfinite(monthly) or monthly <= 0:
        raise IndieError("the monthly income target is a number above 0")
    if by:
        try:
            date.fromisoformat(by)
        except ValueError:
            raise IndieError("--by is a date like 2027-12-31") from None
    g = {"monthly": monthly, "by": by, "set_at": _stamp()}
    store.write_yaml(_file("goal.yaml"), g)
    return g


def add_income(month: str, amount: float, bet: str = "") -> None:
    if not re.fullmatch(r"\d{4}-(0[1-9]|1[0-2])", month):
        raise IndieError("the month looks like 2026-11")
    if not math.isfinite(amount) or amount < 0:
        raise IndieError("the amount is a number, 0 or more")
    if bet and bet not in bets():
        raise IndieError(f"no bet {bet} (careerbrew indie bets)")
    store.append_jsonl(_file("income.jsonl"), [{"month": month, "amount": amount, "bet": bet, "at": _stamp()}])


def progress(at: datetime | None = None) -> dict[str, Any]:
    """Independent income against the target, as percentages only (the last full month and the month before)."""
    at = at or calendar.now()
    g = goal()
    first = at.date().replace(day=1)
    last = (first - timedelta(days=1)).strftime("%Y-%m")
    prev = ((first - timedelta(days=1)).replace(day=1) - timedelta(days=1)).strftime("%Y-%m")
    rows = store.read_jsonl(_file("income.jsonl"))
    by_month: dict[str, float] = {}
    for r in rows:
        by_month[r["month"]] = by_month.get(r["month"], 0.0) + float(r["amount"])
    target = float(g.get("monthly") or 0)

    def pct(month: str) -> float | None:
        return round(100 * by_month[month] / target, 1) if target and month in by_month else None

    return {"set": bool(target), "by": g.get("by", ""), "month": last, "percent": pct(last), "previous_percent": pct(prev), "months_recorded": len(by_month)}


# ---------- bets ----------
def bets() -> dict[str, dict[str, Any]]:
    return (store.read_yaml(_file("bets.yaml"), {}) or {}).get("bets", {})


def _save(all_bets: dict[str, dict[str, Any]]) -> None:
    store.write_yaml(_file("bets.yaml"), {"bets": all_bets})


def git_out(path: Path | str, *args: str) -> str:
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}  # a hook's GIT_DIR must not redirect the read
    try:
        r = subprocess.run(["git", "-C", str(path), *args], capture_output=True, text=True, timeout=30, check=False, env=env)
    except (OSError, subprocess.TimeoutExpired):
        return ""
    return r.stdout.strip() if r.returncode == 0 else ""


def bet_id(name: str) -> str:
    return _ID.sub("-", name.lower()).strip("-")[:40]


def _roots() -> list[Path]:
    return [Path(p.strip()).expanduser() for p in str(config.get("indie_roots")).split(",") if p.strip()]


def found_repos() -> list[dict[str, str]]:
    """Each repo under `indie_roots` (two levels; linked worktrees have no `.git` folder and are skipped), one per origin."""
    want = {bet_id(n) for n in str(config.get("indie_repos")).split(",") if n.strip()}
    out: dict[str, dict[str, str]] = {}
    seen: set[str] = set()
    for root in _roots():
        if not root.is_dir():
            continue
        level1 = sorted(d for d in root.iterdir() if d.is_dir() and not d.name.startswith("."))
        level2 = sorted(e for d in level1 if not (d / ".git").exists() and not d.name.endswith(".noindex") for e in d.iterdir() if e.is_dir() and not e.name.startswith("."))
        for d in [*level1, *level2]:
            ident = bet_id(d.name)
            if not (d / ".git").is_dir() or not ident or (want and ident not in want) or ident in out:
                continue
            origin = git_out(d, "remote", "get-url", "origin")
            if origin and origin in seen:
                continue
            seen.add(origin)
            out[ident] = {"id": ident, "path": str(d), "origin": origin}
    return list(out.values())


def scan() -> dict[str, list[str]]:
    """Add a `proposed` bet for each new repo; keep every existing bet as it is."""
    all_bets = dict(bets())
    added = []
    found = found_repos()
    for r in found:
        if r["id"] not in all_bets:
            all_bets[r["id"]] = {"path": r["path"], "origin": r["origin"], "kind": "", "hypothesis": "", "metric": "", "success": None, "pivot": None, "higher_is_better": True, "measure": "", "state": "proposed", "decided_at": ""}
            added.append(r["id"])
        elif all_bets[r["id"]].get("path") != r["path"]:
            all_bets[r["id"]] = {**all_bets[r["id"]], "path": r["path"]}
    if added or all_bets != bets():
        _save(all_bets)
    missing = sorted(i for i, b in all_bets.items() if not Path(b["path"]).is_dir())
    events.emit("indie.scan", found=len(found), added=len(added), missing=len(missing))
    return {"added": added, "missing": missing, "bets": sorted(all_bets)}


def bet(ident: str) -> dict[str, Any]:
    b = bets().get(ident)
    if b is None:
        raise IndieError(f"no bet {ident} (careerbrew indie bets; careerbrew indie scan finds new repos)")
    return b


def ready(b: dict[str, Any]) -> list[str]:
    """What a bet still lacks before it may get agent time."""
    return [f for f in ("kind", "hypothesis", "metric") if not b.get(f)] + [f for f in ("success", "pivot") if b.get(f) is None]


def set_bet(ident: str, **fields: Any) -> dict[str, Any]:
    all_bets = dict(bets())
    b = dict(bet(ident))
    for k, v in fields.items():
        if v is None:
            continue
        if k == "kind" and v not in KINDS:
            raise IndieError(f"a bet's kind is one of: {', '.join(KINDS)}")
        if k in ("success", "pivot") and not math.isfinite(float(v)):
            raise IndieError(f"{k} is a number")
        if k in ("hypothesis", "metric", "measure"):
            v = " ".join(str(v).split())
            if len(v) > 400:
                raise IndieError(f"keep the {k} under 400 characters")
        b[k] = v
    if b.get("measure"):
        try:
            shlex.split(b["measure"])
        except ValueError:
            raise IndieError("the measure command has an unbalanced quote") from None
    all_bets[ident] = b
    _save(all_bets)
    return b


def decide(ident: str, decision: str) -> dict[str, Any]:
    """The owner's call on a bet. `activate` needs the full hypothesis; any decision clears a pivot hold."""
    if decision not in DECISIONS:
        raise IndieError(f"decide one of: {', '.join(DECISIONS)}")
    b = bet(ident)
    if DECISIONS[decision] == "active" and (lack := ready(b)):
        raise IndieError(f"bet {ident} cannot be active yet: it lacks {', '.join(lack)} (careerbrew indie bet {ident} --kind ... --hypothesis ... --metric ... --success N --pivot N)")
    all_bets = dict(bets())
    all_bets[ident] = {**b, "state": DECISIONS[decision], "decided_at": _stamp(), "decided_readings": len(readings(ident))}
    _save(all_bets)
    events.emit("indie.decide", active=int(DECISIONS[decision] == "active"), bets=len(all_bets))
    return all_bets[ident]


# ---------- the bet's own metric ----------
def status(value: float | None, b: dict[str, Any]) -> str:
    if value is None or b.get("success") is None or b.get("pivot") is None:
        return "no-data"
    hib = b.get("higher_is_better", True)
    ok = value >= b["success"] if hib else value <= b["success"]
    bad = value < b["pivot"] if hib else value > b["pivot"]
    return "KEEP" if ok else "PIVOT" if bad else "WATCH"


def readings(ident: str = "") -> list[dict[str, Any]]:
    return [r for r in store.read_jsonl(_file("measures.jsonl")) if not ident or r.get("bet") == ident]


def measure(ident: str, value: float | None = None) -> dict[str, Any]:
    """Record one reading of the bet's metric: the given value, or the last line its `measure` command prints (run in the repo, no shell)."""
    b = bet(ident)
    if value is None:
        if not b.get("measure"):
            raise IndieError(f"bet {ident} has no measure command: pass --value, or set one with careerbrew indie bet {ident} --measure '<command>'")
        try:
            r = subprocess.run(shlex.split(b["measure"]), cwd=b["path"], capture_output=True, text=True, timeout=120, check=False)
        except (OSError, subprocess.TimeoutExpired) as e:
            raise IndieError(f"the measure command of {ident} did not run ({type(e).__name__})") from None
        if r.returncode != 0:
            raise IndieError(f"the measure command of {ident} failed (exit {r.returncode}): no reading recorded")
        lines = [x for x in r.stdout.strip().splitlines() if x.strip()]
        try:
            value = float(lines[-1].strip())
        except (IndexError, ValueError):
            raise IndieError(f"the measure command of {ident} must print a number on its last line (exit {r.returncode})") from None
    if not math.isfinite(value):
        raise IndieError("a reading is a finite number")
    row = {"bet": ident, "at": _stamp(), "value": value, "status": status(value, b)}
    store.append_jsonl(_file("measures.jsonl"), [row])
    events.emit("indie.measure", keep=int(row["status"] == "KEEP"), pivot=int(row["status"] == "PIVOT"))
    return row


def held(ident: str, b: dict[str, Any], rows: list[dict[str, Any]] | None = None) -> bool:
    """Two PIVOT readings in a row since the owner's last decision: the bet gets no session until the owner decides."""
    mine = [r for r in (rows if rows is not None else readings()) if r.get("bet") == ident][int(b.get("decided_readings") or 0) :]
    return len(mine) >= 2 and all(r["status"] == "PIVOT" for r in mine[-2:])


# ---------- outcomes, hours, asks ----------
def _ref(path: str) -> str:
    return git_out(path, "symbolic-ref", "--short", "refs/remotes/origin/HEAD") or "HEAD"


def outcome_times(b: dict[str, Any], since: datetime) -> list[datetime]:
    """When each noticed change (feat, fix, perf) reached the repo's default branch since `since`."""
    out = git_out(b["path"], "log", "--first-parent", _ref(b["path"]), f"--since={since.isoformat()}", "--format=%cI%x09%s")
    return [_local(line.split("\t", 1)[0]) for line in out.splitlines() if "\t" in line and OUTCOME.match(line.split("\t", 1)[1])]


def day_rows() -> list[dict[str, Any]]:
    return store.read_jsonl(_file("days.jsonl"))


def log_day(ident: str, hours: float, asks: int = 0, day: str = "", session: str = "") -> dict[str, Any]:
    bet(ident)
    if not math.isfinite(hours) or not 0 <= hours <= 24:
        raise IndieError("hours is a number from 0 to 24")
    day = day or calendar.now().date().isoformat()
    row = {"id": f"{day}:{ident}", "day": day, "bet": ident, "hours": round(hours, 2), "asks": int(asks), "session": session}
    store.upsert_jsonl(_file("days.jsonl"), [row], ["hours", "asks", "session"])
    return row


def _adapter_hours(ident: str, days: int) -> float | None:
    """Agent tool time of one repo over the last `days` days, from the owner's `indie_agent_time_command` (JSON with `toolMs`)."""
    cmd = str(config.get("indie_agent_time_command") or "")
    if not cmd or days <= 0:
        return 0.0 if cmd else None
    try:
        argv = [a.replace("{project}", ident).replace("{days}", str(days)) for a in shlex.split(cmd)]
        r = subprocess.run(argv, capture_output=True, text=True, timeout=60, check=False)
        return float(json.loads(r.stdout)["toolMs"]) / 3_600_000
    except (OSError, subprocess.TimeoutExpired, ValueError, KeyError, TypeError):
        return None


def _ratio(a: float | None, b: float) -> float | None:
    return round(a / b, 2) if a is not None and b else None


def review(weeks: int = 4, at: datetime | None = None) -> dict[str, Any]:
    """Per week, oldest first: outcomes, agent-hours, owner asks, and the two cost ratios. Missing hours are `None`, never 0."""
    at = at or calendar.now()
    active = {i: b for i, b in bets().items() if b["state"] != "killed" and Path(b["path"]).is_dir()}
    since = at - timedelta(days=7 * weeks)
    times = {i: outcome_times(b, since) for i, b in active.items()}
    logged = [r for r in day_rows() if r["bet"] in active]
    use_adapter = bool(config.get("indie_agent_time_command"))
    cum = {i: [_adapter_hours(i, 7 * k) for k in range(weeks + 1)] for i in active} if use_adapter else {}
    out_weeks = []
    for k in range(weeks - 1, -1, -1):
        start, end = at - timedelta(days=7 * (k + 1)), at - timedelta(days=7 * k)
        n = sum(1 for ts in times.values() for t in ts if start < t <= end)
        mine = [r for r in logged if start.date() < date.fromisoformat(r["day"]) <= end.date()]
        hours: float | None
        if use_adapter:
            parts = [cum[i][k + 1] - cum[i][k] if cum[i][k + 1] is not None and cum[i][k] is not None else None for i in active]  # type: ignore[operator]
            hours = round(max(0.0, sum(p for p in parts if p is not None)), 2) if parts and None not in parts else None  # one failed read: no-data, never an undercount
        else:
            hours = round(sum(r["hours"] for r in mine), 2) if mine else None
        asks = sum(r.get("asks", 0) for r in mine)
        out_weeks.append({"week_ending": end.date().isoformat(), "outcomes": n, "agent_hours": hours, "asks": asks, "hours_per_outcome": _ratio(hours, n), "asks_per_outcome": _ratio(float(asks), n) if mine else None})
    rows = readings()
    per_bet = []
    for i, b in sorted(active.items()):
        last = [r for r in rows if r["bet"] == i][-1:]
        per_bet.append({"bet": i, "state": b["state"], "kind": b.get("kind", ""), "outcomes": len(times[i]), "metric_status": last[0]["status"] if last else "no-data", "held": held(i, b, rows)})
    half = max(1, weeks // 2)
    old, new = out_weeks[:half], out_weeks[-half:]

    def mean(rows_: list[dict[str, Any]], key: str) -> float | None:
        vals = [r[key] for r in rows_ if r[key] is not None]
        return round(sum(vals) / len(vals), 2) if vals else None

    def cost(rows_: list[dict[str, Any]], key: str, per: str) -> float | None:
        """Total cost over total outcomes for the weeks that have the cost: a week with hours and no outcome still counts."""
        have = [r for r in rows_ if r[key] is not None and r[per] is not None]
        return _ratio(float(sum(r[key] for r in have)), sum(r["outcomes"] for r in have)) if have else None

    trend = {
        "outcomes": {"earlier": mean(old, "outcomes"), "recent": mean(new, "outcomes")},
        "hours_per_outcome": {"earlier": cost(old, "agent_hours", "agent_hours"), "recent": cost(new, "agent_hours", "agent_hours")},
        "asks_per_outcome": {"earlier": cost(old, "asks", "agent_hours"), "recent": cost(new, "asks", "agent_hours")},
    }
    events.emit("indie.review", weeks=weeks, outcomes=sum(w["outcomes"] for w in out_weeks), bets=len(active))
    return {"weeks": out_weeks, "bets": per_bet, "trend": trend, "goal": progress(at), "hours_source": "adapter" if use_adapter else "sessions"}


# ---------- ranking ----------
def ucb(outcomes: float, hours: float, plays: int, total_plays: int, explore: float = EXPLORE) -> float:
    """Upper confidence bound of a bet's payback: outcomes per agent-hour squashed to 0..1, plus a bonus for few tries."""
    y = outcomes / hours if hours > 0 else 0.0
    return y / (y + 1) + explore * math.sqrt(math.log(total_plays + 1) / plays)


def next_action(b: dict[str, Any]) -> dict[str, Any]:
    """The first open, unblocked task in the repo's TASKS.md (id and priority only), or what to do when there is none."""
    path = Path(b["path"]) / "TASKS.md"
    try:
        lines = path.read_text(encoding="utf-8").splitlines()  # a repo's own tracked task list, not a careerbrew data record
    except OSError:
        return {"kind": "plan", "do": "no TASKS.md: add the first task for this bet, with its hypothesis and measurement"}
    priority, task = "", None
    for n, line in enumerate([*lines, "- [ ] end"], 1):
        if m := _PRIORITY.match(line):
            priority = m.group(1)
        if _TASK.match(line):
            if task and not task["blocked"]:
                return {"kind": "task", "id": task["id"], "priority": task["priority"], "line": task["line"], "do": f"deliver task {task['id'] or 'at line ' + str(task['line'])} in TASKS.md"}
            task = {"id": "", "priority": priority, "line": n, "blocked": False}
        elif task and line.startswith("  "):
            if m := re.match(r"\s+- \*\*ID\*\*: (\S+)", line):
                task["id"] = m.group(1)
            if re.match(r"\s+- \*\*Blocked( by)?\*\*:", line):
                task["blocked"] = True
    return {"kind": "plan", "do": "no open unblocked task: add the next task for this bet, or pause the bet"}


def rank(at: datetime | None = None) -> list[dict[str, Any]]:
    """Every bet in the order it should get agent time today, with the reason and its next action."""
    at = at or calendar.now()
    all_bets = bets()
    logged = day_rows()
    rows = readings()
    cap = int(config.get("work_session_cap"))
    total = len(logged)
    scored = []
    for ident, b in all_bets.items():
        mine = [r for r in logged if r["bet"] == ident]
        row: dict[str, Any] = {"bet": ident, "state": b["state"], "kind": b.get("kind", ""), "path": b["path"], "plays": len(mine), "slot": False, "score": None}
        if not Path(b["path"]).is_dir():
            row |= {"order": 5, "reason": "its folder is gone", "next": {"kind": "decide", "do": f"careerbrew indie decide {ident} kill --yes, or restore the folder"}}
        elif b["state"] == "proposed":
            row |= {
                "order": 3,
                "reason": "no income hypothesis yet: it gets no agent time",
                "next": {"kind": "question", "do": f"owner: say how {ident} leads to income (careerbrew indie bet {ident} ..., then careerbrew indie decide {ident} activate --yes)"},
            }
        elif b["state"] == "active" and (lack := ready(b)):
            row |= {"order": 3, "reason": f"it lacks {', '.join(lack)}: it gets no agent time", "next": {"kind": "question", "do": f"owner: complete the bet (careerbrew indie bet {ident} ...)"}}
        elif b["state"] != "active":
            row |= {"order": 4, "reason": f"{b['state']} by the owner", "next": {"kind": "none", "do": f"nothing until the owner decides again (careerbrew indie decide {ident} persevere --yes)"}}
        elif held(ident, b, rows):
            row |= {"order": 2, "reason": "its metric was under the pivot line twice in a row: held", "next": {"kind": "decide", "do": f"owner: careerbrew indie decide {ident} persevere|pivot|kill --yes"}}
        else:
            recent = len(outcome_times(b, at - timedelta(days=28)))
            if mine:
                first = datetime.combine(date.fromisoformat(min(r["day"] for r in mine)), datetime.min.time(), tzinfo=calendar.local_tz())
                got = len(outcome_times(b, first))
                hours = sum(r["hours"] for r in mine)
                row["score"] = round(ucb(got, hours, len(mine), total), 4)
                row["reason"] = f"{got} outcome(s) in {round(hours, 1)} agent-hour(s) over {len(mine)} session(s)"
            else:
                row["reason"] = f"not tried yet ({recent} outcome(s) in the last 28 days)"
            row |= {"order": 1, "recent": recent, "next": next_action(b)}
        scored.append(row)
    scored.sort(key=lambda r: (r["order"], r["score"] is not None, -(r["score"] or 0.0), -r.get("recent", 0), r["bet"]))
    slots = enablers = 0
    for n, r in enumerate(scored, 1):
        r["rank"] = n
        if r["order"] != 1 or slots >= cap:
            continue
        if r["kind"] == "enabler":
            if enablers >= ENABLER_SLOTS:
                r["reason"] += f"; no slot: at most {ENABLER_SLOTS} tool bet a day"
                continue
            enablers += 1
        r["slot"] = True
        slots += 1
    return [{k: v for k, v in r.items() if k not in ("order", "recent")} for r in scored]


CONTRACT = (
    "You are the session for one bet. Work only in its repo. Deliver its next action in full: branch, tests, docs, PR, green checks, merge. "
    "Never ask the owner mid-run: save each ask with `careerbrew work session ask <id> ...`. Claim a shared PR before you write it (`careerbrew work claim take pr:<owner>/<repo>#<n> --session <id>`). "
    'End with `careerbrew work session handoff <id> "<status and next step>"`. An outcome is a merged feat, fix or perf change; a plan is not an outcome.'
)


def plan(at: datetime | None = None) -> dict[str, Any]:
    """The indie day: scan, rank, and say who gets a session. Read-only except for new `proposed` bets."""
    at = at or calendar.now()
    found = scan()
    ranked = rank(at)
    with_next = sum(1 for r in ranked if r["next"]["kind"] != "none")
    with_task = sum(1 for r in ranked if r["next"]["kind"] == "task")
    out = {
        "day": at.date().isoformat(),
        "goal": progress(at),
        "cap": int(config.get("work_session_cap")),
        "bets": ranked,
        "sessions": [r["bet"] for r in ranked if r["slot"]],
        "questions": [r["bet"] for r in ranked if r["next"]["kind"] in ("question", "decide")],
        "coverage": {"repos": len(ranked), "with_next_action": with_next, "with_task": with_task},
        "added": found["added"],
        "contract": CONTRACT,
        "flow": [
            "careerbrew peer status --for indie --json: review what /work learned before you start",
            "ask the owner every item in `questions` in one batched ask; a bet with no hypothesis gets no agent time",
            "for each bet in `sessions`: careerbrew work session open <bet> --id <id>, then deliver its `next` action in its repo",
            "careerbrew indie measure <bet> for each bet with a measure command",
            "careerbrew indie end: record hours and asks, then read careerbrew indie review --json",
        ],
    }
    events.emit("indie.run", bets=len(ranked), sessions=len(out["sessions"]), questions=len(out["questions"]), added=len(found["added"]))
    return out


def end(hours: dict[str, float] | None = None, at: datetime | None = None) -> list[dict[str, Any]]:
    """Record today's agent time and owner asks per bet from the day's project sessions (`--hours bet=H` overrides the clock)."""
    from .work import sessions
    from .workday import questions

    at = at or calendar.now()
    today = at.date().isoformat()
    mine = bets()
    asked = [q.get("step") for q in questions.records(at.date())]  # every ask of the day, answered or open
    out = []
    seen: set[str] = set()
    for s in sessions.listing(include_ended=True):
        if s["project"] not in mine or s["day"] != today or s["project"] in seen:
            continue
        seen.add(s["project"])
        stop = datetime.fromisoformat(s["ended"]) if s["ended"] else at
        took = min(MAX_SESSION_HOURS, max(0.0, (stop - datetime.fromisoformat(s["started"])).total_seconds() / 3600))
        out.append(log_day(s["project"], (hours or {}).get(s["project"], took), asked.count(sessions.step_of(s["id"])), today, s["id"]))
    for ident, h in (hours or {}).items():
        if ident not in seen:
            out.append(log_day(ident, h, 0, today))
    events.emit("indie.end", bets=len(out), hours=round(sum(r["hours"] for r in out), 2), asks=sum(r["asks"] for r in out))
    return out


# ---------- the proof: a seeded simulation against round-robin ----------
def simulate(policy: str, seed: int, yields: tuple[float, ...] = (0.1, 0.2, 0.3, 0.5, 0.7, 0.9, 1.2), weeks: int = 8, days: int = 5, cap: int = 3, session_hours: float = 2.0) -> list[dict[str, float]]:
    """Per week: outcomes and agent-hours per outcome when `cap` of the bets get a session each day.

    `yields` is each bet's true outcomes per agent-hour (unknown to the policy). `ucb` uses the same score as
    `rank`; `round-robin` gives every bet the same share."""
    if policy not in ("ucb", "round-robin"):
        raise IndieError("policy is ucb or round-robin")
    rng = random.Random(seed)  # noqa: S311 (a seeded simulation, not a secret)
    n = len(yields)
    plays, got, hours = [0] * n, [0] * n, [0.0] * n
    out = []
    turn = 0
    for _week in range(weeks):
        week_outcomes = 0
        for _day in range(days):
            if policy == "round-robin":
                pick = [(turn + j) % n for j in range(cap)]
                turn += cap
            else:
                total = sum(plays)
                pick = sorted(range(n), key=lambda i: (plays[i] > 0, -(ucb(got[i], hours[i], plays[i], total) if plays[i] else 0.0), i))[:cap]
            for i in pick:
                trials = int(session_hours * 4)  # quarter-hour steps
                made = sum(rng.random() < yields[i] / 4 for _ in range(trials))
                plays[i] += 1
                got[i] += made
                hours[i] += session_hours
                week_outcomes += made
        spent = days * cap * session_hours
        out.append({"outcomes": week_outcomes, "hours_per_outcome": round(spent / week_outcomes, 3) if week_outcomes else spent})
    return out


def simulation_gain(first_seed: int = 100, seeds: int = 200) -> dict[str, float]:
    """How much cheaper an outcome is under `ucb` than round-robin in weeks 7 and 8, over `seeds` seeded runs: mean, 10th percentile, worst."""
    gains = []
    for seed in range(first_seed, first_seed + seeds):
        a, b = simulate("ucb", seed), simulate("round-robin", seed)
        mine, base = sum(w["hours_per_outcome"] for w in a[-2:]) / 2, sum(w["hours_per_outcome"] for w in b[-2:]) / 2
        gains.append(1 - mine / base)
    gains.sort()
    return {"mean": round(sum(gains) / len(gains), 3), "p10": round(gains[len(gains) // 10], 3), "worst": round(gains[0], 3), "seeds": seeds}
