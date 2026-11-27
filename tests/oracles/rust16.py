"""rust-16 Python oracle for work-day execution: proofs, timeouts, timing, the watch loop and wrap-up.

The native candidate mirrors this synthetic workload. Every data root, clock and
command runner is synthetic: no real watcher, cycle command or verify command
runs. The plan, the live-feed cycle result, the pass clock and the cycle runner
are supplied facts, so the loop's own decisions are compared, not its
neighbours. The report holds step names, states, counts and times only.
"""

from __future__ import annotations

import contextlib
import fcntl
import io
import json
import os
import sys
from datetime import date, datetime
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))

from careerbrew import cli, store  # noqa: E402
from careerbrew.workday import cron_check, end, gate, plan, status, steps, timing, watch  # noqa: E402

CASES = ("gate-check", "timing-history", "loop-records", "loop-pass", "catch-up", "wrap-control")
CANARY = "SYNTHCANARY-rust16-private"


def typed(value: Any) -> Any:
    if value is None or isinstance(value, (str, bool)):
        return value
    if isinstance(value, int):
        return {"int": str(value)}
    if isinstance(value, float):
        return {"float": str(value)}
    if isinstance(value, (list, tuple)):
        return [typed(item) for item in value]
    if isinstance(value, dict):
        return {key: typed(item) for key, item in value.items()}
    raise TypeError(type(value))


def compact(value: Any) -> str:
    return json.dumps(typed(value), ensure_ascii=False, separators=(",", ":"), sort_keys=True) + "\n"


def error(fn) -> str:
    try:
        fn()
    except (ValueError, steps.StepError) as exc:
        return type(exc).__name__
    return "accepted"


def trace_reset(base: Path) -> None:
    (base / "writes.log").write_text("")


def trace_count(base: Path) -> int:
    path = base / "writes.log"
    return len(path.read_text().splitlines()) if path.exists() else 0


class Workspace:
    def __init__(self, base: Path, name: str):
        self.base = base / name
        self.root = self.base / "data"
        self.root.mkdir(parents=True)
        (self.base / "home").mkdir()
        os.environ.update(
            HOME=str(self.base / "home"),
            CAREERBREW_HOME=str(self.root),
            CAREERBREW_KEY_FILE=str(self.base / "keys/backup.key"),
            CAREERBREW_EVENTS="off",
            CAREERBREW_ENCRYPT="on",
            CAREERBREW_NOW="2026-11-02T09:00:00+00:00",
            CAREERBREW_TEST_OWNER="1",
            CAREERBREW_RUST13_WRITE_TRACE=str(self.base / "writes.log"),
            TZ="UTC",
        )
        os.environ.pop("CAREERBREW_TEST_BUSINESS", None)
        os.environ.pop("CAREERBREW_GOAL_ACTIVE", None)
        with contextlib.redirect_stdout(io.StringIO()):
            cli.main(["init"])
        trace_reset(self.base)


def at(stamp: str) -> None:
    os.environ["CAREERBREW_NOW"] = stamp


def moment(stamp: str) -> datetime:
    return datetime.fromisoformat(stamp)


DAY = date(2026, 11, 2)


def update(step: str, names: list[str], state: str, stamp: str, day: date = DAY, **extra: Any) -> dict[str, Any]:
    at(stamp)
    return status.update(step, names, state, day=day, **extra)


# ---------- gate: proofs, timeouts and retries ----------


