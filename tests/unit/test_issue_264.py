"""Issue #264: one work-day watch loop with a cycle lock, an interval from measured cycle time, and recorded loops."""

from __future__ import annotations

import fcntl
import json

import pytest
import yaml

from careerbrew import cli, plugins
from careerbrew.work import live
from careerbrew.workday import plan, steps, watch

CANARY = "zq-cycle-canary-9"


@pytest.fixture(autouse=True)
def pinned(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-29T09:00+00:00")


class Clock:
    """A fake monotonic clock: each pass reads it twice (start, end), so a pass takes the next listed number of seconds."""

    def __init__(self, seconds: list[float]):
        self.seconds, self.t, self.reads = list(seconds), 0.0, 0

    def __call__(self) -> float:
        self.reads += 1
        if self.reads % 2 == 0:
            self.t += self.seconds.pop(0)
        return self.t


def _plugin(tmp_path, work_steps, allow_exec: bool = True, name: str = "watch-co") -> None:
    cli.main(["init"])
    d = tmp_path / name
    d.mkdir()
    (d / plugins.MANIFEST).write_text(yaml.safe_dump({"name": name, "version": "1.0.0", "work_steps": work_steps}))
    plugins.add(str(d), allow_exec=allow_exec)


def _step(result: dict, name: str) -> dict:
    return next(r for r in result["steps"] if r["step"] == name)


# ---------- acceptance: an overlapping cycle is skipped ----------


def test_a_pass_that_starts_while_one_runs_is_skipped(tmp_path, capsys):
    marker = tmp_path / "ran"
    _plugin(tmp_path, [{"step": "/repo-watch", "scope": "repo", "cycle": f"touch {marker}"}])
    with watch.lock_path().open("a+") as held:
        fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)  # another pass holds the lock
        got = watch.run_pass()
        capsys.readouterr()
        assert cli.main(["day", "loop", "pass"]) == 0, "a skipped pass is no error: the running one does the work"
        out = capsys.readouterr().out
    assert got == {"status": "skipped", "reason": "previous cycle running"}
    assert "skipped: previous cycle running" in out
    assert not marker.exists(), "a skipped pass runs no step cycle"
    assert watch.run_pass()["status"] == "failed", "the lock is free again, but this fixture has no declared feed coverage"
    assert marker.exists()


# ---------- acceptance: the interval follows measured time ----------


def test_the_interval_is_about_1_2_times_the_median_cycle_time():
    assert watch.interval([]) == watch.DEFAULT_SECONDS, "no measurement yet: the default"
    assert watch.interval([1080.0]) == 1296  # an 18-minute cycle: every 21.6 minutes, never every 5
    assert watch.interval([600.0, 1080.0, 1200.0]) == 1296, "the median, so one slow cycle does not set the pace"
    assert watch.interval([2.0]) == watch.MIN_SECONDS, "a quick cycle still waits the floor"
    assert watch.interval([99999.0]) == watch.MAX_SECONDS


def test_each_pass_measures_itself_and_names_the_next_re_arm_time(tmp_path, capsys):
    _plugin(tmp_path, [{"step": "/repo-watch", "scope": "repo", "cycle": "true"}])
    first = watch.run_pass(clock=Clock([600]))
    assert first["seconds"] == 600 and first["interval"] == 720
    assert first["next_at"] == "2026-10-29T09:12+00:00", "the next re-arm time is now plus the measured interval"
    watch.run_pass(clock=Clock([1080]))
    third = watch.run_pass(clock=Clock([1200]))
    assert third["interval"] == 1296, "1.2 x the median of 600, 1080, 1200"
    assert watch.status()["passes"] == [600, 1080, 1200]
    many = [watch.run_pass(clock=Clock([60]))["interval"] for _ in range(watch.WINDOW)]
    assert many[-1] == 72, "only the last WINDOW passes count"
    capsys.readouterr()
    cli.main(["day", "loop", "status"])
    out = capsys.readouterr().out
    assert "next pass at 09:02" in out, "09:00 plus 72 s rounds up: a re-arm at the printed time is never early"
    assert "re-arm: careerbrew day loop pass" in out


def test_a_skipped_pass_is_not_measured(tmp_path):
    cli.main(["init"])
    watch.run_pass(clock=Clock([600]))
    with watch.lock_path().open("a+") as held:
        fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
        watch.run_pass(clock=Clock([1]))
    assert watch.status()["passes"] == [600]


# ---------- acceptance: end of day leaves no recorded loop running ----------


