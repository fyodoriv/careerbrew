"""Issue #259: a work-day step's `verify` proves `done`, a step past its timeout is retried once then failed, and blocked differs from failed."""

from __future__ import annotations

import json

import pytest
import yaml

from careerbrew import cli, config, plugins, report, runs
from careerbrew.workday import gate, plan, status, steps

CANARY = "zq-verify-canary-7"


@pytest.fixture(autouse=True)
def pinned(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")


def _plugin(tmp_path, work_steps, allow_exec: bool = True) -> None:
    cli.main(["init"])
    d = tmp_path / "plug"
    d.mkdir()
    (d / plugins.MANIFEST).write_text(yaml.safe_dump({"name": "gate-co", "version": "1.0.0", "work_steps": work_steps}))
    plugins.add(str(d), allow_exec=allow_exec)


def _row(step: str) -> dict:
    names = [s["step"] for s in plan.build()["steps"]]
    return status.rows(names)[names.index(step)]


def test_a_step_whose_verify_exits_non_zero_is_failed(tmp_path, capsys):
    _plugin(tmp_path, [{"step": "/feed-check", "scope": "feed", "verify": f"sh -c 'echo {CANARY}; echo {CANARY} >&2; exit 3'"}])
    capsys.readouterr()
    assert cli.main(["day", "update", "/feed-check", "--status", "done", "--summary", "stored rows"]) == 1
    out = capsys.readouterr().out
    row = _row("/feed-check")
    assert (row["status"], row["error_class"], row["verify"]) == ("failed", "tool", "fail: exit 3")
    assert row["summary"] == "stored rows", "the step's own summary is kept"
    assert "is failed (tool)" in out and "verify exit 3" in out
    assert CANARY not in out and CANARY not in json.dumps(row), "verify output is never printed or stored"


def test_a_step_whose_verify_passes_is_done(tmp_path, capsys):
    _plugin(tmp_path, [{"step": "/feed-check", "scope": "feed", "verify": "true"}])
    assert cli.main(["day", "update", "/feed-check", "--status", "done"]) == 0
    row = _row("/feed-check")
    assert (row["status"], row["verify"], row["error_class"]) == ("done", "pass", "")


def test_a_step_without_verify_is_done_on_its_word(capsys):
    cli.main(["init"])
    assert cli.main(["day", "update", "/careerbrew", "--status", "done"]) == 0
    assert (_row("/careerbrew")["status"], _row("/careerbrew")["verify"]) == ("done", "")


def test_a_verify_that_hangs_fails_with_a_timeout(tmp_path, monkeypatch):
    monkeypatch.setattr(gate, "VERIFY_SECONDS", 0.5)
    assert gate.run_verify("sleep 5") == (False, "timed out after 0.5 s")
    assert gate.run_verify("/no/such/verify-tool") == (False, "could not start (FileNotFoundError)")
    marker = tmp_path / "made-by-a-second-command"
    assert gate.run_verify(f"echo ok; touch {marker}") == (True, "")
    assert not marker.exists(), "no shell: `;` is an argument to echo, never a second command"


def test_a_step_past_its_timeout_is_retried_once_then_failed(tmp_path, monkeypatch, capsys):
    _plugin(tmp_path, [{"step": "/slow", "scope": "s", "timeout": 5}, {"step": "/quick", "scope": "q", "timeout": 60}])
    assert cli.main(["day", "update", "/slow", "--status", "running"]) == 0
    assert cli.main(["day", "update", "/quick", "--status", "running"]) == 0
    first = _row("/slow")
    assert (first["started"], first["attempt"]) == ("2026-10-28T09:00:00+00:00", 1), "the row is written at step start"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:06+00:00")
    capsys.readouterr()
    assert cli.main(["day", "check"]) == 0
    assert "/slow timed out after 5 min: retry 2 of 2" in capsys.readouterr().out
    retry = _row("/slow")
    assert (retry["status"], retry["retry"], retry["next"]) == ("waiting", True, "run /slow again: attempt 2 of 2, after a timeout")
    assert _row("/quick")["status"] == "running", "a step inside its timeout is left alone"
    assert cli.main(["day", "update", "/slow", "--status", "running"]) == 0
    assert _row("/slow")["attempt"] == 2
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:12+00:00")
    capsys.readouterr()
    assert cli.main(["day", "check", "--json"]) == 0
    assert json.loads(capsys.readouterr().out) == {"checked": 2, "retry": [], "failed": ["/slow"], "ready": []}, "--json prints only JSON"
    final = _row("/slow")
    assert (final["status"], final["error_class"], final["timed_out"]) == ("failed", "tool", True)
    assert final["next"].startswith("read the log of /slow")


def test_a_rerun_later_in_the_day_starts_again_at_attempt_one(tmp_path, monkeypatch):
    _plugin(tmp_path, [{"step": "/slow", "scope": "s", "timeout": 5}])
    names = [s["step"] for s in plan.build()["steps"]]
    status.update("/slow", names, "running")
    status.update("/slow", names, "done")
    status.update("/slow", names, "running")
    assert _row("/slow")["attempt"] == 1, "only a timeout retry counts as a second attempt"


def test_the_default_timeout_is_a_setting(capsys, monkeypatch):
    cli.main(["init"])
    config.set_value("work_step_timeout", "10")
    assert {s["step"]: s["timeout"] for s in plan.build()["steps"]} == {"/careerbrew": 10}
    cli.main(["day", "update", "/careerbrew", "--status", "running"])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:09+00:00")
    assert plan.build()["dashboard"]["rows"][0]["overdue"] is False
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:11+00:00")
    assert plan.build()["dashboard"]["rows"][0]["overdue"] is True, "a crash leaves a running row the next pass flags"
    capsys.readouterr()
    assert cli.main(["day", "run", "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["dashboard"]["rows"][0]["overdue"] is True


def test_a_blocked_row_without_a_question_record_is_refused(capsys):
    cli.main(["init"])
    capsys.readouterr()
    assert cli.main(["day", "update", "/careerbrew", "--status", "blocked"]) == 2
    out = capsys.readouterr().out
    assert "blocked needs a question record" in out and "breaks US-100" in out
    assert _row("/careerbrew")["status"] == "waiting", "a refused update writes nothing"
    assert cli.main(["day", "update", "/careerbrew", "--status", "blocked", "--needs-you", "pick a synthetic slot"]) == 0
    row = _row("/careerbrew")
    assert row["status"] == "blocked" and row["next"].startswith("answer the ask")


def test_a_failed_row_needs_an_error_class(capsys):
    cli.main(["init"])
    capsys.readouterr()
    assert cli.main(["day", "update", "/careerbrew", "--status", "failed"]) == 2
    assert "failed needs an error class" in capsys.readouterr().out
    assert cli.main(["day", "update", "/careerbrew", "--status", "failed", "--error-class", "auth"]) == 0
    row = _row("/careerbrew")
    assert (row["status"], row["error_class"]) == ("failed", "auth") and "token" in row["next"]
    with pytest.raises(ValueError, match="unknown error class"):
        status.update("/careerbrew", ["/careerbrew"], "failed", error_class="cosmic")
    with pytest.raises(ValueError, match="only with --status failed"):
        status.update("/careerbrew", ["/careerbrew"], "done", error_class="env")
    status.update("/careerbrew", ["/careerbrew"], "done")
    assert (_row("/careerbrew")["error_class"], _row("/careerbrew")["next"]) == ("", ""), "done clears the class and next action"


@pytest.mark.parametrize(
    "raw,msg",
    [
        ({"step": "/a", "verify": ""}, "`verify` must be a command"),
        ({"step": "/a", "verify": "true\nrm -rf x"}, "one line"),
        ({"step": "/a", "verify": 3}, "`verify` must be a command"),
        ({"step": "/a", "verify": "sh -c 'exit 1"}, "unclosed quote"),
        ({"step": "/a", "timeout": 0}, "`timeout` is whole minutes"),
        ({"step": "/a", "timeout": "30m"}, "`timeout` is whole minutes"),
        ({"step": "/a", "timeout": True}, "`timeout` is whole minutes"),
    ],
)
def test_bad_verify_or_timeout_is_refused(raw, msg):
    with pytest.raises(steps.StepError, match=msg):
        steps.entry(raw, "plugin:t")


def test_the_page_tells_blocked_from_failed(tmp_path):
    cli.main(["init"])
    config.set_value("work_steps", "/standup, /deploy")
    names = [s["step"] for s in plan.build()["steps"]]
    status.update("/standup", names, "blocked", needs_you=f"approve the {CANARY} draft")
    status.update("/deploy", names, "failed", error_class="network")
    r = runs.work_day(plan.build())
    why = {i.id: i.why for i in r.blockers}
    assert why["/standup"].startswith("blocked: waits on you")
    assert why["/deploy"].startswith("failed (network): run /deploy again")
    assert "1 blocked, 1 failed" in r.headline
    assert CANARY not in json.dumps(report.summary(r)), "the ask stays on the page only"


def test_a_long_running_step_never_times_out(tmp_path, monkeypatch):
    _plugin(tmp_path, [{"step": "/feed", "longRunning": True, "uses": "browser:feed"}])
    cli.main(["day", "update", "/feed", "--status", "running"])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T17:00+00:00")
    assert [r["overdue"] for r in plan.build()["dashboard"]["rows"] if r["step"] == "/feed"] == [False]
    assert [s["timeout"] for s in plan.build()["steps"] if s["step"] == "/feed"] == [None], "no timeout shows on a step that never ends"
    assert gate.check(plan.build()["steps"]) == {"checked": 0, "retry": [], "failed": []}
    with pytest.raises(steps.StepError, match="long-running"):
        steps.entry({"step": "/f", "longRunning": True, "timeout": 5}, "plugin:t")


def test_a_check_leaves_a_step_that_restarted_since_it_read_the_rows(tmp_path, monkeypatch):
    _plugin(tmp_path, [{"step": "/slow", "scope": "s", "timeout": 5}])
    names = [s["step"] for s in plan.build()["steps"]]
    status.update("/slow", names, "running")
    seen = _row("/slow")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:03+00:00")
    status.update("/slow", names, "done")
    status.update("/slow", names, "running")
    assert status.timed_out("/slow", names, seen["started"]) is None, "the row the check read is gone: change nothing"
    assert _row("/slow")["status"] == "running"


def test_a_running_row_without_a_start_time_is_overdue():
    from careerbrew.calendar import now

    assert gate.overdue({"status": "running", "started": ""}, 30, now()) is True


def test_an_error_class_with_done_is_refused_before_verify_runs(tmp_path, capsys):
    marker = tmp_path / "verify-ran"
    _plugin(tmp_path, [{"step": "/feed-check", "scope": "feed", "verify": f"touch {marker}"}])
    capsys.readouterr()
    assert cli.main(["day", "update", "/feed-check", "--status", "done", "--error-class", "env"]) == 2
    assert not marker.exists(), "a refused update never runs the verify"


def test_a_plugin_verify_runs_only_after_allow_exec(tmp_path, capsys):
    marker = tmp_path / "verify-ran"
    _plugin(tmp_path, [{"step": "/feed-check", "scope": "feed", "verify": f"touch {marker}"}], allow_exec=False)
    capsys.readouterr()
    assert cli.main(["day", "update", "/feed-check", "--status", "done"]) == 2
    assert not marker.exists(), "no plugin code runs without --allow-exec (docs/plugins.md Guarantees)"
    assert "--allow-exec" in capsys.readouterr().out
    assert _row("/feed-check")["status"] != "done", "a refused update writes nothing"


def test_an_agent_sees_whether_a_step_has_verify_not_its_command(tmp_path, capsys, monkeypatch):
    _plugin(tmp_path, [{"step": "/feed-check", "scope": "feed", "verify": "check-internal-host"}])
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    capsys.readouterr()
    cli.main(["day", "run", "--json"])
    out = capsys.readouterr().out
    assert "check-internal-host" not in out
    assert [s["verify"] for s in json.loads(out)["steps"] if s["step"] == "/feed-check"] == [True]
