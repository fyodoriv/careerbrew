import json
import os
import signal
import subprocess
import time
from datetime import date, datetime
from zoneinfo import ZoneInfo

import pytest

from careerbrew import calendar, store
from careerbrew.day import planner, results, watch
from careerbrew.work import evidence, priorities

from . import watch_gate_child
from .conftest import FIXTURES

TZ = ZoneInfo("Etc/GMT+4")
DAY = date(2026, 10, 19)  # a Monday


@pytest.fixture
def cal(home, monkeypatch):
    (home / "day").mkdir(parents=True, exist_ok=True)
    (home / "day" / "calendars.yaml").write_text(f"calendars:\n  - {{kind: ics, path: '{FIXTURES}/calendar/work.ics'}}\n  - {{kind: graph, token: env:CB_GRAPH}}\n")
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http"))
    monkeypatch.setenv("CB_GRAPH", "test-token")
    return home


def test_calendar_merges_ics_recurring_and_graph(cal):
    evs, errors = calendar.events_for(DAY)
    assert errors == []
    busy = [(e.start.strftime("%H:%M"), e.title) for e in evs if e.busy]
    assert busy == [("09:30", "Team standup"), ("13:00", "1:1 with manager"), ("15:00", "Design review")]
    assert {e.title for e in evs if not e.busy} >= {"Company holiday in another office", "Optional talk", "Focus time"}


def test_literal_ics_link_is_refused(home):
    (home / "day").mkdir(parents=True, exist_ok=True)
    (home / "day" / "calendars.yaml").write_text("calendars:\n  - {kind: ics, url: 'https://calendar.example.com/private-abc/basic.ics'}\n")
    evs, errors = calendar.events_for(DAY)
    assert evs == [] and errors == ["ics: SecretError"]


def test_plan_uses_free_blocks_calibration_and_carry_over(cal):
    priorities.add("write design doc", 3, 3, 2, ["design"])
    priorities.add("fix flaky test", 3, 3, 1, [])
    priorities.add("huge migration", 5, 1, 9, [])
    store.append_jsonl(cal / "metrics.jsonl", [{"metric": "estimate_ratio", "value": 1.5, "at": "2026-10-10"}])
    plan = planner.build(DAY)
    assert plan["calibration"] == 1.5
    assert [m["title"] for m in plan["meetings"]] == ["Team standup", "1:1 with manager", "Design review"]
    starts = {s["task"]: s["start"][11:16] for s in plan["schedule"]}
    assert starts["t002"] < "13:00" or starts["t001"] < "13:00"
    assert "t003" in plan["unplaced"], "a 9h task (13.5h calibrated) cannot fit"
    for s in plan["schedule"]:  # no task overlaps a meeting
        for m in plan["meetings"]:
            assert s["end"] <= m["start"] or s["start"] >= m["end"]
    # tomorrow: yesterday's planned open tasks come first
    nxt = planner.build(date(2026, 10, 20))
    assert nxt["schedule"][0]["carry_over"] is True


def test_next_step_prepare_meeting_task_and_after_hours(cal):
    priorities.add("fix flaky test", 3, 3, 0.5, [])
    planner.build(DAY)
    at = lambda h, m: datetime(2026, 10, 19, h, m, tzinfo=TZ)  # noqa: E731
    assert planner.next_step(at(9, 22))["kind"] == "prepare"
    assert planner.next_step(at(9, 35))["kind"] == "meeting"
    task = planner.next_step(at(10, 0))
    assert task["kind"] == "task" and task["task"] == "t001" and task["reason"]
    assert planner.next_step(at(18, 30))["kind"] == "done"


def test_results_append_is_idempotent(home):
    rows = [{"id": "a", "at": "x", "kind": "commit", "title": "t"}]
    assert results.append(rows, DAY) == 1
    assert results.append(rows, DAY) == 0
    assert results.summary(DAY) == {"commit": 1}


