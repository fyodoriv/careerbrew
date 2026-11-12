"""The one work-day watch loop (issue #264): a pass runs the live feed cycle and each watch-capable step's `cycle`.

Before this, four watchers ran on their own clocks (a repo watcher, a checkup's watch phase, the
live-feed cron, careerbrew's day watcher): cycles overlapped, timers ran out and were re-armed by
hand, and nothing recorded them, so a loop could outlive the day. Now:

- One pass (`careerbrew day loop pass`): under the pass lock, run `work live cycle` when the feed
  runs, then each step's `cycle:` command (split like a shell line, run without a shell, output
  discarded, `CAREERBREW_WATCH_PHASE=skip` set so the step skips its own watch phase). A step with
  `watch:` inputs (`repo:<path>`, `feed:<source>`, `file:<path>`) runs only when one changed since
  its last good run; a step without inputs runs every pass. A plugin's cycle runs only after
  `careerbrew plugin add <src> --allow-exec`.
- The lock: a pass that starts while one runs returns `skipped: previous cycle running` at once.
- The interval: `FACTOR` times the median of the last `WINDOW` measured passes, clamped to
  `MIN_SECONDS`..`MAX_SECONDS`; `DEFAULT_SECONDS` before the first one. A pass names its next re-arm
  time; `careerbrew day loop run` re-arms itself until end of day.
- Loops: every loop of the day is recorded (`{kind, id, step}`): the watch loop itself and a running
  live feed automatically, a cron, monitor, background agent, or session by the agent that starts
  it (`day loop add`). `day loop end` stops the loops careerbrew owns and lists the rest, and exits
  0 only when none is left running. A loop left running from an earlier day shows as stale.

Lifecycle: state is `<data root>/day/loops.json` (encrypted at rest): measured pass seconds, each
step's input digest, the next re-arm time, and the loop records (kinds, ids, step names, times;
never a command or its output). Every change reads, changes, and writes it under
`day/.loops.json.lock`; a pass holds `day/.loop.lock` (an fcntl lock the OS frees on exit) for its
whole run. Nothing is kept in process memory between calls; the file lives until the data root is
removed.
"""

from __future__ import annotations

import contextlib
import fcntl
import hashlib
import math
import os
import re
import shlex
import statistics
import subprocess
import time
from collections.abc import Callable, Iterator
from datetime import date, datetime, timedelta
from pathlib import Path
from typing import Any

from .. import events, paths, store

FACTOR = 1.2
WINDOW = 10
DEFAULT_SECONDS = 300
MIN_SECONDS = 60
MAX_SECONDS = 3600
INPUT_KINDS = ("repo", "feed", "file")
INPUT = re.compile(r"^(?:" + "|".join(INPUT_KINDS) + r"):\S+$")
OWNED = ("watch", "live-feed", "day-watcher")  # loops careerbrew can stop itself
KINDS = (*OWNED, "cron", "monitor", "agent", "session")
LOOP_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$")
SELF_ID = "day-loop"
LIVE_ID = "live-feed"
WATCHER_ID = "day-watcher"
SLICE_SECONDS = 5.0  # the run loop re-checks end of day this often while it waits
SKIP_ENV = "CAREERBREW_WATCH_PHASE"
MAX_CYCLE = 500


class WatchError(ValueError):
    pass


# ---------- step fields (read by steps.entry) ----------


def check_fields(fields: dict[str, Any], where: str) -> dict[str, Any]:
    """A step's `cycle` (one command line, like `verify`) and `watch` (inputs that trigger a re-run), checked."""
    from .steps import StepError, _strings

    commands = {}
    for key in ("cycle", "review_ask_cycle"):
        cycle = fields.get(key)
        if cycle is None:
            continue
        if not isinstance(cycle, str) or not cycle.strip():
            raise StepError(f"{where}: `{key}` must be a command, such as `careerbrew work feed cycle`")
        if len(cycle) > MAX_CYCLE or any(c in cycle for c in "\n\r\0"):
            raise StepError(f"{where}: `{key}` must be one line of at most {MAX_CYCLE} characters")
        try:
            shlex.split(cycle)
        except ValueError:
            raise StepError(f"{where}: `{key}` has an unclosed quote") from None
        commands[key] = cycle.strip()
    inputs = _strings(fields.get("watch", []), "watch", where)
    for i in inputs:
        if not INPUT.match(i):
            raise StepError(f"{where}: `watch` entry {i[:40]!r} must start with repo:, feed:, or file: (for example feed:chat)")
    return {"cycle": "", "watch": inputs, **commands}


