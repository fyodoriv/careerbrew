"""The work day's question records (issue #261): every ask is data, rechecked before the one batched ask.

A record is `{id, step, text, options, pick, createdAt, recheck}`, plus `source` (who wrote the
recheck), `steps` (the steps it holds up, when more than its own), and `closed` / `closedAt` (why
and when it closed). It lives in the day's status file, `day/work-<day>.json` under `questions`
(encrypted at rest through `store`), next to the rows, so the handoff and the next morning can
recheck it: `open_records()` reads today's file and the earlier days' (up to `CARRY_DAYS` back).
A record closes in the file it lives in; nothing is copied into a later day.

The step contract: a step never asks the user itself. It returns each question as a record
(`careerbrew day question add`), and after the last step `careerbrew day question recheck` runs
every recheck, drops the closed items, and lists the rest with their age for one batched ask.

Where records come from: a step (`source` `agent`, or `plugin:<name>` for a plugin's step), a
blocked or needs-you row (`row`, one per step, id `b-...`; clearing the row's ask closes it), and
`careerbrew day start` (the day's one sign-in record, id `signin`, `careerbrew`).

`recheck` says whether the item is still open:
- empty: it stays open until someone closes it (`careerbrew day question close <id>`).
- `https://...` (or `http://`): careerbrew never fetches it. The ask marks it `agent`: an agent checks the link
  and closes the item when it is done.
- `careerbrew:signin`: the `work_signin_probe` setting; signed in closes the item.
- anything else: a one-line command, split like a shell line and run without a shell, its output
  never read (`gate.run_verify`). Exit 0 means still open; any other exit code means closed. A
  command from a plugin runs only when the owner added the plugin with `--allow-exec`; otherwise
  the item stays in the ask as `unknown`, with that fix.

Privacy: `text`, `options`, `pick`, and `recheck` are business data (a command or a link may name
a company host). `public()` is the content-free view for an agent without the policy, and events
carry counts only.

Lifecycle: no process memory; each call reads and writes the day file under its lock
(`.work-<day>.json.lock`, shared with `status`).
"""

from __future__ import annotations

import fcntl
import hashlib
import shlex
from collections.abc import Callable
from datetime import date, datetime, timedelta
from typing import Any

from .. import events, store

SIGNIN = "careerbrew:signin"
REVIEW_PR = "careerbrew:review-pr"
CARRY_DAYS = 14
MAX_TEXT = 200
MAX_OPTION = 80
MAX_OPTIONS = 6
MAX_RECHECK = 500
SOURCES = ("agent", "owner", "row", "careerbrew")
PUBLIC = ("id", "step", "steps", "source", "createdAt", "day", "closed", "closedAt")


def _now() -> datetime:
    from ..calendar import now

    return now()


def _today() -> date:
    return _now().date()


def kind(recheck: str) -> str:
    """The kind of recheck, without its private input."""
    if not recheck:
        return "none"
    if recheck.startswith(("https://", "http://")):
        return "url"
    if recheck.startswith(REVIEW_PR):
        return "review-pr"
    return "signin" if recheck == SIGNIN else "command"


def qid(step: str, text: str) -> str:
    return "q-" + hashlib.sha256(f"{step}\n{text}".encode()).hexdigest()[:8]


def row_id(step: str) -> str:
    return "b-" + hashlib.sha256(step.encode()).hexdigest()[:8]


