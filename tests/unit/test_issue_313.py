"""Issue #313: each review asked of you opens one review session record; a repeat opens none; nothing is sent."""

import json
from pathlib import Path

import pytest

from careerbrew.work import listen, live

FIX = Path(__file__).resolve().parents[1] / "fixtures"
LISTEN = FIX / "live" / "listen"
REVIEW = "1792936860.000200"
PR = "https://github.com/example-shop/api/pull/9"


@pytest.fixture
def inbox(home, tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-25T16:00")
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIX / "http"))
    d = tmp_path / "inbox"
    d.mkdir()
    live.start((LISTEN / "sources.yaml").read_text().replace("@DIR@", str(d)))
    (d / "chat.json").write_text((LISTEN / "chat.json").read_text())
    return d


def _follow(rec: dict, ts: str) -> dict:
    return next(f for f in rec["followups"] if f["id"] == f"chat:{ts}")


def test_a_new_review_ask_opens_one_review_session(inbox):
    rec = live.cycle()
    key = listen.review_key(PR)
    assert f"review session opened: {key}" in _follow(rec, REVIEW)["actions"]
    sessions = listen.review_sessions()
    assert list(sessions) == [key]
    s = sessions[key]
    assert s["url"] == PR and s["status"] == "requested" and s["event"] == f"chat:{REVIEW}"
    assert rec["review_sessions"] == 1
    assert any("review session" in a and "never post" in a for a in rec["actions"]), rec["actions"]


def test_a_re_review_ping_for_the_same_pr_opens_none(inbox):
    live.cycle()
    ping = {"ts": "1792937999.000999", "text": f"@owner friendly ping, can you re-review {PR}", "actor": "other", "kind": "mention", "to_me": True, "channel": "team-alpha", "thread": "chat:r1", "from": "Bo Chen"}
    (inbox / "chat.json").write_text(json.dumps({"ok": True, "messages": [ping]}))
    rec = live.cycle()
    assert "review session kept: one is already open for this request" in _follow(rec, "1792937999.000999")["actions"]
    assert len(listen.review_sessions()) == 1 and rec["review_sessions"] == 0


def test_a_review_session_record_holds_no_message_text(inbox):
    live.cycle()
    s = next(iter(listen.review_sessions().values()))
    assert set(s) == {"event", "url", "status", "opened"}
    assert "please review" not in json.dumps(s)
