"""Issue #216: grade a PM update against the next ladder level; tagged lines feed promo coverage."""

from __future__ import annotations

import json

import pytest

from careerbrew.cli import main
from careerbrew.work import ladder_grade, promo

LINES = ["Checkout: shipped the synthetic retry; fixed a flaky test.", "Search: wrote a design doc for the synthetic index.", "Billing: fixed the synthetic rounding."]


@pytest.fixture(autouse=True)
def now(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:00")
    monkeypatch.setenv("CAREERBREW_TEST_OWNER", "1")


def test_levels_lists_each_dimension_with_its_description_and_examples():
    dims = {d["id"]: d for d in ladder_grade.levels("generic-swe", "senior")}
    assert dims["delivery"]["description"].startswith("Ships multi-week")
    assert dims["delivery"]["looks_like"] and dims["delivery"]["doesnt_look_like"]
    assert set(dims) == {c["id"] for c in promo.ladders()["generic-swe"]["competencies"]}


def test_an_unknown_level_is_an_error():
    with pytest.raises(promo.LadderError):
        ladder_grade.levels("generic-swe", "wizard")


def test_each_line_is_tagged_and_coverage_is_strong_some_or_none():
    g = ladder_grade.grade(LINES, "generic-swe", "senior")
    cov = {r["id"]: r["coverage"] for r in g["coverage"]}
    assert cov["quality"] == "strong"  # two fix lines
    assert cov["design"] == "some"
    assert cov["ownership"] == "none"
    hints = {h["id"]: h for h in g["suggestions"]}
    assert hints["design"]["line"] == LINES[1]  # quotes the owner's own line, never a new fact
    assert "no true fact fits" in hints["ownership"]["hint"]
    assert "quality" not in hints


def test_recorded_lines_count_in_promo_once():
    before = {r["id"]: r["credit"] for r in promo.assess("generic-swe", "senior")["competencies"]}
    g = ladder_grade.grade(LINES, "generic-swe", "senior")
    assert ladder_grade.record(g, "2026-10-21") == 3
    assert ladder_grade.record(g, "2026-10-21") == 0  # a regrade adds nothing
    after = {r["id"]: r["credit"] for r in promo.assess("generic-swe", "senior")["competencies"]}
    assert after["design"] == before["design"] + 1


def test_cli_levels_json(capsys):
    assert main(["ladder", "levels", "--level", "staff", "--json"]) == 0
    out = json.loads(capsys.readouterr().out)
    assert out["level"] == "staff" and out["dimensions"]


def test_cli_levels_error_names_the_story(capsys):
    assert main(["ladder", "levels", "--ladder", "nope"]) == 2
    assert "breaks US-88" in capsys.readouterr().out


def test_agents_see_coverage_but_no_line_text(capsys, monkeypatch):
    monkeypatch.delenv("CAREERBREW_TEST_OWNER")
    monkeypatch.setattr("careerbrew.day.log.pm_report", lambda d: {"body": "\n".join(LINES)})
    assert main(["day", "pm-grade", "--json"]) == 0
    text = capsys.readouterr().out
    assert "synthetic" not in text and "coverage" in text


def test_an_untagged_task_gets_the_ladder_bonus_from_its_title():
    from careerbrew.work import priorities

    priorities.add("Write a design doc for the synthetic cache", 3, 3, 1.0, [])
    priorities.add("Rename a synthetic variable", 3, 3, 1.0, [])
    ranked = priorities.rank({"design": 1.0})
    assert ranked[0]["title"].startswith("Write a design doc")
    assert ranked[0]["parts"]["ladder_dims"] == ["design"]


def test_the_pm_report_carries_the_owner_only_grading_but_not_in_its_body():
    from careerbrew.day import log

    pm = log.pm_report(log.save())
    assert pm["ladder"]["coverage"] and "coverage" not in pm["body"]
