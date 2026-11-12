"""The day watcher: a light background loop that appends to the results-of-the-day list.

Commands (cli `careerbrew day watch ...`): on, off, pause, resume, status, once.

Design for low cost and safe interruption:
- one process, one instance: an exclusive fcntl lock on `day/watch.lock`, held
  for the process lifetime and released by the OS when the process dies; a
  status probe (`running()`) tests it with a shared lock, so probes at once
  never see each other as a watcher, and a starting watcher waits a probe out;
- control by a state file (`day/watch.json`: on | paused | off) plus signals:
  SIGUSR1 wakes the loop to re-read the state, SIGTERM finishes the current
  source and exits;
- each cycle runs sources one by one (the work feed: Slack, PagerDuty, Outlook; evidence: git, gh,
  connectors; project memory files, both ways; calendar);
  after each source, its checkpoint (`day/cursors.json`) is written
  atomically, and result rows are appended idempotently by id. A kill at any
  point loses at most the source in flight, and the next run redoes it without
  duplicates;
- polls only inside work hours, backs off when nothing changes (5 → 30 min),
  runs at nice 10, and logs CPU time and peak memory per cycle;
- macOS notifications (redacted, counts or task id + title): the day's plan
  once per morning, a new interview email, and a changed next-task suggestion;
  after each cycle, opt-in notifications about important new events
  (`careerbrew.notify`: priority, quiet hours, daily cap, batching, actions).
- network sources (the work feed, calendar, mail) run at most every 15 minutes.
"""

from __future__ import annotations

import contextlib
import fcntl
import json
import os
import resource
import signal
import subprocess
import sys
import time
from collections.abc import Callable, Iterator
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any

from .. import calendar, events, paths, store
from ..work import evidence
from . import inbox, planner, results
from . import log as daylog

BASE_INTERVAL = 300
MAX_INTERVAL = 1800


def _dir() -> Path:
    d = paths.sub("day", ".keep").parent
    d.mkdir(parents=True, exist_ok=True)
    return d


def state_path() -> Path:
    return _dir() / "watch.json"


def lock_path() -> Path:
    return _dir() / "watch.lock"


def cursors_path() -> Path:
    return _dir() / "cursors.json"


STATE_VERSION = 2  # bump when the state shape changes; read_state fills new keys for old files


def read_state() -> dict[str, Any]:
    st = store.read_json(state_path(), {"state": "off"}) or {"state": "off"}
    if int(st.get("version", 1)) < STATE_VERSION:
        st = {"reloads": 0, "cpu_history": [], **st, "version": STATE_VERSION}
    return st


def code_dir() -> Path:
    return Path(os.environ.get("CAREERBREW_CODE_DIR") or Path(__file__).resolve().parents[1])


def code_stamp() -> str:
    """Fingerprint of the running code: every .py file's path, size, and mtime (cheap: no file reads)."""
    parts = []
    for f in sorted(code_dir().rglob("*.py")):
        st = f.stat()
        parts.append(f"{f.relative_to(code_dir())}:{st.st_size}:{st.st_mtime_ns}")
    return store.digest(parts)


def precheck() -> bool:
    """Import the new code in a child process before replacing ourselves; a broken update keeps the old code running."""
    cmd = os.environ.get("CAREERBREW_RELOAD_PRECHECK")
    check = "import pathlib, sys\nfor p in pathlib.Path(sys.argv[1]).rglob('*.py'):\n    compile(p.read_text(encoding='utf-8'), str(p), 'exec')\nimport careerbrew.day.watch, careerbrew.cli\n"
    args = cmd.split() if cmd else [sys.executable, "-c", check, str(code_dir())]
    return subprocess.run(args, capture_output=True, timeout=60, check=False).returncode == 0


@contextlib.contextmanager
def _state_lock() -> Iterator[None]:
    """Serialize read-modify-write of watch.json between the CLI and the running watcher."""
    fh = (_dir() / "watch.state.lock").open("a+")
    try:
        fcntl.flock(fh, fcntl.LOCK_EX)
        yield
    finally:
        fcntl.flock(fh, fcntl.LOCK_UN)
        fh.close()


def update_state(fn: Callable[[dict[str, Any]], dict[str, Any]]) -> dict[str, Any]:
    """Atomically apply fn(current state) -> fields to merge. No update is ever lost."""
    with _state_lock():
        cur = read_state()
        st = {**cur, **fn(cur)}
        store.write_json(state_path(), st)
        return st


