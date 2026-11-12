"""A malformed legacy ask must not hide the valid sibling."""

import pytest

from careerbrew.work import feed


@pytest.mark.parametrize("legacy_time", [{}, {"at": None}], ids=["missing", "null"])
def test_open_asks_keeps_valid_sibling(legacy_time):
    valid = {
        "id": "valid",
        "thread": "synthetic-thread",
        "triage": "act",
        "at": "2026-10-19T15:00:00-04:00",
    }
    legacy = {**valid, "id": "legacy"}
    legacy.pop("at")
    legacy.update(legacy_time)

    assert feed.open_asks([legacy, valid]) == [valid]
