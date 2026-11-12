"""Issue #60: the daily log splits Blockers (stuck now) from Risks (may go wrong later); stale sources are housekeeping."""

from datetime import date

import pytest

from careerbrew.day import log as daylog
from careerbrew.work import priorities, projects

FRIDAY = date(2026, 10, 16)


@pytest.fixture
def day(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    priorities.add("Ship the export", 4, 4, 1, ["impact"], due="2026-10-14")
    priorities.add("Write the queue doc", 3, 3, 1, ["design"], due="2026-10-19")
    projects.add("billing", memory="notes/billing.md")
    return FRIDAY


def test_overdue_is_a_blocker_due_soon_is_a_risk_stale_source_is_housekeeping(day):
    d = daylog.build(day)
    assert d["blockers"] == ["t001 Ship the export is overdue (due 2026-10-14)"]
    assert d["risks"] == ["t002 Write the queue doc is due 2026-10-19"]
    assert not any("not read" in x for x in d["blockers"] + d["risks"]), "stale sources are not blockers"
    assert any("billing: memory" in x for x in d["housekeeping"])
    md = daylog.render(d)
    assert "## Blockers\n" in md and "## Risks\n" in md and "## Housekeeping\n" in md
    assert "Blockers and risks" not in md
    assert md.index("billing: memory") > md.index("## Housekeeping")


def test_pm_update_lists_blockers_but_not_housekeeping(day):
    body = daylog.pm_report(daylog.build(day))["body"]
    assert "overdue: ship the export (due 2026-10-14)" in body and "not read" not in body
