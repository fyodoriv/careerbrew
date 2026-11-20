"""Peer improvement (US-113): `/indie` and `/work` learn from each other.

Each side keeps its lessons in its skill's `LESSONS.md` (process rules only, never business text):

    - [L-work-3] <a rule that this side keeps> (metric step_fail_rate: 0.2 -> 0.1)
    - [L-indie-2] adopted: <note>            a verdict on the other side's lesson: adopted, declined or trial
    Peer source reviewed at: <commit>

`status` detects what a side has not looked at yet: the peer's lessons with no verdict, and the peer's source
commits since the marker. `experiment` trials one change at a time on a side's own metric and ends
KEEP, REVERT or NEUTRAL, so a lesson moves across only when the number says so.
"""

from __future__ import annotations

import os
import re
from datetime import date, datetime, timedelta
from pathlib import Path
from typing import Any

from . import calendar, events, indie, paths, store

STORY = "US-113"
SIDES: dict[str, dict[str, Any]] = {
    "indie": {
        "skill": "skills/careerbrew-indie",
        "sources": ("commands/indie.md", "skills/careerbrew-indie", "src/careerbrew/indie.py"),
        "persona": "the founder: bold, picks the bet that pays back, cuts what does not, counts outcomes per agent-hour",
        "metrics": {"hours_per_outcome": False, "asks_per_outcome": False, "outcomes_per_week": True},
    },
    "work": {
        "skill": "skills/careerbrew-work-day",
        "sources": ("commands/work.md", "skills/careerbrew-work-day", "src/careerbrew/workday"),
        "persona": "the operator: careful, proves each step, keeps every promise, counts failed and blocked steps",
        "metrics": {"step_fail_rate": False, "step_blocked_rate": False, "step_minutes": False},
    },
}
PEER = {"indie": "work", "work": "indie"}
VERDICTS = ("adopted", "declined", "trial")
BASELINE_DAYS = 14
MIN_DAYS = 3  # a trial shorter than this says nothing
MARGIN = 0.10  # KEEP or REVERT needs a 10% move; less is NEUTRAL
_LESSON = re.compile(r"^- \[(L-(indie|work)-(\d+))\]\s*(.*)$")
_MARK = re.compile(r"^Peer source reviewed at: ([0-9a-f]{7,40})\s*$")


class PeerError(ValueError):
    pass


def _side(side: str) -> str:
    if side not in SIDES:
        raise PeerError("a side is indie or work")
    return side


def root() -> Path:
    """The careerbrew checkout whose skills hold the lessons (`CAREERBREW_PEER_ROOT` points tests at a scratch copy)."""
    return Path(os.environ.get("CAREERBREW_PEER_ROOT") or paths.REPO_ROOT)


def lessons_path(side: str) -> Path:
    return root() / SIDES[_side(side)]["skill"] / "LESSONS.md"


def read_lessons(side: str) -> dict[str, Any]:
    """A side's own lessons, its verdicts on the peer's lessons, and its review marker."""
    own: dict[str, str] = {}
    verdicts: dict[str, str] = {}
    mark = ""
    try:
        text = lessons_path(side).read_text(encoding="utf-8")  # a tracked repo file of process rules, not a data record
    except OSError:
        text = ""
    for line in text.splitlines():
        if m := _MARK.match(line.strip()):
            mark = m.group(1)
        if not (m := _LESSON.match(line.strip())):
            continue
        if m.group(2) == side:
            own[m.group(1)] = m.group(4)
        elif (word := m.group(4).split(":", 1)[0].strip().lower()) in VERDICTS:
            verdicts[m.group(1)] = word
    return {"own": own, "verdicts": verdicts, "mark": mark}


def _known(mark: str) -> str:
    """The marker when it names a commit here, else "": an unknown marker hides nothing, so the window applies."""
    return mark if mark and indie.git_out(root(), "rev-parse", "--verify", "--quiet", f"{mark}^{{commit}}") else ""


def _peer_commits(side: str, mark: str) -> list[str]:
    """Short ids of the commits that changed the peer's sources since this side's marker (30 days with no marker)."""
    mark = _known(mark)
    span = [f"{mark}..HEAD"] if mark else [f"--since={(calendar.now() - timedelta(days=30)).isoformat()}", "HEAD"]
    out = indie.git_out(root(), "log", "--format=%h", *span, "--", *SIDES[PEER[side]]["sources"])
    return out.split()


