"""Dashboard: the most efficient work action per horizon, with why (US-41)."""

from datetime import timedelta

from careerbrew import calendar
from careerbrew.cli import main  # noqa: F401  (imports the CLI so plugins load the same way)
from careerbrew.work import evidence, focus, priorities

HORIZONS = ["now", "today", "week", "month", "quarter", "year", "company"]


def _at(day_offset=0, hour=10):
    base = calendar.now().replace(hour=hour, minute=0, second=0, microsecond=0)
    while base.weekday() >= 5:
        base += timedelta(days=1)
    return base + timedelta(days=day_offset)


def test_every_horizon_has_an_item_with_a_why(home):
    now = _at()
    due_soon = (now.date() + timedelta(days=1)).isoformat()
    due_month = (now.date() + timedelta(days=20)).isoformat()
    priorities.add("fix flaky checkout test", 4, 4, 1, ["quality"], due=due_soon)
    priorities.add("write design doc for queue", 3, 3, 2, ["design"])
    for i in range(12):
        priorities.add(f"small ticket {i}", 2, 2, 3, [])
    priorities.add("migrate billing to new API", 5, 2, 16, ["impact"], due=due_month)
    for i in range(5):
        evidence.note(f"feat: shipped thing {i}", ["delivery"])
    d = focus.build(now)
    assert [h["id"] for h in d["horizons"]] == HORIZONS
    for h in d["horizons"]:
        assert h["items"], (h["id"], h["empty"])
        assert all(it["title"] and it["why"] for it in h["items"]), h["id"]
    by = {h["id"]: h for h in d["horizons"]}
    assert "t001" in [it["ref"] for it in by["today"]["items"]], "the due-tomorrow task is today's work"
    assert "migrate billing" in " ".join(it["title"] for it in by["month"]["items"]), "a 16h task is monthly work"
    assert "priority" in by["quarter"]["items"][0]["why"] and "/ effort" in by["quarter"]["items"][0]["why"]
    assert "generic ladder" in by["company"]["items"][0]["why"], "no plugin: says the ladder is generic"
    assert not {it["ref"] for it in by["year"]["items"]} & {it["ref"] for it in by["quarter"]["items"]}, "year adds what quarter does not cover"
    titles = [it.get("ref") for h in ("today", "week", "month") for it in by[h]["items"]]
    assert len(titles) == len(set(titles)), "a task appears in one horizon only"


def test_empty_data_gives_actionable_empty_states(home):
    d = focus.build(_at())
    by = {h["id"]: h for h in d["horizons"]}
    assert by["now"]["items"] and by["now"]["items"][0]["why"]
    assert not by["today"]["items"] and "Add a task" in by["today"]["empty"]
    assert by["quarter"]["items"], "no evidence: every competency is a gap"


def test_focus_writes_no_future_plan(home):
    priorities.add("task", 3, 3, 1, [])
    now = _at()
    focus.build(now)
    written = {p.name for p in (home / "day").glob("plan-*.json")}
    assert written <= {f"plan-{now.date().isoformat()}.json"}, "only today's plan, as the Today view does"


def test_company_ladder_from_plugin_is_named(home, tmp_path):
    root = tmp_path / "p"
    (root / "ladders").mkdir(parents=True)
    (root / "ladders" / "x.yaml").write_text(
        "id: acme-eng\nlevels: [senior]\ncompetencies:\n"
        "  - {id: impact, name: Business impact, weight: 5, signals: {tags: [impact]}, expect: {senior: 2}}\n"
        "  - {id: craft, name: Craft, weight: 1, signals: {tags: [quality]}, expect: {senior: 2}}\n"
    )
    (root / "careerbrew-plugin.yaml").write_text("name: acme\nversion: 0.1.0\nprovides: {ladders: ladders/*.yaml}\nsettings_defaults: {ladder: acme-eng, target_level: senior}\n")
    assert main(["plugin", "add", str(root)]) == 0
    by = {h["id"]: h for h in focus.build(_at())["horizons"]}
    item = by["company"]["items"][0]
    assert "Business impact" in item["title"] and "company's ladder" in item["why"]


def test_why_quotes_the_numbers_rank_used(home):
    now = _at()
    priorities.add("tiny fix", 3, 3, 0.1, [])
    tasks = priorities.load()
    tasks.append({"id": "t002", "title": "legacy", "impact": "high", "urgency": None, "estimate_h": 1, "competencies": [], "due": (now.date() - timedelta(days=3)).isoformat(), "status": "open"})
    priorities.save(tasks)
    by = {h["id"]: h for h in focus.build(now)["horizons"]}
    whys = " ".join(it["why"] for h in ("now", "today", "week", "month") for it in by[h]["items"])
    assert "sqrt(0.25h)" in whys and "sqrt(0.1h)" not in whys, "the divisor rank() really used"
    assert "impact high" not in whys and "urgency None" not in whys and "impact 3" in whys
    assert "overdue by 3 days" in whys and "due in -" not in whys
    assert "rank 1 of 2" in whys and "rank 2 of 2" in whys and "the most value per hour among" not in whys