def write_state(**fields: Any) -> dict[str, Any]:
    return update_state(lambda _cur: fields)


def pid_alive(pid: int | None) -> bool:
    if not pid:
        return False
    try:
        os.kill(pid, 0)
    except (ProcessLookupError, PermissionError):
        return False
    return True


PROBE_WAIT_S = 2.0  # a starting watcher waits this long for status probes, which hold the lock for microseconds


def running() -> bool:
    """True when some process holds the lock (the reliable check; pid files can be stale).

    The probe takes a shared lock: probes never block each other, and only the watcher's exclusive lock refuses it.
    """
    try:
        with lock_path().open("a+") as fh:
            fcntl.flock(fh, fcntl.LOCK_SH | fcntl.LOCK_NB)
            fcntl.flock(fh, fcntl.LOCK_UN)
            return False
    except BlockingIOError:
        return True


@contextlib.contextmanager
def _hold_lock(wait_s: float = 0.0) -> Iterator[None]:
    fh = lock_path().open("a+")
    start = time.monotonic()
    deadline = start + wait_s
    while True:
        try:
            fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)
            break
        except BlockingIOError:
            # Only a watcher refuses this start; a status probe in flight is waited out.
            now = time.monotonic()
            if now >= deadline and (running() or now >= start + PROBE_WAIT_S):
                fh.close()
                raise RuntimeError("another watcher is already running (see `careerbrew day watch status`)") from None
            time.sleep(0.05)
    try:
        yield
    finally:
        fcntl.flock(fh, fcntl.LOCK_UN)
        fh.close()


# ---------- sources ----------


def _src_evidence(day) -> list[dict[str, Any]]:
    evidence.collect()
    return results.evidence_rows(evidence.load(), day)


def _src_calendar(day) -> list[dict[str, Any]]:
    """Meetings that ended today. A private entry, or one the work feed keeps personal (a rule, your mark), is flagged personal."""
    from ..work import feed

    now = calendar.now()
    evs, _ = calendar.events_for(day)
    personal = feed.personal_check()
    rows = []
    for e in evs:
        if not (e.busy and e.end <= now):
            continue
        rid = f"meeting:{e.start.isoformat()}:{store.digest(e.title)[:8]}"
        probe = {"id": e.uid or rid, "at": e.start.isoformat(), "title": e.title or "Meeting", "text": e.title, "source": "outlook" if e.uid else "calendar", "kind": "meeting"}
        flag = e.personal or personal(probe)
        rows.append(
            {
                "id": rid,
                "at": e.end.isoformat(),
                "kind": "meeting",
                "title": e.title,
                "source": "calendar",
                "origin": "user",
                **({"ref": e.uid} if e.uid else {}),
                **({"personal": True} if flag else {}),
                **({"led": True} if e.led else {}),
                **({"recurring": True} if e.recurring else {}),
            }
        )
    return rows


def _src_feed(day) -> list[dict[str, Any]]:
    """Slack asks, PagerDuty pages, and Outlook mail into the work feed (issue #72). Its evidence reaches results via the evidence source."""
    from ..work import feed

    feed.pull()
    return []


def _src_memory(day) -> list[dict[str, Any]]:
    """Project memory files into their projects, then matching work items into those files as notes (work/memory_sync.py).
    Notes are business content in a file an agent reads, so they need a plugin policy `agent_may_read_business`, as research does."""
    from .. import plugins
    from ..work import memory_sync

    memory_sync.run(allowed=bool(plugins.effective_policy().get("agent_may_read_business")))
    return []


def _src_mail(day) -> list[dict[str, Any]]:
    """Interview emails and LinkedIn alerts (metadata only). Job-hunt mail is never kept on a corporate machine."""
    from .. import machine, mail
    from ..jobs import linkedin

    if not mail.accounts():
        return []
    msgs, _errors = mail.fetch(days=1)
    groups = mail.classify(msgs)
    if machine.role() == "corporate":
        return []
    linkedin.add_alert_postings(groups["linkedin_alert"])
    rows = [{"id": f"mail:{store.digest(m.id)[:16]}", "at": m.received, "kind": "mail-interview", "title": m.subject, "source": "mail", "origin": "user"} for m in groups["interview"]]
    if rows:
        store.append_jsonl(paths.sub("jobs", "inbox.jsonl"), [{"id": m.id, "sender_domain": m.sender.split("@")[-1], "subject": m.subject, "received": m.received} for m in groups["interview"]])
    return rows