def measure(side: str, metric: str, days: int, at: datetime | None = None) -> float | None:
    """A side's metric over the trailing `days` days from this machine's own records; `None` when there is no data."""
    if metric not in SIDES[_side(side)]["metrics"]:
        raise PeerError(f"{side} metrics: {', '.join(SIDES[side]['metrics'])}")
    at = at or calendar.now()
    days = max(1, days)
    if side == "work":
        from .workday import timing

        steps = timing.summary(at.date(), days)["steps"]
        runs = sum(s["runs"] for s in steps)
        if not runs:
            return None
        if metric == "step_minutes":
            took = [s["median"] for s in steps if s["median"] is not None]
            return round(sum(took) / len(took) / 60, 2) if took else None
        return round(sum(s["failed" if metric == "step_fail_rate" else "blocked"] for s in steps) / runs, 3)
    since = at - timedelta(days=days)
    live = {i: b for i, b in indie.bets().items() if b["state"] != "killed" and Path(b["path"]).is_dir()}
    outcomes = sum(len(indie.outcome_times(b, since)) for b in live.values())
    if metric == "outcomes_per_week":
        return round(outcomes * 7 / days, 2) if live else None
    rows = [r for r in indie.day_rows() if r["bet"] in live and date.fromisoformat(r["day"]) > since.date()]
    # Hours exist only for recorded days: count the outcomes of those bet-days, or the cost reads too low.
    logged = {(r["bet"], r["day"]) for r in rows}
    outcomes = sum(1 for i, b in live.items() for t in indie.outcome_times(b, since) if (i, t.date().isoformat()) in logged)
    if not rows or not outcomes:
        return None
    return round(sum(r["hours"] if metric == "hours_per_outcome" else r.get("asks", 0) for r in rows) / outcomes, 2)


def _file() -> Path:
    return paths.sub("peer", "experiments.jsonl")


def experiments() -> list[dict[str, Any]]:
    return store.read_jsonl(_file())


def open_experiment(side: str) -> dict[str, Any] | None:
    return next((e for e in experiments() if e["side"] == side and not e.get("closed")), None)


def start(side: str, change: str, metric: str, lesson: str = "") -> dict[str, Any]:
    """Start the one open trial of a side: a process change, the metric it should move, and the peer lesson it tries (if any)."""
    _side(side)
    change = " ".join(change.split())
    if not 8 <= len(change) <= 300:
        raise PeerError("describe the change in one line of 8 to 300 characters")
    if lesson and not ((m := _LESSON.match(f"- [{lesson}]")) and m.group(2) == PEER[side]):
        raise PeerError(f"--lesson names a lesson of {PEER[side]}, like L-{PEER[side]}-1 (careerbrew peer status --for {side})")
    if cur := open_experiment(side):
        raise PeerError(f"{side} already trials {cur['id']}: one change at a time, or the number cannot say which change moved it (careerbrew peer experiment --side {side} --close)")
    base = measure(side, metric, BASELINE_DAYS)
    row = {
        "id": f"x-{side}-{sum(1 for e in experiments() if e['side'] == side) + 1}",
        "side": side,
        "change": change,
        "metric": metric,
        "lesson": lesson,
        "started": calendar.now().isoformat(timespec="seconds"),
        "baseline": base,
        "closed": "",
        "value": None,
        "verdict": "",
    }
    store.append_jsonl(_file(), [row])
    events.emit("peer.experiment", started=1, closed=0, keep=0)
    return row


def verdict(side: str, metric: str, baseline: float | None, value: float | None) -> str:
    if baseline is None or value is None:
        return "NO-DATA"
    sign = 1 if SIDES[side]["metrics"][metric] else -1
    if baseline == 0:  # no ratio from zero: any move away from it is a full move
        return "NEUTRAL" if value == 0 else "KEEP" if value * sign > 0 else "REVERT"
    better = (value - baseline) / baseline * sign
    return "KEEP" if better >= MARGIN else "REVERT" if better <= -MARGIN else "NEUTRAL"