def test_watch_once_appends_today_evidence_and_is_cheap(home, pytestconfig):
    root = home.parent / "watch-cycle"
    root.mkdir(mode=0o700)
    for folder in ("home", "tmp", "config", "cache", "state"):
        (root / folder).mkdir(mode=0o700)
    (root / ".owner").write_text(watch_gate_child.MARKER, encoding="utf-8")
    pressure = watch_gate_child.PRESSURE_MIB if getattr(pytestconfig, "_careerbrew_watch_child_pressure", False) else 0
    argv = watch_gate_child.command(pressure)
    with (root / "stdout.json").open("xb") as stdout, (root / "stderr.txt").open("xb") as stderr:
        child = subprocess.Popen(argv, cwd=root, env=watch_gate_child.environment(root), stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr)
        timed_out = False
        try:
            child.wait(timeout=45)
        except subprocess.TimeoutExpired:
            timed_out = True
            child.kill()
            child.wait(timeout=3)
        except BaseException:
            if child.returncode is None:
                child.kill()
                child.wait(timeout=3)
            raise
    (root / "terminal.json").write_text(
        json.dumps({"pid": child.pid, "exit": child.returncode, "timed_out": timed_out, "reaped": True}),
        encoding="utf-8",
    )
    assert child.returncode == 0 and not timed_out and not (root / "stderr.txt").read_bytes(), "watch child setup or cleanup failed"
    record = json.loads((root / "stdout.json").read_bytes())
    assert (record["pid"], record["ppid"], record["pressure_mib"], record["denied"]) == (child.pid, os.getpid(), pressure, [])
    first, second = record["cycles"]
    assert first["added"] == 1 and first["cpu_ms"] < 500
    assert first["rss_mb"] < 120
    assert second["added"] == 0
    assert record["cursors"] == {"evidence": True, "calendar": True}


def test_paused_cycle_skips_sources(home):
    evidence.note("feat: shipped search", ["delivery"])
    watch.write_state(state="paused")
    assert watch.run_cycle()["added"] == 0


def _env(home):
    return {**os.environ, "CAREERBREW_NOTIFY": "off", "CAREERBREW_EVENTS": "on"}


def _wait(cond, timeout=15):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if cond():
            return True
        time.sleep(0.1)
    return False


def test_background_watcher_single_instance_pause_resume_kill_and_resume(home):
    evidence.note("feat: first", ["delivery"])
    st = watch.start(interval=1)
    try:
        assert _wait(watch.running)
        assert _wait(lambda: results.summary().get("note") == 1)
        assert watch.start(interval=1)["note"] == "already running"
        nice_before = os.getpriority(os.PRIO_PROCESS, 0)
        with pytest.raises(RuntimeError, match="already running"):
            watch.loop(interval=1, max_cycles=1)
        assert os.getpriority(os.PRIO_PROCESS, 0) == nice_before, "a refused second instance must not renice the caller"
        watch.pause()
        time.sleep(1.5)
        evidence.note("feat: while paused", ["delivery"])
        time.sleep(2.5)
        assert results.summary().get("note") == 1, "a paused watcher appends nothing"
        watch.resume()
        assert _wait(lambda: results.summary().get("note") == 2)
        # hard kill (no cleanup), then a fresh start resumes without duplicates
        os.kill(int(st["pid"]), signal.SIGKILL)
        assert _wait(lambda: not watch.running())
        evidence.note("feat: after kill", ["delivery"])
        watch.start(interval=1)
        assert _wait(lambda: results.summary().get("note") == 3)
        ids = [r["id"] for r in results.load()]
        assert len(ids) == len(set(ids))
    finally:
        watch.stop()
    assert not watch.running()


def test_cli_watch_status_and_once(home, capsys):
    from careerbrew.cli import main

    evidence.note("feat: x", ["delivery"])
    assert main(["day", "watch", "once"]) == 0
    assert main(["day", "watch", "status"]) == 0
    out = capsys.readouterr().out
    assert "1 new result" in out and "state: off" in out and "today: note=1" in out


def test_plan_week_places_each_task_once_on_workdays_only(cal):
    priorities.add("write design doc", 3, 3, 2, ["design"])
    priorities.add("fix flaky test", 3, 3, 1, [])
    priorities.add("big migration", 5, 2, 6, [])
    week = planner.plan_week(date(2026, 10, 23), days=4)  # Fri, Sat, Sun, Mon
    names = [d["weekday"] for d in week]
    assert names == ["Fri", "Sat", "Sun", "Mon"]
    sat, sun = week[1], week[2]
    assert sat["workday"] is False and sat["schedule"] == [] and sun["schedule"] == []
    placed = [s["task"] for d in week for s in d["schedule"]]
    assert sorted(placed) == sorted(set(placed)), "a task is planned once in the week"
    assert set(placed) == {"t001", "t002", "t003"}
    assert not list((cal / "day").glob("plan-*.json")), "planning ahead writes nothing"


