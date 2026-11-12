"""Issue #185, part 1: the owner's inbox decisions change later decisions (ranks, the question budget, what plugins learn)."""

import json

import pytest

from careerbrew import config, signals
from careerbrew.cli import main
from careerbrew.day import inbox


@pytest.fixture
def day(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:00")
    return home


def _proposal(n, plugin):
    return {"id": f"inb-{plugin}-{n}", "type": "proposal", "kind": "plugin", "origin": f"plugin:{plugin}", "text": f"{plugin} idea {n}", "why": "w", "data": {"plugin": plugin, "ref": f"r{n}"}}


def _question(n, kind, project):
    return {"id": f"inb-q-{kind}-{project}-{n}", "type": "question", "kind": kind, "text": f"q {n}", "why": "w", "data": {"project": project}}


def _decide(items):
    """Store items as decided, in order, the way accept, dismiss, and answer leave them."""
    rows = []
    for n, (item, status) in enumerate(items):
        rows.append({**item, "status": status, "created_at": f"2026-10-20T09:{n:02d}:00", "decided_at": f"2026-10-20T10:{n:02d}:00"})
    inbox._save(rows)


def test_a_plugin_the_owner_keeps_dismissing_ranks_below_one_with_no_decisions(day):
    _decide([(_proposal(n, "noisy"), "dismissed") for n in range(4)])
    inbox._merge({i["id"]: i for i in (_proposal(9, "noisy"), _proposal(9, "quiet"))})
    order = [i["data"]["plugin"] for i in inbox.open_items() if i["type"] == "proposal"]
    assert order == ["quiet", "noisy"]
    assert signals.weights()[("plugin", "plugin:noisy")] == pytest.approx(0.333, abs=0.001)


def test_a_question_kind_skipped_three_times_for_one_project_is_not_asked_again(day):
    _decide([(_question(n, "competency", "billing"), "dismissed") for n in range(3)])
    inbox._merge({q["id"]: q for q in (_question(9, "competency", "billing"), _question(9, "competency", "ledger"))})
    asked = {i["data"]["project"] for i in inbox.load() if i["status"] == "open"}
    assert asked == {"ledger"}, "billing's competency questions stop; ledger's still come"


def test_an_answer_ends_a_run_of_skips(day):
    _decide([(_question(0, "memory", "billing"), "dismissed"), (_question(1, "memory", "billing"), "dismissed"), (_question(2, "memory", "billing"), "answered"), (_question(3, "memory", "billing"), "dismissed")])
    inbox._merge({q["id"]: q for q in (_question(9, "memory", "billing"),)})
    assert any(i["status"] == "open" for i in inbox.load())


def test_the_question_budget_goes_first_to_the_kinds_the_owner_answers(day, monkeypatch):
    monkeypatch.setattr(config, "get", lambda k: 1 if k == "questions_per_day" else config.DEFAULTS[k])
    _decide([(_question(0, "impact", "billing"), "answered"), (_question(1, "competency", "ledger"), "dismissed"), (_question(2, "competency", "ledger"), "dismissed")])
    inbox._merge({q["id"]: q for q in (_question(9, "competency", "ledger"), _question(9, "impact", "billing"))})
    assert [i["kind"] for i in inbox.load() if i["status"] == "open"] == ["impact"]


def test_a_plugin_reads_back_the_weight_its_next_suggestions_get(day, capsys):
    _decide([(_proposal(0, "acme"), "accepted"), (_proposal(1, "acme"), "dismissed"), (_proposal(2, "acme"), "dismissed"), (_proposal(3, "acme"), "dismissed")])
    assert signals.plugin_rates("acme") == {"accepted": 1, "dismissed": 3, "weight": 0.667}


def test_accept_dismiss_and_answer_stamp_when_they_decided(day):
    inbox._save([{**_proposal(1, "acme"), "status": "open"}, {**_question(1, "impact", "billing"), "status": "open", "data": {"ref": "x", "project": "billing"}}])
    assert inbox.dismiss("inb-acme-1")["decided_at"].startswith("2026-10-21T10:00:00")


def test_signals_say_what_each_decision_changed(day, capsys, monkeypatch):
    monkeypatch.setattr("careerbrew.cli.may_show_business", lambda: True)  # an owner's terminal; agents see counts only
    _decide([(_proposal(n, "noisy"), "dismissed") for n in range(4)] + [(_question(n, "competency", "billing"), "dismissed") for n in range(3)])
    assert main(["signals"]) == 0
    out = capsys.readouterr().out
    assert "plugin       from plugin:noisy: 0 accepted, 4 dismissed -> plugin items from plugin:noisy rank lower (weight 0.333)" in out
    assert "no new competency questions about project:billing for 14 days after you skipped the last 3 (undo: careerbrew signals reset competency project:billing)" in out
    assert main(["signals", "--json"]) == 0
    rows = json.loads(capsys.readouterr().out)
    assert {r["kind"] for r in rows} == {"plugin", "competency"}
    assert main(["signals"]) == 0 and "change what careerbrew does now" in capsys.readouterr().out


def test_no_decisions_change_nothing(day, capsys):
    inbox._merge({i["id"]: i for i in (_proposal(1, "b"), _proposal(1, "a"))})
    assert [i["data"]["plugin"] for i in inbox.open_items()] == ["b", "a"], "with no decisions the order is unchanged"
    assert main(["signals"]) == 0 and "no decisions yet" in capsys.readouterr().out


def _impact(n, tool):
    return {"id": f"inb-impact-{tool}-{n}", "type": "question", "kind": "impact", "text": f"impact {n}", "why": "w", "data": {"title": f"t{n}", "source": tool}}


def test_skipping_one_tools_impact_questions_mutes_only_that_tool(day):
    """Three skipped impact questions about GitHub PRs must not silence the ones about Jira tickets."""
    _decide([(_impact(n, "github"), "dismissed") for n in range(3)])
    inbox._merge({q["id"]: q for q in (_impact(9, "github"), _impact(9, "jira"))})
    assert [i["data"]["source"] for i in inbox.load() if i["status"] == "open"] == ["jira"]


def test_each_question_kind_has_a_source_of_its_own():
    assert signals.source({"kind": "impact", "data": {"title": "x", "source": "github"}}) == "github"
    assert signals.source({"kind": "project", "data": {"kind": "repo", "ref": "acme/shop"}}) == "repo"
    assert signals.source({"kind": "competency", "data": {"task": "t1", "source": "agent"}}) == "agent"
    assert signals.source({"kind": "memory", "data": {"project": "billing"}}) == "project:billing"
    assert signals.source({"kind": "plugin", "origin": "plugin:acme", "data": {"project": "billing"}}) == "plugin:acme"


def test_a_mute_ends_after_its_days_and_one_question_comes_through(day, monkeypatch):
    _decide([(_question(n, "competency", "billing"), "dismissed") for n in range(3)])  # last skip 2026-10-20
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T10:00")  # 16 days later: past MUTE_DAYS
    inbox._merge({q["id"]: q for q in (_question(9, "competency", "billing"),)})
    assert [i["id"] for i in inbox.load() if i["status"] == "open"] == ["inb-q-competency-billing-9"]


def test_reset_forgets_a_signal_and_brings_its_questions_back(day, capsys):
    _decide([(_question(n, "competency", "billing"), "dismissed") for n in range(3)])
    assert main(["signals", "reset", "competency", "project:billing"]) == 0
    assert "3 decision(s) forgotten" in capsys.readouterr().out
    assert signals.weights() == {}
    inbox._merge({q["id"]: q for q in (_question(9, "competency", "billing"),)})
    assert any(i["status"] == "open" for i in inbox.load())
    with pytest.raises(ValueError, match="no signal for nope"):
        signals.reset("nope", "x")
    assert main(["signals", "reset"]) == 2


def test_one_dismiss_all_counts_as_one_decision(day):
    rows = [{**_proposal(n, "acme"), "status": "dismissed", "created_at": "2026-10-20T09:00:00", "decided_at": "2026-10-20T10:00:00", "decided_group": "grp-1@2026-10-20T10:00:00"} for n in range(10)]
    inbox._save(rows)
    assert signals.plugin_rates("acme") == {"accepted": 0, "dismissed": 1, "weight": 0.667}


def test_old_decisions_fade_after_the_window(day, monkeypatch):
    _decide([(_proposal(n, "acme"), "dismissed") for n in range(5)])
    monkeypatch.setenv("CAREERBREW_NOW", "2027-02-01T10:00")  # over 90 days later
    assert signals.plugin_rates("acme")["weight"] == 1.0


def test_pr_and_github_impact_questions_are_one_source(day):
    assert signals.key(_impact(1, "pr")) == signals.key(_impact(1, "github")) == ("impact", "github")


def test_accept_and_answer_stamp_when_they_decided_too(day):
    inbox._save([{**_proposal(1, "acme"), "status": "open"}])
    assert inbox.accept("inb-acme-1")["decided_at"].startswith("2026-10-21T10:00:00")
    inbox._save([{"id": "inb-q", "type": "question", "kind": "impact", "text": "q", "why": "w", "data": {"title": "t", "source": "github"}, "status": "open"}])
    assert inbox.answer("inb-q", "cut p99 by 30%")["decided_at"].startswith("2026-10-21T10:00:00")


def test_expired_and_reopened_items_are_not_decisions(day):
    inbox._save([{**_proposal(1, "acme"), "status": "expired"}, {**_proposal(2, "acme"), "status": "open", "keep": True}])
    assert signals.weights() == {}
