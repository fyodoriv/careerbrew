"""Issue #63: one `outlook` connector (Microsoft Graph) feeds evidence, mail metadata, and the calendar."""

import json
from datetime import date

import pytest

from careerbrew import calendar, connectors, mail

from .conftest import FIXTURES

OUTLOOK = {"kind": "outlook", "token": "env:CB_TEST_TOKEN"}


@pytest.fixture
def http(monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http"))
    monkeypatch.setenv("CB_TEST_TOKEN", "test-token-not-real")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T10:00+00:00")


def _connect(home, conns):
    (home / "work").mkdir(parents=True, exist_ok=True)
    (home / "work" / "connectors.yaml").write_text(json.dumps({"connectors": conns}))


def test_outlook_is_a_connector_kind_with_a_guide():
    assert "outlook" in connectors.KINDS
    assert "Calendars.ReadBasic" in connectors.GUIDE["outlook"]["scope"]


def test_outlook_evidence_is_meetings_you_organized(home, http):
    items = connectors.run(OUTLOOK, "user")
    assert [i["title"] for i in items] == ["Organized: Design review"]
    assert "design" in items[0]["tags"] and items[0]["kind"] == "outlook"
    assert items[0]["at"] == "2026-10-19T19:00:00Z"  # timezone-aware, as promo compares it with aware times


def test_outlook_connector_feeds_todays_meetings_without_calendars_yaml(home, http):
    _connect(home, [OUTLOOK])
    evs, errors = calendar.events_for(date(2026, 10, 19))
    assert errors == [] and [e.title for e in evs] == ["Focus time", "Design review"]


def test_outlook_calendar_not_doubled_when_also_in_calendars_yaml(home, http):
    _connect(home, [OUTLOOK])
    (home / "day").mkdir(parents=True, exist_ok=True)
    (home / "day" / "calendars.yaml").write_text(json.dumps({"calendars": [{"kind": "graph", "token": "env:CB_TEST_TOKEN"}]}))
    evs, _ = calendar.events_for(date(2026, 10, 19))
    assert len(evs) == 2


def test_outlook_connector_feeds_mail_metadata(home, http):
    _connect(home, [OUTLOOK])
    assert [a["kind"] for a in mail.accounts()] == ["graph"]


def test_policy_without_outlook_keeps_calendar_and_mail_off(home, http, monkeypatch):
    _connect(home, [OUTLOOK])
    monkeypatch.setattr(connectors.plugins, "effective_policy", lambda: {"allowed_connectors": ["github"], "allowed_hosts": None})
    assert calendar.events_for(date(2026, 10, 19)) == ([], []) and mail.accounts() == []


def test_verify_outlook_calls_me(home, http):
    res = connectors.verify(OUTLOOK, "test-token-not-real")
    assert res["ok"] and "outlook: token works" in res["message"]
