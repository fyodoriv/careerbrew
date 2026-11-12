"""Issue #145: the PM update says when it last changed, and whether it is the generated draft or the owner's edit."""

import os
from datetime import date, datetime

from careerbrew import calendar
from careerbrew.day import log as daylog

DAY = date(2026, 10, 16)


def _written_at(path, local: str) -> None:
    """Set a file's last write to a local wall time, as if it was written then."""
    t = datetime.fromisoformat(local).replace(tzinfo=calendar.local_tz()).timestamp()
    os.utime(path, (t, t))


def test_the_generated_draft_is_dated_by_the_day_logs_last_change(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    d = daylog.save(DAY)
    _written_at(daylog.path_for(DAY), "2026-10-16T15:45")
    pm = daylog.pm_report(d)
    assert pm["edited"] is False
    assert pm["updated_at"].startswith("2026-10-16T15:45"), pm["updated_at"]


def test_the_owners_edit_is_dated_by_its_last_save_and_a_reset_goes_back_to_the_draft(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    d = daylog.save(DAY)
    _written_at(daylog.path_for(DAY), "2026-10-16T09:10")
    daylog.save_pm_draft(DAY.isoformat(), "My own words")
    _written_at(daylog.pm_draft_path(DAY.isoformat()), "2026-10-16T15:45")
    pm = daylog.pm_report(d)
    assert pm["edited"] is True and pm["updated_at"].startswith("2026-10-16T15:45"), pm
    daylog.save_pm_draft(DAY.isoformat(), "")
    pm = daylog.pm_report(d)
    assert pm["edited"] is False and pm["updated_at"].startswith("2026-10-16T09:10"), "a reset shows the generated draft's time"


def test_a_log_that_was_never_kept_has_no_time(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    assert daylog.pm_report(daylog.build(DAY))["updated_at"] == ""
