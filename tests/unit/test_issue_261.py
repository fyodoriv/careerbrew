"""Issue #261: work-day questions are records with a recheck; the batched ask drops closed items and shows ages; steps never ask mid-run."""

from __future__ import annotations

import json
from datetime import date

import pytest
import yaml

from careerbrew import cli, config, events, paths, plugins, runs, store
from careerbrew.workday import plan, questions, status

CANARY = "zq-question-canary-3"


@pytest.fixture(autouse=True)
def pinned(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")


def _plugin(tmp_path, work_steps, allow_exec: bool = False, name: str = "ask-co") -> None:
    d = tmp_path / name
    d.mkdir()
    (d / plugins.MANIFEST).write_text(yaml.safe_dump({"name": name, "version": "1.0.0", "work_steps": work_steps}))
    plugins.add(str(d), allow_exec=allow_exec)


def _ids(got: list[dict]) -> list[str]:
    return [q["id"] for q in got]


def test_a_question_is_a_record_stored_encrypted_with_the_days_work_data(capsys):
    cli.main(["init"])
    capsys.readouterr()
    assert cli.main(["day", "question", "add", "/careerbrew", f"Merge the {CANARY} draft?", "--option", "merge", "--option", "wait", "--pick", "merge", "--recheck", "true"]) == 0
    out = capsys.readouterr().out
    assert CANARY not in out, "question text is business data: not on stdout unless business may show"
    got = questions.records()
    assert len(got) == 1
    q = got[0]
    assert set(q) >= {"id", "step", "text", "options", "pick", "createdAt", "recheck"}
    assert (q["step"], q["options"], q["pick"], q["recheck"], q["source"]) == ("/careerbrew", ["merge", "wait"], "merge", "true", "agent")
    assert q["createdAt"].startswith("2026-10-28T09:00")
    raw = status.path_for(date(2026, 10, 28)).read_bytes()
    assert CANARY.encode() not in raw, "the record is encrypted at rest"
    assert q["id"] in out


def test_the_ask_drops_a_record_whose_recheck_says_closed_and_shows_each_age(monkeypatch, capsys):
    cli.main(["init"])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T06:00+00:00")
    questions.add("/careerbrew", "still open?", recheck="true")
    questions.add("/careerbrew", "already closed?", recheck="false")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T08:30+00:00")
    questions.add("/careerbrew", "check the link", recheck="https://example.com/pr/1")
    questions.add("/careerbrew", "no recheck at all")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    capsys.readouterr()
    assert cli.main(["day", "question", "recheck", "--json"]) == 0
    got = json.loads(capsys.readouterr().out)
    asked = {q["id"]: q for q in got["ask"]}
    assert len(asked) == 3 and len(got["dropped"]) == 1, "the closed item is dropped, the rest stay"
    assert got["dropped"][0]["age"] == "3 h" and got["dropped"][0]["why"] == "recheck: closed (exit 1)"
    ages = sorted(q["age"] for q in got["ask"])
    assert ages == ["3 h", "30 min", "30 min"], "each item shows its age"
    states = sorted(q["state"] for q in got["ask"])
    assert states == ["agent", "open", "open"], "a URL recheck is the agent's to check; no recheck stays open"
    assert got["agent"] == [q["id"] for q in got["ask"] if q["state"] == "agent"]
    assert "text" not in json.dumps(got["ask"]) and "example.com" not in json.dumps(got), "no business text or link for an agent without the policy"
    assert len(questions.open_records()) == 3, "the drop is saved: the next ask does not show it"
    closed = [q for q in questions.records() if q["closed"]]
    assert len(closed) == 1 and closed[0]["closedAt"].startswith("2026-10-28T09:00")


def test_a_recheck_command_runs_without_a_shell_and_its_output_is_never_read(tmp_path, capsys):
    cli.main(["init"])
    questions.add("/careerbrew", "open?", recheck=f"sh -c 'echo {CANARY}; echo {CANARY} >&2; exit 0'")
    questions.add("/careerbrew", "shell words stay words", recheck=f"echo {CANARY} > {tmp_path}/written")
    capsys.readouterr()
    assert cli.main(["day", "question", "recheck"]) == 0
    out = capsys.readouterr().out
    log = events.log_path()
    assert CANARY not in out and (not log.exists() or CANARY not in log.read_text())
    assert not (tmp_path / "written").exists(), "no shell: `>` is an argument to echo, not a redirect"
    assert "2 to ask, 0 dropped" in out


def test_a_plugin_recheck_runs_only_when_the_plugin_may_run_commands(tmp_path, capsys):
    cli.main(["init"])
    _plugin(tmp_path, [{"step": "/plugin-step", "scope": "x"}])
    marker = tmp_path / "ran"
    assert cli.main(["day", "question", "add", "/plugin-step", "approve?", "--recheck", f"touch {marker}"]) == 0
    q = questions.records()[0]
    assert q["source"] == "plugin:ask-co", "a plugin step's question comes from the plugin"
    capsys.readouterr()
    cli.main(["day", "question", "recheck", "--json"])
    got = json.loads(capsys.readouterr().out)
    assert not marker.exists(), "no --allow-exec: the plugin's command never runs"
    assert got["ask"][0]["state"] == "unknown" and "--allow-exec" in got["ask"][0]["why"], "it stays in the ask, with the fix"
    plugins.add(str(tmp_path / "ask-co"), allow_exec=True)
    cli.main(["day", "question", "recheck", "--json"])
    assert marker.exists(), "with --allow-exec it runs"


def test_text_shows_only_when_business_data_may_show(monkeypatch, capsys):
    cli.main(["init"])
    questions.add("/careerbrew", f"Ship {CANARY}?", options=["ship", "hold"], pick="ship", recheck="true")
    capsys.readouterr()
    cli.main(["day", "question", "list"])
    assert CANARY not in capsys.readouterr().out
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    cli.main(["day", "question", "list"])
    out = capsys.readouterr().out
    assert CANARY in out and "ship (pick)" in out and "0 min" in out


def test_bad_records_are_refused_with_the_fix(capsys):
    cli.main(["init"])
    capsys.readouterr()
    assert cli.main(["day", "question", "add", "/careerbrew", "x?", "--option", "a", "--pick", "b"]) == 2
    assert "--pick must be one of the --option values" in capsys.readouterr().out
    assert cli.main(["day", "question", "add", "/careerbrew", "x?", "--recheck", "sh -c 'open"]) == 2
    assert "unclosed quote" in capsys.readouterr().out
    assert cli.main(["day", "question", "add", "/not-a-step", "x?"]) == 2
    assert "not in today's plan" in capsys.readouterr().out
    assert cli.main(["day", "question", "close", "q-00000000"]) == 2
    assert "no open question q-00000000" in capsys.readouterr().out


def test_the_same_question_twice_is_one_record_and_keeps_its_first_age(monkeypatch):
    cli.main(["init"])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T07:00+00:00")
    a = questions.add("/careerbrew", "same?")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    b = questions.add("/careerbrew", "same?", options=["yes", "no"])
    assert a["id"] == b["id"] and len(questions.records()) == 1
    assert questions.records()[0]["createdAt"].startswith("2026-10-28T07:00") and questions.records()[0]["options"] == ["yes", "no"]


def test_an_open_question_carries_to_the_next_morning_and_closes_in_its_own_day(monkeypatch, capsys):
    cli.main(["init"])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-27T17:00+00:00")
    q = questions.add("/careerbrew", "yesterday's ask", recheck="false")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    assert _ids(questions.open_records()) == [q["id"]], "the next morning sees yesterday's open question"
    capsys.readouterr()
    cli.main(["day", "question", "recheck", "--json"])
    got = json.loads(capsys.readouterr().out)
    assert got["dropped"][0]["id"] == q["id"] and got["dropped"][0]["age"] == "16 h"
    assert questions.records(date(2026, 10, 27))[0]["closed"], "it closes in the day file it lives in"
    assert questions.records() == [], "nothing is copied into today's file"


def test_a_blocked_row_is_a_question_record_and_clearing_it_closes_it(capsys):
    cli.main(["init"])
    assert cli.main(["day", "update", "/careerbrew", "--status", "blocked", "--needs-you", "Pick a reviewer"]) == 0
    got = questions.open_records()
    assert len(got) == 1 and got[0]["step"] == "/careerbrew" and got[0]["text"] == "Pick a reviewer" and got[0]["source"] == "row"
    cli.main(["day", "update", "/careerbrew", "--status", "blocked", "--needs-you", "Pick a reviewer today"])
    assert len(questions.open_records()) == 1 and questions.open_records()[0]["text"] == "Pick a reviewer today", "one record per row"
    cli.main(["day", "update", "/careerbrew", "--status", "done", "--needs-you", ""])
    assert questions.open_records() == [], "the row no longer waits on you: its question is closed"


def test_every_writer_of_the_day_file_keeps_the_keys_it_does_not_own(monkeypatch):
    cli.main(["init"])
    config.set_value("work_checkouts", "")
    q = questions.add("/careerbrew", "kept?")
    cli.main(["day", "update", "/careerbrew", "--status", "running", "--handled", "pr:example/app#1"])
    cli.main(["day", "start"])
    cli.main(["day", "check"])
    assert _ids(questions.open_records()) == [q["id"]], "rows, start, and check keep the question records"
    questions.add("/careerbrew", "another")
    data = store.read_json(status.path_for(date(2026, 10, 28)))
    assert data["rows"] and data["handled"] == {"pr:example/app#1": "/careerbrew"} and data["start"], "a question write keeps rows, handled, and start"


def test_the_signin_question_lives_in_the_question_store(tmp_path):
    cli.main(["init"])
    config.set_value("work_checkouts", "")
    _plugin(tmp_path, [{"step": "/review", "uses": ["browser:sso"]}])
    config.set_value("work_signin_probe", "false")
    cli.main(["day", "start"])
    got = {q["id"]: q for q in questions.open_records()}
    assert set(got) == {"signin"} and got["signin"]["recheck"] == questions.SIGNIN and got["signin"]["steps"] == ["/review"]
    assert "questions" not in status.start_record(), "the start record keeps no second copy"
    p = plan.build()
    assert _ids(p["start_of_day"]["questions"]) == ["signin"] and _ids(p["question_records"]) == ["signin"]
    page = runs.work_day(p)
    assert page.next[0].id == "ask-signin"
    config.set_value("work_signin_probe", "true")
    r = questions.recheck()
    assert _ids(r["dropped"]) == ["signin"], "the sign-in recheck is the probe: signed in closes it"
    config.set_value("work_signin_probe", "false")
    cli.main(["day", "start"])
    config.set_value("work_signin_probe", "true")
    cli.main(["day", "start"])
    assert questions.open_records() == [], "a passing day start closes it too"


def test_the_page_lists_each_open_question_with_its_age(monkeypatch):
    cli.main(["init"])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T07:00+00:00")
    q = questions.add("/careerbrew", f"Ship {CANARY}?", options=["ship", "hold"], pick="ship")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    page = runs.work_day(plan.build())
    item = next(i for i in page.next if i.id == f"ask-{q['id']}")
    assert CANARY not in item.label and "2 h" in item.label and CANARY in item.detail and "pick: ship" in item.detail
    assert item.command == "careerbrew day question recheck"


def test_the_step_contract_says_return_questions_never_ask():
    p = plan.build()
    assert "never asks the user itself" in p["questions"] and "careerbrew day question add" in p["questions"] and "careerbrew day question recheck" in p["questions"]
    skill = (paths.REPO_ROOT / "skills" / "careerbrew-work-day" / "SKILL.md").read_text()
    assert "never asks the user itself" in skill and "careerbrew day question recheck" in skill


def test_an_answered_row_ask_is_not_asked_again_when_the_row_changes(capsys):
    cli.main(["init"])
    cli.main(["day", "update", "/careerbrew", "--status", "blocked", "--needs-you", "Pick a reviewer"])
    ident = questions.open_records()[0]["id"]
    assert cli.main(["day", "question", "close", ident]) == 0
    cli.main(["day", "update", "/careerbrew", "--status", "running"])
    assert questions.open_records() == [], "the row still holds the old ask, but it was answered: it does not come back"
    assert cli.main(["day", "question", "add", "/careerbrew", "same?"]) == 0
    cli.main(["day", "question", "close", questions.open_records()[0]["id"]])
    capsys.readouterr()
    cli.main(["day", "question", "add", "/careerbrew", "same?"])
    assert "already closed" in capsys.readouterr().out and questions.open_records() == []


def test_a_carried_question_raised_again_is_one_record_in_its_first_day(monkeypatch, tmp_path):
    cli.main(["init"])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-27T17:00+00:00")
    cli.main(["day", "update", "/careerbrew", "--status", "blocked", "--needs-you", "Pick a reviewer"])
    q = questions.add("/careerbrew", "still?")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    cli.main(["day", "update", "/careerbrew", "--status", "blocked", "--needs-you", "Pick a reviewer"])
    questions.add("/careerbrew", "still?")
    got = questions.open_records()
    assert len(got) == 2 and len({g["id"] for g in got}) == 2 and {g["day"] for g in got} == {"2026-10-27"}, "no copy in today's file"
    assert q["id"] in _ids(got) and questions.records() == []
    config.set_value("work_checkouts", "")
    _plugin(tmp_path, [{"step": "/review", "uses": ["browser:sso"]}])
    config.set_value("work_signin_probe", "false")
    cli.main(["day", "start"])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-29T09:00+00:00")
    cli.main(["day", "start"])
    assert _ids(plan.build()["start_of_day"]["questions"]) == ["signin"], "one sign-in record across days"


def test_an_agent_sees_no_plugin_name_and_cannot_borrow_another_plugins_consent(tmp_path, capsys):
    cli.main(["init"])
    _plugin(tmp_path, [{"step": "/plugin-step", "scope": "x"}], name="secret-co")
    _plugin(tmp_path, [{"step": "/other-step", "scope": "y"}], allow_exec=True, name="open-co")
    capsys.readouterr()
    assert cli.main(["day", "question", "add", "/plugin-step", "approve?", "--recheck", "true", "--plugin", "open-co"]) == 2
    assert "leave out --plugin" in capsys.readouterr().out
    cli.main(["day", "question", "add", "/plugin-step", "approve?", "--recheck", "true"])
    capsys.readouterr()
    cli.main(["day", "question", "recheck", "--json"])
    out = capsys.readouterr().out
    cli.main(["day", "question", "list"])
    out += capsys.readouterr().out
    assert "secret-co" not in out and '"source": "plugin"' in out


def test_an_answered_ask_raised_on_a_later_day_or_blocked_again_is_asked_again(monkeypatch):
    cli.main(["init"])
    cli.main(["day", "update", "/careerbrew", "--status", "blocked", "--needs-you", "Pick a reviewer"])
    questions.close(questions.open_records()[0]["id"])
    cli.main(["day", "update", "/careerbrew", "--status", "blocked", "--needs-you", "Pick a reviewer"])
    assert len(questions.open_records()) == 1, "marked blocked again: the same ask is asked again"
    questions.close(questions.open_records()[0]["id"])
    q = questions.add("/careerbrew", "Send the standup draft?")
    questions.close(q["id"])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-30T09:00+00:00")
    again = questions.add("/careerbrew", "Send the standup draft?")
    assert not again["closed"] and _ids(questions.open_records()) == [q["id"]], "a daily ask answered yesterday is asked again today"
