import pytest

from careerbrew import goals, machine


def _today_tasks(plan):
    return [s["task"] for s in plan["schedule"]]


def test_setting_the_upcoming_goal_replans_today_with_study_first(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    r = goals.set_upcoming({"kind": "learn", "title": "Learn hashing", "topics": ["arrays and hashing"], "date": "2026-11-01", "minutes_per_day": 60})
    tasks = _today_tasks(r["plan"])
    assert tasks and all(t.startswith("goal:") for t in tasks[:2]), "study blocks come first"
    assert "goal:exercise" in tasks
    assert r["upcoming"]["minutes_per_day"] == 60


def test_changing_the_goal_changes_the_schedule_at_once(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    goals.set_upcoming({"kind": "learn", "title": "Learn hashing", "topics": ["arrays and hashing"]})
    r = goals.set_upcoming({"kind": "promotion", "title": "Promotion case", "topics": []})
    tasks = _today_tasks(r["plan"])
    assert "goal:promotion" in tasks and "goal:exercise" not in tasks
    assert [h["title"] for h in r["history"]] == ["Learn hashing"], "the old goal is kept in history"


def test_bad_goals_are_refused_with_a_clear_reason(home):
    with pytest.raises(goals.GoalError, match="kind"):
        goals.set_upcoming({"kind": "vacation", "title": "x"})
    with pytest.raises(goals.GoalError, match="title"):
        goals.set_upcoming({"kind": "learn", "title": " "})
    with pytest.raises(goals.GoalError, match="date"):
        goals.set_upcoming({"kind": "learn", "title": "x", "date": "next week"})
    with pytest.raises(goals.GoalError, match="minutes_per_day"):
        goals.set_upcoming({"kind": "learn", "title": "x", "minutes_per_day": 2000})


def test_long_term_goals_are_the_owners_list(home):
    d = goals.set_long_term([{"text": "Staff engineer by 2027", "by": "2027-06-30"}, {"text": "  "}])
    assert [g["text"] for g in d["long_term"]] == ["Staff engineer by 2027"]


def test_goal_day_is_light_review_and_corporate_machines_get_no_job_hunt_study(home, monkeypatch):
    from datetime import date

    goals.set_upcoming({"kind": "interview", "title": "Onsite", "company": "Datadog", "date": "2026-10-19"})
    [b] = goals.study_blocks(date(2026, 10, 19))
    assert b["id"] == "goal:review"
    monkeypatch.setattr(machine, "role", lambda: "corporate")
    assert goals.study_blocks(date(2026, 10, 10)) == []


def test_interview_goal_focus_follows_the_company_loop(home):
    goals.set_upcoming({"kind": "interview", "title": "Onsite", "company": "Datadog"})
    f = goals.focus()
    assert "exercise" in f["activities"] and "system-design" in f["activities"]
    assert "system design" in f["card_topics"]


def test_any_upcoming_goal_can_change_and_the_nearest_drives_the_plan(home, monkeypatch):
    from datetime import date

    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    queue = [
        {"kind": "promotion", "title": "Promotion packet", "date": "2026-12-01"},
        {"kind": "learn", "title": "Learn hashing", "topics": ["arrays and hashing"], "date": "2026-10-25"},
        {"kind": "custom", "title": "Someday"},
    ]
    r = goals.set_queue(queue)
    assert r["upcoming"]["title"] == "Learn hashing", "the nearest dated goal drives the schedule"
    assert "goal:exercise" in [s["task"] for s in r["plan"]["schedule"]]
    edited = [{**g, "date": "2026-10-20"} if g["title"] == "Promotion packet" else g for g in r["queue"]]
    r = goals.set_queue(edited)
    assert r["upcoming"]["title"] == "Promotion packet", "moving a goal earlier makes it current"
    assert "goal:promotion" in [s["task"] for s in r["plan"]["schedule"]]
    assert [h["title"] for h in r["history"]] == ["Learn hashing"]
    assert goals._current(r["queue"], date(2026, 10, 22))["title"] == "Learn hashing", "when a goal's date passes, the next one takes over"
    assert goals._current(r["queue"], date(2026, 12, 2))["title"] == "Someday", "after every date passes, the undated goal is next"
    r = goals.remove(r["upcoming"]["id"])
    assert r["upcoming"]["title"] == "Learn hashing"
    with pytest.raises(goals.GoalError, match="same title"):
        goals.set_queue([{"kind": "learn", "title": "A"}, {"kind": "learn", "title": "A"}])
