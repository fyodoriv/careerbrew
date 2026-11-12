"""The end of the work day by clock (issue #265): wrap-up, then end of day, reusing what careerbrew has.

- Wrap-up (`tick`): from `work_end` minus `work_wrap_minutes` (a setting), the first watch pass of the
  day (`careerbrew day loop pass`) switches to wrap-up once: it writes tomorrow's handoff and pops up
  one reminder (`notify.remind`, which keeps the opt-in rules). The loops keep running.
- End of day (`finish`): `careerbrew day end`, or by itself on the first pass at or after `work_end`.
  It saves the day log, reads careerbrew's PM draft (`day log`; the owner's edit wins), writes the
  handoff again, stops the loops (`watch.end`), and renders the day's page. It writes no report of
  its own: the PM draft is the day's update, edited and sent from the Dashboard.
- The handoff (`write_handoff`): each PR a row links or a step handled (section `prs`, with a
  recheck that fails once the PR merged or closed) and each open question record (`questions`).
  Promises show live (`handoff.with_promises`), and items an agent added stay.
- The next morning (`morning`): `careerbrew day start` reruns each handoff item's recheck, closes
  what is done (a question record closes in its own file), drops it from the handoff, and keeps
  the result in the start record.

Privacy: the day file gets `wrapup` and `end`, and the start record `handoff`: times, counts, ids,
sections, states, and reasons, never item text. Item text and the PM draft are business data and
print only to the owner. Events carry counts.

Lifecycle: no process memory. Each record is read, changed, and written under the day file's lock
(`.work-<day>.json.lock`, shared with `status` and `questions`), and every other key is kept.
"""

from __future__ import annotations

import fcntl
import re
import shlex
from datetime import date, datetime, time, timedelta
from typing import Any

from .. import events, store

OWNER = "end-of-day"
DASHBOARD = "careerbrew open --view dashboard"
COMMAND = "careerbrew day end"
PR_URL = re.compile(r"^https?://[^/\s]+/([\w.-]+/[\w.-]+)/pull/(\d+)(?:[/?#]\S*)?$")
PR_ITEM = re.compile(r"^pr:([\w.-]+/[\w.-]+)#(\d+)$")
# A PR's recheck exits 1 only on a MERGED or CLOSED answer: a gh that is missing, signed out, or offline keeps it open.
OPEN_PR = 's=$(gh pr view "$0" --json state -q .state) || exit 0; case "$s" in MERGED|CLOSED) exit 1 ;; esac'
OPEN_PR_IN = 's=$(gh pr view "$0" --repo "$1" --json state -q .state) || exit 0; case "$s" in MERGED|CLOSED) exit 1 ;; esac'


def _now() -> datetime:
    from ..calendar import now

    return now()


def _stamp(at: datetime) -> str:
    return at.isoformat(timespec="minutes")


def _window(at: datetime, control: dict[str, Any]) -> tuple[datetime, datetime]:
    from .. import config

    end_at = datetime.combine(at.date(), time.fromisoformat(str(config.get("work_end"))), at.tzinfo)
    chosen = control.get("at")
    if chosen:
        wrap_at = datetime.combine(at.date(), time.fromisoformat(chosen), at.tzinfo)
        return wrap_at, wrap_at + timedelta(minutes=int(config.get("work_wrap_minutes")))
    return end_at - timedelta(minutes=int(config.get("work_wrap_minutes"))), end_at


def times(at: datetime | None = None) -> tuple[datetime, datetime]:
    """(wrap-up start, work end) for the day of `at`, in `at`'s time zone."""
    at = at or _now()
    return _window(at, _data(at.date()).get("wrap_control") or {})


