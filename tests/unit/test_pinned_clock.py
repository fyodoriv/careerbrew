"""The web views read one clock: with CAREERBREW_NOW pinned, Review and the Work view use the pinned time (US-75).

The visual tests pin the server at Monday 2026-10-19 09:00. Their pictures showed Review reading the wall clock:
today's plan counted as the last workday's (plan_done_rate 0, PIVOT) and the evidence was days old. The Work view
ranked tasks and graded the promotion case by the wall clock too, while the Dashboard used the pinned one, so the
two disagreed and the pictures would change as days pass. Each default now is `calendar.now()`.

The sweep (task pinned-clock-sweep) moved the rest: Teach me's today, cards and drills, approvals, goals history,
the job cooldown, mistakes, prep runs, and every other stored stamp. `test_clock_reads.py` keeps it that way.
"""

from __future__ import annotations

import importlib.util
import json
import time
from datetime import UTC, date, datetime, timedelta
from pathlib import Path

import pytest

from careerbrew import config, paths, review, store
from careerbrew.work import evidence, priorities, promo

from .conftest import FIXTURES

PINNED = "2026-01-05T09:00"  # a Monday before any real date this suite runs on
PINNED_UTC = "2026-01-05T13:00"  # the same time (Etc/GMT+4, a fixed UTC-4) as the stamps stored in UTC show it


@pytest.fixture(autouse=True)
def _utc_minus_4(monkeypatch):
    """This file's stamps assume UTC-4, the suite default; set here so no caller changes it. Test-function scope."""
    monkeypatch.setenv("TZ", "Etc/GMT+4")


@pytest.mark.parametrize("visual", [True, False])
def test_visual_seed_pins_watcher_metric_before_persisted_review(monkeypatch, visual):
    from careerbrew import cli
    from careerbrew.day import watch

    root = Path(__file__).resolve().parents[2]
    spec = importlib.util.spec_from_file_location("careerbrew_visual_seed", root / "tests/e2e/web_seed.py")
    assert spec and spec.loader
    seed = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(seed)
    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    monkeypatch.setenv("CAREERBREW_VISUAL_SEED", "1" if visual else "0")
    monkeypatch.setenv("CAREERBREW_TEST_OWNER", "1")
    monkeypatch.setenv("CB_SLACK", "synthetic-not-a-token")
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http"))

    def fixture_call(*argv, **kwargs):
        if argv == ("init",):
            paths.sub("jobs").mkdir(parents=True)
            paths.sub("day").mkdir(parents=True)
        elif argv == ("day", "watch", "once"):
            watch.write_state(cpu_history=[780], synthetic_marker="preserve")
        elif argv == ("review",):
            assert cli.main(["review"]) == 0

    monkeypatch.setattr(seed, "cb", fixture_call)
    seed.main(FIXTURES)
    state = watch.read_state()
    assert state["cpu_history"] == ([120] if visual else [780])
    assert state["synthetic_marker"] == "preserve"
    reports = list(paths.sub("out", "reports").glob("review-*.json"))
    assert len(reports) == 1
    result = json.loads(reports[0].read_text())
    summary = result.get("summary", result)
    assert any(row["id"] == "watcher_cpu_ms" for row in summary["done" if visual else "next"])
    if visual:
        assert 'CAREERBREW_VISUAL_SEED: "1"' in (root / "web/playwright.visual.config.ts").read_text()
    assert "CAREERBREW_VISUAL_SEED" not in (root / "web/playwright.config.ts").read_text()


@pytest.fixture(params=["Etc/GMT+4", "Europe/Berlin", "Australia/Sydney", "Pacific/Kiritimati"])
def machine_tz(request):
    """The machine's own zone (the `TZ` of this test process), apart from the settings zone (Etc/GMT+4).

    Lifecycle: test-function scope; the old `TZ` and the C library's zone come back at teardown.
    """
    with pytest.MonkeyPatch.context() as mp:
        config.set_value("timezone", "Etc/GMT+4")  # an empty setting would follow `TZ`
        mp.setenv("TZ", request.param)
        time.tzset()
        yield request.param
    time.tzset()


