"""Standup speaking bullets keep their structure across the report exits."""

import json
from html.parser import HTMLParser

import pytest

from careerbrew import store
from careerbrew.work import evidence, standup

POINTS = ["Shipped the API (2 PRs)", "Next: the CSV format", "Ask: a review from the data team"]


@pytest.fixture
def rows(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    standup.save_rows(json.dumps([{"title": "Order export", "owner": "me", "aliases": ["Order export"]}]))


def test_three_talking_bullets_reach_the_report_and_html_as_three_items(rows):
    standup.save_notes(talk=json.dumps({"Order export": POINTS}))
    report = standup.build()
    row = report["rows"][0]
    assert row["talking_points"] == POINTS
    assert row["talking_point"] == " ".join(POINTS)
    page = standup.render(report)

    class TalkItems(HTMLParser):
        def __init__(self):
            super().__init__()
            self.inside = False
            self.items = 0

        def handle_starttag(self, tag, attrs):
            if tag == "ul" and ("class", "talk") in attrs:
                self.inside = True
            if tag == "li" and self.inside:
                self.items += 1

        def handle_endtag(self, tag):
            if tag == "ul":
                self.inside = False

    parser = TalkItems()
    parser.feed(page)
    assert parser.items == 3
    assert all(point in page for point in POINTS)


def test_a_string_is_one_folded_bullet_and_keeps_paragraph_display(rows):
    standup.save_notes(talk=json.dumps({"Order export": "  Shipped\n the API.  "}))
    report = standup.build()
    assert report["rows"][0]["talking_points"] == ["Shipped the API."]
    assert '<p class="talk">Shipped the API.' in standup.render(report)


@pytest.mark.parametrize("value", [[], ["one"] * 6, ["x" * 201], "x" * 201, [""], ["one", 2]])
def test_invalid_bullet_lists_or_long_strings_are_refused_with_a_fix(rows, value):
    with pytest.raises(standup.StandupError, match="fix:"):
        standup.save_notes(talk=json.dumps({"Order export": value}))


def test_each_bullet_folds_whitespace_and_accepts_the_boundaries(rows):
    standup.save_notes(talk=json.dumps({"Order export": [" one\n two ", *["x" * 200 for _ in range(4)]]}))
    assert standup.build()["rows"][0]["talking_points"] == ["one two", *["x" * 200 for _ in range(4)]]


def test_generated_fallback_keeps_three_latest_evidence_items_as_separate_bullets(rows):
    store.write_jsonl(evidence.evidence_path(), [{"id": f"note-{n}", "kind": "note", "title": f"Order export: shipped stage {n}", "at": f"2026-10-16T14:0{n}:00-04:00", "tags": [], "origin": "user"} for n in range(4)])
    points = standup.build()["rows"][0]["talking_points"]
    assert len(points) == 3
    assert [f"stage {n}" in points[i] for i, n in enumerate((3, 2, 1))] == [True, True, True]
    assert all("stage 0" not in point for point in points)


def test_bullets_do_not_bypass_a_failed_proof_gate(rows):
    standup.save_notes(talk=json.dumps({"Order export": POINTS}), proof=json.dumps({"Order export": {"verdict": "fail", "proofs": [], "reason": "missing live proof"}}))
    report = standup.build()
    assert report["rows"] == [] and report["counts"]["failed"] == 1