def control(at: str = "", hold: bool = False) -> dict[str, Any]:
    """Move or hold only today's wrap-up. Explicit owner `day end` overrides it."""
    from .. import config
    from .status import path_for

    if at:
        if not re.fullmatch(r"(?:[01]\d|2[0-3]):[0-5]\d", at):
            raise ValueError("--at must be a local time HH:MM, from 00:00 to 23:59")
        chosen = time.fromisoformat(at)
        if chosen.hour * 60 + chosen.minute + int(config.get("work_wrap_minutes")) >= 24 * 60:
            raise ValueError("wrap-up and clock ending belong to today's record: choose a same-day wrap time or hold, then use explicit careerbrew day end")
    if at or hold:
        day = _now().date()
        path = path_for(day)
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.with_name(f".{path.name}.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            data = _data(day)
            if data.get("end"):
                raise ValueError("the day already ended; start the next work day instead")
            prior = data.get("wrap_control") or {}
            data["wrap_control"] = {"at": at or prior.get("at", ""), "hold": hold, "changed_at": _stamp(_now())}
            data.pop("wrapup", None)
            store.write_json(path, {"rows": [], "handled": {}, **data, "day": day.isoformat()})
        events.emit("workday.end", status="held" if hold else "moved", action="control", by_clock=False)
    return view()


def _data(day: date) -> dict[str, Any]:
    from .status import _data as status_data

    return status_data(day)


def _put(day: date, key: str, value: dict[str, Any], only_if_missing: bool = False, respect_hold: bool = False, clock_at: datetime | None = None) -> bool:
    """Write one key of the day file under its lock; every other key stays. False when `only_if_missing` and it is there."""
    from .status import path_for

    path = path_for(day)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.with_name(f".{path.name}.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        data = _data(day)
        if respect_hold and (data.get("wrap_control") or {}).get("hold"):
            return False
        if clock_at is not None:
            wrap_at, end_at = _window(clock_at, data.get("wrap_control") or {})
            if clock_at < (end_at if key == "end" else wrap_at):
                return False
        if only_if_missing and data.get(key):
            return False
        store.write_json(path, {"rows": [], "handled": {}, **data, "day": day.isoformat(), key: {**(data.get(key) or {}), **value}})
    return True


def phase(at: datetime | None = None) -> str:
    """`work`, `wrap-up`, or `ended` for the day of `at`."""
    at = at or _now()
    data = _data(at.date())
    if data.get("end"):
        return "ended"
    if (data.get("wrap_control") or {}).get("hold"):
        return "held"
    return "wrap-up" if data.get("wrapup") or at >= times(at)[0] else "work"


def view(at: datetime | None = None) -> dict[str, Any]:
    """The plan's `wrap_up`: when wrap-up starts, the phase now, and the end-of-day command. Changes nothing."""
    at = at or _now()
    return {"at": _stamp(times(at)[0]), "phase": phase(at), "command": COMMAND, "hold": bool((_data(at.date()).get("wrap_control") or {}).get("hold")), "end_at": _stamp(times(at)[1])}


# ---------- the handoff ----------


def _prs(day: date) -> list[dict[str, Any]]:
    """Each PR a row links or a step handled today, once, with a recheck that exits 0 only while it is open."""
    from .status import handled, stored

    found: dict[tuple[str, str], dict[str, Any]] = {}
    for row in stored(day).values():
        m = PR_URL.match(row.get("link") or "")
        if m:
            key = (m[1].lower(), m[2])
            found[key] = {"text": f"PR {m[1]}#{m[2]} from {row['step']}", "link": row["link"], "recheck": shlex.join(["sh", "-c", OPEN_PR, row["link"]])}
    for name, step in handled(day).items():
        m = PR_ITEM.match(name)
        if m and (m[1].lower(), m[2]) not in found:
            found[(m[1].lower(), m[2])] = {"text": f"PR {m[1]}#{m[2]} from {step}", "link": "", "recheck": shlex.join(["sh", "-c", OPEN_PR_IN, m[2], m[1]])}
    return list(found.values())


def write_handoff(day: date | None = None) -> dict[str, int]:
    """Write today's end-of-day handoff items (open PRs, open question records); returns counts by section."""
    from ..day import handoff
    from . import questions

    day = day or _now().date()
    items = [handoff.item("prs", p["text"], p["link"], recheck=p["recheck"], source="careerbrew") for p in _prs(day)]
    for q in questions.open_records(day):
        items.append(handoff.item("questions", q["text"], recheck=q.get("recheck", ""), source=q.get("source", ""), id=q["id"], question=q["id"]))
    handoff.replace_owned(OWNER, items, day)
    every = handoff.load(day)
    return {"prs": sum(1 for i in items if i["section"] == "prs"), "questions": sum(1 for i in items if i["section"] == "questions"), "other": sum(1 for i in every if i.get("owner") != OWNER)}


# ---------- wrap-up and end of day ----------


def _remind(end_at: datetime) -> str:
    from .. import notify

    return notify.remind("Work day: wrap-up", f"The work day ends at {end_at.strftime('%H:%M')}. Tomorrow's handoff is written. Next: careerbrew day end")


def tick(at: datetime | None = None, auto_end: bool = True) -> dict[str, Any]:
    """The clock check each watch pass makes: start wrap-up once (handoff and one reminder), and end the day at `work_end`."""
    at = at or _now()
    day = at.date()
    wrap_at, end_at = times(at)
    out: dict[str, Any] = {"phase": "work", "wrap_at": _stamp(wrap_at), "end_at": _stamp(end_at), "started": False, "reminded": "", "ended": False}
    if _data(day).get("end"):
        return {**out, "phase": "ended"}
    if (_data(day).get("wrap_control") or {}).get("hold"):
        return {**out, "phase": "held"}
    if at < wrap_at:
        return out
    out["phase"] = "wrap-up"
    if _put(day, "wrapup", {"at": _stamp(at), "reminded": ""}, only_if_missing=True, respect_hold=True, clock_at=at):  # the lock makes this once a day
        from ..work import sessions

        out["sessions"] = sessions.request_wrap(request_id=_stamp(wrap_at))
        counts = write_handoff(day)
        out.update(started=True, reminded=_remind(end_at), handoff=counts)
        _put(day, "wrapup", {"reminded": out["reminded"], "handoff": counts}, respect_hold=True, clock_at=at)
        events.emit("workday.end", status="wrap-up", action="remind", reminded=out["reminded"] == "sent", prs=counts["prs"], questions=counts["questions"])
    wrap_at, end_at = times(at)
    if auto_end and at >= end_at:
        result = finish(by="clock", at=at)
        if result.get("phase") not in ("held", "work", "wrap-up"):
            out.update(phase="ended", ended=True)
    out.update(phase=phase(at), wrap_at=_stamp(wrap_at), end_at=_stamp(end_at))
    return out


def finish(by: str = "owner", at: datetime | None = None) -> dict[str, Any]:
    """End of day, once: it claims `end` first, so a second call (the owner and the clock at once) returns the first's record.
    Then the day log saved, careerbrew's PM draft read, the handoff written, the loops stopped, the page rendered."""
    from .. import report, runs
    from ..day import log as daylog
    from . import plan, watch

    at = at or _now()
    day = at.date()
    if by == "clock" and (_data(day).get("wrap_control") or {}).get("hold"):
        return {"phase": "held"}
    if by == "clock" and at < times(at)[1]:
        return {"phase": phase(at)}
    if not _put(day, "end", {"at": _stamp(at), "by": by}, only_if_missing=True, respect_hold=by == "clock", clock_at=at if by == "clock" else None):  # the lock makes this once a day
        done = _data(day).get("end")
        if not done:
            return {"phase": phase(at)}
        return {
            **done,
            "already": True,
            "pm": {**done.get("pm", {}), "subject": "", "body": ""},
            "handoff": done.get("handoff") or {"prs": 0, "questions": 0, "other": 0},
            "loops": {"stopped": [], "running": watch.status()["running"]},
            "dashboard": DASHBOARD,
        }
    counts = write_handoff(day)
    from ..work import sessions

    sessions.request_wrap()
    _put(day, "wrapup", {"at": _stamp(at), "reminded": "not needed", "handoff": counts}, only_if_missing=True)
    pm = daylog.pm_report(daylog.save(day))
    loops = watch.end()
    page = report.write(runs.work_day(plan.build(day)))
    body = pm["body"]
    rec = {
        "at": _stamp(at),
        "by": by,
        "pm": {"edited": pm["edited"], "lines": sum(1 for x in body.splitlines() if x.strip()), "checks": len(pm["checks"]), "unlinked": len(daylog.unlinked_refs(body)), "updated_at": pm["updated_at"]},
        "handoff": counts,
        "loops": {"stopped": len(loops["stopped"]), "running": len(loops["running"])},
        "page": str(page or ""),
    }
    _put(day, "end", rec)
    events.emit("workday.end", status="ended", action="end", by_clock=by == "clock", prs=counts["prs"], questions=counts["questions"], loops_left=len(loops["running"]), pm_edited=pm["edited"])
    return {**rec, "pm": {**rec["pm"], "subject": pm["subject"], "body": body}, "loops": loops, "dashboard": DASHBOARD}


# ---------- the next morning ----------


def last_handoff_day(day: date) -> date | None:
    """The newest handoff file before `day`, even when its items were all dropped: a second morning check never
    falls back to an older day's handoff."""
    from ..day import handoff

    days = (date.fromisoformat(p.stem.removeprefix("handoff-")) for p in handoff.path_for(day).parent.glob("handoff-*.json"))
    return max((d for d in days if d < day), default=None)


def morning(day: date | None = None) -> dict[str, Any]:
    """Recheck each item of the last handoff: `open`, `closed` (dropped, and a question closed), `agent`, or `unknown`."""
    from ..day import handoff
    from . import questions

    day = day or _now().date()
    h_day = last_handoff_day(day)
    if not h_day:
        return {"from": "", "items": []}
    items = handoff.load(h_day)
    live = {q["id"]: q for q in questions.open_records(day)}
    out = []
    for i in items:
        if i["section"] == "questions":
            q = live.get(i.get("question") or i["id"])
            if q is None:
                state, why = "closed", "the question was closed since"
            else:
                state, why = questions.check_one(q)
                if state == "closed":
                    questions.close(q["id"], why, date.fromisoformat(q["day"]))
        elif i.get("recheck"):
            state, why = questions.check_one({"recheck": i["recheck"], "source": i.get("source") or "agent"})
        else:
            state, why = "open", "no recheck: verify it live before acting"
        if state == "closed":
            handoff.done(i["id"], h_day)
        out.append({"id": i["id"], "section": i["section"], "state": state, "why": why})
    events.emit("workday.morning", items=len(out), closed=sum(1 for r in out if r["state"] == "closed"), open=sum(1 for r in out if r["state"] == "open"))
    return {"from": h_day.isoformat(), "items": out}
