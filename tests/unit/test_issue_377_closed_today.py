"""Registered synthetic backend histories for the Today closed-ask count."""

import json
import os
import time

import pytest

from careerbrew import config, connectors, server, store
from careerbrew.work import feed

AS_OF = "2026-11-03T12:00:00-04:00"
DAY = "2026-11-03"
WORLD = os.environ.get("ISSUE377_CLOSED_TODAY_WORLD", "after")
OLD_KEYS = {"asks", "total", "limit", "events", "overdue_hours"}


@pytest.fixture
def owned_feed(home, monkeypatch, tmp_path):
    if WORLD not in {"before", "after"}:
        pytest.fail("BLOCKED: invalid closed-today fixture world")
    monkeypatch.setenv("CAREERBREW_NOW", AS_OF)
    config.set_value("timezone", "Etc/GMT+4")
    config.set_value("work_start", "09:00")
    config.set_value("work_end", "17:30")
    config.set_value("feed_ask_overdue_hours", "8")

    def deny_transport(*_args, **_kwargs):
        pytest.fail("BLOCKED: closed-today history attempted connector transport")

    monkeypatch.setattr(connectors, "http_json", deny_transport)
    monkeypatch.setattr(connectors, "http_get", deny_transport)
    # The machine zone (UTC) differs from the setting, so the day must come from the setting.
    with pytest.MonkeyPatch.context() as mp:
        mp.setenv("TZ", "UTC")
        time.tzset()
        yield tmp_path
    time.tzset()


def _stamp(clock):
    return f"{DAY}T{clock}Z"


def _row(event_id, thread, clock, *, actor="other", personal=False):
    return {
        "id": event_id,
        "source": "slack",
        "kind": "dm",
        "thread": f"slack:synthetic-{thread}",
        "at": _stamp(clock),
        "title": f"Synthetic {event_id}",
        "actor": actor,
        "to_me": actor != "me",
        "personal": personal,
        "url": f"https://example.com/synthetic/{event_id}",
    }


def _reply(event_id, thread, clock, *, personal=False):
    return _row(event_id, thread, clock, actor="me", personal=personal)


def _ingest(*rows):
    assert feed.ingest(list(rows))["new"] == len(rows)


def _mark(monkeypatch, event_id, value, clock):
    with monkeypatch.context() as patch:
        patch.setenv("CAREERBREW_NOW", _stamp(clock))
        row = feed.mark_ask(event_id, value)
    if value == "ask":
        assert "ask_mark" not in row and "ask_marked_at" not in row
    else:
        assert (row["ask_mark"], row["ask_marked_at"]) == (value, _stamp(clock))
    return {"id": event_id, "value": value, "mark": row.get("ask_mark"), "at": row.get("ask_marked_at")}


def _view(*args):
    path = feed.events_path()
    before = path.read_bytes(), path.stat().st_mtime_ns
    response = server.feed_asks(*args)
    assert (path.read_bytes(), path.stat().st_mtime_ns) == before
    assert {key for key in response if key != "closed_today"} == OLD_KEYS
    return response


def _open_ids(response):
    return [row["id"] for row in response["asks"]]


def _retain(path, case, views, marks=()):
    with (path / f"{case}.json").open("x", encoding="utf-8") as stream:
        json.dump({"case": case, "world": WORLD, "views": views, "marks": marks}, stream, indent=2)
        stream.write("\n")


def test_closed_today_P01_first_reply_local_day(owned_feed):
    _ingest(
        _row("ask-a", "a", "03:40:00"),
        _reply("reply-a-first", "a", "03:59:59"),
        _reply("reply-a-repeat", "a", "04:01:00"),
        _row("ask-b", "b", "03:50:00"),
        _reply("reply-b-first", "b", "04:00:00"),
        _reply("reply-b-repeat", "b", "05:00:00"),
        _row("ask-b-later", "b", "05:30:00"),
    )
    current = _view()
    assert current["total"] == 1 and _open_ids(current) == ["ask-b-later"]
    _retain(owned_feed, "P01", [current])
    assert current.get("closed_today") == 1


