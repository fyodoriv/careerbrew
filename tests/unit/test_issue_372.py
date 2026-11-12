"""The PM impact notes track the shown body and its evidence."""

from datetime import date

import pytest

from careerbrew.day import log as daylog

DAY = date(2026, 10, 16)
BILLING = "Billing: verified invoice export."
SEARCH = "Search: checked the indexing delay."
BILLING_NOTE = "Checked: invoices export again (1 evidence link)."
SEARCH_NOTE = "Checked: indexing delay fell (1 evidence link)."


@pytest.fixture
def report(monkeypatch):
    # Freeze the generated text and evidence at the projection boundary.
    monkeypatch.setattr(daylog, "_pm_build", lambda _: ([BILLING, SEARCH], {"Billing": BILLING_NOTE, "Search": SEARCH_NOTE}))
    return daylog.build(DAY)


def test_saved_today_and_tomorrow_lines_get_their_own_notes(report):
    lines = ["• Verify the invoice guide", "• Check the fallback path", "• Review the release checklist"]
    saved = f"Today:\n{lines[0]}\n{lines[1]}\n\nTomorrow:\n{lines[2]}"
    daylog.save_pm_draft(DAY.isoformat(), saved)
    pm = daylog.pm_report(report)
    assert pm["body"] == saved
    notes = [(row["label"], row["note"]) for row in pm["why"]]
    print(f"Displayed body: {pm['body']!r}, ordered notes: {notes!r}")
    assert len(notes) == len(lines)
    for (label, note), line in zip(notes, lines, strict=True):
        assert label == line.lstrip("• ")
        assert note.startswith("Not sure:")


def test_duplicate_project_label_does_not_transfer_evidence(report):
    saved = f"{BILLING}\nBilling: plan a new reconciliation check."
    daylog.save_pm_draft(DAY.isoformat(), saved)
    pm = daylog.pm_report(report)
    assert pm["body"] == saved
    notes = [(row["label"], row["note"]) for row in pm["why"]]
    print(f"Displayed body: {pm['body']!r}, ordered notes: {notes!r}")
    assert len(notes) == 2
    assert notes[0][1] == BILLING_NOTE
    assert notes[1][1].startswith("Not sure:")


def test_reset_restores_generated_notes_without_adding_them_to_body(report):
    daylog.save_pm_draft(DAY.isoformat(), "Today:\n• Review the fallback path")
    daylog.save_pm_draft(DAY.isoformat(), "")
    pm = daylog.pm_report(report)
    assert pm["body"] == f"{BILLING}\n{SEARCH}"
    assert [row["note"] for row in pm["why"]] == [BILLING_NOTE, SEARCH_NOTE]