def gate_case(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "gate-check")
    names = ["/careerbrew", "/standup", "/long", "/fresh"]
    planned = [
        {"step": "/careerbrew", "timeout": 30},
        {"step": "/standup", "timeout": 30},
        {"step": "/long", "timeout": 5, "long_running": True},
        {"step": "/fresh", "timeout": 30},
    ]
    nexts = [
        gate.next_action("/careerbrew", state, cls)
        for state, cls in [("blocked", ""), ("failed", "env"), ("failed", "auth"), ("failed", "network"), ("failed", "conflict"), ("failed", "tool"), ("failed", "other"), ("done", ""), ("waiting", "")]
    ]
    t = moment("2026-11-02T09:00:00+00:00")
    probes = [
        {"status": "done", "started": "2026-11-02T07:00:00+00:00"},
        {"status": "running"},
        {"status": "running", "started": ""},
        {"status": "running", "started": "not a time"},
        {"status": "running", "started": "2026-11-02T08:40:00+00:00"},
        {"status": "running", "started": "2026-11-02T08:30:00+00:00"},
        {"status": "running", "started": "2026-11-02T08:29:59+00:00"},
        {"status": "running", "started": "2026-11-02T03:29:00-05:00"},
        {"status": "running", "started": "2026-11-02T08:29+00:00"},
    ]
    overdue = [gate.overdue(row, 30, t) for row in probes]
    update("/standup", names, "running", "2026-11-02T08:00:00+00:00")
    update("/long", names, "running", "2026-11-02T08:00:00+00:00")
    update("/careerbrew", names, "running", "2026-11-02T08:40:00+00:00")
    update("/fresh", names, "running", "2026-11-02T08:50:00+00:00", summary=CANARY, needs_you="Ask " + CANARY)
    at("2026-11-02T09:00:00+00:00")
    first = gate.check(planned)
    second_run = update("/standup", names, "running", "2026-11-02T09:05:00+00:00")
    at("2026-11-02T09:40:00+00:00")
    second = gate.check(planned)
    trace_reset(ws.base)
    at("2026-11-02T09:41:00+00:00")
    third = gate.check(planned)
    idle_writes = trace_count(ws.base)
    rows = status.rows(names, DAY)
    return {
        "next": nexts,
        "overdue": overdue,
        "first": first,
        "second_attempt": second_run["attempt"],
        "second": second,
        "third": third,
        "idle_writes": idle_writes,
        "rows": [{k: r[k] for k in ("step", "status", "attempt", "retry", "timed_out", "error_class", "next", "started", "ended", "seconds")} for r in rows],
        "may_run": [gate.may_run("careerbrew"), gate.may_run("settings"), gate.may_run("plugin:acme")],
        "counts": status.counts(rows),
    }


# ---------- timing: durations, medians, slow flags and the history summary ----------


def timing_case(base: Path) -> dict[str, Any]:
    Workspace(base, "timing-history")
    names = ["/careerbrew", "/standup", "/new"]

    def ran(step: str, day: date, start: str, end_: str, final: str) -> None:
        d = day.isoformat()
        update(step, names, "running", f"{d}T{start}+00:00", day)
        extra: dict[str, Any] = {}
        if final == "blocked":
            extra["needs_you"] = "Owner picks"
        if final == "failed":
            extra["error_class"] = "network"
        update(step, names, final, f"{d}T{end_}+00:00", day, **extra)

    ran("/careerbrew", date(2026, 10, 26), "09:00:00", "09:01:00", "done")
    ran("/careerbrew", date(2026, 10, 27), "09:00:00", "09:02:01", "done")
    ran("/careerbrew", date(2026, 10, 28), "09:00:00", "09:03:00", "done")
    ran("/careerbrew", date(2026, 10, 29), "09:00:00", "09:10:00", "failed")
    ran("/careerbrew", date(2026, 10, 30), "09:00:00", "09:04:00", "done")
    ran("/standup", date(2026, 10, 29), "10:00:00", "10:00:30", "done")
    ran("/standup", date(2026, 10, 30), "10:00:00", "10:20:00", "blocked")
    ran("/standup", date(2026, 10, 31), "10:00:00", "10:00:40", "done")
    ran("/careerbrew", DAY, "08:00:00", "08:06:40", "done")
    ran("/standup", DAY, "08:00:00", "08:00:20", "done")
    update("/new", names, "running", "2026-11-02T08:30:00+00:00")
    at("2026-11-02T09:00:00+00:00")
    rows = status.rows(names, DAY)
    seconds = [None, 0, 45, 59, 60, 89, 90, 150, 210, 3599, 3600, 3629, 3631, 3900, 7170]
    declared = [
        {"step": "/careerbrew", "from": "careerbrew"},
        {"step": "/a", "from": "plugin:acme"},
        {"step": "/b", "from": "plugin:acme"},
        {"step": "/c", "from": "settings"},
        {"step": "/d", "from": "plugin:zeta"},
    ]
    return {
        "row_seconds": [r.get("seconds") for r in rows],
        "medians": [timing.median("/careerbrew", DAY), timing.median("/standup", DAY), timing.median("/careerbrew", date(2026, 10, 30)), timing.median("/new", DAY)],
        "flags": timing.flags(rows, DAY),
        "week": timing.summary(DAY),
        "three": timing.summary(DAY, 3),
        "shown": [timing.shown(s) for s in seconds],
        "labels": timing.labels(declared, ["/careerbrew", "/gone", "/a", "/old", "/d"]),
        "history_days": sorted(timing.history()["days"]),
    }


