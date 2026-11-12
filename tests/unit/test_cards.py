"""Flashcard practice: FSRS, recall before reveal, interleaving, daily new cap, leeches, stats."""

from datetime import UTC, datetime, timedelta

import pytest

from careerbrew.tutor import cards

T0 = datetime(2026, 10, 19, 9, 0, tzinfo=UTC)


def _deck(n_per_topic=3, topics=("coding", "behavioral", "system design")):
    return [{"id": f"{t[:3]}-{i}", "question": f"{t} q{i}", "answer": f"{t} a{i}", "topic": t, "tags": [t]} for t in topics for i in range(n_per_topic)]


def test_add_and_list_a_card(home):
    c = cards.add("What is a heap?", "A tree with the heap property.", ["Computer Science"])
    assert c["topic"] == "computer science"
    assert c["id"] in cards.all_cards()


def test_card_needs_a_question_and_an_answer(home):
    with pytest.raises(ValueError, match="question"):
        cards.add("  ", "x", [])
    with pytest.raises(ValueError, match="answer"):
        cards.add("q", "", [])


def test_new_cards_are_interleaved_by_topic_and_capped_per_day(home):
    cards.write_deck("test", _deck())
    q = cards.queue(at=T0, new_per_day=5)
    assert len(q) == 5, "daily new-card cap"
    topics = [c["topic"] for c in q]
    assert topics[0] != topics[1] != topics[2], "consecutive cards come from different topics"


def test_reviewed_cards_come_back_when_due_and_new_cap_counts_today(home):
    cards.write_deck("test", _deck(1))
    first = cards.queue(at=T0, new_per_day=2)
    for c in first:
        cards.review(c["id"], "good", at=T0)
    assert len(cards.queue(at=T0 + timedelta(minutes=1), new_per_day=2)) == 0, "both new slots used today"
    later = cards.queue(at=T0 + timedelta(days=30), new_per_day=2)
    assert {c["id"] for c in first} <= {c["id"] for c in later}, "due cards return"
    assert all(c["mode"] in ("review", "new") for c in later)


def test_grades_change_the_interval(home):
    cards.write_deck("test", _deck(1, ("coding",)) + [{"id": "x2", "question": "q", "answer": "a", "topic": "coding"}])
    easy = cards.review("cod-0", "easy", at=T0)
    again = cards.review("x2", "again", at=T0)
    assert easy["due"] > again["due"]


def test_unknown_rating_or_card_is_refused(home):
    cards.write_deck("test", _deck(1, ("coding",)))
    with pytest.raises(ValueError, match="rating"):
        cards.review("cod-0", "meh", at=T0)
    with pytest.raises(KeyError):
        cards.review("nope", "good", at=T0)


def test_a_leech_is_suspended_after_repeated_lapses(home):
    cards.write_deck("test", _deck(1, ("coding",)))
    at, row = T0, {}
    for _ in range(cards.LEECH_LAPSES):  # learn it, then forget it, again and again
        cards.review("cod-0", "good", at=at)
        at += timedelta(days=30)
        cards.review("cod-0", "good", at=at)
        at += timedelta(days=60)
        row = cards.review("cod-0", "again", at=at)
        at += timedelta(days=1)
    assert row["leech"] is True
    assert "cod-0" not in {c["id"] for c in cards.queue(at=at + timedelta(days=30))}
    cards.unsuspend("cod-0")
    assert "cod-0" in {c["id"] for c in cards.queue(at=at + timedelta(days=30))}


def test_stats_report_due_retention_and_topics(home):
    cards.write_deck("test", _deck(2, ("coding", "behavioral")))
    cards.review("cod-0", "good", at=T0)
    cards.review("cod-1", "again", at=T0)
    s = cards.stats(at=T0)
    assert s["total"] == 4 and s["new"] == 2 and s["reviewed_today"] == 2
    assert s["retention_30d"] == 0.5
    assert s["by_topic"]["coding"]["total"] == 2


def test_rewriting_a_deck_keeps_review_history(home):
    cards.write_deck("notion", _deck(1, ("coding",)))
    cards.review("cod-0", "good", at=T0)
    cards.write_deck("notion", _deck(1, ("coding",)))
    assert cards.stats(at=T0)["reviewed_today"] == 1
    assert "cod-0" not in {c["id"] for c in cards.queue(at=T0 + timedelta(minutes=1))}, "scheduling state survives a re-import"
