"""The work day's truth rules (issue #259): a `verify` proof for `done`, a timeout with one retry, and blocked versus failed.

- Verify: a step may declare `verify:`, a one-line command (split like a shell line, run without a
  shell) whose exit code proves its result.
  `careerbrew day update <step> --status done` runs it, and the row is `done` only when it exits 0;
  otherwise the row is `failed` with class `tool`. Its output is never read, printed, or stored.
- Timeout: each step has `timeout` minutes (its own `timeout:`, else the `work_step_timeout` setting).
  A `running` row older than that is overdue: the runner crashed or the step hangs. `careerbrew day
  check` (the next pass) sets an overdue row back to `waiting` for one retry, and marks it `failed`,
  class `tool`, when the retry is overdue too.
- Blocked versus failed: `blocked` waits on the owner and must carry its question (`needs_you`);
  `failed` means something broke and must carry an error class. Both rows get a next action.

Lifecycle: pure functions plus `check`, which reads and writes the day's status file through `status`.
"""

from __future__ import annotations

import shlex
import subprocess
from datetime import datetime, timedelta
from typing import Any

ERROR_CLASSES = ("env", "auth", "network", "conflict", "tool")
MAX_ATTEMPTS = 2  # the first run plus one retry after a timeout
VERIFY_SECONDS: float = 120

_NEXT = {
    "env": "install or fix the tool or setting {step} needs, then run {step} again",
    "auth": "sign in again or renew the token {step} uses (careerbrew secret set <name>), then run {step} again",
    "network": "run {step} again when the network or the service is back",
    "conflict": "resolve the conflict by hand, then run {step} again",
    "tool": "read the log of {step}, fix the tool or the step, then run {step} again",
}
ASK = "answer the ask in the end-of-steps questions, then run {step} again"
RETRY = "run {step} again: attempt {n} of {max}, after a timeout"


def next_action(step: str, status: str, error_class: str = "") -> str:
    """The generic next action for a blocked or failed row (no row text, so agents may read it); empty otherwise."""
    if status == "blocked":
        return ASK.format(step=step)
    if status == "failed":
        return _NEXT.get(error_class, _NEXT["tool"]).format(step=step)
    return ""


def may_run(origin: str) -> bool:
    """A plugin's `verify` runs only when the owner added the plugin with `--allow-exec`, like a command evidence source."""
    from .. import plugins

    if not origin.startswith("plugin:"):
        return True
    name = origin.split(":", 1)[1]
    return any(p["name"] == name and p.get("exec_allowed") for p in plugins.installed())


def run_verify(command: str) -> tuple[bool, str]:
    """(passed, why) for a step's verify command. Its output goes nowhere: it may hold business data."""
    try:
        argv = shlex.split(command)  # no shell: a step that needs one says `sh -c '...'`
        done = subprocess.run(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=VERIFY_SECONDS, check=False)  # noqa: S603 (the owner's installed step)
    except subprocess.TimeoutExpired:
        return False, f"timed out after {VERIFY_SECONDS:g} s"
    except (OSError, ValueError) as e:
        return False, f"could not start ({type(e).__name__})"
    return (True, "") if done.returncode == 0 else (False, f"exit {done.returncode}")


def overdue(row: dict[str, Any], minutes: int, now: datetime) -> bool:
    """A `running` row whose start is more than `minutes` ago: the step hangs, or the runner crashed mid-step."""
    if row.get("status") != "running":
        return False
    if not row.get("started"):
        return True  # a running row with no start time (written before #259) cannot prove it is inside its limit
    try:
        started = datetime.fromisoformat(row["started"])
    except ValueError:
        return True  # a start time nobody can read cannot prove the step is still inside its limit
    return now - started > timedelta(minutes=minutes)


def check(steps: list[dict[str, Any]]) -> dict[str, Any]:
    """The next pass over today's rows: retry each overdue step once, then mark it failed. Returns step names only."""
    from ..calendar import now
    from . import status

    names = [s["step"] for s in steps]
    limits = {s["step"]: s["timeout"] for s in steps if not s.get("long_running")}  # a long-running step never ends, so never times out
    at = now()
    out: dict[str, Any] = {"checked": 0, "retry": [], "failed": []}
    for row in status.rows(names):
        if row["status"] != "running" or row["step"] not in limits:
            continue
        out["checked"] += 1
        if not overdue(row, limits[row["step"]], at):
            continue
        changed = status.timed_out(row["step"], names, row.get("started", ""))
        if changed is not None:
            out["retry" if changed["status"] == "waiting" else "failed"].append(row["step"])
    return out