# ---------- loop records, the interval and the goal/cron warning ----------


def loop_records(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "loop-records")
    at("2026-11-01T22:00:00+00:00")
    early = [watch.add("watch", watch.SELF_ID), watch.add("cron", "cron-1", "/careerbrew")]
    at("2026-11-02T09:00:00+00:00")
    added = [
        watch.add("watch", watch.SELF_ID),
        watch.add("monitor", "mon-1", "careerbrew"),
        watch.add("cron", "cron-1"),
        watch.add("agent", "agent.one:2", "/standup"),
        watch.add("session", "sess-1", session_record="rec-1"),
        watch.add("session", "sess-1", session_record="rec-1"),
    ]
    errors = [
        error(lambda: watch.add("bogus", "x")),
        error(lambda: watch.add("cron", "bad id")),
        error(lambda: watch.add("cron", "")),
        error(lambda: watch.add("cron", "x" * 65)),
        error(lambda: watch.add("cron", "ok-id", "bad step!")),
        error(lambda: watch.add("session", "cron-1", session_record="rec-2")),
        error(lambda: watch.add("session", "sess-1", session_record="rec-2")),
        error(lambda: watch.stop("nope")),
    ]
    stopped = [watch.stop("mon-1"), watch.stop("nope", missing_ok=True), watch.stop("sess-1", session_record="other", missing_ok=True)]
    before_end = watch.status()
    envs = [{}, {"CAREERBREW_GOAL_ACTIVE": "1"}, {"CAREERBREW_GOAL_ACTIVE": "TRUE"}, {"CAREERBREW_GOAL_ACTIVE": "0"}, {"CAREERBREW_GOAL_ACTIVE": "maybe"}]
    cron = [cron_check.check(env)[0] for env in envs]
    intervals = [watch.interval(x) for x in ([], [10], [100, 200, 300], [1000, 1000, 1000], [5000], [49.9], [100, 200], [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 400])]
    sample = ws.base / "input.txt"
    missing = watch.fingerprint(["file:/nonexistent/rust16-missing"])
    feed = watch.fingerprint(["feed:chat"])
    sample.write_text("one")
    fp1 = watch.fingerprint([f"file:{sample}"])
    fp2 = watch.fingerprint([f"file:{sample}"])
    sample.write_text("two two")
    fp3 = watch.fingerprint([f"file:{sample}"])
    ended = watch.end()
    after = watch.status()
    cron_after = cron_check.check({})[0]
    bad = Workspace(base, "loop-invalid")
    invalid = []
    for document in (
        [],
        {"passes": {}},
        {"inputs": []},
        {"loops": [{"id": "x", "kind": "weird"}]},
        {"loops": [{"id": "", "kind": "cron"}]},
        {"loops": [{"id": "x", "kind": "cron", "step": "", "day": "2026-11-02", "started": "2026-11-02T09:00+00:00"}]},
        {"loops": [{"id": "x", "kind": "cron", "step": "", "day": "2026-13-02", "started": "2026-11-02T09:00+00:00", "stopped": ""}]},
        {"loops": [{"id": "x", "kind": "cron", "step": "", "day": "2026-11-02", "started": "later", "stopped": ""}]},
        {"loops": [{"id": "x", "kind": "cron", "step": "", "day": "2026-11-02", "started": "2026-11-02T09:00+00:00", "stopped": "soon"}]},
        {"loops": [{"id": "x", "kind": "cron", "step": "", "day": "2026-11-02", "started": "2026-11-02T09:00+00:00", "stopped": ""}]},
    ):
        store.write_json(watch.state_path(), document)
        invalid.append(error(watch.status))
    del bad
    return {
        "early": early,
        "added": added,
        "errors": errors,
        "stopped": stopped,
        "before_end": before_end,
        "cron": cron,
        "intervals": intervals,
        "fingerprints": [missing, feed, fp1 == fp2, fp1 != fp3, len(fp1)],
        "ended": ended,
        "after": after,
        "cron_after": cron_after,
        "invalid": invalid,
    }


# ---------- one pass of the watch loop ----------

PASS_STEPS: list[dict[str, Any]] = []
LIVE = {"status": "ok"}


class Clock:
    def __init__(self) -> None:
        self.values = [100.0, 112.5, 200.0, 230.25, 300.0, 300.0, 400.0, 461.0, 500.0, 512.0, 600.0, 612.0, 700.0, 712.0]

    def __call__(self) -> float:
        return self.values.pop(0)