def _src_changes(day) -> list[dict[str, Any]]:
    """Ladder, plugin, and company knowledge changes become pivot rows (local hashes only; no fetch)."""
    from .. import changes

    found = changes.check(fetch=False)
    if found:
        notify(f"{len(found)} change(s) in ladders, plugins, or company data: careerbrew re-planned. Details: careerbrew day results")
    return [
        {
            "id": f"pivot:{c['kind']}:{c['id']}:{store.digest(c)[:8]}",
            "at": calendar.now().isoformat(timespec="seconds"),
            "kind": "pivot",
            "title": f"{c['change']} {c['kind']} {c['id']}: {c.get('action', '')}",
            "source": "changes",
            "origin": "user",
        }
        for c in found
    ]


SOURCES = {"feed": _src_feed, "evidence": _src_evidence, "memory": _src_memory, "calendar": _src_calendar, "mail": _src_mail, "changes": _src_changes}
MIN_GAP_S = {"feed": 900, "calendar": 900, "mail": 900, "changes": 3600}  # network and rebuild sources run less often


def _weekly_metrics() -> None:
    from .. import metrics_share

    metrics_share.maybe_write()


def _learn_sent(day) -> None:
    """Revisit sent days of retained own Slack originals, including edits with no new results."""
    from datetime import timedelta

    from . import pm_learn

    days = pm_learn.retained_sent_days() | {(day - timedelta(days=1)).isoformat(), day.isoformat()}
    for sent_day in sorted(d for d in days if d <= day.isoformat()):
        pm_learn.learn_sent(sent_day)


def _rebuild(day) -> None:
    """After new rows: research stale project sources, regenerate proposals and questions, rewrite the daily log."""
    from .. import plugins
    from ..work import research

    steps = (
        ("research", lambda: research.refresh(read_allowed=bool(plugins.effective_policy().get("agent_may_read_business")))),
        ("inbox", lambda: inbox.regenerate(day)),
        ("log", lambda: daylog.save(day)),
        ("pm-sent", lambda: _learn_sent(day)),
        ("metrics", _weekly_metrics),
    )
    for name, fn in steps:
        try:
            fn()
        except Exception as exc:  # noqa: BLE001 (one broken step must not stop the watcher)
            events.emit("day.source.error", source_kind=name, error=type(exc).__name__)
    counts = {"proposals": 0, "questions": 0}
    for i in inbox.open_items():
        counts[i["type"] + "s"] = counts.get(i["type"] + "s", 0) + 1
    key = f"{counts['proposals']}:{counts['questions']}"
    if (counts["proposals"] or counts["questions"]) and read_state().get("last_inbox") != key:
        write_state(last_inbox=key)
        notify(f"careerbrew has {counts['proposals']} proposal(s) and {counts['questions']} question(s) for you. Open: careerbrew")


def run_cycle() -> dict[str, Any]:
    """One pass over every source with per-source checkpoints. Safe to kill at any point."""
    before = resource.getrusage(resource.RUSAGE_SELF)
    day = results.today()
    cursors = store.read_json(cursors_path(), {}) or {}
    added = 0
    new_mail = 0
    for name, fn in SOURCES.items():
        if read_state().get("state") == "paused":
            break
        last = (cursors.get(name) or {}).get("at")
        # The throttle cursors and cycle stats are the watcher's own, on the wall clock: they measure real elapsed time.
        if last and name in MIN_GAP_S and (datetime.now() - datetime.fromisoformat(last)).total_seconds() < MIN_GAP_S[name]:
            continue
        try:
            rows = fn(day)
            n = results.append(rows, day)
            added += n
            if name == "mail":
                new_mail = n
            cursors[name] = {"at": datetime.now().isoformat(timespec="seconds"), "ok": True}
        except Exception as exc:  # noqa: BLE001 (a broken source must not stop the watcher)
            cursors[name] = {"at": datetime.now().isoformat(timespec="seconds"), "ok": False, "error": type(exc).__name__}
            events.emit("day.source.error", source_kind=name, error=type(exc).__name__, story="us-26")
        store.write_json(cursors_path(), cursors)  # checkpoint after every source
    if added or not daylog.path_for(day).exists():
        _rebuild(day)
    else:
        try:
            _learn_sent(day)
        except Exception as exc:  # noqa: BLE001 (a learning error must not stop the watcher)
            events.emit("day.source.error", source_kind="pm-sent", error=type(exc).__name__)
    _notify_events()
    _telemetry()
    after = resource.getrusage(resource.RUSAGE_SELF)
    cpu_ms = int(((after.ru_utime - before.ru_utime) + (after.ru_stime - before.ru_stime)) * 1000)
    rss_mb = round(after.ru_maxrss / (1024 * 1024 if sys.platform == "darwin" else 1024), 1)
    if new_mail:
        notify(f"{new_mail} new interview email(s). Log each stage with: careerbrew jobs log <company> <tier> <stage>")
    events.emit("day.cycle", added=added, cpu_ms=cpu_ms, rss_mb=rss_mb)
    stats = {"added": added, "cpu_ms": cpu_ms, "rss_mb": rss_mb, "at": datetime.now().isoformat(timespec="seconds")}
    update_state(lambda s: {"last_cycle": stats, "cpu_history": (s.get("cpu_history") or [])[-49:] + [cpu_ms]})
    return stats