def test_no_calendar_says_not_connected_instead_of_zero_meetings(home):
    # #54: without a calendar, "0 meetings" and a focus number were shown as facts
    plan = planner.build(DAY, persist=False)
    assert plan["calendar_connected"] is False
    assert all(d["calendar_connected"] is False for d in planner.plan_week(DAY, 3))


def test_a_configured_calendar_is_connected(cal):
    assert planner.build(DAY, persist=False)["calendar_connected"] is True


def test_plan_week_says_a_task_is_too_big_for_one_day_not_no_room(home):
    """US-75 pictures: Week said "No room in these 14 days" for a 12h task while each day had 8h 30m free."""
    big = priorities.add("plan the Q1 platform roadmap", 4, 3, 12, [])
    week = planner.plan_week(DAY, 14)
    last = week[-1]
    assert last["unplaced"] == [], "the days have room: nothing waits for lack of room"
    assert last["too_big"] == [{"task": big["id"], "minutes": 720}]
    assert last["day_focus_minutes"] == 408, "8h 30m free x focus factor 0.8"
    labelled = server_week_label(week)
    assert labelled["too_big"][0]["title"] == "plan the Q1 platform roadmap"


def test_plan_week_keeps_no_room_for_full_days_apart_from_too_big(home):
    for i in range(4):
        priorities.add(f"big ticket {i}", 2, 2, 6, [])
    priorities.add("plan the Q1 platform roadmap", 4, 3, 12, [])
    last = planner.plan_week(DAY, 2)[-1]
    assert last["unplaced"] == ["t003", "t004"], "two days hold one 6h ticket each; the rest wait for room"
    assert [b["task"] for b in last["too_big"]] == ["t005"]


def test_plan_week_today_matches_the_day_plan_with_carry_over(home):
    """The week's first day is today's plan (free time from now, yesterday's open tasks first), so no task is planned twice."""
    priorities.add("write design doc", 3, 3, 2, ["design"])
    priorities.add("fix flaky test", 3, 3, 1, [])
    priorities.add("small cleanup", 1, 1, 1.5, [])
    store.write_json(planner.plan_path(date(2026, 10, 18)), {"schedule": [{"task": "t003"}]})  # yesterday planned the cleanup
    now = datetime(2026, 10, 19, 14, 0, tzinfo=TZ)
    plan = planner.build(DAY, now, persist=False)
    first = planner.plan_week(DAY, 3, now)[0]
    assert [(s["task"], s["start"], s["carry_over"]) for s in first["schedule"]] == [(s["task"], s["start"], s["carry_over"]) for s in plan["schedule"]]
    assert first["schedule"][0]["task"] == "t003", "yesterday's open task comes first"
    assert first["free_minutes"] == plan["free_minutes"] == 210, "free time left after 14:00"


def server_week_label(week):
    from careerbrew import server

    return server._label_plan(week[-1], *server._task_labels())


_NEXT_FEED_AT = "2026-10-19T15:00:00-04:00"
_NEXT_FEED_NOW = "2026-10-19T17:16:00-04:00"
_NEXT_FEED_URL = "https://example.com/next-step/"
_NEXT_FEED_FALLBACK = "14 min free and nothing fits: review a PR or clear your inbox."


def _next_feed_row(event_id, kind="dm", **overrides):
    return {
        "id": event_id,
        "kind": kind,
        "at": _NEXT_FEED_AT,
        "title": f"Synthetic next step {event_id}",
        "url": _NEXT_FEED_URL + event_id,
        "to_me": True,
        **overrides,
    }


def _next_feed_world(monkeypatch, root, rows, now=_NEXT_FEED_NOW):
    from careerbrew import config
    from careerbrew.work import feed

    monkeypatch.setenv("CAREERBREW_HOME", str(root))
    monkeypatch.setenv("CAREERBREW_NOW", now)
    config.set_value("work_end", "17:30")
    assert feed.ingest(rows)["new"] == len(rows)


