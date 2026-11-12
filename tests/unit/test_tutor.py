from datetime import UTC, datetime, timedelta

import pytest
from fsrs import Rating

from careerbrew import charts
from careerbrew.tutor import engine as tutor


def test_score_to_rating_thresholds():
    assert tutor.score_to_rating(0.4) == Rating.Again
    assert tutor.score_to_rating(0.6) == Rating.Hard
    assert tutor.score_to_rating(0.8) == Rating.Good
    assert tutor.score_to_rating(1.0) == Rating.Easy


def test_new_items_teach_first_then_reviews_come_back_due():
    t0 = datetime(2026, 1, 1, tzinfo=UTC)
    it = tutor.next_item(["system design"], at=t0)
    assert it["mode"] == "learn" and it["topic"] == "system design" and it["teach"]
    row = tutor.record(it["id"], ["requirements"], at=t0)
    assert row["rating"] == "again" and "estimates" in row["missed"]
    again = tutor.next_item(["system design"], at=t0 + timedelta(days=2))
    assert again["id"] == it["id"] and again["mode"] == "review"


def test_mastery_and_readiness_rise_with_good_answers():
    t0 = datetime(2026, 1, 1, tzinfo=UTC)
    assert tutor.readiness(["behavioral"], t0) == 0.0
    full = ["star", "ownership", "result", "tradeoff", "learning"]
    tutor.record("beh-conflict-1", full, at=t0)
    tutor.record("beh-impact-1", full, at=t0)
    assert tutor.readiness(["behavioral"], t0) > 0.9


def test_unknown_criteria_rejected():
    with pytest.raises(ValueError):
        tutor.record("beh-conflict-1", ["vibes"])


def test_feedback_names_missed_criteria():
    row = tutor.record("arrays-two-pointers-1", ["correct"])
    text = " ".join(tutor.feedback(row))
    assert "complexity" in text.lower() and "again" in text


def test_chart_html_is_self_contained_with_tables():
    rep = charts.Report("t")
    rep.bars("b", [("x", 0.5)])
    rep.line("l", [("d1", 0.1), ("d2", 0.9)])
    html = rep.html()
    assert "<svg" in html and "Table view" in html and "prefers-color-scheme:dark" in html
    assert "http://" not in html and "https://" not in html and "<script" not in html
