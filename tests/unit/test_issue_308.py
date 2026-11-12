"""Readiness dispatch does not wait for unrelated stage peers."""

import json

import pytest

from careerbrew import cli
from careerbrew.workday import plan, stages, status, steps


def declared(*raw):
    return [steps.entry(r, "setting") for r in raw]


def test_ready_successor_does_not_wait_for_an_unrelated_running_step():
    found, _ = stages.build(
        declared(
            {"step": "/tools", "scope": "tools"},
            {"step": "/service", "scope": "service"},
            {"step": "/status", "scope": "status", "after": "/tools"},
            {"step": "/prs", "scope": "prs", "after": ["/tools", "/service"]},
        )
    )
    rows = [{"step": "/tools", "status": "done"}, {"step": "/service", "status": "running"}]
    assert stages.ready(found, rows) == ["/status"]
    assert next(s for s in found if s["step"] == "/status")["ready_when"]["after"] == ["/tools"]


@pytest.mark.parametrize(("dependency", "expected"), [("blocked", ["/status"]), ("failed", [])])
def test_blocked_dependency_releases_work_but_failed_dependency_does_not(dependency, expected):
    found, _ = stages.build(declared({"step": "/tools", "scope": "tools"}, {"step": "/status", "scope": "status", "after": "/tools"}))
    assert stages.ready(found, [{"step": "/tools", "status": dependency}]) == expected


def test_running_and_newly_ready_steps_never_share_a_resource_or_scope():
    found, _ = stages.build(
        declared(
            {"step": "/a", "scope": "repo"},
            {"step": "/b", "scope": "repo/web"},
            {"step": "/c", "uses": "browser:sso"},
            {"step": "/d", "uses": "browser:sso"},
        )
    )
    assert stages.ready(found, [{"step": "/a", "status": "running"}]) == ["/c"]


def test_independent_long_running_work_starts_after_careerbrew_without_waiting_for_checkups():
    found, _ = stages.build(declared("/careerbrew", {"step": "/slow", "scope": "slow"}, {"step": "/feed", "longRunning": True, "scope": "feed"}))
    assert stages.ready(found, [{"step": "/careerbrew", "status": "done"}]) == ["/slow", "/feed"]
    by = {s["step"]: s for s in found}
    assert by["/feed"]["stage"] == by["/slow"]["stage"] == 2


def test_long_loop_waits_for_finite_work_with_the_same_resource_and_explains_it():
    found, _ = stages.build(declared({"step": "/feed", "longRunning": True, "uses": "browser:sso"}, {"step": "/check", "uses": "browser:sso"}))
    loop = found[0]
    assert loop["ready_when"]["after"] == ["/check"]
    assert "resource" in loop["why"]
    assert stages.ready(found, []) == ["/check"]
    assert stages.ready(found, [{"step": "/check", "status": "done"}]) == ["/feed"]


def test_undeclared_scope_preserves_serial_safety():
    found, _ = stages.build(declared("/careerbrew", "/standup", {"step": "/other", "scope": "other"}))
    assert stages.ready(found, [{"step": "/careerbrew", "status": "done"}]) == ["/standup"]


def test_day_check_json_lists_ready_steps_without_changing_day_next(monkeypatch, capsys):
    found, _ = stages.build(declared({"step": "/a", "scope": "a"}, {"step": "/b", "scope": "b", "after": "/a"}))
    for s in found:
        s["timeout"] = 30
    monkeypatch.setattr(plan, "build", lambda: {"steps": found})
    status.update("/a", ["/a", "/b"], "done")
    assert cli.main(["day", "check", "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["ready"] == ["/b"]


def test_explicit_dependencies_reorder_unknown_scope_steps_before_serial_barriers():
    found, _ = stages.build(declared({"step": "/report", "after": "/refresh"}, "/refresh"))
    assert {s["step"]: s["stage"] for s in found} == {"/report": 2, "/refresh": 1}
