"""Issue #263: per-step timing on each work-day row, a history of durations across days, a slow flag, and the day as a run page."""

from __future__ import annotations

import json

import pytest
import yaml

from careerbrew import cli, config, paths, plugins, report, runs
from careerbrew.workday import plan, status, timing

CANARY = "zq-timing-canary-3"


@pytest.fixture(autouse=True)
def pinned(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")


def _names() -> list[str]:
    return [s["step"] for s in plan.build()["steps"]]


def _row(step: str) -> dict:
    return next(r for r in plan.build()["dashboard"]["rows"] if r["step"] == step)


def _run(monkeypatch, day: str, step: str, minutes: int, end: str = "done", **kw) -> None:
    """Start `step` at 09:00 on `day` and end it `minutes` later."""
    names = _names()
    monkeypatch.setenv("CAREERBREW_NOW", f"{day}T09:00+00:00")
    status.update(step, names, "running")
    monkeypatch.setenv("CAREERBREW_NOW", f"{day}T09:{minutes:02d}+00:00")
    status.update(step, names, end, **kw)


def _setup(steps: str = "/standup, /deploy") -> None:
    cli.main(["init"])
    config.set_value("work_steps", steps)


def test_a_finished_step_records_its_end_and_duration(monkeypatch, capsys):
    _setup()
    _run(monkeypatch, "2026-10-28", "/standup", 18)
    row = _row("/standup")
    assert (row["started"], row["ended"], row["seconds"]) == ("2026-10-28T09:00:00+00:00", "2026-10-28T09:18:00+00:00", 1080)
    capsys.readouterr()
    assert cli.main(["day", "run", "--json"]) == 0
    shown = next(r for r in json.loads(capsys.readouterr().out)["dashboard"]["rows"] if r["step"] == "/standup")
    assert (shown["ended"], shown["seconds"]) == ("2026-10-28T09:18:00+00:00", 1080), "agents see the timing: it is content-free"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T10:00+00:00")
    status.update("/standup", _names(), "running")
    again = _row("/standup")
    assert (again["started"], again["ended"], again["seconds"]) == ("2026-10-28T10:00:00+00:00", "", None), "a new start clears the old end"


def test_a_summary_update_after_done_keeps_the_duration(monkeypatch):
    _setup()
    _run(monkeypatch, "2026-10-28", "/standup", 7)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T11:00+00:00")
    status.update("/standup", _names(), summary="posted")
    assert _row("/standup")["seconds"] == 420, "only a change of status ends the step"


def test_a_step_that_never_ran_has_an_end_but_no_duration(monkeypatch):
    _setup()
    status.update("/deploy", _names(), "blocked", needs_you="pick a slot")
    row = _row("/deploy")
    assert (row["ended"], row["seconds"]) == ("2026-10-28T09:00:00+00:00", None)


def test_the_history_keeps_duration_and_final_status_per_step_per_day(monkeypatch):
    _setup()
    _run(monkeypatch, "2026-10-26", "/standup", 5)
    _run(monkeypatch, "2026-10-27", "/standup", 6)
    _run(monkeypatch, "2026-10-27", "/deploy", 9, "failed", error_class="network")
    _run(monkeypatch, "2026-10-27", "/deploy", 12, summary=f"shipped {CANARY}")  # the final status of the day wins
    days = timing.history()["days"]
    assert days == {
        "2026-10-26": {"/standup": {"seconds": 300, "status": "done"}},
        "2026-10-27": {"/standup": {"seconds": 360, "status": "done"}, "/deploy": {"seconds": 720, "status": "done"}},
    }
    raw = timing.history_path().read_bytes()
    assert b"/standup" not in raw and CANARY.encode() not in raw, "the file is encrypted at rest and holds no row text"


def test_a_timed_out_step_is_recorded_as_failed(monkeypatch):
    cli.main(["init"])
    d = paths.data_root().parent / "plug"
    d.mkdir()
    (d / plugins.MANIFEST).write_text(yaml.safe_dump({"name": "time-co", "version": "1.0.0", "work_steps": [{"step": "/slow", "scope": "s", "timeout": 5}]}))
    plugins.add(str(d), allow_exec=False)
    from careerbrew.workday import gate

    for at in ("09:00", "09:06"):
        monkeypatch.setenv("CAREERBREW_NOW", f"2026-10-28T{at}+00:00")
        status.update("/slow", _names(), "running")
        monkeypatch.setenv("CAREERBREW_NOW", f"2026-10-28T{at[:3]}{int(at[3:]) + 6:02d}+00:00")
        gate.check(plan.build()["steps"])
    row = _row("/slow")
    assert (row["status"], row["ended"], row["seconds"]) == ("failed", "2026-10-28T09:12:00+00:00", 360)
    assert timing.history()["days"]["2026-10-28"]["/slow"] == {"seconds": 360, "status": "failed"}


def test_a_slow_step_is_flagged_against_its_median(monkeypatch):
    _setup()
    for day, minutes in (("2026-10-23", 5), ("2026-10-24", 6), ("2026-10-26", 4)):
        _run(monkeypatch, day, "/standup", minutes)
    _run(monkeypatch, "2026-10-27", "/standup", 50, "failed", error_class="tool")  # a failed run's time is not the step's normal time
    _run(monkeypatch, "2026-10-28", "/standup", 18)
    row = _row("/standup")
    assert (row["median"], row["slow"]) == (300, True), "18 min is more than twice the 5 min median of the earlier done runs"
    config.set_value("work_slow_percent", "400")
    assert _row("/standup")["slow"] is False, "the factor is a setting: 18 min is under 4 x 5 min"


def test_no_flag_before_enough_history(monkeypatch):
    _setup()
    _run(monkeypatch, "2026-10-26", "/standup", 1)
    _run(monkeypatch, "2026-10-27", "/standup", 1)
    _run(monkeypatch, "2026-10-28", "/standup", 30)
    row = _row("/standup")
    assert (row["median"], row["slow"]) == (None, False), f"under {timing.MIN_SAMPLES} earlier runs there is no median to judge by"


def test_a_finished_day_is_listed_by_report_latest(monkeypatch, capsys):
    _setup()
    for day in ("2026-10-23", "2026-10-24", "2026-10-26"):
        _run(monkeypatch, day, "/standup", 5)
    _run(monkeypatch, "2026-10-28", "/standup", 18, summary=f"posted {CANARY}")
    _run(monkeypatch, "2026-10-28", "/deploy", 3, "failed", error_class="network")
    assert cli.main(["day", "render"]) == 0
    capsys.readouterr()
    got = report.latest("work-day")
    assert got is not None and got["counts"]["slow"] == 1
    assert "1 slow" in got["headline"] and "21 min of steps" in got["headline"]
    done = {i["id"]: i for i in got["done"]}
    assert "took 18 min (slow: median 5 min)" in done["/standup"]["evidence"]
    assert "took 3 min" in {i["id"]: i for i in got["blockers"]}["/deploy"]["why"]
    assert CANARY not in json.dumps(got), "row text stays on the page only"
    assert cli.main(["report", "latest", "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["run"] == "work-day"


def test_day_history_answers_how_the_week_went(monkeypatch, capsys):
    _setup()
    for day, end in (("2026-10-26", "done"), ("2026-10-27", "failed"), ("2026-10-28", "failed")):
        _run(monkeypatch, day, "/deploy", 4, end, **({"error_class": "env"} if end == "failed" else {}))
    _run(monkeypatch, "2026-10-28", "/standup", 2)
    _run(monkeypatch, "2026-10-01", "/standup", 2)  # outside the week
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T12:00+00:00")
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    capsys.readouterr()
    assert cli.main(["day", "history", "--json", "--days", "7"]) == 0
    got = json.loads(capsys.readouterr().out)
    deploy = next(s for s in got["steps"] if s["step"] == "/deploy")
    assert (deploy["runs"], deploy["done"], deploy["failed"], deploy["median"]) == (3, 1, 2, 240)
    assert got["steps"][0]["step"] == "/deploy", "the step that fails most comes first"
    assert [d["day"] for d in got["days"]] == ["2026-10-28", "2026-10-27", "2026-10-26"]
    assert cli.main(["day", "history"]) == 0
    assert "/deploy" in capsys.readouterr().out


def test_day_history_hides_plugin_step_names_from_agents(monkeypatch, capsys):
    cli.main(["init"])
    d = paths.data_root().parent / "plug"
    d.mkdir()
    (d / plugins.MANIFEST).write_text(yaml.safe_dump({"name": "hist-co", "version": "1.0.0", "work_steps": [{"step": f"/{CANARY}", "scope": "s"}]}))
    plugins.add(str(d))
    _run(monkeypatch, "2026-10-28", f"/{CANARY}", 3)
    _run(monkeypatch, "2026-10-28", "/careerbrew", 1)
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    capsys.readouterr()
    assert cli.main(["day", "history", "--json"]) == 0
    out = capsys.readouterr().out
    assert CANARY not in out, "a plugin's step name may name the company"
    assert {s["step"] for s in json.loads(out)["steps"]} == {"/careerbrew", "plugin:hist-co step 1"}


def test_the_history_keeps_a_bounded_number_of_days(monkeypatch):
    _setup()
    monkeypatch.setattr(timing, "KEEP_DAYS", 2)
    for day in ("2026-10-26", "2026-10-27", "2026-10-28"):
        _run(monkeypatch, day, "/standup", 1)
    assert sorted(timing.history()["days"]) == ["2026-10-27", "2026-10-28"]


def test_the_page_without_history_still_renders(monkeypatch):
    _setup()
    _run(monkeypatch, "2026-10-28", "/standup", 0)
    r = runs.work_day(plan.build())
    assert "took 0 s" in next(i for i in r.done if i.id == "/standup").evidence
    assert r.counts["slow"] == 0


def test_a_corrupt_history_never_breaks_the_work_day(monkeypatch):
    _setup()
    timing.history_path().parent.mkdir(parents=True, exist_ok=True)
    timing.history_path().write_bytes(b"not a vault file")
    _run(monkeypatch, "2026-10-28", "/standup", 4)
    aside = sorted(timing.history_path().parent.glob("work-history.json.unreadable-*"))
    assert [a.read_bytes() for a in aside] == [b"not a vault file"], "an unreadable history (corrupt, or another key) is moved aside, never overwritten"
    timing.history_path().write_bytes(b"a second unreadable file")
    _run(monkeypatch, "2026-10-28", "/deploy", 2)  # same pinned second: the second copy gets its own name
    aside = sorted(timing.history_path().parent.glob("work-history.json.unreadable-*"), key=lambda a: a.stat().st_mtime_ns)
    assert sorted(a.read_bytes() for a in aside) == [b"a second unreadable file", b"not a vault file"], "a second copy never overwrites the first"
    assert _row("/standup")["seconds"] == 240, "update, plan, and flags still work"
    assert timing.history()["days"] == {"2026-10-28": {"/deploy": {"seconds": 120, "status": "done"}}}, "a fresh history starts after each unreadable one"
    from careerbrew import store

    store.write_json(timing.history_path(), {"days": {"2026-10-27": "junk", "2026-10-28": {"/standup": {"seconds": 240, "status": "done"}}}})
    assert sorted(timing.history()["days"]) == ["2026-10-28"], "a day that is not a mapping is dropped"