def close(side: str) -> dict[str, Any]:
    """End a side's open trial: measure since its start, compare with the baseline, and say which lesson line to write."""
    cur = open_experiment(_side(side))
    if cur is None:
        raise PeerError(f"{side} has no open trial (careerbrew peer experiment --side {side} --start ...)")
    now = calendar.now()
    ran = (now - datetime.fromisoformat(cur["started"])).days
    if ran < MIN_DAYS:
        raise PeerError(f"{cur['id']} ran {ran} day(s): a trial needs at least {MIN_DAYS} before its number means anything")
    value = measure(side, cur["metric"], ran, now)
    done = {**cur, "closed": now.isoformat(timespec="seconds"), "value": value, "verdict": verdict(side, cur["metric"], cur["baseline"], value)}
    store.upsert_jsonl(_file(), [done], ["closed", "value", "verdict"])
    events.emit("peer.experiment", started=0, closed=1, keep=int(done["verdict"] == "KEEP"))
    return {**done, "write": lesson_line(done)}


def lesson_line(e: dict[str, Any]) -> str:
    """The line the agent adds to the side's LESSONS.md (by pull request) for a closed trial."""
    if e["verdict"] == "NO-DATA":
        return ""  # no number: neither adopted nor declined
    numbers = f"(metric {e['metric']}: {e['baseline']} -> {e['value']}, {e['verdict']})"
    if e["lesson"]:
        word = "adopted" if e["verdict"] == "KEEP" else "declined"
        return f"- [{e['lesson']}] {word}: {e['change']} {numbers}"
    if e["verdict"] != "KEEP":
        return ""
    n = max([int(k.rsplit("-", 1)[1]) for k in read_lessons(e["side"])["own"]] or [0]) + 1
    return f"- [L-{e['side']}-{n}] {e['change']} {numbers}"


def status(side: str, at: datetime | None = None) -> dict[str, Any]:
    """What `side` should look at before it runs: the peer's unreviewed lessons and source commits, and both sides' numbers."""
    peer = PEER[_side(side)]
    mine, theirs = read_lessons(side), read_lessons(peer)
    mine["mark"] = _known(mine["mark"])
    unreviewed = [k for k in theirs["own"] if k not in mine["verdicts"]]
    commits = _peer_commits(side, mine["mark"])
    cur = open_experiment(side)
    todo = []
    if cur:
        todo.append(f"finish trial {cur['id']} first: careerbrew peer experiment --side {side} --close")
    elif unreviewed:
        todo.append(f"read {unreviewed[0]} in {SIDES[peer]['skill']}/LESSONS.md; trial it (careerbrew peer experiment --side {side} --start '<change>' --metric <metric> --lesson {unreviewed[0]}) or write a declined line with the reason")
    if commits:
        todo.append(f"read the {len(commits)} peer commit(s) (git log {mine['mark'] + '..HEAD' if mine['mark'] else '--since=30.days'} -- {' '.join(SIDES[peer]['sources'])}), then move the marker line in {SIDES[side]['skill']}/LESSONS.md")
    out = {
        "side": side,
        "persona": SIDES[side]["persona"],
        "peer": peer,
        "peer_persona": SIDES[peer]["persona"],
        "unreviewed_lessons": unreviewed,
        "trials": sorted(k for k, v in mine["verdicts"].items() if v == "trial"),
        "adopted": sum(1 for v in mine["verdicts"].values() if v == "adopted"),
        "declined": sum(1 for v in mine["verdicts"].values() if v == "declined"),
        "peer_commits": commits,
        "reviewed_at": mine["mark"],
        "open_trial": {k: cur[k] for k in ("id", "metric", "lesson", "started", "baseline")} if cur else None,
        "metrics": {m: measure(side, m, BASELINE_DAYS, at) for m in SIDES[side]["metrics"]},
        "peer_metrics": {m: measure(peer, m, BASELINE_DAYS, at) for m in SIDES[peer]["metrics"]},
        "next": todo,
    }
    events.emit("peer.status", unreviewed=len(unreviewed), commits=len(commits), trials=int(bool(cur)))
    return out


__all__ = ["SIDES", "PeerError", "close", "experiments", "measure", "start", "status"]
