"""Midday review inventory through the ordinary work-day watch pass."""

import json
import shlex

import pytest
import yaml

from careerbrew import cli, plugins
from careerbrew.work import review_ask
from careerbrew.workday import gate, questions, review_cycle, watch

URL = "https://github.com/example/review-demo/pull/41"
READY = {
    "url": URL,
    "owned": True,
    "open": True,
    "draft": False,
    "checks_green": True,
    "mergeable": True,
    "threads_resolved": True,
    "reviewer_requested": False,
    "review_submitted": False,
}


@pytest.fixture
def inventory(tmp_path, home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-04T15:00+00:00")
    cli.main(["init"])
    root = tmp_path / "review-plugin"
    root.mkdir()
    data = root / "inventory.json"
    (root / plugins.MANIFEST).write_text(
        yaml.safe_dump(
            {
                "name": "review-plugin",
                "version": "1.0.0",
                "work_steps": [{"step": "/review-prs", "review_ask_cycle": f"cat {shlex.quote(str(data))}"}],
            }
        )
    )
    plugins.add(str(root), allow_exec=True)

    def run(*rows, complete=True):
        data.write_text(json.dumps({"complete": complete, "pull_requests": rows}))
        return watch.run_pass()

    return run


def test_ready_transition_once_across_passes_and_days(inventory, monkeypatch):
    monkeypatch.setattr(review_ask, "recheck_pr", lambda _: ("open", "still open"))
    inventory(dict(READY, checks_green=False))
    assert questions.open_records() == []
    result = inventory(READY, READY)
    (first,) = questions.open_records()
    assert first["recheck"] == f"careerbrew:review-pr {URL}"
    assert URL not in json.dumps(result)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T15:00+00:00")
    inventory(dict(READY, url=URL.replace("/example/", "/EXAMPLE/")))
    (repeated,) = questions.open_records()
    assert repeated["id"] == first["id"]
    assert repeated["createdAt"] == first["createdAt"]
    monkeypatch.setattr(review_ask, "recheck_pr", lambda _: ("closed", "reviewer requested"))
    inventory()
    assert questions.open_records() == []


@pytest.mark.parametrize("field", [key for key in READY if key != "url"])
def test_missing_and_ineligible_facts_do_not_offer(inventory, field):
    inventory({key: value for key, value in READY.items() if key != field})
    assert questions.open_records() == []
    inventory(dict(READY, **{field: not READY[field]}))
    assert questions.open_records() == []


def test_incomplete_inventory_creates_no_question(inventory):
    result = inventory(READY, complete=False)
    row = next(row for row in result["steps"] if row["step"] == "/review-prs")
    assert row["result"] == "failed"
    assert questions.open_records() == []


def test_unapproved_plugin_cannot_read_inventory(inventory, monkeypatch):
    monkeypatch.setattr(gate, "may_run", lambda _: False)

    def unexpected(*args):
        pytest.fail("review inventory ran without permission")

    monkeypatch.setattr(review_cycle, "_inventory", unexpected)
    result = inventory(READY)
    row = next(row for row in result["steps"] if row["step"] == "/review-prs")
    assert row["result"] == "blocked"
    assert questions.open_records() == []


def test_existing_manual_question_and_conflicting_rows(inventory):
    inventory(READY, dict(READY, checks_green=False))
    assert questions.open_records() == []
    manual = questions.add("/review-prs", "Review this PR?", recheck=f"careerbrew:review-pr {URL}")
    inventory(READY)
    (only,) = questions.open_records()
    assert only["id"] == manual["id"]
