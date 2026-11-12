"""Issue #185, part 3: goals and teach signals change what Teach me shows next."""

from datetime import UTC, datetime

from careerbrew import goals, teacher
from careerbrew.tutor import cards, exercises
from careerbrew.work import promo

T0 = datetime(2026, 10, 21, 9, 0, tzinfo=UTC)
CRASH = "def contains_duplicate(nums):\n    return nums[0] in nums[1:]\n"


def _deck(topics=("coding", "behavioral", "system design"), n=3):
    return [{"id": f"{t[:3]}-{i}", "question": f"{t} q{i}", "answer": f"{t} a{i}", "topic": t, "tags": [t]} for t in topics for i in range(n)]


def test_the_goals_card_topics_come_first_among_new_cards(home):
    cards.write_deck("test", _deck())
    q = cards.queue(at=T0, new_per_day=3, topics=["system design"])
    assert [c["topic"] for c in q] == ["system design"] * 3
    assert [c["topic"] for c in cards.queue(at=T0, new_per_day=3)] != ["system design"] * 3, "with no topics the deck interleaves"


def test_teach_me_passes_the_goal_topics_to_the_card_queue(home, monkeypatch):
    seen = {}
    real = cards.queue
    monkeypatch.setattr(cards, "queue", lambda **kw: seen.update(kw) or real(**kw))
    goals.set_upcoming({"kind": "learn", "title": "Design", "topics": ["system design"], "minutes_per_day": 60})
    teacher.next_step()
    assert "system design" in (seen.get("topics") or [])


def test_dropping_a_card_takes_it_out_of_review(home):
    goals.set_upcoming({"kind": "custom", "title": "Practice", "minutes_per_day": 90})
    c = cards.add("What is a heap?", "A tree with the heap property.", ["cs"])
    teacher.complete({"kind": "validate", "card": {**c, "deck": "mine"}}, {"decision": "drop"})
    assert c["id"] not in {x["id"] for x in cards.queue(new_per_day=50)}


def test_an_exercise_on_the_retry_list_comes_back_after_three_days(home, monkeypatch):
    goals.set_upcoming({"kind": "custom", "title": "Practice", "minutes_per_day": 90})
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T09:00+00:00")
    exercises.submit("contains-duplicate", CRASH, seconds=60, hints_used=0)
    s = teacher.state()
    s["retry_later"] = ["contains-duplicate"]
    teacher._save_state(s)
    assert teacher._retry_due() is None, "not before three days"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-24T10:00+00:00")
    step = teacher.next_step()
    assert step["kind"] == "exercise" and step.get("retry") and step["exercise"]["id"] == "contains-duplicate"


def test_low_ratings_on_card_steps_put_exercises_before_due_cards(home, monkeypatch):
    goals.set_upcoming({"kind": "custom", "title": "Practice", "minutes_per_day": 90})
    monkeypatch.setattr(cards, "queue", lambda **kw: [{"id": "c1", "question": "q", "mode": "review", "topic": "t"}])
    assert teacher.next_step()["kind"] == "card", "a due card comes first by default"
    teacher.feedback("too many cards", "card", 1)
    teacher.feedback("cards again", "card", 2)
    assert teacher.next_step()["kind"] in ("lesson", "exercise"), "two low ratings move cards later"


def test_one_low_rating_is_not_enough(home, monkeypatch):
    goals.set_upcoming({"kind": "custom", "title": "Practice", "minutes_per_day": 90})
    monkeypatch.setattr(cards, "queue", lambda **kw: [{"id": "c1", "question": "q", "mode": "review", "topic": "t"}])
    teacher.feedback("too many cards", "card", 1)
    assert teacher.next_step()["kind"] == "card"


def test_a_long_term_goal_that_names_a_competency_raises_its_gap(home):
    before = {g["id"]: g["priority"] for g in promo.gaps(promo.assess("generic-swe", "senior"))}
    goals.set_long_term([{"text": "Lead the platform's technical design reviews this year"}])
    after = {g["id"]: g for g in promo.gaps(promo.assess("generic-swe", "senior"))}
    boosted = [i for i, g in after.items() if g["goal"]]
    assert boosted, "a goal naming design raises that competency"
    for i in boosted:
        assert after[i]["priority"] == round(before[i] * promo.GOAL_BOOST, 3)
    assert all(after[i]["priority"] == before[i] for i in after if i not in boosted)


def test_an_exercise_on_the_retry_list_waits_and_is_listed_once(home, monkeypatch):
    goals.set_upcoming({"kind": "custom", "title": "Practice", "minutes_per_day": 90})
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T09:00+00:00")
    s = teacher.state()
    s["retry_later"] = ["contains-duplicate"]
    teacher._save_state(s)
    assert exercises.next_exercise(skip={"contains-duplicate"}) is None or exercises.next_exercise(skip={"contains-duplicate"})["id"] != "contains-duplicate"
    step = teacher.next_step()
    assert not (step["kind"] == "exercise" and step["exercise"]["id"] == "contains-duplicate"), "it waits; it is not handed back at once"


def test_goal_words_match_by_stem_and_skip_generic_words():
    comp = {"id": "mentoring", "name": "Mentoring and growing others"}
    assert promo._named_by_goal(comp, {"mentor", "juniors"})
    assert not promo._named_by_goal({"id": "code-quality", "name": "Code quality"}, {"code"}), "generic words do not boost"


def test_a_retry_shown_twice_is_still_the_retry_until_it_is_tried(home, monkeypatch):
    goals.set_upcoming({"kind": "custom", "title": "Practice", "minutes_per_day": 90})
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T09:00+00:00")
    exercises.submit("contains-duplicate", CRASH, seconds=60, hints_used=0)
    s = teacher.state()
    s["retry_later"] = ["contains-duplicate"]
    teacher._save_state(s)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-24T10:00+00:00")
    teacher.next_step()  # the view, then a refetch of it
    step = teacher.next_step()
    assert step.get("retry") and step["exercise"]["id"] == "contains-duplicate", "reading the view must not use up the retry"
    teacher.complete(step, {"code": CRASH})
    assert "contains-duplicate" not in teacher.state().get("retry_later", []), "a tried retry leaves the list"
    assert teacher.state().get("retry_since", {}).get("contains-duplicate")
