"""Issue #214: impact as work happens: expected at the start, measured with evidence at delivery, fed forward."""

from __future__ import annotations

from datetime import date

import pytest

from careerbrew.cli import main
from careerbrew.day import inbox, log, results
from careerbrew.work import evidence, priorities


@pytest.fixture(autouse=True)
def now(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:00")
    monkeypatch.setenv("CAREERBREW_TEST_OWNER", "1")


def _task(title: str = "Fix the synthetic retry") -> str:
    return priorities.add(title, 3, 3, 1.0, [])["id"]


def test_expected_then_measured_impact_is_kept_on_the_task():
    tid = _task()
    priorities.record_impact(tid, expected="fewer failed exports", who="support", metric="failed exports per day")
    t = priorities.record_impact(tid, result="failed exports 40 to 3 per day", evidence_links=["https://example.com/pr/1"])
    assert t["expected_impact"] == {"hypothesis": "fewer failed exports", "who": "support", "metric": "failed exports per day"}
    assert t["measured_impact"]["result"] == "failed exports 40 to 3 per day"
    assert t["measured_impact"]["evidence"] == ["https://example.com/pr/1"]
    note = [e for e in evidence.load() if e.get("kind") == "note"][-1]
    assert note["impact"] == "failed exports 40 to 3 per day" and note["links"] == ["https://example.com/pr/1"]


def test_a_result_that_cannot_be_checked_is_marked_not_sure():
    t = priorities.record_impact(_task(), result="support seems happier", unsure=True)
    assert t["measured_impact"]["result"] == "Not sure: support seems happier"


def test_the_daily_update_names_todays_measured_impact():
    priorities.record_impact(_task(), result="failed exports 40 to 3 per day")
    lines = log.pm_lines(log.build(date(2026, 10, 21)), learn=False)
    assert any("impact: failed exports 40 to 3 per day" in ln for ln in lines), lines


def test_an_impact_question_is_answered_from_the_record():
    day = date(2026, 10, 21)
    results.append([{"id": "pr:1", "at": "2026-10-21T09:00:00", "kind": "pr", "title": "Retry failed synthetic exports", "url": "https://example.com/pr/1"}], day)
    asked = [i for i in inbox.candidates(day) if i.get("kind") == "impact"]
    assert asked, "without a record the question is asked"
    priorities.record_impact(_task(), result="failed exports 40 to 3 per day", evidence_links=["https://example.com/pr/1"])
    assert not [i for i in inbox.candidates(day) if i.get("kind") == "impact"]


def test_empty_or_unknown_impact_is_refused(capsys):
    tid = _task()
    assert main(["work", "impact", tid]) == 2
    assert main(["work", "impact", "t999", "--result", "x"]) == 2
    assert capsys.readouterr().out.count("breaks US-87") == 2