# ---------- the interval ----------


def interval(seconds: list[float]) -> int:
    """Seconds until the next pass: FACTOR times the median measured pass, so cycles never pile up."""
    if not seconds:
        return DEFAULT_SECONDS
    return max(MIN_SECONDS, min(MAX_SECONDS, math.ceil(FACTOR * statistics.median(seconds[-WINDOW:]))))


# ---------- state ----------


def state_path() -> Path:
    return paths.sub("day", "loops.json")


def lock_path() -> Path:
    return state_path().parent / ".loop.lock"


def _today() -> str:
    from .status import today

    return today().isoformat()


def _stamp() -> str:
    from ..calendar import now

    return now().isoformat(timespec="minutes")


def _load() -> dict[str, Any]:
    st = store.read_json(state_path(), {})
    if not isinstance(st, dict) or not isinstance(st.get("passes", []), list) or not isinstance(st.get("inputs", {}), dict) or not isinstance(st.get("loops", []), list):
        raise WatchError("saved loop records are invalid; repair the loop store before continuing")
    if any(not isinstance(lp, dict) or not lp.get("id") or lp.get("kind") not in KINDS for lp in st.get("loops", [])):
        raise WatchError("saved loop records are invalid; repair the loop store before continuing")
    for lp in st.get("loops", []):
        if any(not isinstance(lp.get(k), str) for k in ("id", "step", "started", "day", "stopped")):
            raise WatchError("saved loop records are invalid; repair the loop store before continuing")
        try:
            datetime.fromisoformat(lp["started"])
            date.fromisoformat(lp["day"])
            if lp["stopped"]:
                datetime.fromisoformat(lp["stopped"])
        except ValueError:
            raise WatchError("saved loop records are invalid; repair the loop store before continuing") from None
    st.setdefault("passes", [])
    st.setdefault("inputs", {})
    st.setdefault("loops", [])
    return st


