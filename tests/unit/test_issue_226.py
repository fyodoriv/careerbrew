"""Issue #226: content rules for PM update drafts. The rules a machine can check run on every draft and edit
(`content_checks`); every line gets an owner-only "why it matters" note, with "Not sure:" when nothing checked it."""

from __future__ import annotations

from datetime import date

import pytest

from careerbrew.cli import main
from careerbrew.day import log as daylog
from careerbrew.day import results
from careerbrew.work import priorities

DAY = date(2026, 10, 21)


@pytest.fixture(autouse=True)
def now(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:00")


def _row(rid: str, kind: str, title: str) -> dict:
    return {"id": rid, "at": "2026-10-21T09:00", "kind": kind, "title": title, "source": "evidence", "origin": "user"}


def _rules(text: str, d: dict | None = None) -> list[str]:
    return [c["rule"] for c in daylog.content_checks(text, d or {"done": {}})]


@pytest.mark.parametrize(
    "line",
    [
        "billing: export done; next: import, ETA Friday.",
        "billing: export done; next: import, about 2 days left.",
        "billing: next: import, should take 3 hours.",
        "billing: next: import (~4h).",
        "billing: next: import, my estimate is one sprint.",
    ],
)
def test_a_time_estimate_is_flagged(line):
    assert _rules(line) == ["time-estimate"]


@pytest.mark.parametrize(
    "line",
    [
        "billing: export and import (2 PRs in 3 repos); overdue: close the books (due 2026-10-20).",
        "billing: cut the export from 40 to 3 failures a day ([dashboard](https://example.com/d/1)).",
        "billing: led the planning meeting; aligned on the 3 next steps.",
    ],
)
def test_facts_counts_and_linked_numbers_pass(line):
    assert _rules(line) == []


def test_a_big_claim_with_no_evidence_link_is_flagged():
    assert _rules("billing: made the export 3x faster.") == ["number-without-evidence"]
    assert _rules("billing: cut failures by 90%.") == ["number-without-evidence"]
    assert _rules("billing: cut failures by 90% (https://example.com/d/1).") == []


def test_next_that_names_done_work_is_flagged():
    d = {"done": {"Shipped": ["feat: export the invoices (#4)"]}}
    (check,) = daylog.content_checks("billing: export the invoices (1 PR); next: export the invoices.", d)
    assert check == {"rule": "next-done", "line": "billing: export the invoices (1 PR); next: export the invoices.", "fix": check["fix"]}
    assert "not done" in check["fix"]
    assert _rules("billing: export the invoices (1 PR); next: import the invoices.", d) == []


def test_the_generated_draft_passes_every_rule():
    results.append([_row("pr:acme/shop#1", "pr", "Build the error card (#1)"), _row("pr:acme/shop#2", "pr", "Speed up search 3x (#2)")], DAY)
    priorities.add("Plan the next sprint (about 2 days)", 3, 3, 16.0, [])
    pm = daylog.pm_report(daylog.build(DAY))
    assert pm["checks"] == [], pm["draft"]


def test_the_report_checks_the_owners_saved_edit():
    d = daylog.build(DAY)
    daylog.save_pm_draft(DAY.isoformat(), "shop: next: the error card, ETA Friday.")
    assert [c["rule"] for c in daylog.pm_report(d)["checks"]] == ["time-estimate"]


def test_every_line_gets_a_note_and_an_unchecked_one_says_not_sure():
    results.append([_row("pr:acme/shop#1", "pr", "Build the error card (#1)")], DAY)
    tid = priorities.add("Fix the failed exports", 4, 4, 1.0, [])["id"]
    priorities.record_impact(tid, expected="fewer failed exports", who="support")
    pm = daylog.pm_report(daylog.build(DAY))
    notes = {w["label"]: w["note"] for w in pm["why"]}
    assert len(notes) == len(pm["draft"].splitlines()), "one note per line"
    assert notes["Other"].startswith("Not sure: helps support; expected: fewer failed exports"), notes
    assert notes["shop"].startswith("Not sure: no impact recorded yet"), notes
    priorities.record_impact(tid, result="failed exports 40 to 3 per day", evidence_links=["https://example.com/pr/1"])
    notes = {w["label"]: w["note"] for w in daylog.pm_report(daylog.build(DAY))["why"]}
    assert not notes["Other"].startswith("Not sure:") and "checked: failed exports 40 to 3 per day" in notes["Other"]


def test_day_send_names_the_checks_and_an_agent_sees_only_the_count(capsys):
    daylog.save_pm_draft(DAY.isoformat(), "shop: next: the secret-canary card, ETA Friday.")
    assert main(["day", "send"]) == 2
    out = capsys.readouterr().out
    assert "content checks: 1 (time-estimate)" in out and "secret-canary" not in out.split("content checks")[1]
