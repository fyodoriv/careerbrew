"""Jira: a rejected token is a source error, and every page is read (issue #19)."""

import io
import urllib.error

import pytest

from careerbrew import connectors

CFG = {"kind": "jira", "token": "env:CB_TEST_TOKEN", "base_url": "https://jira.example.com", "email": "me@example.com"}


@pytest.fixture(autouse=True)
def token(monkeypatch):
    monkeypatch.setenv("CB_TEST_TOKEN", "synthetic-token")
    monkeypatch.delenv("CAREERBREW_HTTP_FIXTURES", raising=False)


def _issue(key):
    return {"key": key, "fields": {"summary": f"done {key}", "resolutiondate": "2026-10-01T00:00:00.000+0000", "issuetype": {"name": "Task"}}}


def test_401_on_myself_is_a_source_error_not_an_empty_success(home, monkeypatch):
    def fake(url, headers, body=None):
        if url.endswith("/myself"):
            raise urllib.error.HTTPError(url, 401, "Unauthorized", {}, io.BytesIO(b""))
        return {"issues": [], "isLast": True}

    monkeypatch.setattr(connectors, "http_json", fake)
    with pytest.raises(connectors.ConnectorError, match="rejected the token"):
        connectors.jira(CFG, "user")


def test_two_pages_are_both_read(home, monkeypatch):
    seen = []

    def fake(url, headers, body=None):
        if url.endswith("/myself"):
            return {"accountId": "x"}
        seen.append(url)
        if "nextPageToken=" not in url:
            return {"issues": [_issue(f"PROJ-{i}") for i in range(100)], "nextPageToken": "tok2", "isLast": False}
        return {"issues": [_issue("PROJ-100")], "isLast": True}

    monkeypatch.setattr(connectors, "http_json", fake)
    got = connectors.jira(CFG, "user")
    assert len(got) == 101 and len(seen) == 2 and "nextPageToken=tok2" in seen[1]
