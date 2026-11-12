"""Daily log, inbox (proposals and questions), watcher rebuild, config, and the one-command start (US-52)."""

from datetime import date
from pathlib import Path

import pytest

from careerbrew import config, goals, vault
from careerbrew.cli import main
from careerbrew.day import inbox, results, watch
from careerbrew.day import log as daylog
from careerbrew.work import evidence, priorities, projects

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "http"
FRIDAY = date(2026, 10, 16)


@pytest.fixture
def day(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    results.append(
        [
            {"id": "pr:1", "at": "2026-10-16T11:00", "kind": "pr", "title": "Backfill old invoices for PROJ-55", "source": "evidence", "origin": "user"},
            {"id": "meeting:1", "at": "2026-10-16T10:00", "kind": "meeting", "title": "Billing sync", "source": "calendar", "origin": "user"},
        ],
        FRIDAY,
    )
    return FRIDAY


def test_log_groups_today_plans_the_next_workday_and_is_kept_encrypted(day, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T17:45")  # the log at the end of the day: the task did not get done
    priorities.add("Write the queue design doc", 3, 3, 1, ["design"])
    d = daylog.save(day)
    assert d["done"]["Shipped"] == ["Backfill old invoices for PROJ-55"] and d["done"]["Meetings"] == ["Billing sync"]
    assert d["tomorrow"] == "2026-10-19", "Friday's plan is for Monday"
    assert any("Write the queue design doc" in x for x in d["planned"])
    raw = daylog.path_for(day).read_bytes()
    assert raw.startswith(vault.MAGIC) and b"Backfill" not in raw
    assert daylog.history() == ["2026-10-16"] and "## Done today" in daylog.read(day)
    pm = daylog.pm_report(d)
    assert "backfill old invoices" in pm["body"] and "next: write the queue design doc" in pm["body"] and "impact" not in pm["body"].lower()


def test_proposals_task_done_and_new_ticket_task(day, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIX))
    monkeypatch.setenv("CB_T", "synthetic")
    assert main(["connect", "jira", "--ref", "env:CB_T", "--base-url", "https://jira.example.com", "--email", "me@example.com", "--no-verify"]) == 0
    t = priorities.add("Backfill old invoices", 3, 3, 2, [])
    inbox.regenerate(day)
    kinds = {i["kind"] for i in inbox.open_items()}
    assert {"task-done", "new-task"} <= kinds
    done = next(i for i in inbox.open_items() if i["kind"] == "task-done")
    inbox.accept(done["id"])
    assert next(x for x in priorities.load() if x["id"] == t["id"])["status"] == "done"
    new = next(i for i in inbox.open_items() if i["kind"] == "new-task")
    inbox.accept(new["id"])
    assert any(x["title"].startswith("PROJ-") for x in priorities.load())
    inbox.regenerate(day)
    assert done["id"] not in {i["id"] for i in inbox.open_items()}, "a decided item never comes back"


def test_goal_suggestion_needs_approval_in_the_web_app(day):
    inbox.regenerate(day)
    g = next(i for i in inbox.open_items() if i["kind"] == "goal")
    with pytest.raises(inbox.InboxError, match="web app only"):
        inbox.accept(g["id"])
    assert goals.load()["queue"] == []
    inbox.accept(g["id"], from_ui=True)
    assert goals.load()["queue"][0]["kind"] == "promotion"


def test_questions_are_answered_and_applied(day):
    t = priorities.add("Plan the migration", 3, 3, 1, [])
    projects.add("billing")
    projects.add_source("billing", "repo", "acme/billing")
    inbox.regenerate(day)
    q = {i["kind"]: i for i in inbox.open_items() if i["type"] == "question"}
    assert {"impact", "project", "competency", "memory"} <= set(q)
    inbox.answer(q["impact"]["id"], "cut invoice time from 4 h to 10 min")
    assert any("4 h to 10 min" in str(e.get("impact")) for e in evidence.load())
    inbox.answer(q["project"]["id"], "billing")
    assert "PROJ-55" in {s["ref"] for s in projects.show("billing")["sources"]}
    with pytest.raises(inbox.InboxError, match="unknown competencies"):
        inbox.answer(q["competency"]["id"], "telepathy")
    inbox.answer(q["competency"]["id"], "delivery")
    assert next(x for x in priorities.load() if x["id"] == t["id"])["competencies"] == ["delivery"]
    inbox.answer(q["memory"]["id"], "work/billing.md")
    assert projects.show("billing")["memory"] == "work/billing.md"


def test_question_cap(day):
    for i in range(10):
        priorities.add(f"task number {i}", 3, 3, 1, [])
    config.set_value("questions_per_day", "3")
    inbox.regenerate(day)
    assert sum(1 for i in inbox.open_items() if i["type"] == "question") == 3


def test_watcher_cycle_with_new_rows_rebuilds_log_and_inbox(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    monkeypatch.setattr(watch, "notify", lambda text: None)
    row = {"id": "pr:9", "at": "2026-10-16T15:00", "kind": "pr", "title": "Ship the export", "source": "evidence", "origin": "user"}
    monkeypatch.setattr(watch, "SOURCES", {"evidence": lambda d: [row]})
    stats = watch.run_cycle()
    assert stats["added"] == 1
    assert "Ship the export" in daylog.read(FRIDAY)
    assert any(i["kind"] == "impact" for i in inbox.open_items())


def test_config_defaults_set_validate_and_unset(home, capsys):
    assert config.get("work_end") == "17:30"
    config.set_value("work_end", "18:00")
    assert {r["key"]: r for r in config.effective()}["work_end"]["source"] == "settings.yaml"
    with pytest.raises(ValueError, match="HH:MM"):
        config.set_value("work_end", "25:00")
    with pytest.raises(KeyError):
        config.set_value("nope", "1")
    config.unset("work_end")
    assert config.get("work_end") == "17:30"
    assert main(["config", "set", "pm_email", "pm@example.com"]) == 0
    assert main(["config", "set", "pm_email", "not-an-email"]) != 0


def test_no_command_starts_everything_and_shows_what_matters(home, monkeypatch, capsys):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    assert main(["--no-watch", "--no-web", "--no-refresh"]) == 0
    out = capsys.readouterr().out
    assert "for you: " in out and "task(s) done today" in out, "the run page's summary line answers first"
    assert daylog.read(FRIDAY) is not None
    main(["config", "set", "run_pages", "false"])
    capsys.readouterr()
    assert main(["--no-watch", "--no-web", "--no-refresh"]) == 0
    out = capsys.readouterr().out
    assert "today:" in out and "for you:" in out and "careerbrew config set" in out, "with pages off, the text prints as before"


def test_study_blocks_never_reach_the_log_or_pm_update(day, monkeypatch):
    from careerbrew.day import planner

    real = planner.plan_week
    monkeypatch.setattr(planner, "plan_week", lambda start, days, now=None: [{**d, "schedule": [{"task": "goal:cards", "title": "Flashcards"}, *d["schedule"]]} for d in real(start, days, now)])
    d = daylog.build(day)
    assert not any("goal:" in x or "Flashcards" in x for x in d["planned"])
    assert "Flashcards" not in daylog.pm_report(d)["body"]


def test_job_hunt_and_unknown_rows_never_reach_the_log_or_pm(day):
    results.append(
        [
            {"id": "mail:1", "at": "2026-10-16T12:00", "kind": "mail-interview", "title": "Onsite with OtherCo", "source": "mail", "origin": "user"},
            {"id": "x:1", "at": "2026-10-16T12:30", "kind": "mystery", "title": "Something odd", "source": "x", "origin": "user"},
            {"id": "bad", "kind": "pr"},
        ],
        day,
    )
    d = daylog.build(day)
    pm = daylog.pm_report(d)["body"]
    assert "OtherCo" not in pm and "Something odd" not in pm and "\n- \n" not in pm + "\n"
    assert "OtherCo" not in daylog.render(d)
    inbox.regenerate(day)  # a row with no title must not crash the inbox


def test_dismissed_item_survives_a_concurrent_regenerate(day, monkeypatch):
    priorities.add("Backfill old invoices", 3, 3, 2, [])
    inbox.regenerate(day)
    item = next(i for i in inbox.open_items() if i["kind"] == "task-done")
    real = inbox.candidates

    def slow_candidates(d=None):
        out = real(d)
        inbox.dismiss(item["id"])  # the user dismisses while the watcher is still computing
        return out

    monkeypatch.setattr(inbox, "candidates", slow_candidates)
    inbox.regenerate(day)
    assert item["id"] not in {i["id"] for i in inbox.open_items()}


def test_web_start_never_writes_the_token_to_a_log(home, monkeypatch):
    import subprocess

    from careerbrew import paths, server

    seen = {}

    class Proc:
        pid = 1

        def poll(self):
            return 1  # the child died at once (port in use)

        def kill(self):
            pass

    def fake_popen(cmd, **kw):
        seen["stdout"] = kw["stdout"]
        return Proc()

    monkeypatch.setattr(subprocess, "Popen", fake_popen)
    monkeypatch.setattr(server, "listening_pids", lambda port: [])  # never look at, or stop, a real server on 8799 (issue #118)
    with pytest.raises(RuntimeError, match="serve_port"):
        server.start_background(8799)
    assert seen["stdout"] is subprocess.DEVNULL
    assert "token=" not in (paths.data_root() / "logs" / "web.log").read_text() if (paths.data_root() / "logs" / "web.log").exists() else True


def test_plan_and_done_rows_carry_details(day, monkeypatch):
    """Issue #61: each Plan and Done row has a source link, project, why, related refs, and history."""
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T17:45")  # after work hours: the task is Monday's
    monkeypatch.setattr(evidence, "load", lambda: [{"id": "pr:1", "url": "https://github.example.com/o/r/pull/7"}])
    t = priorities.add("Ship PROJ-9 retries", 3, 3, 1, [], due="2026-10-30", source="agent", why="from ticket PROJ-9", links=["https://jira.example.com/browse/PROJ-9"])
    priorities.save([{**x, "status": "open"} for x in priorities.load()])
    projects.add("billing")
    projects.link_task("billing", t["id"])
    d = daylog.build(day)
    shipped = d["details"]["done"]["Shipped"][0]
    assert shipped["url"] == "https://github.example.com/o/r/pull/7" and "PROJ-55" in shipped["related"]
    assert shipped["why"].startswith("Found by the evidence source") and shipped["history"][0]["at"] == "2026-10-16T11:00"
    assert len(d["details"]["planned"]) == len(d["planned"])
    p = next(x for x in d["details"]["planned"] if x["task"] == t["id"])
    assert p["project"] == "billing" and p["url"].endswith("PROJ-9") and "PROJ-9" in p["related"]
    assert "from ticket PROJ-9" in p["why"] and [h["event"] for h in p["history"]] == ["added by agent", "due"]


def test_every_suggestion_says_what_why_and_what_accept_and_dismiss_change(day, monkeypatch):
    """Issue #62: a suggestion names the thing, its evidence, and the effect of each button."""
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIX))
    monkeypatch.setenv("CB_T", "synthetic")
    assert main(["connect", "jira", "--ref", "env:CB_T", "--base-url", "https://jira.example.com", "--email", "me@example.com", "--no-verify"]) == 0
    priorities.add("Backfill old invoices", 3, 3, 2, [])
    inbox.regenerate(day)
    proposals = [i for i in inbox.open_items() if i["type"] == "proposal"]
    assert {"task-done", "new-task", "goal"} <= {i["kind"] for i in proposals}
    for i in proposals:
        for field in ("what", "why", "accept", "dismiss"):
            assert str(i.get(field) or "").strip(), f"{i['kind']} has no {field}"
    new = next(i for i in proposals if i["kind"] == "new-task")
    assert "PROJ-" in new["what"] and "assigned to you" in new["why"] and "Week" in new["accept"]
    done = next(i for i in proposals if i["kind"] == "task-done")
    assert "Backfill" in done["why"] and "done" in done["accept"]


