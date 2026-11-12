"""Registered PR claim identity and preservation controls (#357)."""

import pytest

from careerbrew import actions, cli
from careerbrew.work import sessions


@pytest.fixture
def opened(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T09:00:00+00:00")
    assert cli.main(["init"]) == 0
    for project in ("billing", "ledger"):
        assert cli.main(["work", "project", "add", project]) == 0
    sessions.open_session("billing", "sess-a")
    sessions.open_session("ledger", "sess-b")


@pytest.mark.parametrize(
    ("original", "alias", "other"),
    [
        ("pr:Example/Tools#17", "pr:example/tools#17", "pr:example/tools#18"),
        ("pr:Example/Tools#18", "pr:EXAMPLE/TOOLS#18", "pr:example/tools#17"),
        ("pr:Example/Other#17", "pr:example/other#17", "pr:example/tools#17"),
        ("pr:Org/Repo-1#33", "pr:org/repo-1#33", "pr:org/repo-2#33"),
    ],
)
def test_pr_claim_identity_history(opened, capsys, original, alias, other):
    private_note = "synthetic-private-handoff-357"
    sessions.write_handoff("sess-a", private_note)
    capsys.readouterr()
    assert cli.main(["work", "claim", "take", original, "--session", "sess-a"]) == 0
    assert private_note not in capsys.readouterr().out
    result = actions.run("work.claim.take", {"resource": alias, "session": "sess-b"})
    assert result["exit"] == 1 and "skip it" in result["output"]
    assert private_note not in result["output"] and "billing" not in result["output"]
    sessions.take(other, "sess-b")
    assert sessions.holder(alias)["session"] == "sess-a"
    assert sessions.take(alias, "sess-a")["renewed"] is True
    assert sessions.release(alias, "sess-a")
    assert sessions.holder(original) is None
    assert sessions.holder(other)["session"] == "sess-b"


def test_invalid_pr_refuses_without_write(opened):
    before = sessions.path().read_bytes()
    for value in (
        "pr:tools#17",
        "pr:https://github.com/example/tools/pull/17",
        "pr:github.example.com/example/tools#17",
        "pr:example/tools#0",
        "pr:example/tools#01",
        "pr:/tools#17",
        "pr:example/#17",
        "pr:example/tools #17",
        " pr:example/tools#17",
        "pr:example/tools#17 ",
        "pr:./tools#1",
        "pr:example/..#1",
        "pr:example/tools/extra#1",
        "pr:" + "a" * 194 + "/r#1",
    ):
        with pytest.raises(sessions.SessionError):
            sessions.take(value, "sess-a")
        assert sessions.path().read_bytes() == before


def test_pr_claim_accepts_exact_length_limit(opened):
    value = "pr:" + "a" * 193 + "/r#1"
    assert len(value) == 200
    sessions.take(value, "sess-a")
    assert sessions.holder(value)["session"] == "sess-a"


def test_existing_resource_expiry_and_release(opened, monkeypatch):
    with pytest.raises(sessions.SessionError, match="no open session") as caught:
        sessions.take("browser:sso", "sess-not-open")
    assert "careerbrew work session open" in str(caught.value)
    sessions.take("browser:sso", "sess-a")
    assert sessions.take("browser:sso", "sess-a")["renewed"] is True
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T09:31:00+00:00")
    assert sessions.holder("browser:sso") is None
    sessions.take("browser:sso", "sess-b")
    assert sessions.release("browser:sso", "sess-b")


def test_cli_and_action_share_claim_without_private_text(opened, capsys):
    sessions.write_handoff("sess-a", "synthetic-private-handoff-357")
    capsys.readouterr()
    assert cli.main(["work", "claim", "take", "pr:Example/Tools#17", "--session", "sess-a"]) == 0
    capsys.readouterr()
    result = actions.run("work.claim.take", {"resource": "pr:example/tools#17", "session": "sess-b"})
    assert result["exit"] == 1 and "skip it" in result["output"]
    assert "synthetic-private-handoff-357" not in result["output"] and "billing" not in result["output"]