def _telemetry() -> None:
    """Anonymous usage counts, at most once a day (US-89; careerbrew.telemetry never raises)."""
    from .. import telemetry

    telemetry.maybe_send()


def _notify_events() -> None:
    """Opt-in notifications about important new events (issue #74; careerbrew.notify). A failure never stops the cycle."""
    from .. import notify as notifications

    try:
        notifications.run()
    except Exception as exc:  # noqa: BLE001 (notifications must not stop the watcher)
        events.emit("day.source.error", source_kind="notify", error=type(exc).__name__, story="us-62")


def _in_work_hours(now: datetime) -> bool:
    s = planner.settings()
    if now.weekday() >= 5 and not s.get("watch_weekends", False):
        return False
    start = planner._at(now.date(), s["work_start"]) - timedelta(minutes=30)
    end = planner._at(now.date(), s["work_end"]) + timedelta(minutes=30)
    return start <= now <= end


def notify(text: str) -> None:
    if os.environ.get("CAREERBREW_NOTIFY", "on") == "off" or sys.platform != "darwin":
        return
    from .. import privacy

    msg = privacy.redact(text)[:180]
    # The message is an argv item read by the script, never spliced into AppleScript source.
    script = ["-e", "on run argv", "-e", 'display notification (item 1 of argv) with title "careerbrew"', "-e", "end run"]
    subprocess.run(["osascript", *script, msg], check=False, capture_output=True, timeout=10)


def _maybe_plan() -> None:
    """Once per workday: build the plan and say it is ready (counts only, no titles)."""
    today = results.today().isoformat()
    if read_state().get("last_plan_day") == today:
        return
    try:
        plan = planner.build()
    except Exception:  # noqa: BLE001
        return
    write_state(last_plan_day=today)
    notify(f"Today's plan is ready: {len(plan['schedule'])} task(s) around {len(plan['meetings'])} meeting(s). Run: careerbrew day plan")


def _maybe_suggest() -> None:
    try:
        nxt = planner.next_step()
    except Exception:  # noqa: BLE001
        return
    key = f"{nxt.get('kind')}:{nxt.get('task', '')}"
    if nxt.get("kind") in ("task", "prepare", "log") and read_state().get("last_suggestion") != key:
        write_state(last_suggestion=key)
        planner.log_suggestion(nxt)
        title = f" {nxt['title']}" if nxt.get("title") else ""
        notify(f"Next:{title} — {nxt['text']}")


def _reexec(interval: int | None) -> None:
    """Replace this process with the updated code: same pid, same state file, same results."""
    args = [sys.executable, "-m", "careerbrew.day.watch"] + ([f"--interval={interval}"] if interval else [])
    os.environ["CAREERBREW_RELOADED"] = "1"
    os.execv(sys.executable, args)  # noqa: S606 (re-exec of our own entry point)