def test_plan_for_tomorrow_leaves_out_what_today_still_holds(home, monkeypatch):
    """US-75 pictures: at 09:00 Today's "Plan for <next workday>" repeated the two tasks planned for today."""
    flaky = priorities.add("Fix the flaky checkout test", 3, 3, 0.5, [])
    doc = priorities.add("Write the queue design doc", 3, 3, 1.5, ["design"])
    billing = priorities.add("Migrate billing jobs", 5, 3, 6, [])
    monday = date(2026, 10, 19)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    planned = [x.split()[0] for x in daylog.build(monday)["planned"]]
    assert planned == [billing["id"]], "today's tasks are not planned again for tomorrow"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T17:45")
    planned = [x.split()[0] for x in daylog.build(monday)["planned"]]
    assert planned == [flaky["id"], doc["id"]], "after work hours, today's unfinished tasks come first tomorrow"


def test_a_past_day_log_plans_its_next_workday_in_full(home, monkeypatch):
    """A log rebuilt for an earlier day: that day is over, so its next workday gets the top tasks."""
    flaky = priorities.add("Fix the flaky checkout test", 3, 3, 0.5, [])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T09:00")
    assert [x.split()[0] for x in daylog.build(FRIDAY)["planned"]] == [flaky["id"]]


GENERIC_IDS = "collaboration, delivery, design, impact, leadership, ownership, quality"