class Runner:
    def __init__(self) -> None:
        self.calls: list[list[Any]] = []

    def __call__(self, command: str, seconds: float) -> tuple[bool, str]:
        self.calls.append([command, seconds])
        return (False, "exit 1") if command == "false" else (True, "")


def patch_plan() -> None:
    plan.build = lambda *args, **kwargs: {"steps": [dict(s) for s in PASS_STEPS]}
    watch._live_cycle = lambda: dict(LIVE)


def loop_pass(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "loop-pass")
    sample = ws.base / "input.txt"
    sample.write_text("one")
    PASS_STEPS[:] = [
        {"step": "/careerbrew", "from": "careerbrew", "cycle": "careerbrew work feed cycle", "watch": [], "timeout": 30},
        {"step": "/repo", "from": "settings", "cycle": "sync-repo --quick", "watch": [f"file:{sample}"], "timeout": 10},
        {"step": "/plugin", "from": "plugin:acme", "cycle": "acme run", "watch": [], "timeout": 30},
        {"step": "/fail", "from": "settings", "cycle": "false", "watch": [], "timeout": 0},
        {"step": "/quiet", "from": "settings", "cycle": "", "watch": [], "timeout": 30},
    ]
    patch_plan()
    clock, runner = Clock(), Runner()
    out: dict[str, Any] = {}
    out["first"] = watch.run_pass(clock=clock, runner=runner)
    at("2026-11-02T09:20:00+00:00")
    out["second"] = watch.run_pass(clock=clock, runner=runner)
    sample.write_text("two two")
    at("2026-11-02T09:40:30+00:00")
    LIVE["status"] = "failed"
    out["third"] = watch.run_pass(clock=clock, runner=runner)
    LIVE["status"] = "ok"
    with watch.lock_path().open("a+") as held:
        fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
        out["skipped"] = watch.run_pass(clock=clock, runner=runner)
        fcntl.flock(held, fcntl.LOCK_UN)
    at("2026-11-02T10:00:00+00:00")
    out["end"] = watch.end()
    trace_reset(ws.base)
    out["ended"] = watch.run_pass(clock=clock, runner=runner, restart=False)
    out["ended_writes"] = trace_count(ws.base)
    at("2026-11-02T10:05:00+00:00")
    out["restarted"] = watch.run_pass(clock=clock, runner=runner)
    out["calls"] = runner.calls
    out["status"] = watch.status()
    return out


# ---------- turn-start catch-up ----------


def catch_up(base: Path) -> dict[str, Any]:
    Workspace(base, "catch-up")
    PASS_STEPS[:] = [{"step": "/careerbrew", "from": "careerbrew", "cycle": "careerbrew work feed cycle", "watch": [], "timeout": 30}]
    patch_plan()
    LIVE["status"] = "ok"
    clock, runner = Clock(), Runner()
    original = watch.run_pass

    def run_pass(restart: bool = True) -> dict[str, Any]:
        return original(clock=clock, runner=runner, restart=restart)

    watch.run_pass = run_pass
    try:
        out: dict[str, Any] = {}
        out["none"] = watch.catch_up()
        out["first_pass"] = run_pass()
        at("2026-11-02T09:03:00+00:00")
        out["early"] = watch.catch_up()
        at("2026-11-02T09:10:00+00:00")
        out["late"] = watch.catch_up()
        out["again"] = watch.catch_up()
        at("2026-11-02T09:30:00+00:00")
        due = watch._catchup_due(watch._load(), moment("2026-11-02T09:30:00+00:00"))
        with watch._changing() as st:
            st["catchup"] = {"due": due, "pid": os.getpid(), "at": "2026-11-02T09:29:00.000000+00:00", "status": "running"}
        out["busy"] = watch.catch_up()
        with watch._changing() as st:
            st["catchup"] = {"due": due, "pid": 0, "at": "2026-11-02T09:29:00.000000+00:00", "status": "running"}
        out["crashed"] = watch.catch_up()
        with watch._changing() as st:
            st["catchup"] = {"due": "loop:2026-11-02T09:31:00+00:00", "pid": 0, "at": "x", "status": "done"}
            st["next_at"] = "2026-11-02T09:31"
        at("2026-11-02T09:40:00+00:00")
        out["done_claim"] = watch.catch_up()
        probes = []
        late = moment("2026-11-02T17:10:00+00:00")
        st = watch._load()
        for next_at in ("2026-11-02T17:00+00:00", "2026-11-01T17:00+00:00", "2026-11-02T17:05+00:00", "garbage", "", "2026-11-02T12:04:00-05:00"):
            probes.append(watch._catchup_due({**st, "next_at": next_at}, late))
        probes.append(watch._catchup_due({**st, "next_at": "", "ended": "2026-11-02T16:00+00:00"}, late))
        probes.append(watch._catchup_due({**st, "next_at": "", "ended": "2026-11-01T16:00+00:00"}, late))
        probes.append(watch._catchup_due({**st, "next_at": ""}, moment("2026-11-02T17:05:00+00:00")))
        at("2026-11-02T17:10:00+00:00")
        end.control(at="16:00")
        probes.append(watch._catchup_due({**st, "next_at": ""}, late))
        end.control(hold=True)
        probes.append(watch._catchup_due({**st, "next_at": ""}, late))
        out["probes"] = probes
        out["status"] = {k: v for k, v in watch.status().items() if k != "running"}
        return out
    finally:
        watch.run_pass = original