def test_end_of_day_stops_each_loop_and_the_list_is_empty(tmp_path, capsys):
    cli.main(["init"])
    (tmp_path / "chat.json").write_text("{}")
    live.save_sources(yaml.safe_dump({"sources": [{"name": "chat", "kind": "chat", "adapter": "file", "inbox": str(tmp_path / "chat.json")}]}))
    live.start()
    watch.run_pass()  # records the loop itself and the running live feed
    watch.add("cron", "cron-7f3a", "/repo-watch")
    kinds = sorted(lp["kind"] for lp in watch.status()["running"])
    assert kinds == ["cron", "live-feed", "watch"]
    capsys.readouterr()
    assert cli.main(["day", "loop", "end"]) == 1, "a loop careerbrew cannot stop keeps the day open"
    out = capsys.readouterr().out
    assert "cron-7f3a" in out and "careerbrew day loop stop --id cron-7f3a" in out
    assert not live.status()["running"], "careerbrew stops the loops it owns"
    assert [lp["id"] for lp in watch.status()["running"]] == ["cron-7f3a"]
    assert cli.main(["day", "loop", "stop", "--id", "cron-7f3a"]) == 0
    capsys.readouterr()
    assert cli.main(["day", "loop", "end"]) == 0
    assert "0 loop(s) running" in capsys.readouterr().out
    assert watch.status()["running"] == []


def test_a_loop_left_from_an_earlier_day_shows_as_stale(tmp_path, monkeypatch, capsys):
    cli.main(["init"])
    watch.add("monitor", "mon-1")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-30T09:00+00:00")
    st = watch.status()
    assert [lp["id"] for lp in st["stale"]] == ["mon-1"], "a loop that survived the day is named the next morning"
    capsys.readouterr()
    cli.main(["day", "loop", "pass"])
    out = capsys.readouterr().out
    assert "survived an earlier day: mon-1" in out
    assert "careerbrew day loop stop --id mon-1" in out
    assert watch.end()["running"] == [watch.status()["running"][0]]
    watch.stop("mon-1")
    assert watch.status()["stale"] == []


def test_the_run_loop_re_arms_itself_and_ends_with_the_day(tmp_path):
    cli.main(["init"])
    slept: list[float] = []

    def sleep(seconds: float) -> None:
        slept.append(seconds)
        if sum(slept) == 150:
            watch.end()  # the owner says end of day 30 s into the second wait

    passes = watch.run(sleep=sleep, clock=Clock([100, 100, 100]))
    assert passes == 2, "the loop stops at the first check after end of day"
    assert sum(slept) == 150, "it waits the measured 120 s between passes, and stops within one slice of end of day"
    assert max(slept) <= watch.SLICE_SECONDS
    assert watch.status()["running"] == []


def test_a_pass_after_end_of_day_restarts_the_loop_but_the_run_loop_does_not(tmp_path):
    cli.main(["init"])
    watch.run_pass()
    watch.end()
    assert watch.run_pass(restart=False) == {"status": "ended"}, "the run loop's own pass never clears end of day"
    assert watch.status()["running"] == []
    assert watch.run_pass()["status"] == "failed", "an explicit pass starts again and reports its missing feed coverage"
    assert [lp["id"] for lp in watch.status()["running"]] == [watch.SELF_ID]


def test_the_day_watcher_is_recorded_and_stopped_at_end_of_day(tmp_path, monkeypatch):
    from careerbrew.day import watch as day_watch

    cli.main(["init"])
    stopped: list[bool] = []
    monkeypatch.setattr(day_watch, "running", lambda: True)
    monkeypatch.setattr(day_watch, "stop", lambda: stopped.append(True) or {"running": False})
    watch.run_pass()
    assert watch.WATCHER_ID in [lp["id"] for lp in watch.status()["running"]]
    assert watch.end()["running"] == []
    assert stopped == [True]


def test_careerbrews_own_loop_from_an_earlier_day_is_replaced_not_stale(tmp_path, monkeypatch):
    cli.main(["init"])
    watch.run_pass()  # never ended
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-30T09:00+00:00")
    got = watch.run_pass()
    assert got["stale"] == [], "the loop never asks the owner to stop itself"
    assert [lp["day"] for lp in watch.status()["running"]] == ["2026-10-30"]


def test_a_bad_loop_record_is_refused(tmp_path):
    cli.main(["init"])
    with pytest.raises(watch.WatchError, match="kind"):
        watch.add("daemon", "x")
    with pytest.raises(watch.WatchError, match="id"):
        watch.add("cron", "has space")
    with pytest.raises(watch.WatchError, match="no running loop"):
        watch.stop("never-added")


# ---------- watch inputs, the cycle entry, and the step's own watch phase ----------


def test_a_step_with_watch_inputs_runs_only_when_an_input_changed(tmp_path):
    marker, src = tmp_path / "count", tmp_path / "input.txt"
    src.write_text("a")
    _plugin(tmp_path, [{"step": "/repo-watch", "scope": "repo", "cycle": f"sh -c 'echo x >> {marker}'", "watch": [f"file:{src}"]}])
    assert _step(watch.run_pass(), "/repo-watch")["result"] == "ok"
    assert _step(watch.run_pass(), "/repo-watch")["result"] == "unchanged"
    src.write_text("bb")
    assert _step(watch.run_pass(), "/repo-watch")["result"] == "ok"
    assert marker.read_text().count("x") == 2


def test_a_step_without_watch_inputs_runs_every_pass(tmp_path):
    marker = tmp_path / "count"
    _plugin(tmp_path, [{"step": "/repo-watch", "scope": "repo", "cycle": f"sh -c 'echo x >> {marker}'"}])
    watch.run_pass()
    watch.run_pass()
    assert marker.read_text().count("x") == 2


