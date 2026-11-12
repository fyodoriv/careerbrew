from datetime import UTC, datetime, timedelta

from careerbrew import plugins, prep, review, store
from careerbrew.day import planner, watch
from careerbrew.jobs import sources, strategy
from careerbrew.work import evidence, priorities


def test_story_metrics_compute_from_real_data(ats_sources, acme_plugin, monkeypatch):
    home = ats_sources
    now = datetime.now(UTC)
    assert all(v is None for k, v in review.story_metrics(now).items() if k != "eligible_postings")
    sources.scan()
    strategy.choose(sources.load()[0].id, "target")  # loops_sourced measures the jobs the owner chose
    strategy.build()
    asks = [i["id"] for i in prep.checklist("Stripe")["items"] if not i["auto"]]
    prep.run("Stripe", answers=dict.fromkeys(asks, True))
    store.append_jsonl(home / "jobs" / "inbox.jsonl", [{"id": "m1", "received": (now - timedelta(hours=5)).isoformat()}])
    strategy.log_application("Stripe", "target", "screen")
    plugins.add(str(acme_plugin), allow_exec=True)
    evidence.collect()
    watch.write_state(cpu_history=[10, 20, 30, 400])
    priorities.add("a", 3, 3, 1, [])
    planner.log_suggestion({"kind": "task", "task": "t001"})
    priorities.done("t001", 1)
    store.write_json(home / "day" / f"plan-{(now - timedelta(days=1)).date().isoformat()}.json", {"schedule": [{"task": "t001"}, {"task": "t009"}]})
    m = review.story_metrics(now)
    assert m["loops_sourced"] is not None and 0 <= m["loops_sourced"] <= 1
    assert m["prep_score"] > 0 and m["eligible_postings"] >= 1
    assert m["interview_followup"] == 1.0
    assert m["plugin_setup_minutes"] is not None and m["plugin_setup_minutes"] < 10
    assert m["watcher_cpu_ms"] == 400.0
    assert m["suggestion_follow_rate"] == 1.0
    assert m["plan_done_rate"] == 0.5
    rows = {r["metric"]: r["status"] for r in review.run(now)}
    assert rows["interview_followup"] == "KEEP" and rows["plan_done_rate"] == "WATCH"


def test_suggestion_follow_rate_uses_local_days_across_utc_midnight(home, monkeypatch):
    """23:30 at UTC-4 is the next day in UTC; a task done then still counts as the same local day."""
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T23:30-04:00")
    priorities.add("a", 3, 3, 1, [])
    planner.log_suggestion({"kind": "task", "task": "t001"})
    tasks = priorities.load()
    tasks[0].update(status="done", done_at="2026-10-20T03:45:00+00:00")
    priorities.save(tasks)
    assert review.story_metrics(datetime(2026, 10, 20, 4, 0, tzinfo=UTC))["suggestion_follow_rate"] == 1.0


def test_a_plugin_refresh_never_makes_setup_minutes_negative(home, acme_plugin):
    # #54: refresh reinstalled after evidence existed and the metric went to -2449.8 (shown as KEEP)
    plugins.add(str(acme_plugin), allow_exec=True)
    evidence.collect()
    first = plugins.installed()[0]["first_installed_at"]
    plugins.add(str(acme_plugin), allow_exec=True)  # what `plugin refresh` does
    assert plugins.installed()[0]["first_installed_at"] == first
    m = review.story_metrics(datetime.now(UTC))["plugin_setup_minutes"]
    assert m is not None and m >= 0