# ---------- the movable, holdable wrap-up ----------


def wrap_control(base: Path) -> dict[str, Any]:
    Workspace(base, "wrap-control")
    out: dict[str, Any] = {}
    out["default"] = [x.isoformat() for x in end.times()]
    out["view"] = end.view()
    out["errors"] = [error(lambda a=a: end.control(at=a)) for a in ("25:00", "9:00", "23:31", "12:60", "noon", "16:00:00")]
    out["noop"] = end.control()
    out["moved"] = end.control(at="16:00")
    out["moved_times"] = [x.isoformat() for x in end.times()]
    out["phases"] = [end.phase(moment(s)) for s in ("2026-11-02T15:59:00+00:00", "2026-11-02T16:00:00+00:00", "2026-11-02T16:10:00+00:00")]
    out["tick_before"] = end.tick(moment("2026-11-02T15:59:00+00:00"))
    out["late_move"] = end.control(at="23:29")
    out["held"] = end.control(hold=True)
    out["held_tick"] = end.tick(moment("2026-11-02T23:50:00+00:00"))
    out["held_phase"] = end.phase(moment("2026-11-02T23:50:00+00:00"))
    out["unheld"] = end.control(at="10:00")
    out["day_file_keys"] = sorted(status._data(DAY))
    out["wrap_control"] = {k: v for k, v in status._data(DAY)["wrap_control"].items()}
    data = status._data(DAY)
    data["end"] = {"at": "2026-11-02T12:00+00:00", "by": "owner"}
    store.write_json(status.path_for(DAY), data)
    out["after_end"] = error(lambda: end.control(at="11:00"))
    out["ended_phase"] = end.phase()
    out["ended_tick"] = end.tick()
    out["outcomes"] = end.step_outcomes([{"status": "done"}, {"status": "blocked", "needs_you": "x"}, {"status": "failed"}, {"status": "waiting"}, {"status": "odd"}])
    return out


def run(case: str, base: Path) -> dict[str, Any]:
    return {
        "gate-check": gate_case,
        "timing-history": timing_case,
        "loop-records": loop_records,
        "loop-pass": loop_pass,
        "catch-up": catch_up,
        "wrap-control": wrap_control,
    }[case](base)


def main() -> int:
    args = sys.argv[1:]
    if args not in (["--all"],) and not (len(args) == 2 and args[0] == "--case" and args[1] in CASES):
        print("rust16.py: --all | --case <id>", file=sys.stderr)
        return 2
    selected = CASES if args == ["--all"] else (args[1],)
    root = os.environ.get("CAREERBREW_HOME", "")
    if not root:
        print("rust16.py: CAREERBREW_HOME is required", file=sys.stderr)
        return 2
    base = Path(root) / "rust16-workload"
    base.mkdir(parents=True, exist_ok=True)
    original_write_text = store.write_text

    def traced(path: Path, text: str, **kwargs: Any) -> None:
        trace = os.environ.get("CAREERBREW_RUST13_WRITE_TRACE")
        if trace:
            with Path(trace).open("a") as fh:
                fh.write(str(path) + "\n")
        return original_write_text(path, text, **kwargs)

    store.write_text = traced
    report = {"cases": [{"id": case, "result": run(case, base)} for case in selected]}
    sys.stdout.write(compact(report))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
