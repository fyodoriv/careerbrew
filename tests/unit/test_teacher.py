import pytest

from careerbrew import approvals, goals, teacher
from careerbrew.tutor import cards, exercises, mistakes

GOOD = "def contains_duplicate(nums):\n    seen = set()\n    for x in nums:\n        if x in seen:\n            return True\n        seen.add(x)\n    return False\n"
CRASH = "def contains_duplicate(nums):\n    return nums[0] in nums[1:]\n"


@pytest.fixture(autouse=True)
def a_goal(request):
    """Lessons need a goal (#54); a custom goal keeps the default study order."""
    if "no_goal" not in request.node.name and "home" in request.fixturenames:
        request.getfixturevalue("home")
        goals.set_upcoming({"kind": "custom", "title": "Practice", "minutes_per_day": 90})


def test_with_no_goal_teach_me_asks_for_a_goal_not_a_lesson(home):
    s = teacher.next_step()
    assert s["kind"] == "goal" and "goals set" in s["setup"]


def test_with_no_goal_the_setup_is_one_command_that_runs(home):
    """Teach me shows `setup` as code to copy (US-75 visual review): it was "careerbrew goals set (or Goals in the web app)",
    which no shell runs. It must parse as a careerbrew command; the page already names the Goals view beside it."""
    import shlex

    from careerbrew.cli import build_parser

    setup = teacher.next_step()["setup"]
    argv = shlex.split(setup)
    assert argv[0] == "careerbrew" and build_parser().parse_args(argv[1:]).cmd == "goals"
    assert "(" not in setup and "web app" not in setup


def test_a_session_goes_lesson_then_exercise_one_step_at_a_time(home):
    s = teacher.next_step()
    assert s["kind"] == "lesson" and s["pattern"] == "arrays and hashing"
    r = teacher.complete(s, {"confirmed": True})
    assert r["next"]["kind"] == "exercise" and "solution" not in r["next"]["exercise"], "the solution is never sent before an attempt"
    assert r["next"]["exercise"]["hints"] == [], "no hints before the first attempt"


def test_a_failed_attempt_names_the_mistake_and_unlocks_a_hint(home):
    teacher.complete(teacher.next_step(), {"confirmed": True})
    s = teacher.next_step()
    r = teacher.complete(s, {"code": CRASH})
    assert r["result"]["grade"]["correct"] is False and "hint is unlocked" in r["message"]
    assert mistakes.load()[0]["kind"] in ("exception", "edge-case")
    assert any(c["deck"] == "mistakes" for c in cards.all_cards().values()), "a prevention card is added"
    assert len(teacher.next_step()["exercise"]["hints"]) == 1


def test_a_repeated_mistake_becomes_the_next_step(home):
    teacher.complete(teacher.next_step(), {"confirmed": True})
    teacher.complete(teacher.next_step(), {"code": CRASH})
    teacher.complete(teacher.next_step(), {"code": CRASH})
    s = teacher.next_step()
    assert s["kind"] == "mistake" and s["mistake"]["repeat"]
    r = teacher.complete(s, {})
    assert r["next"]["kind"] != "mistake", "acknowledged once, then work continues"


