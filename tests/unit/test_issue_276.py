"""A prep project is done only after its readiness gate: tool, links, questions, quiz (issue #276, US-93)."""

import json
from datetime import date

import pytest

from careerbrew import cli, goals
from careerbrew.day import log, planner
from careerbrew.work import projects


@pytest.fixture
def clock(home, monkeypatch):
    def at(iso):
        monkeypatch.setenv("CAREERBREW_NOW", iso)

    at("2026-11-02T09:00+00:00")
    return at


def _gates(name):
    return {g["gate"]: g["done"] for g in projects.show(name)["prep"]["gates"]}


def test_a_prep_project_starts_with_four_open_gates_and_a_plain_project_has_none(clock):
    projects.add("demo", kind="prep", event_on="2026-11-03")
    projects.add("plain")
    r = projects.show("demo")["prep"]
    assert [g["gate"] for g in r["gates"]] == ["tool", "links", "questions", "quiz"]
    assert (r["done"], r["total"], r["ready"], r["event_on"]) == (0, 4, False, "2026-11-03")
    assert all(g["fix"].startswith("careerbrew work project ") for g in r["gates"])
    assert projects.show("plain")["prep"] is None
    assert "prep" not in next(p for p in projects.watch_list() if p["name"] == "plain")


def test_the_project_is_ready_only_when_all_four_gates_pass(clock):
    projects.add("demo", kind="prep")
    projects.add_source("demo", "url", "https://example.com/rubric")
    projects.add_source("demo", "doc", "pack.md")
    projects.gate("demo", "tool")
    projects.gate("demo", "questions")
    projects.quiz("demo", 8, 0)
    projects.gate("demo", "links", refs=["https://example.com/rubric"])
    assert _gates("demo") == {"tool": True, "links": False, "questions": True, "quiz": True}, "one unread link keeps the links gate open"
    with pytest.raises(projects.ProjectError, match=r"not ready: 1 of 4 gate\(s\) open \(links\); fix: careerbrew work project gate demo links pack.md"):
        projects.archive("demo")
    projects.gate("demo", "links", refs=["pack.md"])
    assert projects.show("demo")["prep"]["ready"] is True
    projects.archive("demo")
    assert projects.list_rows()[0]["archived"] is True


def test_a_check_stamp_is_not_the_owner_reading_a_link(clock):
    """The watcher's research refresh and `absorb` stamp `checked_at` on what careerbrew read; only the owner's mark counts."""
    projects.add("demo", kind="prep")
    projects.add_source("demo", "url", "https://example.com/rubric")
    projects.check("demo")
    g = next(g for g in projects.show("demo")["prep"]["gates"] if g["gate"] == "links")
    assert (g["done"], g["note"], g["fix"]) == (False, "0 of 1 read by you", "careerbrew work project gate demo links https://example.com/rubric")


def test_links_name_each_ref_and_an_unknown_ref_is_refused(clock):
    projects.add("demo", kind="prep")
    projects.add_source("demo", "url", "https://example.com/rubric")
    with pytest.raises(projects.ProjectError, match="name each link you read: careerbrew work project gate demo links <ref>"):
        projects.gate("demo", "links")
    with pytest.raises(projects.ProjectError, match="has no link https://example.com/other"):
        projects.gate("demo", "links", refs=["https://example.com/other"])
    with pytest.raises(projects.ProjectError, match="only the links gate takes refs"):
        projects.gate("demo", "tool", refs=["https://example.com/rubric"])
    projects.gate("demo", "links", refs=["https://example.com/rubric"])
    assert _gates("demo")["links"] is True
    projects.gate("demo", "links", undo=True, refs=["https://example.com/rubric"])
    assert _gates("demo")["links"] is False


def test_a_pack_with_no_links_has_its_links_gate_open_and_the_memory_file_is_not_a_link(clock, tmp_path):
    projects.add("demo", kind="prep", memory=str(tmp_path / "notes.md"))
    projects.check("demo")
    g = next(g for g in projects.show("demo")["prep"]["gates"] if g["gate"] == "links")
    assert (g["done"], g["note"]) == (False, "no links in the pack yet")


