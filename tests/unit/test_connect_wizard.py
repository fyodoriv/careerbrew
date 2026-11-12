"""Guided `careerbrew connect` (issue #18): guide, missing fields, who-am-I check, Jira gateway."""

from pathlib import Path

import pytest

from careerbrew import connectors
from careerbrew.cli import main

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "http"


@pytest.fixture
def http(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIX))
    monkeypatch.setenv("CB_JIRA", "synthetic-scoped-token")


def _saved(home):
    from careerbrew import store

    return store.read_yaml(home / "work" / "connectors.yaml")["connectors"]  # files are encrypted at rest


def test_scoped_token_switches_to_the_gateway(http, home, capsys):
    rc = main(["connect", "jira", "--ref", "env:CB_JIRA", "--base-url", "https://scoped.atlassian.example.com", "--email", "me@example.com"])
    out = capsys.readouterr().out
    assert rc == 0 and "id.atlassian.com/manage-profile/security/api-tokens" in out and "through the API gateway" in out
    (c,) = _saved(home)
    assert c["base_url"] == "https://api.atlassian.com/ex/jira/synthetic-cloud-1" and c["email"] == "me@example.com"


def test_a_token_that_fails_myself_is_rejected_and_not_saved(http, home, capsys):
    rc = main(["connect", "jira", "--ref", "env:CB_JIRA", "--base-url", "https://bad.atlassian.example.com", "--email", "me@example.com"])
    assert rc == 1 and "FAIL  jira rejected the token" in capsys.readouterr().out
    assert not (home / "work" / "connectors.yaml").exists()


def test_missing_email_is_named_with_its_fix(http, home, capsys):
    rc = main(["connect", "jira", "--ref", "env:CB_JIRA", "--base-url", "https://jira.example.com"])
    assert rc == 2 and "missing --email" in capsys.readouterr().out


def test_interactive_asks_for_missing_fields_and_token(http, home, monkeypatch, capsys):
    import getpass
    import sys

    monkeypatch.setattr(sys.stdin, "isatty", lambda: True)
    answers = iter(["https://jira.example.com", "me@example.com"])
    monkeypatch.setattr("builtins.input", lambda _p: next(answers))
    monkeypatch.setattr(getpass, "getpass", lambda _p: "synthetic-token")
    monkeypatch.setattr(connectors.secrets, "store_keychain", lambda svc, v: f"keychain:{svc}")
    assert main(["connect", "jira"]) == 0
    (c,) = _saved(home)
    assert c["token"] == "keychain:careerbrew.jira" and c["base_url"] == "https://jira.example.com"


def test_status_gives_steps_for_an_unresolved_connector(home, monkeypatch):
    monkeypatch.delenv("CB_NOPE", raising=False)
    st = connectors.status({"kind": "jira", "token": "env:CB_NOPE", "base_url": "https://x.example.com"})
    assert not st["ready"] and st["missing"] == ["email"] and "careerbrew connect jira" in st["steps"][1]
    monkeypatch.setenv("CB_OK", "t")
    assert connectors.status({"kind": "github", "token": "env:CB_OK", "user": "me"})["ready"]


def test_unreachable_server_is_a_fail_not_a_crash(home, monkeypatch):
    import urllib.error

    def boom(url, headers, body=None):
        raise urllib.error.URLError("no route")

    monkeypatch.setattr(connectors, "http_json", boom)
    res = connectors.verify({"kind": "github", "user": "me"}, "t")
    assert not res["ok"] and "could not reach" in res["message"]


@pytest.mark.parametrize("argv", [["jira", "--ref", "env:CB_JIRA", "--base-url", "https://jira.example.com", "--email", "me@example.com"], ["outlook", "--client-id", "synthetic-client"]])
def test_connect_refuses_a_connector_the_plugin_policy_leaves_out(http, home, capsys, monkeypatch, argv):
    """The policy check comes first: no token read, no sign-in call, nothing saved."""
    from careerbrew import msauth, secrets

    monkeypatch.setenv("CAREERBREW_TEST_OWNER", "1")
    monkeypatch.setattr(connectors.plugins, "effective_policy", lambda: {"allowed_connectors": ["github"], "allowed_hosts": None})
    monkeypatch.setattr(secrets, "resolve", lambda _ref: pytest.fail("the token must not be read"))
    monkeypatch.setattr(msauth, "start", lambda *_a: pytest.fail("the sign-in must not start"))
    rc = main(["connect", *argv])
    out = capsys.readouterr().out
    assert rc == 3 and f"connector {argv[0]} is not allowed by plugin policy" in out
    assert not (home / "work" / "connectors.yaml").exists()