def age(created: str, at: datetime | None = None) -> str:
    """How long ago a record was raised: `N min`, `N h` (under 2 days), or `N d`."""
    at = at or _now()
    try:
        then = datetime.fromisoformat(created)
    except (TypeError, ValueError):
        return "unknown"
    if then.tzinfo is None:
        then = then.replace(tzinfo=at.tzinfo)
    mins = max(0, int((at - then).total_seconds() // 60))
    if mins < 60:
        return f"{mins} min"
    if mins < 48 * 60:
        return f"{mins // 60} h"
    return f"{mins // (24 * 60)} d"


def _line(field: str, value: str, limit: int) -> str:
    value = " ".join(str(value).split())
    if len(value) > limit:
        raise ValueError(f"keep {field} short: {len(value)} characters, at most {limit}")
    return value


def check_recheck(v: str) -> str:
    """Validate a link, a typed recheck or one command line."""
    v = (v or "").strip()
    if not v or kind(v) in ("url", "signin"):
        if any(c in v for c in "\n\r\0 "):
            raise ValueError("--recheck: a link has no spaces or line breaks")
        return v
    if len(v) > MAX_RECHECK or any(c in v for c in "\n\r\0"):
        raise ValueError(f"--recheck: use one line of at most {MAX_RECHECK} characters")
    if kind(v) == "review-pr":
        _review_pr_url(v)
        return v
    try:
        if not shlex.split(v):
            return ""
    except ValueError:
        raise ValueError("--recheck: the command has an unclosed quote") from None
    return v


def _review_pr_url(value: str) -> str:
    from ..work import review_ask

    prefix, separator, url = value.partition(" ")
    if prefix != REVIEW_PR or not separator:
        raise ValueError("--recheck: use careerbrew:review-pr followed by one HTTPS PR link")
    review_ask.recheck_pr_link(url)
    return url


def record(step: str, text: str, options: list[str] | None = None, pick: str = "", recheck: str = "", source: str = "agent", ident: str = "", steps: list[str] | None = None) -> dict[str, Any]:
    """A new, checked record (not saved)."""
    text = _line("the question", text, MAX_TEXT)
    if not text:
        raise ValueError("the question text is empty")
    opts = [_line("an --option", o, MAX_OPTION) for o in options or []]
    opts = [o for o in dict.fromkeys(opts) if o]
    if len(opts) > MAX_OPTIONS:
        raise ValueError(f"at most {MAX_OPTIONS} --option values")
    pick = _line("--pick", pick, MAX_OPTION)
    if pick and opts and pick not in opts:
        raise ValueError("--pick must be one of the --option values")
    if source not in SOURCES and not (source.startswith("plugin:") and len(source) > 7):
        raise ValueError(f"unknown source {source!r}")
    out: dict[str, Any] = {
        "id": ident or qid(step, text),
        "step": step,
        "text": text,
        "options": opts,
        "pick": pick,
        "createdAt": _now().isoformat(timespec="seconds"),
        "recheck": check_recheck(recheck),
        "source": source,
        "closed": "",
        "closedAt": "",
    }
    if steps:
        out["steps"] = list(steps)
    return out


def _data(day: date) -> dict[str, Any]:
    from .status import _data as status_data

    return status_data(day)


def records(day: date | None = None) -> list[dict[str, Any]]:
    """Every record in one day's file, open and closed."""
    got = _data(day or _today()).get("questions", [])
    return [q for q in got if isinstance(q, dict) and q.get("id")] if isinstance(got, list) else []


def _change(day: date, fn: Callable[[list[dict[str, Any]]], Any]) -> Any:
    """Read, change, and write one day's records under the day file's lock; every other key is kept."""
    from .status import path_for

    path = path_for(day)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.with_name(f".{path.name}.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        have = records(day)
        before = [dict(q) for q in have]
        out = fn(have)
        if have != before:
            store.write_json(path, {"rows": [], "handled": {}, **_data(day), "day": day.isoformat(), "questions": have})
    return out


def _find(ident: str, day: date) -> tuple[date, dict[str, Any]] | None:
    """The newest record with this id in the day or a carried earlier day, and the day it lives in."""
    for d in _days(day):
        for q in reversed(records(d)):
            if q["id"] == ident:
                return d, q
    return None


def save(rec: dict[str, Any], day: date | None = None, reopen: bool = False) -> dict[str, Any]:
    """Add a record, or update the open one with its id wherever it lives (it keeps its first `createdAt`, so its age stays true).

    A closed record of the same day with the same id and text stays closed: an answered ask is not asked again because a
    step or a row wrote it once more that day. On a later day, with new text, or with `reopen` (the sign-in probe failed
    again, a row was marked blocked again) it opens again as a new record today."""
    day = day or _today()
    found = _find(rec["id"], day)
    if found and found[0] == day and found[1].get("closed") and found[1].get("text") == rec["text"] and not reopen:
        return found[1]
    home = found[0] if found and not found[1].get("closed") else day

    def change(have: list[dict[str, Any]]) -> dict[str, Any]:
        for i, q in enumerate(have):
            if q["id"] == rec["id"] and not q.get("closed"):
                have[i] = {**rec, "createdAt": q.get("createdAt") or rec["createdAt"]}
                return have[i]
        have[:] = [q for q in have if q["id"] != rec["id"]] + [rec]
        return rec

    got = _change(home, change)
    events.emit("workday.question", action="add", source=got["source"].split(":", 1)[0], recheck=kind(got["recheck"]), options=len(got["options"]))
    return got


def add(step: str, text: str, options: list[str] | None = None, pick: str = "", recheck: str = "", source: str = "agent", day: date | None = None) -> dict[str, Any]:
    return save(record(step, text, options, pick, recheck, source), day)


def _days(day: date) -> list[date]:
    """The day and the earlier days whose open records still count, newest first."""
    return [day - timedelta(days=n) for n in range(CARRY_DAYS + 1)]


def open_records(day: date | None = None) -> list[dict[str, Any]]:
    """Open records of the day and the days before it (oldest first), each with its `day` and `age`."""
    day = day or _today()
    at = _now()
    out: dict[str, dict[str, Any]] = {}
    for d in reversed(_days(day)):
        for q in records(d):
            out.pop(q["id"], None)  # one record per id: the newest (a closed newer one hides an older open one)
            if not q.get("closed"):
                out[q["id"]] = {**q, "day": d.isoformat(), "age": age(q.get("createdAt", ""), at)}
    return list(out.values())


def close(ident: str, why: str = "answered", day: date | None = None, *, expected: dict[str, Any] | None = None) -> bool:
    """Close an open record in the file it lives in. False when no open record has that id."""
    stamp = _now().isoformat(timespec="seconds")
    days = [date.fromisoformat(expected["day"])] if expected is not None else _days(day or _today())
    for d in days:

        def change(have: list[dict[str, Any]]) -> bool:
            for q in have:
                if q["id"] == ident and not q.get("closed"):
                    if expected is not None and q != {k: v for k, v in expected.items() if k not in {"day", "age"}}:
                        return False
                    q.update(closed=why, closedAt=stamp)
                    return True
            return False

        if _change(d, change):
            events.emit("workday.question", action="close")
            return True
    return False


def from_row(row: dict[str, Any], day: date | None = None, reopen: bool = False) -> None:
    """A status row's ask is a record too: one per step while `needs_you` holds text; clearing it closes the record.

    `reopen`: the row was just marked blocked again, so an answered ask with the same text is asked again."""
    ident = row_id(row["step"])
    if row.get("needs_you"):
        save(record(row["step"], row["needs_you"], source="row", ident=ident), day, reopen=reopen)
    else:
        close(ident, "the row no longer waits on you", day)


def _signin_open() -> tuple[str, str]:
    from .. import config
    from .start import probe

    command = config.get("work_signin_probe") or ""
    if not command:
        return "unknown", "no work_signin_probe set"
    ok, why = probe(command)
    return ("closed", "signed in") if ok else ("open", why)


def check_one(q: dict[str, Any]) -> tuple[str, str]:
    """(`open` | `closed` | `agent` | `unknown`, why) for one record. Runs its recheck; never reads its output."""
    from .gate import may_run, run_verify

    k = kind(q.get("recheck", ""))
    if k == "none":
        return "open", "no recheck: close it by hand"
    if k == "url":
        return "agent", "an agent checks the link, then closes the item when it is done"
    if k == "signin":
        return _signin_open()
    source = q.get("source", "agent")
    if source.startswith("plugin:") and not may_run(source):
        return "unknown", "the recheck comes from a plugin that may not run commands: careerbrew plugin add <src> --allow-exec"
    if k == "review-pr":
        from ..work import review_ask

        try:
            url = _review_pr_url(q["recheck"])
        except ValueError:
            return "unknown", "review recheck: invalid PR link"
        return review_ask.recheck_pr(url)
    ok, why = run_verify(q["recheck"])
    if ok:
        return "open", "recheck: still open"
    if why.startswith("exit "):
        return "closed", f"recheck: closed ({why})"
    return "unknown", f"recheck {why}"


def public(q: dict[str, Any]) -> dict[str, Any]:
    """A record without its business text: ids, step, source, age, and the recheck's kind."""
    out = {k: q[k] for k in (*PUBLIC, "age", "state", "why") if k in q}
    if str(out.get("source", "")).startswith("plugin:"):
        out["source"] = "plugin"  # a plugin's name may name the company (#260)
    out["recheck"] = kind(q.get("recheck", ""))
    out["options"] = len(q.get("options", []))
    return out


def recheck(day: date | None = None) -> dict[str, Any]:
    """The batched ask: run every open record's recheck, close the closed ones, and list the rest with their age."""
    from ..work import sessions

    ask, dropped, deferred = [], [], []
    for q in open_records(day):
        if sessions.quiet_question(q["step"], q["day"], (day or _today()).isoformat()):
            deferred.append(q["id"])
            continue
        state, why = check_one(q)
        if state == "closed":
            expected = q if kind(q["recheck"]) == "review-pr" else None
            closed = close(q["id"], why, date.fromisoformat(q["day"]), expected=expected)
            if expected is not None and not closed:
                ask.append({**q, "state": "unknown", "why": "review recheck: question changed"})
                continue
            dropped.append({"id": q["id"], "step": q["step"], "age": q["age"], "why": why})
        else:
            ask.append({**q, "state": state, "why": why})
    out = {"checked": len(ask) + len(dropped), "ask": ask, "dropped": dropped, "agent": [q["id"] for q in ask if q["state"] == "agent"], "deferred": deferred}
    events.emit("workday.recheck", checked=out["checked"], ask=len(ask), dropped=len(dropped), agent=len(out["agent"]), unknown=sum(1 for q in ask if q["state"] == "unknown"))
    return out