def test_three_failures_show_the_solution_and_a_pass_is_graded(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T09:00+00:00")
    teacher.complete(teacher.next_step(), {"confirmed": True})
    for _ in range(3):
        s = teacher.next_step()
        if s["kind"] == "mistake":
            teacher.complete(s, {})
            s = teacher.next_step()
        r = teacher.complete(s, {"code": CRASH})
    assert r["result"]["solution"]
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-24T10:00+00:00")  # it comes back after RETRY_DAYS (issue #185)
    s = teacher.next_step()
    for _ in range(10):
        if s["kind"] == "exercise":
            break
        s = teacher.next_step() if s["kind"] != "mistake" else teacher.complete(s, {})["next"]
    assert s["kind"] == "exercise" and s["exercise"]["id"] == "contains-duplicate"
    r = teacher.complete(s, {"code": GOOD})
    assert r["result"]["grade"]["correct"] and "All tests pass" in r["message"]
    assert exercises.status()["contains-duplicate"]["solved"]


def test_approvals_come_first_and_only_the_decision_is_recorded(home):
    a = approvals.request("Delete the old branch", detail="merged last week", by="claude")
    s = teacher.next_step()
    assert s["kind"] == "approve" and s["approval"]["id"] == a["id"]
    teacher.complete(s, {"decision": "approved", "note": "ok"})
    assert approvals.get(a["id"])["status"] == "approved"
    with pytest.raises(ValueError):
        approvals.decide(a["id"], "rejected")


def test_lesson_not_yet_is_a_concept_mistake_and_comes_back(home):
    s = teacher.next_step()
    r = teacher.complete(s, {"confirmed": False})
    assert "explain it in your own words" in r["message"]
    assert mistakes.load()[0]["kind"] == "concept"


def test_the_daily_budget_ends_the_session_and_cards_validate(home, monkeypatch):
    goals.set_upcoming({"kind": "learn", "title": "Hashing", "topics": ["arrays and hashing"], "minutes_per_day": 10})
    monkeypatch.setattr(teacher, "_today_minutes", lambda: 10)
    assert teacher.next_step()["kind"] == "done"
    monkeypatch.undo()


def test_predicted_feedback_turns_signals_into_proposals(home):
    teacher.complete(teacher.next_step(), {"confirmed": True})
    for _ in range(3):
        s = teacher.next_step()
        if s["kind"] == "mistake":
            teacher.complete(s, {})
            s = teacher.next_step()
        teacher.complete(s, {"code": CRASH})
    teacher.feedback("hints come too late", "exercise", 3)
    signals = {p["signal"] for p in teacher.improvements()}
    assert {"stuck-exercise", "repeat-mistake", "written"} <= signals


def test_leetcode_public_profile_sync_skips_solved_exercises(home, monkeypatch):
    from .conftest import FIXTURES

    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http"))
    r = exercises.sync_leetcode("someone")
    assert r == {"matched": 1, "new": 1, "total": 1, "unknown": 1, "seen": 2}
    assert exercises.status()["contains-duplicate"]["synced"]
    assert exercises.next_exercise() is None, "the only fixture exercise is already solved elsewhere"
    with pytest.raises(ValueError):
        exercises.sync_leetcode("bad name; drop")
    assert exercises.mark_solved(["contains-duplicate"])["new"] == 0


def test_every_teaching_rule_cites_a_known_source():
    import yaml

    from careerbrew import paths

    known = set((yaml.safe_load((paths.REPO_ROOT / "knowledge" / "sources.yaml").read_text()) or {}).get("sources", {}))
    for r in teacher.rules()["rules"]:
        assert r["source"] == "written for careerbrew" or r["source"] in known, r["id"]


def test_a_worked_example_appears_after_two_failures_when_one_exists(home, monkeypatch, tmp_path):
    import shutil

    from .conftest import FIXTURES

    d = tmp_path / "ex"
    d.mkdir()
    shutil.copy(FIXTURES / "exercises" / "001-contains-duplicate.yaml", d)
    import yaml

    second = yaml.safe_load((d / "001-contains-duplicate.yaml").read_text())
    second.update(id="contains-duplicate-2", order=2, title="Any value twice, again?")
    (d / "002-again.yaml").write_text(yaml.safe_dump(second))
    monkeypatch.setenv("CAREERBREW_EXERCISES", str(d))
    teacher.complete(teacher.next_step(), {"confirmed": True})
    teacher.complete(teacher.next_step(), {"code": GOOD})  # solve the first
    for _ in range(2):
        s = teacher.next_step()
        if s["kind"] == "mistake":
            teacher.complete(s, {})
            s = teacher.next_step()
        teacher.complete(s, {"code": CRASH})
    s = teacher.next_step()
    while s["kind"] == "mistake":
        s = teacher.complete(s, {})["next"]
    assert s["exercise"]["id"] == "contains-duplicate-2" and s["exercise"]["worked_example"]["title"] == "Any value twice?"


def test_predicted_mistakes_are_prevented_before_the_next_attempt(home):
    teacher.complete(teacher.next_step(), {"confirmed": True})
    s = teacher.next_step()
    assert s["exercise"]["watch_out"] == []  # nothing predicted yet
    teacher.complete(s, {"code": CRASH})
    s = teacher.next_step()
    while s["kind"] == "mistake":
        s = teacher.complete(s, {})["next"]
    assert s["exercise"]["watch_out"], "after a crash, the exception pitfall is shown up front"
    assert all(i["auto"] for i in teacher.improvements() if i["signal"] in teacher.AUTO)


def test_a_lesson_that_did_not_land_comes_back_with_an_example(home):
    s = teacher.next_step()
    teacher.complete(s, {"confirmed": False})
    s = teacher.next_step()
    while s["kind"] != "lesson":
        s = teacher.complete(s, {})["next"]
    assert s["example"]["solution"]


def test_an_unreachable_leetcode_says_so_instead_of_a_traceback(home, monkeypatch, capsys):
    import urllib.error

    from careerbrew import cli, connectors

    monkeypatch.delenv("CAREERBREW_HTTP_FIXTURES", raising=False)

    def refuse(*a, **k):
        raise urllib.error.URLError("Tunnel connection failed: 403 Forbidden")

    monkeypatch.setattr(connectors, "http_json", refuse)
    assert cli.main(["exercises", "sync", "--leetcode", "someone"]) == 1
    assert "unreachable" in capsys.readouterr().out


def test_full_leetcode_sync_uses_the_owners_cookie_only_when_set(home, monkeypatch):
    from careerbrew import connectors
    from careerbrew.tutor import exercises

    seen: list[dict] = []

    def fake(url, headers, body=None):
        seen.append(headers)
        if "questionList" in body["query"]:
            return {"data": {"questionList": {"totalNum": 1, "data": [{"titleSlug": "contains-duplicate"}]}}}
        return {"data": {"recentAcSubmissionList": []}}

    monkeypatch.setattr(connectors, "http_json", fake)
    assert exercises.sync_leetcode("someone")["matched"] == 0 and len(seen) == 1  # no cookie: public list only
    monkeypatch.setenv("LEETCODE_SESSION", "canary-session")
    assert exercises.sync_leetcode("someone")["matched"] == 1
    assert "canary-session" in seen[-1]["Cookie"]


def test_today_counts_steps_by_local_day_not_utc_day(home):
    # Rows are stamped in UTC; late in a local evening the UTC date is already tomorrow.
    # "Local" is the settings zone that `calendar.now()` uses, not the machine's zone.
    from datetime import datetime, time

    from careerbrew import calendar, store

    today, tz = calendar.now().date(), calendar.local_tz()
    late = datetime.combine(today, time(23, 30), tz)
    early = datetime.combine(today, time(0, 30), tz)
    rows = [{"at": t.astimezone(teacher.UTC).isoformat(timespec="seconds"), "kind": "lesson", "outcome": "ok", "seconds": 60, "ref": "", "rules": ""} for t in (late, early)]
    store.append_jsonl(teacher._log_path(), rows)
    assert teacher.summary()["steps"] == 2