def test_misses_keep_the_quiz_gate_open_until_a_re_quiz_of_every_miss_is_clean(clock):
    projects.add("demo", kind="prep")
    r = projects.quiz("demo", 10, 3)
    assert r["quiz"] == {"at": "2026-11-02T09:00:00+00:00", "asked": 10, "missed": 3, "score": 0.7}
    assert _gates("demo")["quiz"] is False
    with pytest.raises(projects.ProjectError, match="asks each of them again, so --asked is at least 3"):
        projects.quiz("demo", 2, 0)
    projects.quiz("demo", 3, 1)
    assert _gates("demo")["quiz"] is False, "a re-quiz with a miss keeps the gate open"
    projects.quiz("demo", 1, 0)
    assert _gates("demo")["quiz"] is True
    assert projects.show("demo")["prep"]["quizzes"] == 3


def test_the_quiz_result_is_content_free(clock):
    projects.add("demo", kind="prep")
    projects.quiz("demo", 5, 1)
    stored = projects._load()["demo"]["quizzes"]
    assert stored == [{"at": "2026-11-02T09:00:00+00:00", "asked": 5, "missed": 1}]


@pytest.mark.parametrize(("asked", "missed"), [(0, 0), (3, 4), (5, -1), (projects.QUIZ_MAX + 1, 0)])
def test_a_bad_quiz_count_is_refused(clock, asked, missed):
    projects.add("demo", kind="prep")
    with pytest.raises(projects.ProjectError, match="a quiz asks 1 to"):
        projects.quiz("demo", asked, missed)


def test_gates_are_refused_on_a_plain_project_and_the_quiz_is_not_marked_by_hand(clock):
    projects.add("plain")
    with pytest.raises(projects.ProjectError, match="not a prep project"):
        projects.gate("plain", "tool")
    with pytest.raises(projects.ProjectError, match="not a prep project"):
        projects.quiz("plain", 3, 0)
    projects.add("demo", kind="prep")
    with pytest.raises(projects.ProjectError, match="quiz gate is not marked by hand: careerbrew work project quiz demo"):
        projects.gate("demo", "quiz")
    with pytest.raises(projects.ProjectError, match="unknown gate deck"):
        projects.gate("demo", "deck")
    projects.gate("demo", "tool")
    projects.gate("demo", "tool", undo=True)
    assert _gates("demo")["tool"] is False


def test_kind_and_event_day_are_checked_and_set_can_change_them(clock):
    with pytest.raises(projects.ProjectError, match="unknown project kind talk"):
        projects.add("x", kind="talk")
    with pytest.raises(projects.ProjectError, match="looks like 2026-11-03"):
        projects.add("x", kind="prep", event_on="next tuesday")
    with pytest.raises(projects.ProjectError, match="only a prep project has an event day"):
        projects.add("x", event_on="2026-11-03")
    projects.add("x")
    projects.edit("x", kind="prep", event_on="2026-11-05")
    assert projects.show("x")["prep"]["event_on"] == "2026-11-05"
    projects.edit("x", kind="none", event_on="none")
    assert projects.show("x")["prep"] is None


def test_a_cancelled_event_can_still_be_archived_with_force(clock):
    projects.add("demo", kind="prep")
    projects.archive("demo", force=True)
    assert projects.list_rows()[0]["archived"] is True


def test_the_event_day_plan_starts_with_the_open_gates_and_other_days_do_not(clock):
    goals.set_upcoming({"kind": "learn", "title": "Learn hashing", "topics": ["arrays and hashing"], "date": "2026-11-20", "minutes_per_day": 60})
    projects.add("demo", kind="prep", event_on="2026-11-03")
    projects.gate("demo", "tool")
    plan = planner.build(date(2026, 11, 3), persist=False)
    tasks = [s["task"] for s in plan["schedule"]]
    assert tasks[:3] == ["gate:demo:links", "gate:demo:questions", "gate:demo:quiz"], tasks
    assert tasks[3].startswith("goal:"), "the gates come before the goal's study blocks"
    first = plan["schedule"][0]
    assert (first["project"], first["gate"]) == ("demo", True)
    assert first["reason"] == "the event is today and this gate is open (see: careerbrew work project show demo)", "the reason names no link"
    before = planner.build(date(2026, 11, 2), persist=False)["schedule"]
    assert [s["task"] for s in before][:3] == tasks[:3] and "the event is tomorrow" in before[0]["reason"], "an early event gets its gates the day before"
    assert not any(t.startswith("gate:") for t in (s["task"] for s in planner.build(date(2026, 11, 1), persist=False)["schedule"]))
    week = planner.plan_week(date(2026, 11, 2), 3)
    assert [s["task"] for s in week[1]["schedule"]][:3] == tasks[:3], "the Week view shows them on the event's day too"
    assert not any(s["task"].startswith("gate:") for s in week[2]["schedule"]), "and never after the event"


