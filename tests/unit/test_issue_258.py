"""Issue #258: work-day steps declare order, scope, and named shared resources; the plan builds stages from them."""

from __future__ import annotations

import json

import pytest
import yaml

from careerbrew import cli, plugins
from careerbrew.work import projects, research
from careerbrew.workday import plan, stages, status, steps


@pytest.fixture(autouse=True)
def pinned(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")


def _steps(*raw) -> list[dict]:
    return [steps.entry(r, "plugin:t") for r in raw]


def _by(raw) -> dict[str, dict]:
    out, _ = stages.build(_steps(*raw))
    return {s["step"]: s for s in out}


def test_two_steps_that_share_a_uses_entry_land_in_different_stages():
    got = _by([{"step": "/a", "uses": ["browser:sso"]}, {"step": "/b", "uses": ["browser:sso", "file:notes.md"]}, {"step": "/c", "uses": "file:other.md"}])
    assert (got["/a"]["stage"], got["/b"]["stage"], got["/c"]["stage"]) == (1, 2, 1)
    assert "shares browser:sso with /a (stage 1)" in got["/b"]["why"]


def test_overlapping_scopes_split_and_separate_ones_share():
    got = _by([{"step": "/a", "scope": "repo"}, {"step": "/b", "scope": ["repo/web"]}, {"step": "/c", "scope": "repository"}])
    assert (got["/a"]["stage"], got["/b"]["stage"], got["/c"]["stage"]) == (1, 2, 1), "repository is not inside repo"
    assert "scope repo/web overlaps /a's repo" in got["/b"]["why"]


def test_an_independent_long_running_step_starts_early_when_declared_first():
    out, run = stages.build(_steps({"step": "/feed", "longRunning": True, "uses": "browser:feed"}, {"step": "/a", "scope": "x"}, {"step": "/b", "scope": "x"}))
    by = {s["step"]: s for s in out}
    assert [s["stage"] for s in out] == [1, 1, 2]
    assert run[0] == {"stage": 1, "steps": ["/feed", "/a"], "long_running": False}
    assert "long-running" in by["/feed"]["why"]


def test_after_orders_stages_even_against_declared_order():
    got = _by([{"step": "/report", "scope": "r", "after": "/refresh"}, {"step": "/refresh", "scope": "t"}, {"step": "/other", "scope": "o"}])
    assert (got["/refresh"]["stage"], got["/report"]["stage"], got["/other"]["stage"]) == (1, 2, 1)
    assert "after /refresh (stage 1)" in got["/report"]["why"]


def test_a_step_that_declares_nothing_runs_alone_in_its_place():
    got = _by(["/careerbrew", {"step": "/a", "scope": "a"}, {"step": "/b", "scope": "b"}, "/standup", {"step": "/c", "scope": "c"}])
    assert [got[n]["stage"] for n in ("/careerbrew", "/a", "/b", "/standup", "/c")] == [1, 2, 2, 3, 4]
    assert stages.ALONE in got["/standup"]["why"]
    assert "after /standup, which runs alone (stage 3)" in got["/c"]["why"]


def test_undeclared_steps_keep_the_us94_order_one_stage_each():
    got = _by(["/careerbrew", "/standup", "/review-prs"])
    assert [s["stage"] for s in got.values()] == [1, 2, 3]


def test_an_after_name_not_in_the_plan_is_ignored_and_said():
    got = _by([{"step": "/a", "scope": "a", "after": "/gone"}])
    assert got["/a"]["stage"] == 1 and "after /gone: not in today's plan, ignored" in got["/a"]["why"]


def test_impossible_orders_are_refused():
    with pytest.raises(steps.StepError, match="wait for each other"):
        stages.build(_steps({"step": "/a", "scope": "a", "after": "/b"}, {"step": "/b", "scope": "b", "after": "/a"}))
    with pytest.raises(steps.StepError, match="never ends"):
        stages.build(_steps({"step": "/feed", "longRunning": True}, {"step": "/a", "after": "/feed"}))
    with pytest.raises(steps.StepError, match="never ends"):
        stages.build(_steps({"step": "/f1", "long_running": True, "uses": "browser:sso"}, {"step": "/f2", "long_running": True, "uses": "browser:sso"}))


def test_bad_fields_are_refused_with_the_fix():
    for raw, msg in (
        ({"step": "/a", "use": ["browser:sso"]}, "unknown field"),
        ({"step": "/a", "uses": ["printer:1"]}, "kind browser, slack-draft, file, checkout"),
        ({"step": "/a", "uses": ["file:a b"]}, "no spaces"),
        ({"step": "/a", "longRunning": "yes"}, "true or false"),
        ({"step": "/a", "after": ["/a:..:x"]}, "command or skill name"),
        ({"step": "/a", "scope": [1]}, "list of strings"),
    ):
        with pytest.raises(steps.StepError, match=msg):
            steps.entry(raw, "plugin:t")


def test_the_plan_json_shows_each_steps_stage_and_why(tmp_path, capsys):
    cli.main(["init"])
    d = tmp_path / "plug"
    d.mkdir()
    work_steps = [
        {"step": "/feed", "longRunning": True},
        {"step": "/refresh", "scope": "tooling", "uses": ["checkout:~/apps/tooling"]},
        {"step": "/status-report", "scope": "report", "after": "/refresh"},
        {"step": "/review", "uses": ["browser:sso"]},
        {"step": "/drafts", "uses": ["browser:sso", "slack-draft:team"]},
    ]
    (d / plugins.MANIFEST).write_text(yaml.safe_dump({"name": "steps-co", "version": "1.0.0", "work_steps": work_steps}))
    plugins.add(str(d))
    capsys.readouterr()
    assert cli.main(["day", "run", "--json"]) == 0
    p = json.loads(capsys.readouterr().out)
    got = {s["step"]: (s["stage"], s["why"]) for s in p["steps"]}
    assert {k: v[0] for k, v in got.items()} == {"/careerbrew": 1, "/feed": 4, "/refresh": 2, "/status-report": 3, "/review": 2, "/drafts": 3}
    assert all(why for _, why in got.values())
    assert [s["steps"] for s in p["stages"]] == [["/careerbrew"], ["/refresh", "/review"], ["/status-report", "/drafts"], ["/feed"]]
    assert "stages" in p["flow"] and p["handled"] == {}
    assert cli.main(["day", "run"]) == 0
    assert "stages: 4" in capsys.readouterr().out


def test_one_step_owns_a_checkout_or_pr_per_day(capsys):
    names = ["/a", "/b"]
    status.update("/a", names, "done", handled_items=["checkout:~/apps/x", "pr:org/repo#12"])
    status.update("/a", names, handled_items=["checkout:~/apps/x"])  # the same step again is fine
    assert status.handled() == {"checkout:~/apps/x": "/a", "pr:org/repo#12": "/a"}
    with pytest.raises(ValueError, match="already handled today by /a: skip it"):
        status.update("/b", names, "done", handled_items=["pr:org/repo#12"])
    assert status.rows(names)[1]["status"] == "waiting", "a refused claim writes nothing"
    with pytest.raises(ValueError, match="checkout:<path> or pr:"):
        status.update("/b", names, handled_items=["branch:main"])
    from datetime import date

    assert status.handled(date(2026, 10, 29)) == {}, "ownership is per day"


def test_day_update_handled_reaches_the_plan(capsys):
    cli.main(["init"])
    assert cli.main(["day", "update", "/careerbrew", "--handled", "checkout:~/apps/x"]) == 0
    assert plan.build()["handled"] == {"checkout:~/apps/x": "/careerbrew"}


def test_refresh_skips_a_source_another_step_checked_since_the_list_was_read(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-01T09:00+00:00")
    projects.add("billing")
    projects.add_source("billing", "url", "https://example.com/a")
    projects.add_source("billing", "url", "https://example.com/b")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    listed = projects.stale_sources()
    assert len(listed) == 2
    projects.check("billing", "https://example.com/a", "checked by another step")  # after the list was read
    monkeypatch.setattr(projects, "stale_sources", lambda: listed)
    read: list[str] = []
    monkeypatch.setattr(research, "read", lambda kind, ref, since: read.append(ref) or {"notes": [], "proposed": [], "title": ""})
    monkeypatch.setattr(research, "write_memory", lambda *a, **k: "")
    r = research.refresh()
    assert read == ["https://example.com/b"], "the source checked today is not read again"
    assert (r["refreshed"], r["skipped"]) == (1, 1)


def test_day_update_names_the_story_when_the_steps_cannot_be_staged(tmp_path, capsys):
    cli.main(["init"])
    d = tmp_path / "loop"
    d.mkdir()
    loop = [{"step": "/x", "scope": "a", "after": "/y"}, {"step": "/y", "scope": "b", "after": "/x"}]
    (d / plugins.MANIFEST).write_text(yaml.safe_dump({"name": "loop-co", "version": "1.0.0", "work_steps": loop}))
    plugins.add(str(d))
    capsys.readouterr()
    assert cli.main(["day", "update", "/careerbrew", "--status", "done"]) == 2
    out = capsys.readouterr().out
    assert "wait for each other" in out and "breaks US-94" in out
    assert status.rows(["/careerbrew"])[0]["status"] == "waiting", "a plan that cannot run takes no update"
