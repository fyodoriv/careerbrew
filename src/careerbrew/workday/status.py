"""The day's status rows: one per plan step, in `<data root>/day/work-<day>.json` (encrypted at rest).

A row is `{step, status, summary, needs_you, link, at}`. `summary`, `needs_you`, and `link` may hold
business text, so they print to the owner's terminal and the page only.

Issue #259 adds the truth fields, all content-free: `started` and `attempt` (written when the step
starts, so a crash leaves a `running` row the next pass flags), `retry` (set back to waiting after a
timeout), `timed_out`, `verify` (`pass`, `fail: <why>`, or empty), `error_class` (a failed row's
class), and `next` (the generic next action of a blocked or failed row). Issue #263 adds `ended` and `seconds` (the step's
duration), set when the status changes to done, blocked, or failed (`timing.py`), and keeps each step's
final status and duration per day in a history file. A `blocked` row must carry
its question (`needs_you`); a `failed` row must carry an error class (`gate.py`). Every update reads, changes,
and writes the file under its lock (`.work-<day>.json.lock`), so steps that run in parallel
(one subagent each) never lose a row.

The file also keeps `start` (issue #260): today's `careerbrew day start` record (each checkout's
refresh status and commits, the shell checks, and the sign-in probe; its one question record is in `questions`).
It holds paths, commits, status words, and step names only.

The file also keeps `questions` (issue #261): the day's question records (`questions.py`), which
hold business text. Every writer here keeps the keys it does not own (`{**_data(day), ...}`).

The file also keeps `handled` (issue #258): `{item: step}` for each checkout or PR a step already
handled today (`checkout:<path>`, `pr:<repo>#<n>`). One step owns an item per day: a later step skips
it, and a second step that claims it is refused. Items are ids and paths, never message text.
"""

from __future__ import annotations

import fcntl
import re
from datetime import date
from pathlib import Path
from typing import Any

from .. import events, paths, store
from .gate import ERROR_CLASSES, MAX_ATTEMPTS, RETRY, next_action
from .steps import normalize
from .timing import close, record

STATUSES = ("waiting", "running", "done", "blocked", "failed")
MAX_TEXT = 200
HANDLED = re.compile(r"^(?:checkout|pr):\S{1,200}$")


def today() -> date:
    from ..calendar import now

    return now().date()


def path_for(day: date) -> Path:
    return paths.sub("day", f"work-{day.isoformat()}.json")


def _data(day: date | None) -> dict[str, Any]:
    data = store.read_json(path_for(day or today()), {}) or {}
    return data if isinstance(data, dict) else {}


def blank(step: str) -> dict[str, Any]:
    return {"step": step, "status": "waiting", "summary": "", "needs_you": "", "link": "", "at": "", "started": "", "attempt": 0, "retry": False, "timed_out": False, "verify": "", "error_class": "", "next": "", "ended": "", "seconds": None}


def stored(day: date | None = None) -> dict[str, dict[str, Any]]:
    """The day's rows by step, each with every field (a row written before #259 reads its new fields as empty)."""
    return {r["step"]: {**blank(r["step"]), **r} for r in _data(day).get("rows", []) if isinstance(r, dict) and r.get("step")}


def handled(day: date | None = None) -> dict[str, str]:
    """`{item: step}`: the checkouts and PRs steps already handled today; a later step skips them."""
    got = _data(day).get("handled", {})
    return {k: v for k, v in got.items() if isinstance(k, str) and isinstance(v, str)} if isinstance(got, dict) else {}


def rows(steps: list[str], day: date | None = None) -> list[dict[str, Any]]:
    """A row per plan step, in plan order: the stored row, or a new waiting one."""
    have = stored(day)
    return [have.get(s) or blank(s) for s in steps]


def _text(field: str, value: str) -> str:
    value = " ".join(str(value).split())
    if len(value) > MAX_TEXT:
        raise ValueError(f"keep --{field.replace('_', '-')} short: {len(value)} characters, at most {MAX_TEXT} (put the detail behind --link)")
    return value


