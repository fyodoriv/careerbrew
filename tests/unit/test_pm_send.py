"""Send the PM update to Slack by default; email stays a draft option (goal 2026-10-20)."""

from pathlib import Path

import pytest

from careerbrew import config, connectors
from careerbrew.cli import main
from careerbrew.day import log as daylog

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "http"


@pytest.fixture
def slack(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIX))
    monkeypatch.setenv("CB_SLACK", "synthetic")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    assert main(["connect", "slack", "--ref", "env:CB_SLACK", "--no-verify"]) == 0
    config.set_value("pm_slack", "#billing-pm")


def test_slack_is_the_default_and_posts_the_text(slack, monkeypatch):
    sent = {}
    real = connectors.http_json

    def spy(url, headers, body=None):
        sent.update(url=url, body=body)
        return real(url, headers, body)

    monkeypatch.setattr(connectors, "http_json", spy)
    assert config.get("pm_send_via") == "slack"
    r = daylog.send_pm("Done today: shipped the export")
    assert r["via"] == "slack" and r["to"] == "#billing-pm" and r["ts"]
    assert sent["url"].endswith("/api/chat.postMessage") and sent["body"] == {"channel": "#billing-pm", "text": "Done today: shipped the export"}


def test_missing_destination_and_missing_connector_name_the_fix(home):
    with pytest.raises(daylog.SendError, match="config set pm_slack"):
        daylog.send_pm("x")
    config.set_value("pm_slack", "#pm")
    with pytest.raises(daylog.SendError, match="connect slack"):
        daylog.send_pm("x")


def test_slack_refusal_is_reported(slack, monkeypatch):
    monkeypatch.setattr(connectors, "http_json", lambda url, headers, body=None: {"ok": False, "error": "channel_not_found"})
    with pytest.raises(daylog.SendError, match="channel_not_found"):
        daylog.send_pm("x")


def test_cli_send_needs_yes_and_email_is_a_draft(slack, capsys):
    assert main(["day", "send"]) == 2
    assert "add --yes" in capsys.readouterr().out
    assert main(["day", "send", "--yes"]) == 0
    assert "sent to Slack #billing-pm" in capsys.readouterr().out
    config.set_value("pm_email", "pm@example.com")
    assert main(["day", "send", "--via", "email"]) == 0
    assert "mailto:pm%40example.com?subject=" in capsys.readouterr().out


def test_issue375_missing_connector_post_refuses_before_secrets_or_http(home, monkeypatch):
    from careerbrew import secrets, server

    config.set_value("pm_slack", "#billing-pm")
    attempts = []

    def forbidden(*_args, **_kwargs):
        attempts.append("send boundary")
        pytest.fail("missing Slack connector must refuse before secret resolution or HTTP")

    monkeypatch.setattr(secrets, "resolve", forbidden)
    monkeypatch.setattr(connectors, "http_json", forbidden)
    with pytest.raises(server.ApiError, match="careerbrew connect slack") as caught:
        server.post("/api/pm/send", {"text": "Synthetic PM update"})
    assert caught.value.status == 400
    assert attempts == []


@pytest.mark.parametrize("configured", [False, True])
def test_issue375_pm_view_reports_connector_presence_without_reading_secrets(home, monkeypatch, configured):
    from careerbrew import secrets, server

    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    config.set_value("pm_slack", "#billing-pm")
    token_ref = "env:CB_SYNTHETIC_SLACK_375"
    if configured:
        monkeypatch.setenv("CB_SYNTHETIC_SLACK_375", "synthetic-test-value")
        assert main(["connect", "slack", "--ref", token_ref, "--no-verify"]) == 0

    def forbidden(*_args, **_kwargs):
        pytest.fail("PM view must not resolve a secret or call HTTP")

    monkeypatch.setattr(secrets, "resolve", forbidden)
    monkeypatch.setattr(connectors, "http_json", forbidden)
    pm = server.daylog_view()["pm"]
    assert pm["slack_configured"] is configured
    assert pm["to"] == "#billing-pm"
    assert token_ref not in repr(pm)
