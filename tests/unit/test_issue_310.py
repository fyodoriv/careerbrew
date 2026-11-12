"""Quick dispatch steps are declared for the owner's initial turn."""

import pytest

from careerbrew.workday import plan, stages, steps


def test_operator_turn_field_is_normalized_and_has_a_short_default_timeout():
    item = steps.entry({"step": "/sessions", "operatorTurn": True, "scope": "sessions"}, "setting")
    assert item["operator_turn"] is True and item["timeout"] == 3
    assert steps.entry({"step": "/sessions", "operator_turn": True, "timeout": 5}, "setting")["timeout"] == 5


@pytest.mark.parametrize(
    "raw",
    [
        {"step": "/sessions", "operatorTurn": "yes"},
        {"step": "/sessions", "operatorTurn": True, "longRunning": True},
    ],
)
def test_invalid_operator_turn_steps_are_refused(raw):
    with pytest.raises(steps.StepError, match="operator"):
        steps.entry(raw, "setting")


def test_operator_turn_step_cannot_depend_on_later_background_work():
    found = [steps.entry(r, "setting") for r in [{"step": "/check", "scope": "check"}, {"step": "/sessions", "operatorTurn": True, "scope": "sessions", "after": "/check"}]]
    with pytest.raises(steps.StepError, match="operatorTurn"):
        stages.build(found)


def test_plan_lists_initial_turn_dispatch_steps_after_day_start(monkeypatch):
    found = [steps.entry(r, "setting") for r in ["/careerbrew", {"step": "/sessions", "operatorTurn": True, "scope": "sessions"}]]
    monkeypatch.setattr(steps, "declared", lambda: found)
    got = plan.build()
    assert got["start_of_day"]["operator_turn"] == ["/sessions"]
    assert "owner" in got["flow"] and "before" in got["flow"]


def test_operator_dispatch_precedes_background_work_even_when_declared_last():
    found = [steps.entry(r, "setting") for r in ["/careerbrew", "/slow", {"step": "/dispatch", "operatorTurn": True, "scope": "sessions"}]]
    planned, _ = stages.build(found)
    by = {s["step"]: s for s in planned}
    assert by["/dispatch"]["stage"] == 2 and by["/slow"]["stage"] == 3
    assert stages.ready(planned, [{"step": "/careerbrew", "status": "done"}]) == ["/dispatch"]


def test_failed_signin_blocks_a_browser_dispatcher_without_cycling_or_holding_unrelated_work():
    found = [steps.entry(r, "setting") for r in ["/careerbrew", {"step": "/sessions", "operatorTurn": True, "uses": "browser:sso"}, {"step": "/notes", "scope": "notes"}]]
    planned, _ = stages.build(found, late={"/sessions"})
    by = {s["step"]: s for s in planned}
    assert by["/sessions"]["ready_when"]["signin_required"] is True
    assert stages.ready(planned, [{"step": "/careerbrew", "status": "done"}]) == ["/notes"]


def test_start_output_does_not_offer_to_run_a_failed_signin_dispatcher_later(monkeypatch, capsys):
    from careerbrew import cli
    from careerbrew.workday import start

    declared = [steps.entry({"step": "/dispatch", "operatorTurn": True, "uses": "browser:sso"}, "setting")]
    monkeypatch.setattr(plan, "build", lambda: {"steps": declared})
    monkeypatch.setattr(start, "run", lambda found: {"refresh": [], "preflight": [], "signin": {"status": "failed", "why": "synthetic probe", "steps": ["/dispatch"]}, "questions": []})
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert cli.main(["day", "start"]) == 0
    out = capsys.readouterr().out
    assert "operator-turn browser steps stay blocked until sign-in succeeds" in out
    assert "/dispatch run after every other step" not in out