def _write(day: date, change) -> dict[str, Any]:
    """Read, change, and write the day's file under its lock; `change(rows, owners)` returns the changed row or raises."""
    path = path_for(day)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.with_name(f".{path.name}.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        have, owners = stored(day), handled(day)
        row = change(have, owners)
        store.write_json(path, {**_data(day), "day": day.isoformat(), "rows": list(have.values()), "handled": owners})
    return row


def _write_if(day: date, change) -> dict[str, Any] | None:
    """Like `_write`, but `change` may return None: then the file is left as it was."""
    path = path_for(day)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.with_name(f".{path.name}.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        have, owners = stored(day), handled(day)
        row = change(have, owners)
        if row is not None:
            store.write_json(path, {**_data(day), "day": day.isoformat(), "rows": list(have.values()), "handled": owners})
    return row


def start_record(day: date | None = None) -> dict[str, Any]:
    """Today's `careerbrew day start` record (issue #260), or {} before it ran."""
    got = _data(day).get("start", {})
    return got if isinstance(got, dict) else {}


def save_start(record: dict[str, Any], day: date | None = None) -> None:
    """Replace today's start record under the file's lock; the rows and `handled` stay as they are."""
    day = day or today()
    path = path_for(day)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.with_name(f".{path.name}.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        store.write_json(path, {"rows": [], "handled": {}, **_data(day), "day": day.isoformat(), "start": record})


def update(
    step: str,
    plan_steps: list[str],
    status: str | None = None,
    summary: str | None = None,
    needs_you: str | None = None,
    link: str | None = None,
    day: date | None = None,
    handled_items: list[str] | None = None,
    error_class: str | None = None,
    verify: str | None = None,
) -> dict[str, Any]:
    """Change one row; a field left as None keeps its value. Refuses a step that is not in today's plan.

    `handled_items` adds checkouts or PRs this step handled; one another step already handled is refused.
    `error_class` goes with a failed row only; `verify` records the verify result (`pass` or `fail: <why>`).
    A blocked row without a question (`needs_you`), or a failed row without an error class, is refused."""
    from ..calendar import now

    step = normalize(step)
    if step not in plan_steps:
        raise ValueError(f"step {step} is not in today's plan (steps: {', '.join(plan_steps)}; see `careerbrew day run`)")
    if status is not None and status not in STATUSES:
        raise ValueError(f"unknown status {status!r}; use one of: {', '.join(STATUSES)}")
    if error_class is not None and error_class not in ERROR_CLASSES:
        raise ValueError(f"unknown error class {error_class!r}; use one of: {', '.join(ERROR_CLASSES)}")
    if link and not link.startswith(("https://", "http://")):
        raise ValueError("--link must be a full http(s):// URL, or empty")
    for item in handled_items or []:
        if not HANDLED.match(item):
            raise ValueError(f"--handled {item[:40]!r}: use checkout:<path> or pr:<repo>#<number> (no spaces)")

    def change(have: dict[str, dict[str, Any]], owners: dict[str, str]) -> dict[str, Any]:
        for item in handled_items or []:
            if owners.get(item, step) != step:
                raise ValueError(f"{item} was already handled today by {owners[item]}: skip it (one step owns a checkout or PR per day)")
            owners[item] = step
        row = dict(have.get(step) or blank(step))
        prev = row["status"]
        for key, value in (("status", status), ("summary", summary), ("needs_you", needs_you), ("link", link)):
            if value is not None:
                row[key] = value if key in ("status", "link") else _text(key, value)
        stamp = now().isoformat(timespec="seconds")
        if status == "running" and prev != "running":  # the step starts: a crash from here leaves this row running
            row.update(attempt=int(row.get("attempt") or 0) + 1 if row.get("retry") else 1, started=stamp, retry=False, timed_out=False, verify="")
        close(row, prev, stamp)  # #263: the end stamp and the duration
        if error_class is not None:
            if row["status"] != "failed":
                raise ValueError("--error-class goes only with --status failed: a failed row says what broke (blocked rows wait on you instead)")
            row["error_class"] = error_class
        if row["status"] != "failed":
            row["error_class"] = ""
        if verify is not None:
            row["verify"] = verify
        if status is not None or needs_you is not None or error_class is not None:
            if row["status"] == "blocked" and not row["needs_you"]:
                raise ValueError('blocked needs a question record: pass --needs-you "<the ask>" (blocked waits on you; when something broke, use --status failed --error-class <class>)')
            if row["status"] == "failed" and not row["error_class"]:
                raise ValueError(f"failed needs an error class: pass --error-class {'|'.join(ERROR_CLASSES)} (failed means something broke; when the step waits on you, use --status blocked --needs-you)")
        if not (row["status"] == "waiting" and row.get("retry")):
            row["next"] = next_action(step, row["status"], row["error_class"])
        row["at"] = stamp
        have[step] = row
        record(day or today(), row)  # under the day file's lock, so the history ends on the row's own last status
        return row

    row = _write(day or today(), change)
    events.emit("workday.update", status=row["status"], needs_you=bool(row["needs_you"]), error_class=row["error_class"])
    return row


def timed_out(step: str, plan_steps: list[str], started: str, day: date | None = None) -> dict[str, Any] | None:
    """A running step went past its timeout: back to waiting for one retry, or failed (class `tool`) after the last try.

    `started` is the start the caller judged; when the row has started again since (or finished), nothing changes and
    this returns None. The retry-or-fail choice reads the locked row's `attempt`."""
    from ..calendar import now

    if step not in plan_steps:
        raise ValueError(f"step {step} is not in today's plan")

    def change(have: dict[str, dict[str, Any]], _owners: dict[str, str]) -> dict[str, Any] | None:
        row = dict(have.get(step) or blank(step))
        if row["status"] != "running" or row.get("started", "") != started:
            return None  # it finished or started again while the check ran
        if int(row.get("attempt") or 1) < MAX_ATTEMPTS:
            row.update(status="waiting", retry=True, next=RETRY.format(step=step, n=int(row.get("attempt") or 1) + 1, max=MAX_ATTEMPTS))
        else:
            row.update(status="failed", error_class="tool", timed_out=True, retry=False, next=next_action(step, "failed", "tool"))
        row["at"] = now().isoformat(timespec="seconds")
        close(row, "running", row["at"])
        have[step] = row
        record(day or today(), row)
        return row

    row = _write_if(day or today(), change)
    if row is not None:
        events.emit("workday.timeout", retry=row["status"] == "waiting", status=row["status"])
    return row


def counts(rows_: list[dict[str, Any]]) -> dict[str, int]:
    out = {s: sum(1 for r in rows_ if r["status"] == s) for s in STATUSES}
    out["needs_you"] = sum(1 for r in rows_ if r.get("needs_you"))
    return out