def _assert_next_feed(nxt, row):
    assert nxt["kind"] == "feed", nxt
    assert nxt["title"] == row["title"]
    assert isinstance(nxt["text"], str) and nxt["text"].strip()
    assert row["title"] not in nxt["text"] and row["url"] not in nxt["text"]
    instruction = nxt["text"].casefold()
    if row["kind"] == "review":
        assert "start" in instruction and "review" in instruction
    else:
        assert "read" in instruction and "respond" in instruction
    assert "task" not in nxt
    assert len(nxt["links"]) == 1
    link = nxt["links"][0]
    assert set(link) == {"label", "url"}
    assert isinstance(link["label"], str) and link["label"].strip()
    assert link["url"] == row["url"]


@pytest.mark.parametrize("case", ["P01", "P02", "P03"])
def test_next_step_open_feed_primary(case, home, monkeypatch, tmp_path):
    from careerbrew import server
    from careerbrew.work import feed

    ask = _next_feed_row("ask-a")
    review = _next_feed_row("review", "review", at="2026-10-19T15:01:00-04:00")
    rows = {
        "P01": [ask],
        "P02": [review],
        "P03": [_next_feed_row("ask-b"), ask, review],
    }[case]
    orders = [rows, rows[::-1]] if case == "P03" else [rows]
    chosen = review if case == "P02" else ask
    observed = []
    for i, order in enumerate(orders):
        with monkeypatch.context() as world:
            _next_feed_world(world, home / str(i), order)
            assert {row["id"] for row in feed.open_asks()} == {row["id"] for row in order}
            if case == "P03":
                # Ingest sorts by timestamp and ID. Reverse the persisted input too.
                by_id = {row["id"]: row for row in feed.load()}
                store.write_jsonl(feed.events_path(), [by_id[row["id"]] for row in order])
                assert [row["id"] for row in feed.load()] == [row["id"] for row in order]
            observed.append({"planner": planner.next_step(), "today": server.today()["next"]})
    store.write_json(tmp_path / f"{case}-observations.json", observed)
    for observation in observed:
        _assert_next_feed(observation["planner"], chosen)
        _assert_next_feed(observation["today"], chosen)


def test_next_step_open_feed_after_only_excludes_closed_and_personal(home, monkeypatch):
    from careerbrew.work import feed

    winner = _next_feed_row("open", at="2026-10-19T15:10:00-04:00")
    _next_feed_world(
        monkeypatch,
        home,
        [
            _next_feed_row("answered", thread="synthetic-thread"),
            _next_feed_row("reply", "message", actor="me", to_me=False, thread="synthetic-thread", at="2026-10-19T15:01:00-04:00"),
            _next_feed_row("marked"),
            _next_feed_row("personal"),
            winner,
        ],
    )
    feed.mark_ask("marked", "done")
    feed.mark("personal", personal=True)
    assert [row["id"] for row in feed.open_asks()] == ["open"]
    _assert_next_feed(planner.next_step(), winner)


def test_next_step_open_feed_before_guard_invalid_rows(home, monkeypatch):
    from careerbrew.work import feed

    _next_feed_world(
        monkeypatch,
        home,
        [
            _next_feed_row("unsafe", url="javascript:void(0)"),
            _next_feed_row("credentials", url="https://sample:sample@example.com/next-step/credentials"),
            _next_feed_row("no-link", url=""),
            _next_feed_row("malformed"),
        ],
    )
    rows = feed.load()
    assert len(rows) == 4
    for row in rows:
        if row["id"] == "malformed":
            row["at"] = "invalid-timestamp"
    store.write_jsonl(feed.events_path(), rows)
    assert {row["id"] for row in feed.open_asks()} == {"unsafe", "credentials", "no-link", "malformed"}
    assert planner.next_step() == {"kind": "small", "text": _NEXT_FEED_FALLBACK}


