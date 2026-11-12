"""Project session rules use current project data and respect output policy."""

import json

import pytest

from careerbrew import actions, cli, events
from careerbrew.work import sessions


@pytest.fixture(autouse=True)
def project(monkeypatch, capsys):
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert cli.main(["init"]) == 0
    assert cli.main(["work", "project", "add", "sample"]) == 0
    capsys.readouterr()


def test_save_and_read_rules(capsys):
    assert cli.main(["work", "project", "rules-set", "sample", "--yes", "--text", "Skip example repository"]) == 0
    capsys.readouterr()
    assert cli.main(["work", "project", "rules", "sample", "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["rules"] == "Skip example repository"


def test_replace_and_clear_rules(capsys):
    assert cli.main(["work", "project", "rules-set", "sample", "--yes", "--text", "first"]) == 0
    assert cli.main(["work", "project", "rules-set", "sample", "--yes", "--text", "second"]) == 0
    capsys.readouterr()
    assert cli.main(["work", "project", "rules", "sample", "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["rules"] == "second"
    assert cli.main(["work", "project", "rules-set", "sample", "--yes", "--clear"]) == 0
    capsys.readouterr()
    assert cli.main(["work", "project", "rules", "sample", "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["rules"] == ""


def test_existing_session_reads_latest_rules_on_next_pass(capsys):
    assert "At the start of each project-session pass" in sessions.CONTRACT
    assert "careerbrew work project rules <project>" in sessions.CONTRACT
    sessions.open_session("sample", "session-1")
    assert cli.main(["work", "project", "rules-set", "sample", "--yes", "--text", "old"]) == 0
    capsys.readouterr()
    assert cli.main(["work", "project", "rules", "sample", "--json"]) == 0
    first_snapshot = json.loads(capsys.readouterr().out)["rules"]
    assert cli.main(["work", "project", "rules-set", "sample", "--yes", "--text", "new"]) == 0
    assert sessions.open_session("sample")["reused"] is True
    capsys.readouterr()
    assert cli.main(["work", "project", "rules", "sample", "--json"]) == 0
    assert first_snapshot == "old"
    assert json.loads(capsys.readouterr().out)["rules"] == "new"


def test_restricted_output_never_shows_rule_text(monkeypatch, capsys):
    canary = "synthetic-rule-canary-438"
    assert cli.main(["work", "project", "rules-set", "sample", "--yes", "--text", canary]) == 0
    capsys.readouterr()
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    assert cli.main(["work", "project", "rules", "sample", "--json"]) != 0
    assert canary not in capsys.readouterr().out


def test_owner_actions_can_save_and_read_rules(monkeypatch, capsys):
    monkeypatch.setattr(cli, "may_show_business", lambda: cli._owner_ui())
    assert actions.run("work.project.rules-set", {"name": "sample", "text": "Owner rule"}, confirm=True)["exit"] == 0
    result = actions.run("work.project.rules", {"name": "sample", "json": True})
    assert result["exit"] == 0
    assert json.loads(result["output"])["rules"] == "Owner rule"
    capsys.readouterr()


def test_restricted_session_can_continue_when_no_rules_exist(monkeypatch, capsys):
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    assert cli.main(["work", "project", "rules", "sample", "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["rules"] == ""


def test_replacement_requires_confirmation_from_cli_and_owner_ui(capsys):
    assert cli.main(["work", "project", "rules-set", "sample", "--text", "first"]) == 1
    assert "not changed" in capsys.readouterr().out
    with pytest.raises(actions.ActionError, match="confirm"):
        actions.run("work.project.rules-set", {"name": "sample", "text": "second"})
    assert cli.main(["work", "project", "rules", "sample", "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["rules"] == ""


def test_rules_do_not_leak_into_restricted_views_or_events(monkeypatch, capsys):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    canary = "SYNTHETIC-RULE-CANARY-438"
    assert cli.main(["work", "project", "rules-set", "sample", "--yes", "--text", canary]) == 0
    sessions.open_session("sample", "session-privacy")
    capsys.readouterr()
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    for command in (["work", "project", "list", "--json"], ["work", "session", "list", "--json"], ["day", "run", "--json"]):
        assert cli.main(command) == 0
        assert canary not in capsys.readouterr().out
    assert canary not in events.log_path().read_text()
