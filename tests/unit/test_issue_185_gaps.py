"""Issue #185, part 4: smaller gaps. Task grades, per-project calibration, snoozes, approval notes, and rating tags."""

import pytest

from careerbrew import approvals, notify, store
from careerbrew.cli import main
from careerbrew.day import planner
from careerbrew.jobs import want
from careerbrew.jobs.sources import Posting
from careerbrew.work import priorities, projects


def test_the_owner_regrades_an_accepted_task(home, capsys):
    t = priorities.add("Review the roadmap doc", 3, 3, 1.0, [], source="proposal", why="a plugin suggested it")
    assert main(["work", "task-set", t["id"], "--impact", "5", "--hours", "2.5"]) == 0
    got = next(x for x in priorities.load() if x["id"] == t["id"])
    assert (got["impact"], got["urgency"], got["estimate_h"]) == (5, 3, 2.5)
    with pytest.raises(ValueError, match="impact is 1 to 5"):
        priorities.regrade(t["id"], impact=9)
    capsys.readouterr()
    assert main(["work", "task-set", t["id"]]) == 2 and "nothing to change" in capsys.readouterr().out


def _ratio(task, value, source="user"):
    store.append_jsonl(planner.paths.sub("metrics.jsonl"), [{"metric": "estimate_ratio", "value": value, "at": "2026-10-20T10:00:00", "task": task, "source": source}])


def test_calibration_uses_the_tasks_project_then_its_source_then_everyones(home):
    projects.add("billing")
    ids = [priorities.add(f"billing {n}", 3, 3, 1.0, [])["id"] for n in range(4)]
    for i in ids[:3]:
        projects.link_task("billing", i)
        _ratio(i, 2.0)
    for n in range(5):
        _ratio(f"t-other-{n}", 1.0)
    new_billing = {"id": ids[3], "source": "user"}
    projects.link_task("billing", ids[3])
    assert planner.calibration(new_billing) == 2.0, "billing's own three finished tasks"
    assert planner.calibration({"id": "t-x", "source": "user"}) == 1.0, "no project: everyone's median"
    for n in range(3):
        _ratio(f"t-agent-{n}", 1.5, source="agent")
    assert planner.calibration({"id": "t-y", "source": "agent"}) == 1.5, "an agent's tasks run long"
    assert planner.calibration() == 1.5, "everyone's median: 5 at 1.0, 3 at 1.5, 3 at 2.0"


def test_done_records_the_task_and_its_source_for_calibration(home):
    t = priorities.add("x", 3, 3, 1.0, [], source="agent", why="CI failed")
    priorities.done(t["id"], 2.0)
    (row,) = [r for r in store.read_jsonl(planner.paths.sub("metrics.jsonl")) if r["metric"] == "estimate_ratio"]
    assert (row["task"], row["source"]) == (t["id"], "agent")


def test_three_snoozes_of_one_kind_lower_its_priority(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:00")
    items = [{"id": f"n-{n}", "kind": "dm", "sub": "dm", "status": "new", "snoozes": [f"2026-10-2{n}T09:00:00-04:00"]} for n in range(3)]
    store.write_json(notify.path(), {"items": items, "popups": []})
    assert notify._snoozed_kinds(notify.calendar.now()) == {"dm": 3}
    monkeypatch.setattr(notify, "_snoozed_kinds", lambda now: {"dm": 3})
    from careerbrew.work import feed

    monkeypatch.setattr(feed, "open_asks", lambda: [{"id": "slack:D1:1", "thread": "slack:D1", "at": "2026-10-21T13:00:00Z", "source": "slack", "kind": "dm", "title": "DM from sam", "to_me": True}])
    (c,) = [c for c in notify.candidates() if c["kind"] != "suggestion"]
    assert "-1: you snoozed 3 like it" in c["priority_why"]


def test_a_snooze_is_counted_on_the_notification(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:00")
    store.write_json(notify.path(), {"items": [{"id": "n-1", "kind": "dm", "sub": "dm", "status": "new", "at": "2026-10-21T13:00:00Z"}], "popups": []})
    notify.act("n-1", "snooze")
    assert len(notify.load()["items"][0]["snoozes"]) == 1


def test_approval_notes_are_guidance_agents_read_when_they_ask(home, capsys):
    a = approvals.request("Open a PR on the billing repo")
    approvals.decide(a["id"], "rejected", "never open PRs on repos outside acme/; ask first")
    assert approvals.guidance()[0]["note"].startswith("never open PRs")
    assert main(["approvals", "request", "Open a PR on the ledger repo"]) == 0
    out = capsys.readouterr().out
    assert "the owner's notes on past requests" in out and "never open PRs on repos outside acme/" in out
    assert main(["approvals", "guidance"]) == 0 and "1 note(s)" in capsys.readouterr().out


def _posting(i, title):
    return Posting(id=f"p{i}", company="Northwind", title=title, location="Remote", remote=True, url="", description="")


def test_a_tag_on_two_low_ratings_lowers_matching_postings(home):
    want.rate("p1", 1, ["crypto"])
    want.rate("p2", 2, ["crypto", "on-call"])
    assert want.tag_words() == ({"crypto"}, set())
    assert want.tag_factor(_posting(3, "Senior Crypto Engineer")) == 0.85
    assert want.tag_factor(_posting(4, "Platform Engineer")) == 1.0
    w, prof, crit = want.weights(), {}, {}
    assert want.score(_posting(3, "Senior Crypto Engineer"), w, prof, crit)[0] < want.score(_posting(4, "Senior Platform Engineer"), w, prof, crit)[0]


def test_every_fifth_rating_refits_the_want_weights(home, monkeypatch, capsys):
    from careerbrew.jobs import sources

    monkeypatch.setattr(sources, "load", lambda: [_posting(n, f"Engineer {n}") for n in range(6)])
    calls = []
    monkeypatch.setattr(want, "learn_weights", lambda *a, **k: calls.append(1) or {"status": "kept", "before": 0.1, "after": 0.6})
    for n in range(5):
        assert main(["jobs", "rate", f"p{n}", str(1 + n % 5)]) == 0
    assert calls == [1] and "your Want weights were refit (agreement 0.1 -> 0.6)" in capsys.readouterr().out


def test_tags_match_whole_words_and_a_posting_counts_once(home):
    want.rate("p1", 1, ["go"])
    want.rate("p2", 2, ["go"])
    assert want.tag_factor(_posting(3, "Senior Engineer at Google")) == 1.0, "go is not inside Google"
    assert want.tag_factor(_posting(4, "Go backend engineer")) == 0.85
    want.rate("p5", 1, ["ai"])
    want.rate("p5", 1, ["ai"])  # one posting rated twice
    assert "ai" not in want.tag_words()[0]
