"""A watchable project list: archive, remove, updates, and `work project list --json` (issues #172 and #173, US-77)."""

import json

import pytest

from careerbrew import cli
from careerbrew.work import projects


@pytest.fixture
def clock(home, monkeypatch):
    def at(iso):
        monkeypatch.setenv("CAREERBREW_NOW", iso)

    at("2026-10-01T09:00+00:00")
    return at


def _cli_json(capsys, *argv):
    assert cli.main(list(argv)) == 0
    return json.loads(capsys.readouterr().out)


def test_archiving_a_project_changes_what_the_active_json_list_returns(clock, capsys):
    projects.add("billing")
    projects.add("ledger")
    assert [p["name"] for p in _cli_json(capsys, "work", "project", "list", "--active", "--json")] == ["billing", "ledger"]
    projects.archive("ledger")
    assert [p["name"] for p in _cli_json(capsys, "work", "project", "list", "--active", "--json")] == ["billing"]
    assert [p["name"] for p in _cli_json(capsys, "work", "project", "list", "--json")] == ["billing"], "active is the default"
    every = _cli_json(capsys, "work", "project", "list", "--all", "--json")
    assert [(p["name"], p["archived"]) for p in every] == [("billing", False), ("ledger", True)]
    assert cli.main(["work", "project", "unarchive", "ledger"]) == 0
    assert "watched again" in capsys.readouterr().out
    assert [p["name"] for p in _cli_json(capsys, "work", "project", "list", "--active", "--json")] == ["billing", "ledger"]


def test_adding_a_project_adds_it_to_the_list_with_its_sources_to_watch(clock, capsys):
    projects.add("billing", stale_after="7d")
    projects.add_source("billing", "tracker", "PROJ-1")
    projects.add_source("billing", "chat", "#billing-team")
    projects.check("billing", "PROJ-1", "epic 3 of 5")
    (p,) = _cli_json(capsys, "work", "project", "list", "--active", "--json")
    assert p["last_checked_at"] == "2026-10-01T09:00:00+00:00" and p["stale"] == 1 and p["stale_after_days"] == 7
    assert [(s["kind"], s["ref"], s["stale"]) for s in p["sources"]] == [("tracker", "PROJ-1", False), ("chat", "#billing-team", True)]
    assert all(s["reader"] for s in p["sources"])


def test_an_archived_project_never_has_stale_sources(clock):
    projects.add("old")
    projects.add_source("old", "doc", "doc-1")
    assert [s["project"] for s in projects.stale_sources()] == ["old"]
    projects.archive("old")
    assert projects.stale_sources() == []


def test_updates_keep_the_latest_and_refuse_a_bad_kind_or_text(clock):
    projects.add("billing")
    for i in range(projects.UPDATES_KEPT + 3):
        projects.record_update("billing", "team", f"change {i}")
    (p,) = projects.watch_list(business=True)
    assert p["latest_update"] == {"at": "2026-10-01T09:00:00+00:00", "kind": "team", "text": f"change {projects.UPDATES_KEPT + 2}"}
    assert len(projects._load()["billing"]["updates"]) == projects.UPDATES_KEPT
    with pytest.raises(projects.ProjectError, match="use one of company, project, team, other"):
        projects.record_update("billing", "gossip", "x")
    with pytest.raises(projects.ProjectError, match="one line"):
        projects.record_update("billing", "team", "two\nlines")
    with pytest.raises(projects.ProjectError, match="needs one line"):
        projects.record_update("billing", "team", "  ")
    with pytest.raises(projects.ProjectError, match="unknown project nope"):
        projects.archive("nope")


def test_without_business_reading_the_list_holds_no_titles_statuses_or_update_text(clock):
    projects.add("billing", title="Billing revamp SYNTHCANARY")
    projects.add_source("billing", "doc", "doc-1")
    projects.check("billing", status="status SYNTHCANARY")
    projects.record_update("billing", "company", "reorg SYNTHCANARY")
    text = json.dumps(projects.watch_list())
    assert "SYNTHCANARY" not in text
    (p,) = projects.watch_list()
    assert p["latest_update"] == {"at": "2026-10-01T09:00:00+00:00", "kind": "company"}
    assert "SYNTHCANARY" in json.dumps(projects.watch_list(business=True))


def test_the_work_api_lists_only_active_projects_to_watch(clock):
    from careerbrew import server

    projects.add("billing")
    projects.add("old")
    projects.archive("old")
    d = server.work()
    assert [p["name"] for p in d["watched"]] == ["billing"]
    assert {p["name"]: p["archived"] for p in d["projects"]} == {"billing": False, "old": True}


def test_remove_needs_yes_and_keeps_the_memory_file(clock, capsys, home):
    notes = home / "work" / "billing-notes.md"
    projects.add("billing", memory=str(notes))
    notes.parent.mkdir(parents=True, exist_ok=True)
    notes.write_text("# notes\n")
    assert cli.main(["work", "project", "remove", "billing"]) == 2
    assert "rerun with --yes" in capsys.readouterr().out and "billing" in projects._load()
    assert cli.main(["work", "project", "remove", "billing", "--yes"]) == 0
    assert "billing" not in projects._load() and notes.read_text() == "# notes\n"


def test_remove_is_refused_while_another_project_names_it(clock):
    projects.add("payments")
    projects.add("billing", parent="payments")
    with pytest.raises(projects.ProjectError, match="billing name payments as a parent or link"):
        projects.remove("payments")
    assert "payments" in projects._load()


def test_the_watcher_skips_an_archived_projects_memory_file(clock, home):
    from careerbrew.work import memory_sync

    projects.add("billing", memory=str(home / "work" / "billing-notes.md"))
    projects.add("old", memory=str(home / "work" / "old-notes.md"))
    projects.archive("old")
    assert [users for _, users in memory_sync._files().values()] == [[("billing", str(home / "work" / "billing-notes.md"))]]


def test_remove_needs_a_confirm_in_the_web_app_too():
    from careerbrew import actions

    assert "work.project.remove" in actions.CONFIRM and "work.project.remove" in actions.YES_FROM_CONFIRM


def test_an_archived_project_is_never_stale_never_asked_about_and_never_matched(clock):
    from careerbrew.day import inbox
    from careerbrew.work import research

    projects.add("old")
    projects.add_source("old", "tracker", "OLD-1")
    asks = lambda: [c for c in inbox.candidates() if c.get("kind") == "memory" and c["data"].get("project") == "old"]  # noqa: E731
    assert asks(), "an active project with no notes file is asked about"
    projects.archive("old")
    assert asks() == []
    assert projects.list_rows()[0]["stale"] == 0 and projects.watch_list()[0]["sources"][0]["stale"] is False
    with pytest.raises(projects.ProjectError, match="no project matches"):
        research.match_project("tracker", "OLD-2")
