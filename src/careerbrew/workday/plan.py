"""The day plan `careerbrew day run --json` prints for `/work`: steps, rules, and the dashboard rows.

It reads local state only (settings, the plugin lock, the machine role, today's rows, each step's
git commit) and changes nothing; `start_of_day` says what `careerbrew day start` refreshes (issue #260), and `question_records` lists the open question records (issue #261). Its text fields are the agent's instructions; `commands/work.md` adds only what they leave out.
"""

from __future__ import annotations

from datetime import date
from typing import Any

from . import end, gate, questions, stages, start, status, steps, timing

WEEKDAYS = ("Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun")
MONTHS = ("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")
DRAFTS = "Every public issue, PR, message, or comment stays a draft until the user says yes; show the exact text first."
STOP_ONLY_FOR = (
    "A merge conflict you cannot resolve.",
    "A destructive or public action that needs approval.",
    "A choice only the user can make.",
)
FLOW = (
    "First run `start_of_day` (`careerbrew day start`). Run /careerbrew inline, then run start_of_day.operator_turn quick dispatchers in readiness order in the main session, in the owner's initial turn, before waiting on background results. If an initial-turn dispatcher requires sign-in, mark it blocked with the existing sign-in ask and continue independent work; do not run it later from a background-started turn. "
    "Then run `careerbrew day check --json` and start each step when its ready list names it; recheck whenever a step finishes or becomes blocked. "
    "stages are only an earliest-start view, never a barrier. ready_when explains prerequisites and resources. Long-running steps start when their own dependencies and resources are ready. "
    "Use only the host's exposed controls; if it cannot dispatch in this turn, record the blocker rather than claiming it started. Never stop to ask continue."
)

TRUTH = (
    "Mark a row running when its step starts and done, blocked, or failed when it ends. A step with `verify` is done only when its verify command passes: "
    "`careerbrew day update <step> --status done` runs it and marks the row failed when it exits non-zero. "
    "blocked means the step waits on the user: pass --needs-you with the question. failed means something broke: pass --error-class "
    + "|".join(gate.ERROR_CLASSES)
    + ". Each such row's `next` is its next action. Before each dispatch and on every watch pass, run `careerbrew day check`: a running row past its `timeout` minutes "
    "goes back to waiting for one retry (run the step again), and is marked failed after the second timeout."
)
HANDLED = (
    "One step owns a checkout or a PR per day. Before a step works on one, skip it when `handled` lists it. After the step, record what it handled: `careerbrew day update <step> --handled checkout:<path> --handled pr:<repo>#<number>`."
)
QUESTIONS = (
    "A step never asks the user itself: it returns each question as a record and goes on. "
    'Save it with `careerbrew day question add <step> "<the ask>" --option <a> --option <b> --pick <your pick> --recheck <check>`, '
    "where the recheck says whether the item is still open: a command (exit 0 means still open; run without a shell, its output never read) or a link you check yourself. "
    "Put any follow-up goal text in the ask. Ask them all once, after the last step: first run `careerbrew day question recheck --json`, which runs every recheck, "
    "drops the closed items, and lists the rest with their age. Check each `agent` item's link and close a done one (`careerbrew day question close <id>`). "
    "The sign-in ask from `careerbrew day start` is the one exception: ask it right away. "
    "In one batched ask, say for each item what it is, how old it is, what each option does, and your pick; close each item the user answers."
)
WATCH = (
    "After the steps, keep one watch loop: run `careerbrew day loop pass` and re-arm it at the `next pass at` time it prints (or keep `careerbrew day loop run` going in its own session); "
    "it runs the live feed and each step's `cycle` when its `watch` inputs change, so never start a step's own watch phase or a second timer. "
    "Record every cron, monitor, background agent, or session you start: `careerbrew day loop add --kind <kind> --id <id> --step <step>`. "
    "Run a long-running step in its own session, and any other step's long work in a background agent that returns only a short report."
)
END_OF_DAY = (
    "Wrap-up starts by clock (`wrap_up.at`: work_end minus work_wrap_minutes): the watch pass writes tomorrow's handoff and reminds the user once; keep the loop running.",
    "Add what only you know to the handoff: `careerbrew day handoff add <section> <text>` for each wait, lesson, and gotcha. Open PRs and open question records go in by themselves, each with its recheck.",
    "End the day with `careerbrew day end` (the first pass after work_end runs it by itself): it reads careerbrew's PM draft, writes the handoff, stops careerbrew's loops, and renders the page. Show its output; write no report of your own.",
    "Stop every loop it lists with its own tool, then `careerbrew day loop stop --id <id>`, until `careerbrew day loop end` exits 0.",
)


def title(day: date) -> str:
    return f"Work day {WEEKDAYS[day.weekday()]} {MONTHS[day.month - 1]} {day.day}"


def machine_rule() -> tuple[str, str]:
    from .. import machine

    role = machine.role()
    if role == "corporate":
        return role, f"Corporate machine: company content goes only to the company plugin, never to a public repo; never push from this machine. {DRAFTS}"
    return role, f"Personal machine: no company content on this machine. {DRAFTS}"


def build(day: date | None = None, *, lookup: start.CheckoutLookup | None = None) -> dict[str, Any]:
    if lookup is None:
        lookup = {}
    day = day or status.today()
    found = steps.declared()
    record = status.start_record(day)
    open_questions = questions.open_records(day)
    sources = start._plugin_sources()
    for s in found:  # each step's resolved source checkout and its commit (issue #260)
        top = start.step_source(s, sources, lookup=lookup)
        s["source"] = {"path": start.shown(top), "commit": start.commit(top)} if top else {"path": None, "commit": ""}
    declared, run_stages = stages.build(found, late=start.late(found, record))
    names = [s["step"] for s in declared]
    from ..calendar import now
    from ..day import pm_promises
    from ..work import sessions

    limits, at = {s["step"]: s["timeout"] for s in declared if not s["long_running"]}, now()  # a long-running step never times out
    rows = [{**r, "overdue": r["step"] in limits and gate.overdue(r, limits[r["step"]], at)} for r in status.rows(names, day)]
    slow = timing.flags(rows, day)  # #263: each row's median and slow flag
    rows = [{**r, **slow[r["step"]]} for r in rows]
    role, rule = machine_rule()
    return {
        "start_of_day": {
            **start.view(found, record, open_questions, lookup=lookup),
            "operator_turn": [s["step"] for s in declared if s["operator_turn"]],
            "operator_turn_blocked": [s["step"] for s in declared if s["ready_when"]["signin_required"]],
        },
        "day": day.isoformat(),
        "machine": role,
        "rule": rule,
        "steps": declared,
        "about_missing": steps.missing_about(declared),  # issue #311: steps with no one-line `about`
        "stages": run_stages,
        "flow": FLOW,
        "truth": TRUTH,
        "handled_rule": HANDLED,
        "handled": status.handled(day),
        "stop_only_for": list(STOP_ONLY_FOR),
        "questions": QUESTIONS,
        "question_records": open_questions,
        "watch": WATCH,
        "end_of_day": list(END_OF_DAY),
        "wrap_up": end.view(),
        "pm_promises": pm_promises.view(day),
        "sessions": sessions.CONTRACT,  # one long-lived session per project, through the store (issue #274)
        "dashboard": {
            "title": title(day),
            "statuses": list(status.STATUSES),
            "counts": status.counts(rows),
            "rows": rows,
            "update": 'careerbrew day update <step> --status <status> [--summary "<one line>"] [--needs-you "<ask>"] [--error-class <class>] [--link <url>]',
            "check": "careerbrew day check",
            "render": "careerbrew day render",
        },
    }
