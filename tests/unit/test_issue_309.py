"""Project source references normalize before storage and before refresh/sync."""

import pytest

from careerbrew import store
from careerbrew.work import memory_sync, projects, research


@pytest.mark.parametrize(
    ("kind", "raw", "norm"),
    [
        ("tracker", "https://jira.example.com/browse/PROJ-7", "PROJ-7"),
        ("repo", "https://github.com/acme/billing/tree/main/src", "acme/billing"),
        ("chat", "#C0123", "C0123"),
        ("chat", "#billing-eng", "#billing-eng"),
    ],
)
def test_source_add_normalizes_and_compares_canonical_refs(home, kind, raw, norm):
    projects.add("billing")
    projects.add_source("billing", kind, raw)
    assert projects.records()["billing"]["sources"][0]["ref"] == norm
    with pytest.raises(projects.ProjectError, match="already"):
        projects.add_source("billing", kind, norm)


def test_item_link_cannot_be_added_as_a_repository(home):
    projects.add("billing")
    with pytest.raises(projects.ProjectError, match="url"):
        projects.add_source("billing", "repo", "https://github.com/acme/billing/pull/41")
    assert projects.records()["billing"]["sources"] == []


def _old_sources():
    projects.add("billing")
    every = projects.records()
    every["billing"]["sources"] = [
        {"kind": "tracker", "ref": "https://jira.example.com/browse/PROJ-7", "checked_at": None, "status": "", "reference": True},
        {"kind": "tracker", "ref": "PROJ-7", "checked_at": None, "status": ""},
        {"kind": "chat", "ref": "#C0123", "checked_at": None, "status": ""},
        {"kind": "chat", "ref": "C0123", "checked_at": None, "status": ""},
        {"kind": "repo", "ref": "https://github.com/acme/billing/tree/main/src", "checked_at": None, "status": ""},
        {"kind": "repo", "ref": "acme/billing", "checked_at": None, "status": ""},
    ]
    store.write_yaml(projects.path(), {"projects": every})


@pytest.mark.parametrize("operation", ["refresh", "sync"])
def test_existing_sources_are_repaired_and_deduplicated_without_losing_reference_choice(home, monkeypatch, operation):
    _old_sources()
    if operation == "refresh":
        monkeypatch.setattr(research, "read", lambda *args: {"title": "channel", "notes": [], "proposed": [], "changed": 0})
        research.refresh()
    else:
        memory_sync.run(allowed=False)
    sources = projects.records()["billing"]["sources"]
    assert [(s["kind"], s["ref"]) for s in sources] == [("tracker", "PROJ-7"), ("chat", "C0123"), ("repo", "acme/billing")]
    assert sources[0]["reference"] is True
    before = projects.path().read_bytes()
    memory_sync.run(allowed=False)
    assert projects.path().read_bytes() == before


def test_promise_tracker_source_is_normalized_but_stays_a_reference(home):
    projects.add_promise("promise", "Send update", "", "https://jira.example.com/browse/PROJ-7", "tracker", {})
    source = projects.records()["promise"]["sources"][0]
    assert source["ref"] == "PROJ-7" and source["reference"] is True


def test_absorb_channel_id_uses_the_same_canonical_ref(home):
    projects.add("billing")
    got = research.absorb("#C0123", "billing", read_allowed=False)
    assert got["ref"] == "C0123" and not got["read"]


def test_a_malformed_legacy_link_does_not_abort_other_source_repairs(home):
    _old_sources()
    every = projects.records()
    every["billing"]["sources"].append({"kind": "url", "ref": "https://[bad", "checked_at": None, "status": ""})
    store.write_yaml(projects.path(), {"projects": every})
    memory_sync.run(allowed=False)
    sources = projects.records()["billing"]["sources"]
    assert sources[0]["ref"] == "PROJ-7"
    assert sources[-1]["ref"] == "https://[bad"


def test_reference_only_chat_promise_keeps_its_exact_thread_link_during_migration(home):
    link = "https://example.slack.com/archives/C0123/p123"
    projects.add_promise("promise", "Send update", "", link, "chat", {})
    projects.normalize_sources()
    assert projects.records()["promise"]["sources"][0]["ref"] == link