def test_a_feed_input_follows_the_live_feed_cursor(tmp_path):
    marker = tmp_path / "count"
    _plugin(tmp_path, [{"step": "/triage", "scope": "feed", "cycle": f"sh -c 'echo x >> {marker}'", "watch": ["feed:chat"]}])
    inbox = tmp_path / "chat.json"
    inbox.write_text(json.dumps({"messages": []}))
    live.save_sources(yaml.safe_dump({"sources": [{"name": "chat", "kind": "chat", "adapter": "file", "inbox": str(inbox)}]}))
    live.start()
    watch.run_pass()
    assert _step(watch.run_pass(), "/triage")["result"] == "unchanged", "nothing new in the feed"
    inbox.write_text(json.dumps({"messages": [{"ts": "1761728400.0001", "channel": "C1", "user": "U1", "text": "synthetic ping"}]}))
    got = watch.run_pass()
    assert got["live"]["status"] in ("ok", "failed")
    assert _step(got, "/triage")["result"] == "ok", "a new feed event re-runs the step"


def test_the_pass_runs_the_live_cycle_and_reports_a_running_one_as_skipped(tmp_path, monkeypatch):
    cli.main(["init"])
    got = watch.run_pass()
    assert got["live"]["status"] == "failed" and got["live"]["health"]["status"] == "missing"
    assert live.status()["cycles"] == 0, "an unconfigured feed is checked, never cycled"
    (tmp_path / "chat.json").write_text("{}")
    live.save_sources(yaml.safe_dump({"sources": [{"name": "chat", "kind": "chat", "adapter": "file", "inbox": str(tmp_path / "chat.json")}]}))
    live.start()
    before = live.status()["cycles"]
    watch.run_pass()
    assert live.status()["cycles"] == before + 1, "the pass runs `work live cycle`, not its own copy"
    with (live._dir() / ".cycle.lock").open("a+") as held:
        fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
        assert watch.run_pass()["live"]["status"] == "skipped"


def test_a_cycle_runs_with_the_steps_own_watch_phase_off_and_its_output_goes_nowhere(tmp_path, capsys):
    seen = tmp_path / "env"
    _plugin(tmp_path, [{"step": "/checkup", "scope": "checkup", "cycle": f"sh -c 'echo {CANARY}; echo $CAREERBREW_WATCH_PHASE > {seen}'"}])
    capsys.readouterr()
    cli.main(["day", "loop", "pass"])
    out = capsys.readouterr().out
    assert seen.read_text().strip() == "skip", "a step run by the loop skips its own watch phase"
    assert CANARY not in out
    assert CANARY not in watch.state_path().read_bytes().decode("latin-1")


def test_a_failed_cycle_is_named_and_the_pass_exits_1(tmp_path, capsys):
    _plugin(tmp_path, [{"step": "/repo-watch", "scope": "repo", "cycle": "sh -c 'exit 4'"}])
    capsys.readouterr()
    assert cli.main(["day", "loop", "pass"]) == 1
    assert "/repo-watch failed: exit 4" in capsys.readouterr().out


def test_a_plugin_cycle_runs_only_after_allow_exec(tmp_path, capsys):
    marker = tmp_path / "ran"
    _plugin(tmp_path, [{"step": "/repo-watch", "scope": "repo", "cycle": f"touch {marker}"}], allow_exec=False)
    got = watch.run_pass()
    assert not marker.exists(), "no plugin code runs without --allow-exec (docs/plugins.md Guarantees)"
    assert _step(got, "/repo-watch")["result"] == "blocked"
    assert "--allow-exec" in _step(got, "/repo-watch")["why"]


@pytest.mark.parametrize(
    ("raw", "msg"),
    [
        ({"step": "/a", "cycle": ""}, "`cycle` must be a command"),
        ({"step": "/a", "cycle": "x\ny"}, "one line"),
        ({"step": "/a", "cycle": "echo 'open"}, "unclosed quote"),
        ({"step": "/a", "watch": ["url:https://x"]}, "repo:, feed:, or file:"),
        ({"step": "/a", "watch": "file:a b"}, "no spaces"),
    ],
)
def test_bad_cycle_or_watch_is_refused(raw, msg):
    with pytest.raises(steps.StepError, match=msg):
        steps.entry(raw, "settings")


def test_an_agent_sees_whether_a_step_has_a_cycle_not_its_command(tmp_path, capsys, monkeypatch):
    _plugin(tmp_path, [{"step": "/repo-watch", "scope": "repo", "cycle": "check-internal-host", "watch": ["repo:~/apps/internal-repo"]}])
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    capsys.readouterr()
    cli.main(["day", "run", "--json"])
    out = capsys.readouterr().out
    assert "check-internal-host" not in out and "internal-repo" not in out
    step = next(s for s in json.loads(out)["steps"] if s["step"] == "/repo-watch")
    assert step["cycle"] is True and step["watch"] == ["repo"]


def test_the_plan_points_the_agent_at_the_one_loop():
    p = plan.build()
    assert "careerbrew day loop pass" in p["watch"]
    assert any("careerbrew day loop end" in line for line in p["end_of_day"])