def loop(interval: int | None = None, max_cycles: int | None = None) -> None:
    woke = {"flag": False}
    stop = {"flag": False}
    reload_req = {"flag": False}
    base = interval or int(planner.settings().get("watch_interval_s", BASE_INTERVAL))
    wait = base
    cycles = 0
    reloaded = os.environ.pop("CAREERBREW_RELOADED", "") == "1"
    # Lock first: a second instance must fail before any process-wide side effect. After a
    # re-exec, wait briefly: the old image's lock fd closes on exec (O_CLOEXEC).
    with _hold_lock(wait_s=5.0 if reloaded else 0.0):
        # Read the level with getpriority: at nice 20, macOS refuses even os.nice(0) (issue #280).
        if os.getpriority(os.PRIO_PROCESS, 0) < 10:
            os.nice(10)
        signal.signal(signal.SIGUSR1, lambda *_: woke.update(flag=True))
        signal.signal(signal.SIGUSR2, lambda *_: (reload_req.update(flag=True), woke.update(flag=True)))
        signal.signal(signal.SIGTERM, lambda *_: stop.update(flag=True))
        stamp = code_stamp()
        # The process's own start, reload, and pause times stay on the wall clock (tests/unit/test_clock_reads.py).
        write_state(pid=os.getpid(), code_stamp=stamp, **({} if reloaded else {"started_at": datetime.now().isoformat(timespec="seconds")}))
        events.emit("day.watch.start", status="reloaded" if reloaded else "ok")
        while not stop["flag"]:
            if reload_req["flag"] or code_stamp() != stamp:
                reload_req["flag"] = False
                if precheck():
                    update_state(lambda s: {"reloads": int(s.get("reloads", 0)) + 1, "reloaded_at": datetime.now().isoformat(timespec="seconds"), "reload_error": None})
                    events.emit("day.watch.reload", status="ok")
                    _reexec(interval)
                stamp = code_stamp()  # broken update: keep running the old code, try again after the next change
                write_state(reload_error="the updated code failed its import check; still running the previous version")
                events.emit("day.watch.reload", status="failed")
                notify("careerbrew update failed its check; the watcher keeps running the previous version")
            st = read_state().get("state")
            if st == "off":
                break
            if st == "on" and (interval or _in_work_hours(calendar.now())):
                stats = run_cycle()
                _maybe_plan()
                _maybe_suggest()
                cycles += 1
                wait = base if stats["added"] else min(MAX_INTERVAL, wait * 2)
                if max_cycles and cycles >= max_cycles:
                    break
            woke["flag"] = False
            deadline = time.monotonic() + wait
            while time.monotonic() < deadline and not woke["flag"] and not stop["flag"]:
                time.sleep(min(1.0, deadline - time.monotonic()))
        write_state(pid=None)
        events.emit("day.watch.stop", status="ok")


# ---------- control (used by the CLI) ----------


def start(interval: int | None = None) -> dict[str, Any]:
    if running():
        st = write_state(state="on")
        _signal(signal.SIGUSR1)
        return {**st, "note": "already running"}
    write_state(state="on")
    log = paths.sub("logs", "watcher.log").open("a")
    cmd = [sys.executable, "-m", "careerbrew.day.watch"] + ([f"--interval={interval}"] if interval else [])
    proc = subprocess.Popen(cmd, stdout=log, stderr=log, stdin=subprocess.DEVNULL, start_new_session=True, env=os.environ.copy())  # noqa: S603
    for _ in range(50):
        if running():
            break
        time.sleep(0.1)
    return write_state(pid=proc.pid)


def _signal(sig: int) -> None:
    pid = read_state().get("pid")
    if pid and pid_alive(int(pid)):
        with contextlib.suppress(ProcessLookupError):
            os.kill(int(pid), sig)


def reload() -> dict[str, Any]:
    """Ask the running watcher to switch to the current code at the next safe point (between sources)."""
    if not running():
        return {**read_state(), "note": "not running"}
    _signal(signal.SIGUSR2)
    return read_state()


def pause() -> dict[str, Any]:
    st = write_state(state="paused", paused_at=datetime.now().isoformat(timespec="seconds"))
    _signal(signal.SIGUSR1)
    return st


def resume() -> dict[str, Any]:
    st = write_state(state="on")
    _signal(signal.SIGUSR1)
    return st


def stop(timeout: float = 10.0) -> dict[str, Any]:
    write_state(state="off")
    _signal(signal.SIGTERM)
    deadline = time.monotonic() + timeout
    while running() and time.monotonic() < deadline:
        time.sleep(0.1)
    return {**read_state(), "running": running()}


def status() -> dict[str, Any]:
    st = read_state()
    return {**st, "running": running(), "today": results.summary(), "cursors": store.read_json(cursors_path(), {})}


if __name__ == "__main__":
    arg = next((a for a in sys.argv[1:] if a.startswith("--interval=")), None)
    loop(interval=int(arg.split("=", 1)[1]) if arg else None)
    print(json.dumps({"exit": "ok"}))