def test_a_monday_event_gets_its_gates_on_the_workday_before(clock):
    projects.add("demo", kind="prep", event_on="2026-11-09")  # a Monday
    friday = planner.build(date(2026, 11, 6), persist=False)["schedule"]
    assert friday[0]["task"] == "gate:demo:tool" and "the event is on Mon Nov 9" in friday[0]["reason"], friday[:1]
    assert not any(s["task"].startswith("gate:") for s in planner.build(date(2026, 11, 5), persist=False)["schedule"]), "two workdays before: none"


def test_the_next_step_skips_a_gate_that_does_not_fit_before_the_next_meeting(clock, monkeypatch):
    clock("2026-11-03T09:00+00:00")
    projects.add("demo", kind="prep", event_on="2026-11-03")
    monkeypatch.setattr(projects, "GATE_MINUTES", 10_000)
    assert planner.next_step()["kind"] != "gate", "a gate longer than the free time is not the next step"


def test_the_next_step_is_an_open_gate_before_any_task(clock):
    clock("2026-11-03T09:00+00:00")
    projects.add("demo", kind="prep", event_on="2026-11-03")
    nxt = planner.next_step()
    assert (nxt["kind"], nxt["task"]) == ("gate", "gate:demo:tool"), nxt
    projects.gate("demo", "tool")
    assert planner.next_step()["task"] == "gate:demo:links", "a gate closed since the plan was written is skipped"


def test_an_archived_or_ready_prep_project_adds_no_gate_blocks(clock):
    projects.add("done", kind="prep", event_on="2026-11-03")
    projects.archive("done", force=True)
    assert projects.gate_blocks(date(2026, 11, 3)) == []


def test_gate_blocks_stay_out_of_the_pm_update(clock):
    projects.add("demo", kind="prep", event_on="2026-11-03")
    assert any(s["task"].startswith("gate:") for s in planner.plan_week(date(2026, 11, 2), 2)[1]["schedule"]), "tomorrow's plan holds the gates"
    d = log.build(date(2026, 11, 2))
    assert "Prep gate" not in json.dumps([d["planned"], d["details"]["planned"]]), "the day log's plan for tomorrow leaves them out"
    assert "Prep gate" not in json.dumps(log.pm_report(d)), "so does the PM update"


def test_cli_prints_the_gates_their_fixes_and_the_list_counts(clock, capsys):
    assert cli.main(["work", "project", "add", "demo", "--kind", "prep", "--event-on", "2026-11-03"]) == 0
    out = capsys.readouterr().out
    assert "it is done only when these 4 gates pass" in out
    assert "fix: careerbrew work project gate demo tool" in out
    assert cli.main(["work", "project", "quiz", "demo", "--asked", "4", "--missed", "1"]) == 0
    out = capsys.readouterr().out
    assert "score 75% (3 of 4), 1 miss(es)" in out
    assert "careerbrew work project quiz demo --asked 1 --missed <n>" in out
    assert cli.main(["work", "project", "list"]) == 0
    assert "prep ready 0/4" in capsys.readouterr().out
    assert cli.main(["work", "project", "list", "--json"]) == 0
    row = json.loads(capsys.readouterr().out)[0]
    assert row["prep"]["quiz"] == {"at": "2026-11-02T09:00:00+00:00", "asked": 4, "missed": 1, "score": 0.75}