def test_closed_today_P02_manual_done_and_undo(owned_feed, monkeypatch):
    _ingest(_row("manual-current", "manual-current", "04:15:00"), _row("manual-prior", "manual-prior", "03:15:00"))
    marks = [_mark(monkeypatch, "manual-prior", "done", "03:59:00")]
    _ingest(_reply("reply-prior", "manual-prior", "04:30:00"))
    before_done = _view()
    assert _open_ids(before_done) == ["manual-current"]

    marks.append(_mark(monkeypatch, "manual-current", "done", "05:00:00"))
    done = _view()
    _ingest(_reply("reply-current", "manual-current", "06:00:00"))
    replied = _view()
    marks.append(_mark(monkeypatch, "manual-current", "ask", "07:00:00"))
    undone = _view()
    assert all(result["total"] == 0 and _open_ids(result) == [] for result in (done, replied, undone))
    views = [before_done, done, replied, undone]
    _retain(owned_feed, "P02", views, marks)
    assert [result.get("closed_today") for result in views] == [0, 1, 1, 1]


def test_closed_today_C01_equal_unrelated_teammate_future(owned_feed):
    _ingest(
        _row("equal", "equal", "05:00:00"),
        _reply("equal-reply", "equal", "05:00:00"),
        _row("unrelated", "unrelated", "05:10:00"),
        _reply("unrelated-reply", "different", "06:00:00"),
        _row("teammate", "teammate", "05:20:00"),
        _row("teammate-reply", "teammate", "06:00:00", actor="other") | {"to_me": False},
        _row("future", "future", "05:30:00"),
        _reply("future-reply", "future", "16:00:01"),
    )
    current = _view()
    # The old matcher already treats an equal or future reply as closing.
    assert current["total"] == 2 and _open_ids(current) == ["teammate", "unrelated"]
    _retain(owned_feed, "C01", [current])
    if WORLD == "after":
        assert current.get("closed_today") == 1


def test_closed_today_C02_private_and_undated_mark(owned_feed, monkeypatch):
    _ingest(
        _row("personal", "personal", "05:00:00", personal=True),
        _reply("personal-answer", "personal", "06:00:00"),
        _row("not-an-ask", "not-an-ask", "05:10:00"),
        _reply("not-an-ask-reply", "not-an-ask", "06:00:00"),
        _row("legacy-done", "legacy-done", "05:20:00"),
        _row("personal-reply-ask", "personal-reply", "05:30:00"),
        _reply("personal-reply", "personal-reply", "06:00:00", personal=True),
    )
    marks = [
        _mark(monkeypatch, "not-an-ask", "not-ask", "06:30:00"),
        _mark(monkeypatch, "legacy-done", "done", "06:40:00"),
    ]
    rows = feed.load()
    next(row for row in rows if row["id"] == "legacy-done").pop("ask_marked_at")
    # The normal mark writer cannot create a legacy mark without its timestamp.
    store.write_jsonl(feed.events_path(), rows)
    current = _view()
    assert current["total"] == 0 and _open_ids(current) == []
    _retain(owned_feed, "C02", [current], marks)
    if WORLD == "after":
        assert current.get("closed_today") == 0


def test_closed_today_C03_total_independent_of_page(owned_feed):
    open_ids = [f"open-{index:02}" for index in range(52)]
    _ingest(
        *(_row(event_id, event_id, "12:00:00") for event_id in open_ids),
        _row("closed-a", "closed-a", "05:00:00"),
        _reply("closed-a-reply", "closed-a", "06:00:00"),
        _row("closed-b", "closed-b", "05:00:00"),
        _reply("closed-b-reply", "closed-b", "06:00:00"),
    )
    views = [_view(), _view(1), _view(52)]
    expected_rows = [
        {
            "id": event_id,
            "source": "slack",
            "kind": "dm",
            "at": "2026-11-03T12:00:00Z",
            "title": f"Synthetic {event_id}",
            "url": f"https://example.com/synthetic/{event_id}",
            "waiting_minutes": 240,
            "work_minutes": 180,
            "overdue": False,
        }
        for event_id in reversed(open_ids)
    ]
    for result, length in zip(views, (50, 1, 52), strict=True):
        old = {key: value for key, value in result.items() if key != "closed_today"}
        assert old == {"asks": expected_rows[:length], "total": 52, "limit": 1000, "events": 56, "overdue_hours": 8}
    _retain(owned_feed, "C03", views)
    if WORLD == "after":
        assert [result.get("closed_today") for result in views] == [2, 2, 2]
