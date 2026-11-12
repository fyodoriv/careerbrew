"""Issue #71: an agent task shows in the inbox at once, and a plugin adds its own explained suggestion."""

import pytest

from careerbrew import connectors, plugins
from careerbrew.cli import main
from careerbrew.day import inbox
from careerbrew.work import priorities

SUGGEST = [
    "day",
    "suggest",
    "--plugin",
    "acme",
    "--ref",
    "doc-7",
    "--what",
    "Review the synthetic roadmap doc",
    "--why",
    "the roadmap doc changed and names you as a reviewer",
    "--accept",
    "Plans time to review it before Friday",
]


@pytest.fixture
def offline(monkeypatch):
    """Showing a new item must not wait on a connector: any HTTP call fails the test."""

    def no_http(*_a, **_k):
        raise AssertionError("the inbox called a connector to show one new item")

    monkeypatch.setattr(connectors, "http_json", no_http)


@pytest.fixture
def acme(acme_plugin, offline):
    plugins.add(str(acme_plugin))


def test_agent_task_shows_in_the_inbox_at_once(home, monkeypatch, offline):
    monkeypatch.delenv("CAREERBREW_TEST_OWNER", raising=False)
    assert main(["work", "task", "Refactor billing", "--source", "agent", "--why", "PR review asked for it"]) == 0
    t = priorities.load()[0]
    items = [i for i in inbox.open_items() if i["kind"] == "agent-task"]
    assert [i["data"]["task"] for i in items] == [t["id"]], "no watcher cycle needed"
    inbox.regenerate()
    assert [i["id"] for i in inbox.open_items() if i["kind"] == "agent-task"] == [items[0]["id"]], "the next cycle keeps it once"


def test_plugin_suggestion_has_its_own_what_why_and_accept(home, acme, capsys):
    assert main([*SUGGEST, "--task", "Review the synthetic roadmap doc"]) == 0
    out = capsys.readouterr().out
    assert "waits in the inbox" in out and "roadmap" not in out, "the reply names the id, never the content"
    (item,) = [i for i in inbox.open_items() if i["kind"] == "plugin"]
    assert item["what"] == "Review the synthetic roadmap doc"
    assert item["why"] == "the roadmap doc changed and names you as a reviewer"
    assert item["accept"].startswith("Plans time to review it before Friday")
    assert 'adds task "Review the synthetic roadmap doc" to Week' in item["accept"], "careerbrew says what it really changes"
    assert "plugin acme" in item["text"] and "does not come back" in item["dismiss"]
    assert priorities.load() == [], "no fake task while the suggestion waits"
    inbox.accept(item["id"])
    (t,) = priorities.load()
    assert t["title"] == "Review the synthetic roadmap doc" and t["status"] == "open" and "plugin acme" in t["why"]


def test_a_suggestion_without_a_task_only_records_the_decision(home, acme, capsys):
    assert main(SUGGEST) == 0
    (item,) = [i for i in inbox.open_items() if i["kind"] == "plugin"]
    assert "careerbrew changes nothing else" in item["accept"]
    inbox.accept(item["id"])
    assert priorities.load() == []
    capsys.readouterr()
    assert main(["day", "suggestions", "--plugin", "acme"]) == 0
    out = capsys.readouterr().out
    assert "doc-7" in out and "accepted" in out, "the plugin reads your decision back"


def test_same_ref_never_duplicates_and_a_dismissal_sticks(home, acme, capsys):
    assert main(SUGGEST) == 0
    assert main(SUGGEST) == 0
    (item,) = [i for i in inbox.open_items() if i["kind"] == "plugin"]
    inbox.regenerate()
    assert [i["id"] for i in inbox.open_items() if i["kind"] == "plugin"] == [item["id"]], "a cycle keeps an open suggestion"
    inbox.dismiss(item["id"])
    assert main(SUGGEST) == 0
    inbox.regenerate()
    assert not [i for i in inbox.open_items() if i["kind"] == "plugin"], "a dismissed suggestion does not come back"
    capsys.readouterr()
    assert main(["day", "suggestions", "--plugin", "acme"]) == 0
    assert "dismissed" in capsys.readouterr().out


def test_only_an_installed_plugin_can_suggest(home, offline, capsys):
    assert main(SUGGEST) == 2
    assert "not installed" in capsys.readouterr().out
    assert not inbox.open_items()


def test_removing_the_plugin_removes_its_suggestions(home, acme, capsys):
    assert main(SUGGEST) == 0
    plugins.remove("acme", purge_data=True)
    assert not [i for i in inbox.load() if i["kind"] == "plugin"], "purge deletes open and decided suggestions"
    capsys.readouterr()
    assert main(["day", "suggestions"]) == 0
    assert "doc-7" not in capsys.readouterr().out


def test_an_uninstalled_plugin_s_open_suggestions_drop_at_the_next_cycle(home, acme):
    assert main(SUGGEST) == 0
    plugins.remove("acme")
    inbox.regenerate()
    assert not [i for i in inbox.open_items() if i["kind"] == "plugin"]
