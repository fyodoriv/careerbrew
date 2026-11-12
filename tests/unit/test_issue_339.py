"""Late cycles, movable wrap-up and fresh session handoffs (#339)."""

from datetime import date

import pytest

from careerbrew import cli, config
from careerbrew.work import sessions
from careerbrew.workday import end, watch


@pytest.fixture(autouse=True)
def morning(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T09:00:00+00:00")
    cli.main(["init"])
    config.set_value("run_pages", "false")
    config.set_value("work_end", "17:30")
    config.set_value("work_wrap_minutes", "30")


def test_held_wrap_never_ends_by_clock(monkeypatch):
    end._put(date(2026, 11, 2), "wrap_control", {"hold": True})
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T18:00:00+00:00")
    result = end.tick()
    assert not result["ended"]
    assert result["phase"] == "held"
    assert "end" not in end._data(date(2026, 11, 2))


def test_an_old_handoff_cannot_archive_a_session_before_wrap_report(monkeypatch):
    cli.main(["work", "project", "add", "wrap-demo"])
    sessions.open_session("wrap-demo", "wrap-agent")
    sessions.write_handoff("wrap-agent", "Earlier checkpoint")
    with sessions._changing() as state:
        state["sessions"][0]["wrap_requested_at"] = "2026-11-02T17:00:00+00:00"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T17:01:00+00:00")
    result = sessions.end("wrap-agent")
    assert not result["ended"]
    assert len(result["missing"]) == 1
    sessions.write_handoff("wrap-agent", "Current step saved and drafts updated; next step is tomorrow")
    assert len(sessions.end("wrap-agent")["ended"]) == 1


def test_wrap_can_move_later_and_hold_is_cleared(capsys):
    assert cli.main(["day", "wrap", "--hold"]) == 0
    assert cli.main(["day", "wrap", "--at", "18:15", "--json"]) == 0
    view = end.view()
    assert not view["hold"]
    assert view["at"].startswith("2026-11-02T18:15")
    assert view["end_at"].startswith("2026-11-02T18:45")


def test_late_pass_runs_once_per_recorded_due_time(monkeypatch):
    with watch._changing() as state:
        state["next_at"] = "2026-11-02T08:50:00+00:00"
    calls = []
    monkeypatch.setattr(watch, "run_pass", lambda **kwargs: calls.append(kwargs) or {"status": "ok"})
    assert watch.catch_up()["status"] == "ran"
    assert watch.catch_up()["status"] == "current"
    assert len(calls) == 1


def test_hold_blocks_clock_but_explicit_owner_end_ends_now(monkeypatch):
    end.control(hold=True)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T18:00:00+00:00")
    assert end.finish(by="clock") == {"phase": "held"}
    assert cli.main(["day", "end", "--json"]) == 0
    assert end.phase() == "ended"


def test_moved_wrap_does_not_run_at_the_old_work_end(monkeypatch):
    end.control(at="18:15")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T17:30:00+00:00")
    assert end.tick()["phase"] == "work"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T18:15:00+00:00")
    assert end.tick()["started"]


def test_catch_up_ignores_on_time_and_previous_day_jobs(monkeypatch):
    monkeypatch.setattr(watch, "run_pass", lambda **kwargs: pytest.fail("job not late"))
    for due in ("2026-11-02T08:55:00+00:00", "2026-11-01T08:50:00+00:00"):
        with watch._changing() as state:
            state["next_at"] = due
        assert watch.catch_up()["status"] == "current"


def test_two_turns_claim_only_one_late_pass(monkeypatch):
    from concurrent.futures import ThreadPoolExecutor
    from threading import Event

    with watch._changing() as state:
        state["next_at"] = "2026-11-02T08:50:00+00:00"
    entered, release = Event(), Event()
    calls = []

    def run(**kwargs):
        calls.append(kwargs)
        entered.set()
        assert release.wait(5)
        return {"status": "ok"}

    monkeypatch.setattr(watch, "run_pass", run)
    with ThreadPoolExecutor(max_workers=2) as pool:
        first = pool.submit(watch.catch_up)
        assert entered.wait(5)
        try:
            assert pool.submit(watch.catch_up).result(timeout=5)["status"] == "current"
        finally:
            release.set()
        assert first.result(timeout=5)["status"] == "ran"
    assert len(calls) == 1


def test_failed_or_skipped_catch_up_can_retry(monkeypatch):
    with watch._changing() as state:
        state["next_at"] = "2026-11-02T08:50:00+00:00"
    monkeypatch.setattr(watch, "run_pass", lambda **kwargs: {"status": "skipped"})
    assert watch.catch_up()["status"] == "skipped"
    monkeypatch.setattr(watch, "run_pass", lambda **kwargs: {"status": "ok"})
    assert watch.catch_up()["status"] == "ran"
    with watch._changing() as state:
        state["next_at"] = "2026-11-02T08:51:00+00:00"
    assert watch.catch_up()["status"] == "ran"


def test_goal_hook_warning_distinguishes_unknown_from_inactive():
    from careerbrew.workday import cron_check

    watch.add("cron", "synthetic-clock")
    unknown = cron_check.check({})[0]
    assert not unknown["ok"] and "unknown" in unknown["value"]
    assert not cron_check.check({"CAREERBREW_GOAL_ACTIVE": "1"})[0]["ok"]
    assert cron_check.check({"CAREERBREW_GOAL_ACTIVE": "0"})[0]["ok"]


def test_wrap_request_is_once_and_handoff_is_strictly_newer():
    from datetime import datetime

    cli.main(["work", "project", "add", "wrap-demo"])
    sessions.open_session("wrap-demo", "wrap-agent")
    assert sessions.request_wrap("first-wrap") == ["wrap-agent"]
    assert sessions.request_wrap("first-wrap") == []
    row = sessions.write_handoff("wrap-agent", "Saved current work")
    assert datetime.fromisoformat(row["handoff_at"]) > datetime.fromisoformat(row["wrap_requested_at"])
    assert sessions._fresh_handoff(row)
    assert sessions.request_wrap("moved-wrap") == ["wrap-agent"]
    assert not sessions._fresh_handoff(sessions.find("wrap-agent"))


def test_handoff_time_comparison_uses_offsets():
    row = {"handoff": "Saved", "wrap_requested_at": "2026-11-02T17:00:00+00:00", "handoff_at": "2026-11-02T13:00:01-04:00"}
    assert sessions._fresh_handoff(row)
    row["handoff_at"] = "2026-11-02T13:00:00-04:00"
    assert not sessions._fresh_handoff(row)


def test_generated_ui_action_moves_and_holds_the_same_clock():
    from careerbrew import actions

    assert actions.run("day.wrap", {"at": "18:10"})["exit"] == 0
    assert end.view()["at"].startswith("2026-11-02T18:10")
    assert actions.run("day.wrap", {"hold": True})["exit"] == 0
    assert end.phase() == "held"


def test_wrap_rejects_bad_times_without_changing_the_control():
    before = end.view()
    for value in ("25:00", "9:00", "17:60", "noon"):
        with pytest.raises(ValueError, match="HH:MM"):
            end.control(at=value)
    assert end.view() == before


def test_clock_ending_obeys_a_moved_wrap_even_when_called_directly(monkeypatch):
    end.control(at="18:15")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T17:30:00+00:00")
    assert end.finish(by="clock") == {"phase": "work"}
    assert "end" not in end._data(date(2026, 11, 2))


def test_repeated_end_keeps_a_project_pending_until_its_fresh_report(monkeypatch):
    cli.main(["work", "project", "add", "wrap-demo"])
    sessions.open_session("wrap-demo", "wrap-agent")
    sessions.write_handoff("wrap-agent", "Earlier checkpoint")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T18:00:00+00:00")
    assert cli.main(["day", "end", "--json"]) == 1
    assert cli.main(["day", "end", "--json"]) == 1
    assert not sessions.end("wrap-agent")["ended"]
    sessions.write_handoff("wrap-agent", "Current step complete and saved")
    assert sessions.end("wrap-agent")["ended"]
    assert cli.main(["day", "end", "--json"]) == 0


def test_yesterdays_ended_record_does_not_block_todays_late_job(monkeypatch):
    with watch._changing() as state:
        state["ended"] = "2026-11-01T17:30+00:00"
        state["next_at"] = "2026-11-02T08:50:00+00:00"
    calls = []
    monkeypatch.setattr(watch, "run_pass", lambda **kwargs: calls.append(kwargs) or {"status": "ok"})
    assert watch.catch_up()["status"] == "ran"
    assert len(calls) == 1


def test_a_day_ended_during_the_claim_is_not_reported_as_a_run(monkeypatch):
    with watch._changing() as state:
        state["next_at"] = "2026-11-02T08:50:00+00:00"
    monkeypatch.setattr(watch, "run_pass", lambda **kwargs: {"status": "ended"})
    assert watch.catch_up()["status"] == "ended"


def test_moved_wrap_cannot_cross_the_day_record_boundary():
    with pytest.raises(ValueError, match="same-day wrap time or hold"):
        end.control(at="23:30")
    assert end.view()["at"].startswith("2026-11-02T17:00")


def test_a_wrap_ending_before_midnight_is_valid():
    got = end.control(at="23:29")
    assert got["end_at"] == "2026-11-02T23:59+00:00"


def test_live_catch_up_claim_cannot_be_stolen_by_a_new_wrap_due(monkeypatch):
    from concurrent.futures import ThreadPoolExecutor
    from datetime import datetime
    from threading import Event

    from careerbrew import calendar

    clock = [datetime.fromisoformat("2026-11-02T09:00:00+00:00")]
    monkeypatch.setattr(calendar, "now", lambda: clock[0])
    with watch._changing() as state:
        state["next_at"] = "2026-11-02T08:50:00+00:00"
    entered, release = Event(), Event()
    calls = []

    def run(**kwargs):
        calls.append(kwargs)
        if len(calls) == 1:
            entered.set()
            assert release.wait(5)
        elif not release.is_set():
            return {"status": "skipped"}
        return {"status": "ok"}

    monkeypatch.setattr(watch, "run_pass", run)
    with ThreadPoolExecutor(max_workers=1) as pool:
        first = pool.submit(watch.catch_up)
        assert entered.wait(5)
        clock[0] = datetime.fromisoformat("2026-11-02T17:06:00+00:00")
        try:
            second = watch.catch_up()
        finally:
            release.set()
        result = first.result(timeout=5)
    assert second["status"] == "current"
    assert watch._load()["catchup"]["due"] == result["due"]
    assert len(calls) == 1
    assert watch.catch_up()["status"] == "ran"
    assert len(calls) == 2


def test_move_wins_before_a_clock_wrap_claim(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T17:00:00+00:00")
    original = end._put
    moved = []

    def put(day, key, value, **kwargs):
        if key == "wrapup" and not moved:
            moved.append(end.control(at="18:15"))
        return original(day, key, value, **kwargs)

    monkeypatch.setattr(end, "_put", put)
    result = end.tick()
    assert result["phase"] == "work"
    assert not result["started"]
    assert "wrapup" not in end._data(date(2026, 11, 2))


def test_move_wins_before_a_clock_end_claim(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T17:30:00+00:00")
    original = end._put
    moved = []

    def put(day, key, value, **kwargs):
        if key == "end" and not moved:
            moved.append(end.control(at="18:15"))
        return original(day, key, value, **kwargs)

    monkeypatch.setattr(end, "_put", put)
    assert end.finish(by="clock") == {"phase": "work"}
    assert "end" not in end._data(date(2026, 11, 2))