def test_a_competency_question_lists_the_ids_to_answer_with(day):
    """US-75 pictures: "Which promotion competency does t001 show? (ids, comma-separated)" named no ids."""
    priorities.add("Fix the flaky checkout test", 3, 3, 0.5, [])
    inbox.regenerate(day)
    q = next(i for i in inbox.open_items() if i["kind"] == "competency")
    assert f"Answer one or more of: {GENERIC_IDS} (comma-separated)" in q["text"]
    inbox.answer(q["id"], "quality, delivery")
    assert priorities.load()[0]["competencies"] == ["quality", "delivery"]


def test_an_open_question_asked_before_gets_the_new_text(day):
    priorities.add("Fix the flaky checkout test", 3, 3, 0.5, [])
    inbox.regenerate(day)
    items = inbox.load()
    q = next(i for i in items if i["kind"] == "competency")
    q["text"] = "Which promotion competency does t001 (Fix the flaky checkout test) show? (ids, comma-separated)"
    inbox._save(items)
    inbox.regenerate(day)
    assert GENERIC_IDS in next(i for i in inbox.open_items() if i["kind"] == "competency")["text"]


def test_an_empty_plan_for_tomorrow_says_today_holds_every_task(home, monkeypatch):
    priorities.add("Fix the flaky checkout test", 3, 3, 0.5, [])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    d = daylog.build(date(2026, 10, 19))
    assert d["planned"] == [] and d["today_holds"] == 1
    assert "- Nothing left for 2026-10-20: today's plan holds every open task." in daylog.render(d)


def test_the_pm_update_names_later_today_work_as_next(day):
    """At 16:00 the 1h task is planned for later today, not tomorrow; the PM update still names it as next."""
    priorities.add("Write the queue design doc", 3, 3, 1, ["design"])
    d = daylog.build(day)
    assert d["planned"] == [] and d["today_holds"] == 1
    assert "next: write the queue design doc" in daylog.pm_report(d)["body"]


def test_a_project_next_step_that_today_holds_is_not_planned_for_tomorrow(home, monkeypatch):
    t = priorities.add("Fix the flaky checkout test", 3, 3, 0.5, [])
    projects.add("checkout")
    projects.link_task("checkout", t["id"])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    assert daylog.build(date(2026, 10, 19))["planned"] == [], "today's plan holds the project's next step"