def test_company_ties_and_empty_company_ladder(home, tmp_path):
    by = {h["id"]: h for h in focus.build(_at())["horizons"]}
    assert "tied with 2 others; lowest coverage first" in by["company"]["items"][0]["why"]
    root = tmp_path / "p"
    (root / "ladders").mkdir(parents=True)
    (root / "ladders" / "x.yaml").write_text("id: acme-eng\nlevels: [senior]\ncompetencies:\n  - {id: a, name: A, weight: 0, signals: {}, expect: {senior: 1}}\n")
    (root / "careerbrew-plugin.yaml").write_text("name: acme\nversion: 0.1.0\nprovides: {ladders: ladders/*.yaml}\nsettings_defaults: {ladder: acme-eng}\n")
    assert main(["plugin", "add", str(root)]) == 0
    co = {h["id"]: h for h in focus.build(_at())["horizons"]}["company"]
    assert not co["items"] and "no weighted competencies" in co["empty"]


def test_tomorrow_lists_the_next_workday_plan_with_project_and_time(home):
    """Issue #64: the Dashboard shows tomorrow's proposed tasks (title, project, time)."""
    from careerbrew.work import projects

    priorities.add("write design doc for queue", 3, 3, 2, ["design"])
    projects.add("billing")
    projects.link_task("billing", "t001")
    now = _at(hour=16)  # 90 minutes left today: the 2h task is tomorrow's
    d = focus.build(now)
    tm = d["tomorrow"]
    nxt = now.date() + timedelta(days=1)
    while nxt.weekday() >= 5:
        nxt += timedelta(days=1)
    assert tm["day"] == nxt.isoformat(), "tomorrow is the next workday"
    assert tm["items"], "a 2h task fits tomorrow"
    it = tm["items"][0]
    assert it["title"] == "write design doc for queue" and it["project"] == "billing" and it["ref"] == "t001"
    assert it["start"] < it["end"] and len(it["start"]) == 5, "times are HH:MM"
    assert not any(str(i["ref"]).startswith("goal:") for i in tm["items"]), "work tasks only"


def test_tomorrow_is_empty_without_tasks(home):
    tm = focus.build(_at())["tomorrow"]
    assert tm["items"] == [] and "Add a task" in tm["empty"]


def _three_tasks():
    """The e2e seed's shape: two short tasks that fit today, and a 6h one that does not fit after them."""
    return [priorities.add("fix flaky checkout test", 3, 3, 0.5, []), priorities.add("write design doc for queue", 3, 3, 1.5, ["design"]), priorities.add("migrate billing jobs", 5, 3, 6, [])]


def test_tomorrow_does_not_repeat_today_and_matches_the_week_plan(home):
    """US-75 pictures: at 09:00 the Tomorrow card listed today's two tasks at today's times, while Week put the 6h task on that day."""
    from careerbrew.day import planner

    flaky, doc, billing = _three_tasks()
    now = _at(hour=9)
    d = focus.build(now)
    today = {s["task"] for s in planner.build(now.date(), now, persist=False)["schedule"]}
    tomorrow = [it["ref"] for it in d["tomorrow"]["items"]]
    assert today == {flaky["id"], doc["id"]}
    assert tomorrow == [billing["id"]], "what today's plan holds is not planned again tomorrow"
    week = {x["day"]: [s["task"] for s in x["schedule"]] for x in planner.plan_week(now.date(), 7, now)}
    assert week[d["tomorrow"]["day"]] == tomorrow, "the Tomorrow card is Week's next workday"
    by = {h["id"]: h for h in d["horizons"]}
    assert f"planned for {date_label(d['tomorrow']['day'])}" in by["week"]["items"][0]["why"], "This week names the same day"


def test_tomorrow_takes_what_today_has_no_time_left_for(home):
    """Late in the day the unfinished tasks no longer fit today, so they move to tomorrow on the card and in Week."""
    from careerbrew.day import planner

    flaky, doc, _ = _three_tasks()
    late = _at(hour=17)
    tm = focus.build(late)["tomorrow"]
    assert planner.build(late.date(), late, persist=False)["schedule"] == [], "30 minutes left: nothing fits today"
    assert [it["ref"] for it in tm["items"]] == [flaky["id"], doc["id"]]
    week = {x["day"]: [s["task"] for s in x["schedule"]] for x in planner.plan_week(late.date(), 7, late)}
    assert week[tm["day"]] == [flaky["id"], doc["id"]]


def test_tomorrow_says_when_today_holds_every_task(home):
    priorities.add("fix flaky checkout test", 3, 3, 0.5, [])
    tm = focus.build(_at(hour=9))["tomorrow"]
    assert tm["items"] == [] and "Today's plan holds every open task" in tm["empty"]


def date_label(day: str) -> str:
    from datetime import date as _date

    return f"{_date.fromisoformat(day).strftime('%a')} {day}"
