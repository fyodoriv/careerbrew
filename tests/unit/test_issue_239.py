"""Issue #239: PM drafts name the meetings the owner led, launches, and initiative check-ins, and rank
low-signal work last. Sources are only what careerbrew already reads: the calendar and the work feed."""

from __future__ import annotations

from datetime import date
from pathlib import Path

import pytest

from careerbrew import calendar, config, store
from careerbrew.day import log as daylog
from careerbrew.day import results, watch
from careerbrew.work import feed, priorities

DAY = date(2026, 10, 21)
ICS = """BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//careerbrew//test//EN
BEGIN:VEVENT
UID:plan@example.com
DTSTART;TZID=Etc/GMT+4:20261021T100000
DTEND;TZID=Etc/GMT+4:20261021T110000
ORGANIZER;CN=Me:mailto:me@example.com
SUMMARY:Billing planning with design and devs
END:VEVENT
BEGIN:VEVENT
UID:standup@example.com
DTSTART;TZID=Etc/GMT+4:20261012T093000
DTEND;TZID=Etc/GMT+4:20261012T094500
RRULE:FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR
ORGANIZER:mailto:me@example.com
SUMMARY:Billing sync
END:VEVENT
BEGIN:VEVENT
UID:other@example.com
DTSTART;TZID=Etc/GMT+4:20261021T130000
DTEND;TZID=Etc/GMT+4:20261021T140000
ORGANIZER:mailto:boss@example.com
SUMMARY:Billing review by the boss
END:VEVENT
END:VCALENDAR
"""


@pytest.fixture(autouse=True)
def now(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T17:00")
    config.set_value("project_priority", "billing=acme/billing|*billing*")


def _row(rid: str, kind: str, title: str, **kw) -> dict:
    return {"id": rid, "at": "2026-10-21T09:00", "kind": kind, "title": title, "source": "evidence", "origin": "user", **kw}


def _calendar(tmp: Path) -> None:
    (tmp / "work.ics").write_text(ICS)
    store.write_yaml(calendar.paths.data_root() / "day" / "calendars.yaml", {"calendars": [{"kind": "ics", "path": str(tmp / "work.ics"), "me": ["me@example.com"]}]})


def _post(eid: str, text: str, thread: str = "", channel: str = "billing-news", **kw) -> dict:
    return {
        "id": eid,
        "source": "slack",
        "kind": "message",
        "at": "2026-10-21T15:00:00Z",
        "title": f"You wrote in #{channel}",
        "actor": "me",
        "channel": channel,
        "url": f"https://chat.example.com/archives/C1/p{eid[-4:]}",
        "thread": thread or eid,
        "text": text,
        **kw,
    }


def test_the_calendar_says_which_meetings_the_owner_led_and_which_repeat(tmp_path):
    _calendar(tmp_path)
    evs = {e.title: e for e in calendar.events_for(DAY)[0]}
    assert evs["Billing planning with design and devs"].led and not evs["Billing planning with design and devs"].recurring
    assert evs["Billing sync"].led and evs["Billing sync"].recurring
    assert not evs["Billing review by the boss"].led


def test_a_led_meeting_gets_named_on_its_projects_line_and_a_recurring_one_does_not(tmp_path):
    _calendar(tmp_path)
    results.append(watch._src_calendar(DAY), DAY)
    (line,) = daylog.pm_lines(daylog.build(DAY), learn=False)
    assert line.startswith("billing: led billing planning with design and devs"), line
    assert "billing sync" not in line.lower() and "2 meetings" in line, "a recurring sync and someone else's meeting are counted, not named"


def test_a_standup_is_never_named_even_when_led(tmp_path):
    results.append([_row("meeting:1", "meeting", "Billing daily standup", led=True)], DAY)
    (line,) = daylog.pm_lines(daylog.build(DAY), learn=False)
    assert "standup" not in line and "1 meeting" in line, line


def test_a_launch_post_leads_its_projects_line_with_a_link():
    results.append([_row("pr:acme/billing#4", "pr", "Export invoices (#4)")], DAY)
    feed.ingest([_post("slack:C1:1.0001", "Billing export is now live for every team!")])
    (line,) = daylog.pm_lines(daylog.build(DAY), learn=False)
    assert line.startswith("billing: launched ([announced in #billing-news](https://chat.example.com/archives/C1/p0001)), export invoices"), line
    assert daylog.unlinked_refs(line) == [], "issue #227: the added part has its link"


def test_an_own_reply_in_a_group_thread_is_a_check_in():
    feed.ingest([_post("slack:C1:2.0002", "Status: on track, next review Friday", thread="slack:C1:1.0000", channel="billing-initiative")])
    (line,) = daylog.pm_lines(daylog.build(DAY), learn=False)
    assert line == "billing: checked in on [#billing-initiative](https://chat.example.com/archives/C1/p0002).", line


def test_a_dm_a_personal_post_or_someone_elses_post_adds_nothing():
    feed.ingest(
        [
            {**_post("slack:D1:3.0003", "we are now live"), "kind": "dm", "channel": ""},
            _post("slack:C1:4.0004", "my side project is now live", personal=True),
            {**_post("slack:C1:5.0005", "billing is now live"), "actor": "other"},
        ]
    )
    assert daylog.pm_lines(daylog.build(DAY), learn=False) == ["Nothing recorded yet."]


def test_the_signal_is_a_word_in_the_analysis_never_the_text():
    feed.ingest([_post("slack:C1:6.0006", "SECRET-CANARY-launch is now live")])
    (row,) = feed.load()
    assert row["signal"] == "launch" and "SECRET-CANARY" not in str(row)


def test_small_ci_or_config_changes_go_last():
    results.append(
        [
            _row("pr:acme/billing#1", "pr", "ci: bump the lint action (#1)"),
            _row("pr:acme/billing#2", "pr", "Close the ledger (#2)"),
            _row("pr:acme/billing#3", "pr", "chore: update config defaults (#3)"),
            _row("pr:acme/billing#5", "pr", "Export invoices (#5)"),
        ],
        DAY,
    )
    (line,) = daylog.pm_lines(daylog.build(DAY), learn=False)
    assert line.startswith("billing: close the ledger, export invoices, bump the lint action, and update config defaults (4 PRs"), line


def test_next_prefers_project_work_over_reviews():
    priorities.add("Review open PRs", 3, 3, 1.0, [])
    priorities.add("Migrate the ledger", 3, 3, 1.0, [])
    d = daylog.build(DAY)
    lines = daylog.pm_lines(d, learn=False)
    nxt = " ".join(lines)
    assert nxt.index("migrate the ledger") < nxt.index("review open PRs"), lines


def test_a_reply_to_an_ask_is_not_a_check_in():
    ask = {**_post("slack:C1:1.0000", "<@U1> can you look?"), "actor": "other", "kind": "mention", "to_me": True, "title": "Mention from sam"}
    feed.ingest([ask, _post("slack:C1:2.0002", "done, see the PR", thread="slack:C1:1.0000", channel="billing-initiative")])
    assert daylog.pm_lines(daylog.build(DAY), learn=False) == ["Nothing recorded yet."]
