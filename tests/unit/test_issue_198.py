"""Issue #198: Active now shows the ranked projects with the numbers of `work priority show`, and the web controls run the same handlers."""

import re

import pytest

from careerbrew import actions, config, server
from careerbrew.cli import main
from careerbrew.work import priorities, projects


@pytest.fixture
def ranked(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T10:00")
    priorities.add("PROJ-9 Add retries", 3, 3, 1, [])
    priorities.add("PROJ-10 Add a dashboard", 3, 3, 1, [])
    config.set_value("project_priority", "billing=PROJ-*, tooling=*/agentbrew")


def test_the_active_now_numbers_match_work_priority_show(ranked, capsys):
    rows = server.priority_view()["projects"]
    assert [(r["rank"], r["name"], r["open_tasks"], r["today"]) for r in rows] == [(1, "billing", 2, 0), (2, "tooling", 0, 0)]
    capsys.readouterr()
    assert main(["work", "priority", "show"]) == 0
    out = capsys.readouterr().out
    for r in rows:
        assert re.search(rf"{r['rank']}\. {r['name']}\s.*{r['open_tasks']} open tasks?, {r['today']} items? today", out)


def test_move_and_remove_from_the_web_run_work_priority_set_and_remove(ranked):
    out = actions.run("work.priority.set", {"project": "tooling", "rank": 1})
    assert out["exit"] == 0 and out["argv"][1:4] == ["work", "priority", "set"]
    assert [r["name"] for r in server.priority_view()["projects"]] == ["tooling", "billing"]
    assert config.get("project_priority") == ["tooling=*/agentbrew", "billing=PROJ-*"], "moving keeps the patterns"
    assert actions.run("work.priority.remove", {"project": "billing"})["exit"] == 0
    assert [r["name"] for r in server.priority_view()["projects"]] == ["tooling"]
    assert actions.run("work.priority.set", {"project": "billing", "rank": 1, "match": ["PROJ-*"]})["exit"] == 0, "undo puts it back"
    assert config.get("project_priority") == ["billing=PROJ-*", "tooling=*/agentbrew"]


def test_the_projects_list_puts_ranked_projects_first_with_their_rank(ranked):
    for name in ("payments", "billing", "invoices", "tooling"):
        projects.add(name)
    rows = server.work()["projects"]
    assert [(r["name"], r["rank"]) for r in rows[:2]] == [("billing", 1), ("tooling", 2)]
    assert {r["name"] for r in rows[2:]} == {"payments", "invoices"} and all(r["rank"] is None for r in rows[2:]), "the rest stay, unranked"


def test_with_no_ranking_the_view_is_empty_and_the_projects_keep_their_order(home):
    for name in ("b", "a"):
        projects.add(name)
    assert server.priority_view() == {"projects": []}
    assert [r["name"] for r in server.work()["projects"]] == [r["name"] for r in projects.tree()]


def test_a_ranked_project_moves_with_its_whole_subtree(ranked):
    """Review of #204: a moved parent kept its children behind, nested under another row."""
    projects.add("aaa")
    projects.add("billing")
    projects.add("billing-api", parent="billing")
    projects.add("billing-db", parent="billing-api")
    projects.add("tooling")
    projects.add("tooling-cli", parent="tooling")
    rows = [(r["name"], r["depth"], r["rank"]) for r in server.work()["projects"]]
    assert rows[:5] == [("billing", 0, 1), ("billing-api", 1, None), ("billing-db", 2, None), ("tooling", 0, 2), ("tooling-cli", 1, None)]
    assert rows[5:] == [("aaa", 0, None)]


def test_a_ranked_child_stays_under_its_ranked_parent_and_leads_when_its_parent_is_not_ranked(ranked):
    projects.add("aaa")
    projects.add("billing", parent="aaa")
    projects.add("billing-api", parent="billing")
    projects.add("tooling", parent="billing")
    rows = [(r["name"], r["depth"], r["rank"]) for r in server.work()["projects"]]
    # billing (#1) leads with its subtree rebased to depth 0; tooling (#2) is inside it, so it stays nested there.
    assert rows == [("billing", 0, 1), ("billing-api", 1, None), ("tooling", 1, 2), ("aaa", 0, None)]
