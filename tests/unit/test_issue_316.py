"""Issue #316: a calendar reply saved as returned; a record with no event id or start is skipped."""

from careerbrew.work import feed

REPLY = {
    "events": [
        {"id": "abc", "start": {"dateTime": "2026-10-29T15:00:00Z"}, "summary": "Design review", "htmlLink": "https://cal.example.com/abc"},
        {"start": {"dateTime": "2026-10-29T16:00:00Z"}, "summary": "No id"},
        {"id": "nostart", "summary": "No start"},
        {"id": "allday", "start": {"date": "2026-10-30"}, "summary": "Offsite"},
    ]
}


def test_a_calendar_reply_stores_events_with_a_stable_id():
    events, skipped, source = feed.raw_events(REPLY, "calendar")
    assert source == "calendar" and skipped == 2
    assert [e["id"] for e in events] == ["calendar:abc:2026-10-29T15:00:00Z", "calendar:allday:2026-10-30"]
    first = events[0]
    assert first["kind"] == "meeting" and first["title"] == "Design review" and first["url"] == "https://cal.example.com/abc"


def test_no_stored_id_is_built_from_an_empty_part():
    events, _skipped, _source = feed.raw_events(REPLY, "calendar")
    assert not any(e["id"].endswith(":") or "::" in e["id"] or "@" in e["id"] for e in events)


def test_a_reply_with_no_events_is_a_quiet_source():
    assert feed.raw_events({"events": []}, "calendar") == ([], 0, "calendar")


def test_an_events_list_without_calendar_starts_is_read_as_before():
    """A paging tool's reply also holds `events`: it is not a calendar reply, so the generic reader keeps it."""
    reply = {"events": [{"id": "pg-1", "at": "2026-10-29T15:00:00Z", "title": "Disk full on db-3"}]}
    assert feed.raw_events(reply, "pager") is None