@contextlib.contextmanager
def _locked() -> Iterator[None]:
    with (state_path().parent / ".loops.json.lock").open("a+") as fh:
        fcntl.flock(fh, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(fh, fcntl.LOCK_UN)


def _save(st: dict[str, Any]) -> None:
    store.write_json(state_path(), st)


@contextlib.contextmanager
def _changing() -> Iterator[dict[str, Any]]:
    """Read, change, and write the state under its write lock, so a pass and an agent's `day loop add` never lose a record."""
    with _locked():
        st = _load()
        yield st
        _save(st)


@contextlib.contextmanager
def _pass_lock() -> Iterator[bool]:
    """True when this pass holds the lock; False at once when another pass runs (it never waits)."""
    fh = lock_path().open("a+")
    try:
        try:
            fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            yield False
            return
        try:
            yield True
        finally:
            fcntl.flock(fh, fcntl.LOCK_UN)
    finally:
        fh.close()


# ---------- loop records ----------


def _record(st: dict[str, Any], kind: str, loop_id: str, step: str) -> None:
    today = _today()
    for lp in st["loops"]:
        if lp["id"] == loop_id and not lp.get("stopped") and kind in OWNED and lp.get("day", today) < today:
            lp["stopped"] = _stamp()  # careerbrew's own loop from an earlier day: today's record replaces it, so it never asks to stop itself
    if any(lp["id"] == loop_id and not lp.get("stopped") for lp in st["loops"]):
        return
    st["loops"].append({"kind": kind, "id": loop_id, "step": step, "day": _today(), "started": _stamp(), "stopped": ""})


def _stop(st: dict[str, Any], loop_id: str) -> int:
    n = 0
    for lp in st["loops"]:
        if lp["id"] == loop_id and not lp.get("stopped"):
            lp["stopped"] = _stamp()
            n += 1
    return n


def _ensure_session(st: dict[str, Any], loop_id: str, session_record: str) -> None:
    same_id = [lp for lp in _running(st) if lp["id"] == loop_id]
    if any(lp.get("kind") != "session" or lp.get("session_record") != session_record for lp in same_id):
        raise WatchError("session loop id conflicts with another running loop; resolve that loop before continuing")
    if not same_id:
        st["loops"].append({"kind": "session", "id": loop_id, "step": "", "day": _today(), "started": _stamp(), "stopped": "", "session_record": session_record})


def add(kind: str, loop_id: str, step: str = "", *, session_record: str = "") -> dict[str, Any]:
    """Record a loop the agent started (a cron, monitor, background agent, or session) so end of day can check it stopped."""
    from .steps import normalize

    if kind not in KINDS:
        raise WatchError(f"loop kind {kind[:20]!r}: use one of {', '.join(KINDS)}")
    if not LOOP_ID.match(loop_id or ""):
        raise WatchError(f"loop id {str(loop_id)[:40]!r}: use the tool's own id, 1 to 64 letters, digits, . _ : - (no spaces)")
    step = normalize(step) if step else ""
    with _changing() as st:
        if session_record:
            _ensure_session(st, loop_id, session_record)
        else:
            _record(st, kind, loop_id, step)
    events.emit("workday.loop", status="added", kind_owned=kind in OWNED)
    return {"kind": kind, "id": loop_id, "step": step}


def stop(loop_id: str, *, session_record: str = "", missing_ok: bool = False) -> dict[str, Any]:
    """Mark a loop stopped after the agent stopped it with its own tool."""
    with _changing() as st:
        if session_record:
            matches = [lp for lp in _running(st) if lp["id"] == loop_id and lp.get("kind") == "session" and lp.get("session_record") == session_record]
            for lp in matches:
                lp["stopped"] = _stamp()
            n = len(matches)
        else:
            n = _stop(st, loop_id)
    if not n and not missing_ok:
        raise WatchError(f"no running loop has id {str(loop_id)[:40]!r} (careerbrew day loop status lists them)")
    return {"id": loop_id, "stopped": n}


def _running(st: dict[str, Any]) -> list[dict[str, Any]]:
    return [lp for lp in st["loops"] if not lp.get("stopped")]


def end() -> dict[str, Any]:
    """End of day: stop every loop careerbrew owns, and list the ones only the agent can stop (each needs `day loop stop`)."""
    from ..day import watch as day_watch
    from ..work import live, sessions

    with sessions._changing() as session_state, _changing() as st:
        owned = [lp for lp in _running(st) if lp["kind"] in OWNED]
        st["ended"] = _stamp()
        for lp in owned:
            _stop(st, lp["id"])
        left = _running(st)
        for session in session_state["sessions"]:
            if not session.get("ended") and not any(lp.get("kind") == "session" and lp.get("id") == session["id"] and lp.get("session_record") == session.get("record_id") and session.get("record_id") for lp in left):
                left.append({"kind": "session", "id": session["id"], "step": "", "started": session.get("started", ""), "day": session.get("day", "")})
    kinds = {lp["kind"] for lp in owned}
    if "live-feed" in kinds:
        live.stop()
    if "day-watcher" in kinds:
        day_watch.stop()
    events.emit("workday.loop", status="ended", stopped=len(owned), left=len(left))
    return {"stopped": owned, "running": left}


# ---------- inputs ----------


def _digest(item: str) -> str:
    kind, _, target = item.partition(":")
    if kind == "file":
        p = Path(target).expanduser()
        try:
            s = p.stat()
            return f"{s.st_size}:{s.st_mtime_ns}"
        except OSError:
            return "missing"
    if kind == "repo":
        repo = str(Path(target).expanduser())
        try:
            refs = subprocess.run(["git", "-C", repo, "for-each-ref", "--format=%(refname) %(objectname)", "refs/heads", "refs/remotes"], capture_output=True, text=True, timeout=30, check=False)  # noqa: S603, S607 (git with fixed arguments)
            head = subprocess.run(["git", "-C", repo, "rev-parse", "HEAD"], capture_output=True, text=True, timeout=30, check=False)  # noqa: S603, S607
        except (OSError, subprocess.TimeoutExpired):
            return "unreadable"
        return hashlib.sha256((refs.stdout + head.stdout).encode()).hexdigest()[:16] if refs.returncode == 0 else "unreadable"
    from ..work import live

    lst = live._state()  # feed:<source>: its cursor and inbox fingerprint move only when the feed stored something new
    return f"{lst['cursors'].get(target, '')}|{lst['seen'].get(target, '')}"


def fingerprint(inputs: list[str]) -> str:
    """One digest of a step's watch inputs: it changes when any input changed (ids and sizes only, never content)."""
    return hashlib.sha256("\n".join(f"{i}={_digest(i)}" for i in inputs).encode()).hexdigest()[:16]


# ---------- one pass ----------


def run_cycle(command: str, seconds: float) -> tuple[bool, str]:
    """(passed, why) for one step's cycle. Its output goes nowhere: it may hold business data."""
    env = {**os.environ, SKIP_ENV: "skip"}
    try:
        argv = shlex.split(command)  # no shell: a step that needs one says `sh -c '...'`
        done = subprocess.run(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env, timeout=seconds, check=False)  # noqa: S603 (the owner's installed step)
    except subprocess.TimeoutExpired:
        return False, f"timed out after {seconds:g} s"
    except (OSError, ValueError) as e:
        return False, f"could not start ({type(e).__name__})"
    return (True, "") if done.returncode == 0 else (False, f"exit {done.returncode}")


def _live_cycle() -> dict[str, Any]:
    from ..work import health, live

    if not live._state()["running"]:
        result = health.snapshot()
        return {"status": "ok" if result["status"] == "healthy" else "failed", "health": result}
    try:
        rec = live.cycle()
    except live.LiveError as e:
        return {"status": "skipped" if "another live cycle" in str(e) else "failed", "why": str(e)[:200]}
    result = health.snapshot()
    return {"status": "ok" if result["status"] == "healthy" else "failed", "not_stored": rec["not_stored"], "health": result}


def _rearm_at(seconds: int) -> str:
    """The next pass time, rounded up to the minute: a re-arm at the printed time never comes early."""
    from ..calendar import now

    at = now() + timedelta(seconds=seconds)
    if at.second or at.microsecond:
        at = at.replace(second=0, microsecond=0) + timedelta(minutes=1)
    return at.isoformat(timespec="minutes")


def _catchup_due(st: dict[str, Any], at: datetime) -> str:
    from . import end

    due = []
    try:
        next_at = datetime.fromisoformat(st.get("next_at") or "")
        if next_at.tzinfo is None:
            next_at = next_at.replace(tzinfo=at.tzinfo)
        if next_at.date() == at.date() and at > next_at + timedelta(minutes=5):
            due.append("loop:" + next_at.isoformat())
    except ValueError:
        pass
    wrap_at = end.times(at)[0]
    if end.phase(at) not in ("ended", "held") and not end._data(at.date()).get("wrapup") and at > wrap_at + timedelta(minutes=5):
        due.append("wrap:" + wrap_at.isoformat())
    ended_today = str(st.get("ended", "")).startswith(at.date().isoformat())
    return "|".join(due) if not ended_today else ""


def _process_alive(pid: int) -> bool:
    if type(pid) is not int or pid <= 0:
        return False
    try:
        os.kill(pid, 0)
        return True
    except (OSError, ValueError):
        return False


def catch_up() -> dict[str, Any]:
    """At turn start, claim a >5-minute late due event once, then use the normal pass lock.

    Claim state belongs to the encrypted watch store. A crashed claimant can be
    replaced when its process no longer lives; a failed/skipped pass can retry.
    """
    from ..calendar import now

    at = now()
    if not _catchup_due(_load(), at):
        return {"status": "current"}
    with _changing() as st:
        due = _catchup_due(st, at)
        claim = st.get("catchup") or {}
        busy = claim.get("status") == "running" and _process_alive(claim.get("pid", 0))
        if not due or busy or (claim.get("due") == due and claim.get("status") == "done"):
            return {"status": "current"}
        owned = {"due": due, "pid": os.getpid(), "at": at.isoformat(timespec="microseconds")}
        st["catchup"] = {**owned, "status": "running"}
    try:
        result = run_pass(restart=False)
    except BaseException:
        with _changing() as st:
            if all((st.get("catchup") or {}).get(k) == v for k, v in owned.items()):
                st["catchup"]["status"] = "failed"
        raise
    with _changing() as st:
        if all((st.get("catchup") or {}).get(k) == v for k, v in owned.items()):
            st["catchup"]["status"] = "retry" if result["status"] == "skipped" else "done"
            st["catchup"]["finished_at"] = now().isoformat(timespec="seconds")
    return {"status": "ran" if result["status"] == "ok" else result["status"], "due": due, "pass": result}


def run_pass(clock: Callable[[], float] = time.monotonic, runner: Callable[[str, float], tuple[bool, str]] = run_cycle, restart: bool = True) -> dict[str, Any]:
    """One pass of the watch loop: lock, the live cycle, each watch-capable step's cycle, then measure and re-arm.

    `restart`: an explicit pass (`day loop pass`) after end of day starts the loop again; a pass the `run` loop
    starts does not, so an end of day that lands between its check and its pass is never lost."""
    from ..day import watch as day_watch
    from . import gate, plan

    with _pass_lock() as got:
        if not got:
            events.emit("workday.loop", status="skipped")
            return {"status": "skipped", "reason": "previous cycle running"}
        start = clock()
        with _changing() as st:
            if str(st.get("ended", "")).startswith(_today()) and not restart:
                return {"status": "ended"}
            st.pop("ended", None)
            _record(st, "watch", SELF_ID, "")
            if day_watch.running():  # careerbrew's background day watcher is a loop of the day too
                _record(st, "day-watcher", WATCHER_ID, "")
            digests = dict(st["inputs"])
        result_live = _live_cycle()
        from ..work import live

        if live._state()["running"]:
            with _changing() as st:
                _record(st, "live-feed", LIVE_ID, "")
        results: list[dict[str, Any]] = []
        fresh: dict[str, str] = {}
        for s in plan.build()["steps"]:
            if not (s.get("cycle") or s.get("review_ask_cycle")):
                continue
            row: dict[str, Any] = {"step": s["step"], "result": "", "why": ""}
            fp = fingerprint(s["watch"]) if s["watch"] else ""
            if not gate.may_run(s["from"]):
                row |= {"result": "blocked", "why": f"the cycle of {s['step']} comes from {s['from']}, which may not run commands: careerbrew plugin add <src> --allow-exec"}
            else:
                minutes = s.get("timeout") or 30
                if s.get("cycle"):
                    if fp and digests.get(s["step"]) == fp:
                        row["result"] = "unchanged"
                    else:
                        ok, why = runner(s["cycle"], float(minutes * 60))
                        row |= {"result": "ok" if ok else "failed", "why": why}
                        if ok and fp:
                            fresh[s["step"]] = fp
                if s.get("review_ask_cycle"):
                    from . import review_cycle

                    ok = review_cycle.run(s, float(minutes * 60))
                    if not ok and row["result"] != "failed":
                        row |= {"result": "failed", "why": "review inventory unavailable or incomplete"}
                    elif ok and not row["result"]:
                        row["result"] = "ok"
            results.append(row)
        seconds = max(0.0, clock() - start)
        with _changing() as st:
            st["inputs"].update(fresh)
            st["passes"] = [*st["passes"], round(seconds, 3)][-WINDOW:]
            every = interval(st["passes"])
            st["interval"] = every
            st["next_at"] = _rearm_at(every)
            st["last"] = {"at": _stamp(), "seconds": round(seconds, 3), "live": result_live["status"], "results": {r["step"]: r["result"] for r in results}}
            next_at, stale = st["next_at"], _stale(st)
        from . import end  # issue #265: the clock starts wrap-up (the cycles keep running) and, at work_end, ends the day

        clock_check = end.tick()  # inside the pass lock: two passes at work_end never both end the day
    failed = sum(1 for r in results if r["result"] == "failed") + (result_live["status"] == "failed")
    events.emit("workday.loop", status="failed" if failed else "ok", steps=len(results), failed=failed, seconds=round(seconds, 1), interval=every)
    return {"status": "failed" if failed else "ok", "seconds": round(seconds, 3), "interval": every, "next_at": next_at, "live": result_live, "steps": results, "stale": stale, "phase": clock_check["phase"], "wrap_up": clock_check}


def _stale(st: dict[str, Any]) -> list[dict[str, Any]]:
    today = _today()
    return [lp for lp in _running(st) if lp.get("day", today) < today]


def status() -> dict[str, Any]:
    st = _load()
    return {
        "passes": st["passes"],
        "interval": interval(st["passes"]),
        "next_at": st.get("next_at", ""),
        "last": st.get("last") or {},
        "ended": st.get("ended", ""),
        "running": _running(st),
        "stale": _stale(st),
    }


def run(sleep: Callable[[float], None] = time.sleep, clock: Callable[[], float] = time.monotonic, max_passes: int | None = None) -> int:
    """The self-re-arming loop: a pass, then wait the measured interval, until end of day (`day loop end`). Returns the passes run."""
    with _changing() as st:
        st.pop("ended", None)
    n = 0
    while max_passes is None or n < max_passes:
        got = run_pass(clock=clock, restart=False)
        if got["status"] == "ended":
            break
        n += got["status"] in {"ok", "failed"}
        wait, waited = float(got.get("interval") or interval(_load()["passes"])), 0.0
        while waited < wait and not _load().get("ended"):  # end of day stops the wait within one slice, not after an hour
            step = min(SLICE_SECONDS, wait - waited)
            sleep(step)
            waited += step
    return n
