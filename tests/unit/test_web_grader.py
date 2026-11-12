"""Regression tests for the web-goal grader findings (2026-10-18)."""

import json

import pytest

from careerbrew import changes, prep, store
from careerbrew.day import planner
from careerbrew.work import priorities

from .test_server import api  # noqa: F401  (fixture)


def test_bad_due_date_is_refused_and_old_bad_data_cannot_break_planning(home):
    with pytest.raises(ValueError, match="due"):
        priorities.add("x", 3, 3, 1, [], due="notadate")
    priorities.add("ok", 3, 3, 1, [])
    tasks = priorities.load()
    tasks.append({"id": "t099", "title": "legacy bad", "impact": 3, "urgency": 3, "estimate_h": 1, "competencies": [], "due": "notadate", "status": "open"})
    priorities.save(tasks)
    assert [t["id"] for t in priorities.rank({})]  # no crash
    planner.build()


def test_done_hours_must_be_positive(home):
    priorities.add("x", 3, 3, 1, [])
    for bad in (0, -3):
        with pytest.raises(ValueError, match="hours"):
            priorities.done("t001", bad)


def test_api_returns_400_not_a_dropped_connection(api):  # noqa: F811
    assert api("/api/tasks", body={"title": "x", "due": "notadate"})[0] == 400
    assert api("/api/week?days=abc")[0] == 400
    assert api("/api/tasks", body=[])[0] == 400
    assert api("/api/tasks/t001/done", body={"hours": -3})[0] in (400, 404)
    assert api("/api/today")[0] == 200


def test_pivot_marks_company_prep_stale_and_prep_shows_it(home):
    store.write_json(home / "changes" / "snapshot.json", changes.current())
    prep.mark_stale("Stripe", "company process changed")
    r = prep.run("Stripe", answers={})
    top = r["gaps"][0]
    assert top["id"] == "company:changed" and "Stripe" in top["text"] or "changed" in top["text"]
    asks = {i["id"]: True for i in prep.checklist("Stripe")["items"] if not i["auto"]}
    prep.run("Stripe", answers=asks)
    assert "company:changed" not in {g["id"] for g in prep.run("Stripe", answers=asks)["gaps"]}, "confirming the re-read clears the mark"


def test_page_change_maps_source_to_companies(home):
    from careerbrew import interview

    stripe = interview.find_company("Stripe")
    sid = stripe["sources"][0]
    assert "Stripe" in changes.companies_for_source(sid)
    json.dumps(changes.companies_for_source(sid))
