"""Time saved: baselines are labeled measured or estimate, weeks group by local date, and only numbers are kept."""

import json

import pytest

from careerbrew import cli, config, time_saved


@pytest.fixture
def home(tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HOME", str(tmp_path))
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-26T09:00:00")
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    return tmp_path


def _log(home, rows):
    path = home / "logs" / "events.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(json.dumps(r) + "\n" for r in rows))
    return path


def test_the_setting_ids_match_the_actions():
    assert {k.removeprefix("time_baseline_") for k in config.DEFAULTS if k.startswith("time_baseline_")} == set(time_saved.ACTIONS)


def test_an_unset_baseline_is_an_estimate_never_measured(home):
    minutes, kind = time_saved.baseline("standup")
    assert minutes == 10
    assert kind.startswith("estimate, ")


def test_a_recorded_baseline_is_measured(home):
    time_saved.set_baseline("standup", "12")
    assert time_saved.baseline("standup") == (12.0, "measured")


def test_an_unknown_action_is_refused(home):
    with pytest.raises(ValueError, match="unknown action"):
        time_saved.set_baseline("lunch", "5")


def test_minutes_saved_are_the_baseline_minus_the_run_time_per_week(home):
    time_saved.set_baseline("standup", "12")
    log = _log(
        home,
        [
            {"ts": "2026-10-20T15:00:00Z", "event": "cli.run", "command": "work.standup", "saves": "standup", "status": "ok", "ms": 60000},
            {"ts": "2026-10-21T15:00:00Z", "event": "cli.run", "command": "work.standup", "saves": "standup", "status": "ok", "ms": 120000},
            {"ts": "2026-10-27T15:00:00Z", "event": "cli.run", "command": "day.plan", "saves": "day_plan", "status": "ok", "ms": 30000},
            {"ts": "2026-10-27T16:00:00Z", "event": "cli.run", "command": "day.plan", "saves": "day_plan", "status": "fail", "ms": 30000},
            {"ts": "2026-10-27T16:00:00Z", "event": "cli.run", "command": "config.show", "status": "ok", "ms": 1000},
        ],
    )
    data = time_saved.compute(log)
    assert [w["week"] for w in data["weeks"]] == ["2026-W43", "2026-W44"]
    assert data["weeks"][0]["minutes_saved"] == 21.0  # (12 - 1) + (12 - 2)
    assert data["weeks"][1]["minutes_saved"] == 9.5  # 10 (estimate) - 0.5; the failed run does not count
    assert data["runs_total"] == 3
    assert data["measured_baselines"] == 1


def test_a_cli_run_of_an_action_logs_its_action_and_seconds(home):
    cli.main(["init"])
    cli.main(["day", "plan"])
    rows = [json.loads(line) for line in (home / "logs" / "events.jsonl").read_text().splitlines()]
    run = [r for r in rows if r.get("event") == "cli.run" and r.get("command") == "day.plan"][-1]
    assert run["saves"] == "day_plan"
    assert isinstance(run["ms"], int)


def test_the_output_holds_numbers_and_ids_only(home):
    _log(home, [{"ts": "2026-10-20T15:00:00Z", "event": "cli.run", "saves": "standup", "status": "ok", "ms": 60000}])
    text = json.dumps(time_saved.compute())
    assert "private" not in text and "/" not in text


def test_field_totals_count_timed_runs_and_errors_under_a_salted_label(home):
    from careerbrew import field

    log = _log(
        home,
        [
            {"ts": "2026-10-20T15:00:00Z", "event": "cli.run", "command": "config.show", "status": "ok"},  # before field recording: no seconds
            {"ts": "2026-10-21T15:00:00Z", "event": "cli.run", "command": "work.standup", "saves": "standup", "status": "ok", "ms": 60000},
            {"ts": "2026-10-21T16:00:00Z", "event": "cli.run", "command": "day.plan", "status": "fail", "ms": 2000},
            {"ts": "2026-10-21T17:00:00Z", "event": "cli.error", "command": "day.plan", "error": "ValueError"},
        ],
    )
    data = field.compute(log)
    assert (data["runs"], data["runs_ok"], data["runs_failed"]) == (3, 1, 2)
    assert data["since"] == "2026-10-21"
    assert data["success_rate"] == 33.3
    assert data["minutes_saved"] == 9.0
    assert data["machine"].startswith("m-") and len(data["machine"]) == 12
    assert data["machine"] == field.compute(log)["machine"]  # stable on one machine
    import platform

    assert platform.node() not in json.dumps(data)


def test_field_totals_with_no_runs_say_so(home):
    from careerbrew import field

    data = field.compute(_log(home, []))
    assert data["runs"] == 0 and data["success_rate"] is None and data["since"] is None


# time-saved-user-time: the web app logs the owner's own seconds from a result's open to Copy, Send, or Accept.


def _web(ts, saves, step, seconds):
    return {"ts": ts, "event": "web.timing", "saves": saves, "step": step, "seconds": seconds}


def test_every_pm_update_and_standup_run_from_the_web_has_the_owners_time(home):
    log = _log(
        home,
        [
            _web("2026-10-26T14:00:00Z", "pm_update", "open", 0.4),
            _web("2026-10-26T14:03:00Z", "pm_update", "copy", 180),
            _web("2026-10-26T15:00:00Z", "standup", "open", 0.2),
            _web("2026-10-26T15:02:00Z", "standup", "copy", 120),
            _web("2026-10-27T14:00:00Z", "pm_update", "open", 0.3),
            _web("2026-10-27T14:04:00Z", "pm_update", "send", 240),
            _web("2026-10-27T16:00:00Z", "inbox", "accept", 30),
        ],
    )
    (week,) = time_saved.compute(log)["weeks"]
    pm, standup, inbox = (week["actions"][a] for a in ("pm_update", "standup", "inbox"))
    assert (pm["runs"], pm["runs_web"], pm["minutes_spent"], pm["time"]) == (2, 2, 7.0, "web")  # 3 + 4 minutes up to copy or send
    assert (standup["runs"], standup["minutes_spent"], standup["time"]) == (1, 2.0, "web")
    assert pm["minutes_saved"] == 23.0  # 2 x 15 (estimate) - 7
    assert (inbox["runs"], inbox["minutes_spent"]) == (1, 0.5)
    assert pm["opened"] == 2, "an open is counted but is not a run"


def test_a_cli_run_keeps_its_cli_time_and_says_so(home):
    log = _log(
        home,
        [
            {"ts": "2026-10-26T15:00:00Z", "event": "cli.run", "command": "work.standup", "saves": "standup", "status": "ok", "ms": 60000},
            _web("2026-10-26T16:00:00Z", "pm_update", "copy", 120),
            {"ts": "2026-10-27T15:00:00Z", "event": "cli.run", "command": "day.send", "saves": "pm_update", "status": "ok", "ms": 6000},
        ],
    )
    (week,) = time_saved.compute(log)["weeks"]
    assert week["actions"]["standup"]["time"] == "cli"
    assert (week["actions"]["pm_update"]["time"], week["actions"]["pm_update"]["runs_cli"], week["actions"]["pm_update"]["runs_web"]) == ("web+cli", 1, 1)
    assert week["actions"]["pm_update"]["minutes_spent"] == 2.1


def test_the_web_timing_event_is_content_free_and_checked(home):
    from careerbrew import server

    assert server.post("/api/timing", {"saves": "standup", "step": "copy", "seconds": 61.25}) == {"logged": True}
    (row,) = [json.loads(line) for line in (home / "logs" / "events.jsonl").read_text().splitlines()]
    assert {k: v for k, v in row.items() if k != "ts"} == {"event": "web.timing", "saves": "standup", "step": "copy", "seconds": 61.2}
    for bad in (
        {"saves": "lunch", "step": "copy", "seconds": 1},
        {"saves": "standup", "step": "paste", "seconds": 1},
        {"saves": "standup", "step": "copy", "seconds": -1},
        {"saves": "standup", "step": "copy", "seconds": True},
        {"saves": "standup", "step": "copy", "seconds": "Shipped the billing ledger"},
        {"saves": "standup", "step": "copy", "seconds": 90000},
    ):
        with pytest.raises(server.ApiError):
            server.post("/api/timing", bad)
    assert len((home / "logs" / "events.jsonl").read_text().splitlines()) == 1


def test_the_dashboard_block_reads_the_weekly_minutes_and_this_week(home):
    from urllib.parse import urlsplit

    from careerbrew import server

    _log(home, [_web("2026-10-26T14:03:00Z", "pm_update", "copy", 180)])
    data = server.view(urlsplit("/api/time-saved"))
    assert data["this_week"] == "2026-W44"
    assert data["weeks"][-1] == {**data["weeks"][-1], "week": "2026-W44", "start": "2026-10-26", "minutes_saved": 12.0}
    assert not (home / "out" / "time-saved.json").exists(), "a read writes no file"


def test_a_timing_post_keeps_the_view_cache(home):
    import urllib.request

    from careerbrew import server

    srv, _t = server.serve_in_thread(0, "tok")
    try:
        port = srv.server_address[1]
        cache = srv.RequestHandlerClass.cache
        cache.put("/api/today", "k", 60, b"{}")

        def post(path, body):
            req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=json.dumps(body).encode(), headers={"X-Careerbrew-Token": "tok", "Content-Type": "application/json"})
            with urllib.request.urlopen(req, timeout=10) as r:
                return r.status

        assert post("/api/timing", {"saves": "pm_update", "step": "open", "seconds": 0.5}) == 200
        assert cache.get("/api/today", "k") == b"{}", "logging a timing is not an owner's change: every view keeps its cached answer"
    finally:
        srv.shutdown()
        srv.server_close()