def test_review_measures_at_the_pinned_time(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    evidence.note("feat: shipped order export", ["delivery"])  # stamped with the pinned clock
    store.write_json(paths.sub("day", "plan-2026-01-05.json"), {"day": "2026-01-05", "schedule": [{"task": "t001"}]})
    values = review.measure()
    assert values["evidence_freshness_days"] == 0.0, "a note made now is 0 days old"
    assert values["plan_done_rate"] is None, "today's plan is not the last workday's plan, and no earlier plan exists"


def test_review_run_records_the_pinned_time(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    review.run()
    assert store.read_jsonl(paths.sub("review.jsonl"))[-1]["at"].startswith("2026-01-05T09:00")


def test_task_ranking_counts_days_to_due_from_the_pinned_day(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    priorities.add("Migrate billing jobs", 5, 3, 6, [], due="2026-01-07")
    task = next(t for t in priorities.rank({}) if t["title"] == "Migrate billing jobs")
    assert task["parts"]["due_days"] == 2


def test_promotion_coverage_counts_recent_evidence_from_the_pinned_time(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    evidence.note("Mentored a new hire", ["leadership"], at="2025-02-01T09:00:00-04:00")  # 11 months before the pinned day
    asm = promo.assess("generic-swe", "senior")
    lead = next(r for r in asm["competencies"] if r["id"] == "leadership")
    assert lead["credit"] == 1.0, "evidence inside the 12-month window counts in full"
    assert asm["at"].startswith("2026-01-05T09:00")


def test_a_task_is_added_and_done_at_the_pinned_time(monkeypatch):
    """Today's Plan rows show a task's history: under the pinned clock it said "added" days after the pinned day."""
    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    t = priorities.add("Fix the flaky checkout test", 3, 3, 0.5, [])
    assert t["added_at"].startswith("2026-01-05T09:00")
    assert priorities.done(t["id"], 0.5)["done_at"].startswith("2026-01-05T09:00")


def test_a_suggested_task_done_the_same_pinned_day_counts_as_followed(monkeypatch):
    from careerbrew.day import planner

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    t = priorities.add("Write the queue design doc", 3, 3, 1.5, [])
    planner.log_suggestion({"kind": "task", "task": t["id"]})  # stamped with the pinned clock
    priorities.done(t["id"], 1.5)
    assert review.measure()["suggestion_follow_rate"] == 1.0


def test_teach_me_counts_the_steps_and_minutes_of_the_pinned_day(monkeypatch, machine_tz):
    """Teach me's today (its steps, minutes, and the daily budget) was the machine's date, not the pinned day.

    The machine's zone must not matter: at UTC+12 and later, 12:00Z on 2026-01-05 is already 2026-01-06 there.
    """
    from careerbrew import teacher

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    store.append_jsonl(teacher._log_path(), [{"at": "2026-01-05T12:00:00+00:00", "kind": "card", "outcome": "good", "seconds": 120, "ref": "c1", "rules": ""}])
    teacher._record("card", "good", 60, {"card": {"id": "c2"}})
    assert teacher.log()[-1]["at"].startswith(PINNED_UTC)
    s = teacher.summary()
    assert (s["steps"], s["minutes"]) == (2, 3)
    assert teacher.feedback("The hint came too late.")["at"].startswith(PINNED_UTC)


def test_a_step_done_late_in_the_evening_counts_today_in_any_machine_zone(monkeypatch, machine_tz):
    """Review gap: today came from the settings zone and each row's day from the machine's zone.

    At 20:00 at UTC-4 the stamp is 00:00Z the next day, and in Berlin it is already that next day, so a step just
    recorded counted 0 steps, 0 minutes, and 0 toward the daily budget. Both sides now use the settings zone.
    """
    from careerbrew import teacher

    monkeypatch.setenv("CAREERBREW_NOW", "2026-01-05T20:00")
    teacher._record("card", "good", 120, {"card": {"id": "c1"}})
    assert teacher.log()[-1]["at"] == "2026-01-06T00:00:00+00:00"
    s = teacher.summary()
    assert (s["steps"], s["minutes"], teacher._today_minutes()) == (1, 2, 2)


def test_the_step_timer_measures_real_seconds_under_a_pinned_clock(monkeypatch):
    """The one wall-clock read Teach me keeps: a pinned clock never moves, so every step would take 0 s."""
    from careerbrew import teacher

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    started = (datetime.now(UTC) - timedelta(seconds=90)).isoformat(timespec="seconds")
    teacher.complete({"kind": "mistake", "mistake": {"at": PINNED_UTC, "tip": "Trace the smallest input."}, "started": started}, {})
    assert 85 <= teacher.log()[-1]["seconds"] <= 600


def test_a_card_review_is_stamped_and_counted_on_the_pinned_day(monkeypatch):
    from careerbrew.tutor import cards

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    card = cards.add("What does a hash map trade for O(1) lookups?", "Memory, and the order of keys.", ["arrays and hashing"])
    assert cards.review(card["id"], "good")["at"].startswith(PINNED_UTC)
    assert cards.stats()["reviewed_today"] == 1


def test_a_drill_attempt_is_stamped_at_the_pinned_time(monkeypatch):
    from careerbrew.tutor import engine

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    item_id = sorted(engine.items())[0]
    first = engine.rubrics()[engine.items()[item_id]["rubric"]]["criteria"][0]["id"]
    assert engine.record(item_id, [first])["at"].startswith(PINNED_UTC)


def test_an_approval_is_requested_and_decided_at_the_pinned_time(monkeypatch):
    from careerbrew import approvals

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    item = approvals.request("Send the weekly update")
    assert item["at"].startswith(PINNED_UTC)
    assert approvals.decide(item["id"], "approved")["decided_at"].startswith(PINNED_UTC)


def test_a_replaced_goal_enters_history_at_the_pinned_time(monkeypatch):
    from careerbrew import goals

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    goals.set_upcoming({"kind": "learn", "title": "Learn hashing", "topics": ["arrays and hashing"]})
    r = goals.set_upcoming({"kind": "promotion", "title": "Promotion case", "topics": []})
    assert r["upcoming"]["set_at"].startswith(PINNED_UTC)
    assert r["history"][-1]["replaced_at"].startswith(PINNED_UTC)


def test_the_job_cooldown_counts_back_from_the_pinned_day(monkeypatch):
    """A loop 10 months before the pinned day is still spent; one 13 months before is free again."""
    from careerbrew.jobs import strategy

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    store.append_jsonl(
        strategy.applications_path(),
        [
            {"company": "Northwind", "tier": "target", "stage": "screen", "at": "2025-03-01T10:00:00+00:00", "origin": "user"},
            {"company": "Contoso", "tier": "practice", "stage": "onsite", "at": "2024-12-01T10:00:00+00:00", "origin": "user"},
        ],
    )
    assert strategy.used_companies() == {"northwind"}
    strategy.log_application("Fabrikam", "practice", "applied")
    assert store.read_jsonl(strategy.applications_path())[-1]["at"].startswith(PINNED_UTC)


def test_a_mistake_is_stamped_at_the_pinned_time(monkeypatch):
    from careerbrew.tutor import mistakes

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    assert mistakes.record("contains-duplicate", "arrays and hashing", "edge-case", "empty input")["at"].startswith(PINNED_UTC)


def test_a_prep_run_a_story_and_a_stale_mark_are_dated_at_the_pinned_time(monkeypatch):
    from careerbrew import prep

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    prep.run("Amazon", answers={})
    assert store.read_json(prep.history_path("Amazon"), {})["runs"][-1]["at"].startswith(PINNED_UTC)
    assert prep.add_story("Led the outage review", ["leadership"])["added"] == "2026-01-05"
    prep.mark_stale("Amazon", "the loop changed")
    assert (prep.stale_mark("Amazon") or {}).get("at") == "2026-01-05"


def test_a_job_rating_is_stamped_at_the_pinned_time(monkeypatch):
    from careerbrew.jobs import want

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    want.rate("greenhouse:northwind:1", 4)
    assert store.read_jsonl(want.ratings_path())[-1]["at"].startswith(PINNED_UTC)


def test_an_exercise_sync_is_stamped_at_the_pinned_time(monkeypatch):
    from careerbrew.tutor import exercises

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    exercises.mark_solved(["contains-duplicate"])
    assert store.read_json(exercises._synced_path(), {})["at"].startswith(PINNED_UTC)


def test_the_review_dashboard_says_it_was_made_at_the_pinned_time(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    assert f"Generated {PINNED_UTC}" in review.dashboard(review.run())


def test_the_weekly_metrics_cover_the_pinned_week(monkeypatch):
    from careerbrew import metrics_share

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    assert metrics_share.collect()["week"] == metrics_share.week_of(date(2026, 1, 5))


def test_connector_and_mail_fetch_windows_count_back_from_the_pinned_time(monkeypatch):
    from careerbrew import calendar, connectors, mail

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    assert connectors._since({"days": 7}) == calendar.now() - timedelta(days=7)
    assert mail._since(1) == calendar.now() - timedelta(days=1)


def test_a_fetched_page_is_stamped_at_the_pinned_time(monkeypatch):
    from careerbrew import research

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http"))
    assert research.page("https://careers.example.com/interview-process")["fetched_at"].startswith(PINNED_UTC)


def test_a_sync_bundle_is_stamped_at_the_pinned_time(monkeypatch, tmp_path):
    import json

    from careerbrew import sync

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    sync.pair_init()
    sync.export(tmp_path / "bundle.cbs")
    assert json.loads(sync._fernet().decrypt((tmp_path / "bundle.cbs").read_bytes()))["at"].startswith(PINNED_UTC)


def test_a_plugin_install_is_stamped_at_the_pinned_time(monkeypatch, acme_plugin):
    from careerbrew import plugins

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    plugins.add(str(acme_plugin))
    entry = next(iter(store.read_json(plugins.lock_path(), {})["plugins"].values()))
    assert entry["installed_at"].startswith(PINNED_UTC)
    assert entry["first_installed_at"].startswith(PINNED_UTC)


def test_a_pivot_row_lands_in_the_results_at_the_pinned_time(monkeypatch):
    from careerbrew import changes
    from careerbrew.day import watch

    monkeypatch.setenv("CAREERBREW_NOW", PINNED)
    monkeypatch.setattr(changes, "check", lambda fetch=False: [{"kind": "ladder", "id": "generic-swe", "change": "changed", "action": "re-plan"}])
    monkeypatch.setattr(watch, "notify", lambda text: None)
    assert watch._src_changes(None)[0]["at"].startswith(PINNED)


def test_unknown_machine_zone_warns_once_with_the_fix(monkeypatch, capsys):
    """No readable zone: times use UTC, and one line says how to set `timezone` (never a silent UTC)."""
    from careerbrew import calendar

    monkeypatch.setenv("TZ", "Not/AZone")
    monkeypatch.setattr(calendar, "_warned_no_zone", False)
    assert calendar.system_tz() == "UTC" and calendar.system_tz() == "UTC"
    err = capsys.readouterr().err
    assert err.count("careerbrew config set timezone") == 1 and "UTC" in err
    monkeypatch.setenv("TZ", "Etc/GMT+4")
    assert calendar.system_tz() == "Etc/GMT+4" and capsys.readouterr().err == ""