def test_next_step_open_feed_after_only_minute_boundaries(home, monkeypatch):
    observations = []
    for kind, minutes, minute_at, fits in [("review", 9, 21, False), ("review", 10, 20, True), ("dm", 4, 26, False), ("dm", 5, 25, True)]:
        row = _next_feed_row(kind, kind)
        with monkeypatch.context() as world:
            _next_feed_world(world, home / str(minutes), [row], f"2026-10-19T17:{minute_at}:00-04:00")
            observations.append((planner.next_step(), row, minutes, fits))
    for nxt, row, minutes, fits in observations:
        if fits:
            _assert_next_feed(nxt, row)
        else:
            assert nxt == {"kind": "small", "text": f"{minutes} min free and nothing fits: review a PR or clear your inbox."}


def test_next_step_open_feed_before_guard_priority(home, monkeypatch):
    from careerbrew.work import projects

    cases = [
        ("meeting", "2026-10-19T09:35:00-04:00", {"until": "2026-10-19T09:45-04:00"}),
        ("prepare", "2026-10-19T09:22:00-04:00", {"meeting": "Team standup"}),
        ("done", "2026-10-19T17:30:00-04:00", {}),
        ("gate", "2026-11-03T09:00:00+00:00", {"task": "gate:demo:tool"}),
        ("task", "2026-10-19T09:00:00-04:00", {"task": "t001", "title": "Synthetic fitting task"}),
        ("log", _NEXT_FEED_NOW, {}),
    ]
    observations = []
    for kind, now, expected in cases:
        with monkeypatch.context() as world:
            root = home / kind
            _next_feed_world(world, root, [_next_feed_row("priority-ask", at="2026-10-19T08:00:00-04:00")], now)
            if kind in ("meeting", "prepare"):
                store.write_yaml(root / "day" / "calendars.yaml", {"calendars": [{"kind": "ics", "path": str(FIXTURES / "calendar/work.ics")}]})
            elif kind == "gate":
                projects.add("demo", kind="prep", event_on="2026-11-03")
            elif kind == "task":
                priorities.add("Synthetic fitting task", 3, 3, 0.5, [])
            elif kind == "log":
                results.append([{"id": "synthetic-shipped", "at": now, "kind": "pr", "title": "Synthetic shipped work"}], DAY)
            observations.append((planner.next_step(), kind, expected))
    for nxt, kind, expected in observations:
        assert nxt["kind"] == kind, nxt
        assert {key: nxt[key] for key in expected} == expected


def _next_feed_cli_observations(home, monkeypatch, capsys, *, owner_output):
    from argparse import Namespace

    from careerbrew import actions, cli, plugins

    row = _next_feed_row("cli", title="Synthetic restricted title 374", url=_NEXT_FEED_URL + "restricted-link-374")
    _next_feed_world(monkeypatch, home, [row])
    assert not cli.sys.stdout.isatty()
    assert plugins.effective_policy()["agent_may_read_business"] is False
    notifications = []
    monkeypatch.setattr(watch, "notify", notifications.append)
    suggestions = home / "day" / "suggestions.jsonl"
    assert store.read_jsonl(suggestions) == []
    token = actions.OWNER_UI.set(False)
    try:
        assert cli.cmd_day_next(Namespace()) == 0
        restricted_capture = capsys.readouterr()
        restricted = restricted_capture.out + restricted_capture.err
        if owner_output:
            actions.OWNER_UI.set(True)
            assert cli.cmd_day_next(Namespace()) == 0
            owner = capsys.readouterr().out
        else:
            owner = ""
    finally:
        actions.OWNER_UI.reset(token)
    watch._maybe_suggest()
    assert notifications == []
    assert store.read_jsonl(suggestions) == []
    return row, restricted, owner


def test_next_step_open_feed_before_guard_restricted_cli(home, monkeypatch, capsys):
    row, restricted, _ = _next_feed_cli_observations(home, monkeypatch, capsys, owner_output=False)
    assert row["title"] not in restricted and row["url"] not in restricted
    assert "next: " in restricted


def test_next_step_open_feed_after_only_cli_instruction(home, monkeypatch, capsys):
    row, restricted, owner = _next_feed_cli_observations(home, monkeypatch, capsys, owner_output=True)
    nxt = planner.next_step()
    _assert_next_feed(nxt, row)
    assert f"next: {nxt['text']}\n" in restricted
    assert row["title"] not in restricted and row["url"] not in restricted
    assert f"next: {nxt['text']} — {row['title']}\n" in owner
