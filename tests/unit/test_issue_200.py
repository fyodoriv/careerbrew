"""Issue #200: the standup report compares against the previous standup day, never today's own standup.

The clock is pinned with CAREERBREW_NOW. 2026-10-19 is a Monday; the standup is 10:00 for 15 minutes.
"""

from __future__ import annotations

import pytest

from careerbrew import config
from careerbrew.cli import main
from careerbrew.work import standup


def test_a_monday_run_after_the_standup_starts_at_thursdays_standup(monkeypatch):
    config.set_value("standup_days", "0,1,2,3")  # Monday to Thursday
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T10:31")
    w = standup.window()
    assert w["since"] == "Thu Oct 15, 10:00", "never today's own standup"
    assert w["mode"] == "previous-day" and w["compares"] == "Thu Oct 15"


def test_the_previous_day_holds_all_day_and_before_the_standup(monkeypatch):
    for now in ("2026-10-20T08:00", "2026-10-20T10:05", "2026-10-20T23:59"):
        monkeypatch.setenv("CAREERBREW_NOW", now)
        assert standup.window()["since"] == "Mon Oct 19, 10:00", now


def test_since_last_keeps_the_old_window(monkeypatch):
    config.set_value("standup_days", "0,1,2,3")
    config.set_value("standup_window", "since-last")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T10:31")
    w = standup.window()
    assert w["since"] == "Mon Oct 19, 10:00" and w["mode"] == "since-last" and w["compares"] == "Mon Oct 19"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T10:05")
    assert standup.window()["since"] == "Thu Oct 15, 10:00", "during the standup it still counts from the one before"


def test_standup_window_is_a_validated_setting():
    assert config.get("standup_window") == "previous-day"
    assert "standup_window" in {r["key"] for r in config.effective()}
    with pytest.raises(ValueError, match="previous-day"):
        config.set_value("standup_window", "yesterday")
    assert config.set_value("standup_window", "since-last") == "since-last"


def test_the_cli_names_the_day_it_compares_against(monkeypatch, capsys):
    config.set_value("standup_days", "0,1,2,3")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T10:31")
    assert main(["work", "standup"]) == 0
    assert "compares against the Thu Oct 15 standup" in capsys.readouterr().out
