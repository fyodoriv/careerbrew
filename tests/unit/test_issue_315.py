"""Issue #315: a live cycle republishes the standup rows when a new event matches a row, the last publish is stale, or the standup is next."""

from datetime import timedelta

import pytest

from careerbrew import calendar, config
from careerbrew.work import live, standup

ROWS = {"rows": [{"title": "Checkout redesign", "keys": ["SHOP-12"], "aliases": ["checkout"], "repos": [], "project": ""}], "groups": [], "ticket_projects": []}


@pytest.fixture
def clock(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T08:00")  # a Wednesday, two hours before the 10:00 standup
    config.set_value("standup_time", "10:00")
    return calendar.now()


def _event(title: str, project: str = "") -> dict:
    return {"id": f"tickets:{title}", "title": title, "project": project, "at": "2026-10-28T07:55:00Z"}


def test_a_new_event_that_matches_a_row_is_due(clock):
    recent = (clock - timedelta(minutes=5)).isoformat()
    assert standup.due([_event("SHOP-12 moved to review")], ROWS, recent, clock) == "match"
    assert standup.due([_event("Fixed the checkout button")], ROWS, recent, clock) == "match"


def test_a_fresh_publish_with_no_match_is_skipped(clock):
    recent = (clock - timedelta(minutes=5)).isoformat()
    assert standup.due([_event("Lunch order")], ROWS, recent, clock) == ""


def test_a_stale_or_missing_publish_is_due(clock):
    assert standup.due([], ROWS, (clock - timedelta(minutes=61)).isoformat(), clock) == "stale"
    assert standup.due([], ROWS, "", clock) == "stale"


def test_the_cycle_just_before_the_standup_is_due(clock):
    soon = clock.replace(hour=9, minute=50)
    assert standup.due([], ROWS, (soon - timedelta(minutes=5)).isoformat(), soon) == "standup next"


def test_a_publish_records_its_time_and_row_count(clock, monkeypatch):
    monkeypatch.setattr(standup, "load_rows", lambda: ROWS)
    monkeypatch.setattr(standup, "run", lambda now=None: {"rows": [{"title": "Checkout redesign"}]})
    st: dict = {}
    got = live._standup(st, [_event("SHOP-12 moved to review")])
    assert got == {"status": "published", "rows": 1, "reason": "match"}
    assert st["standup_at"] == clock.isoformat(timespec="seconds")
    assert live.standup_line(got) == "standup: 1 rows published (match)"
    assert live.standup_line(live._standup(st, [_event("Lunch order")])) == "standup: skipped, fresh"


def test_a_failed_publish_is_reported_and_never_raises(clock, monkeypatch):
    def boom(now=None):
        raise standup.StandupError("the sheet could not be read")

    monkeypatch.setattr(standup, "load_rows", lambda: ROWS)
    monkeypatch.setattr(standup, "run", boom)
    st: dict = {}
    got = live._standup(st, [_event("SHOP-12 moved to review")])
    assert got["status"] == "failed" and "standup_at" not in st
    assert live.standup_line(got) == "standup: failed (StandupError); the cycle goes on"


def test_no_rows_means_no_publish(clock, monkeypatch):
    monkeypatch.setattr(standup, "load_rows", lambda: {})
    assert live._standup({}, [_event("SHOP-12")]) == {"status": "skipped", "reason": "no rows"}
