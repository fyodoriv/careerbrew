"""Issue #210: a next-day handoff, kept per day and shown first by the next morning's first session."""

from __future__ import annotations

import json

import pytest

from careerbrew import cli
from careerbrew.cli import main
from careerbrew.day import handoff


@pytest.fixture
def day(monkeypatch):
    def at(when: str) -> None:
        monkeypatch.setenv("CAREERBREW_NOW", when)

    at("2026-10-21T17:00")
    return at


def test_the_next_day_shows_the_latest_earlier_handoff(day, capsys, monkeypatch):
    handoff.add("promises", "Send the synthetic doc", "https://example.com/d")
    day("2026-10-22T08:00")
    handoff.add("lessons", "a later item for tomorrow")  # today's own items are not this morning's handoff
    d, items = handoff.latest_before()
    assert str(d) == "2026-10-21" and [i["section"] for i in items] == ["promises"]
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert main(["day", "handoff", "show"]) == 0
    out = capsys.readouterr().out
    assert "handoff from 2026-10-21: 1 item(s). Verify each one live before acting." in out
    assert "Promises you made (1):" in out and "Send the synthetic doc https://example.com/d" in out


def test_start_shows_the_handoff_first(day, capsys, monkeypatch):
    handoff.add("waiting", "Synthetic review on PR 12")
    day("2026-10-22T09:00")
    main(["config", "set", "run_pages", "off"])
    capsys.readouterr()
    main(["--no-watch", "--no-web", "--no-refresh"])
    lines = [ln for ln in capsys.readouterr().out.splitlines() if ln.strip()]
    first = next(i for i, ln in enumerate(lines) if "handoff from 2026-10-21" in ln)
    assert first <= next(i for i, ln in enumerate(lines) if ln.startswith("today:"))


def test_done_drops_an_item_and_a_repeat_is_kept_once(day):
    a = handoff.add("gotchas", "Synthetic gotcha")
    assert handoff.add("gotchas", "Synthetic  gotcha")["id"] == a["id"]
    assert len(handoff.load()) == 1
    assert handoff.done(a["id"]) and handoff.load() == []
    assert not handoff.done(a["id"])


@pytest.mark.parametrize("section,text", [("nope", "x"), ("waiting", "   "), ("waiting", "x" * 201)])
def test_bad_items_are_refused(day, capsys, section, text):
    assert main(["day", "handoff", "add", section, text]) == 2
    assert "breaks US-85" in capsys.readouterr().out


def test_an_agent_sees_ids_not_text(day, capsys, monkeypatch):
    handoff.add("unverified", "Synthetic claim about the deploy", "https://example.com/x")
    day("2026-10-22T09:00")
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    assert main(["day", "handoff", "show", "--json"]) == 0
    data = json.loads(capsys.readouterr().out)
    assert data["verify_live_before_acting"] is True
    assert data["items"][0]["section"] == "unverified" and "text" not in data["items"][0]
    main(["day", "handoff", "show"])
    assert "Synthetic claim" not in capsys.readouterr().out
