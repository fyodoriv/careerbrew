"""Projects absorb links and research on checkup (issue #34)."""

from pathlib import Path

import pytest
import yaml

from careerbrew.work import projects, research

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "http"


@pytest.fixture
def conn(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIX))
    monkeypatch.setenv("CB_T", "synthetic")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-07T09:00+00:00")
    (home / "work").mkdir(parents=True, exist_ok=True)
    from careerbrew import store

    store.write_yaml(
        home / "work" / "connectors.yaml",
        {
            "connectors": [
                {"kind": "jira", "token": "env:CB_T", "base_url": "https://jira.example.com", "email": "me@example.com"},
                {"kind": "github", "token": "env:CB_T", "user": "me"},
                {"kind": "gdocs", "token": "env:CB_T"},
                {"kind": "slack", "token": "env:CB_T"},
            ]
        },
    )
    return home


@pytest.mark.parametrize(
    ("ref", "kind", "norm"),
    [
        ("t001", "task", "t001"),
        ("#billing-eng", "chat", "#billing-eng"),
        ("PROJ-7", "tracker", "PROJ-7"),
        ("https://tracker.example.com/browse/PROJ-9", "tracker", "PROJ-9"),
        ("https://github.com/acme/billing/pull/41", "url", "https://github.com/acme/billing/pull/41"),
        ("https://docs.google.com/document/d/doc-prd-1/edit", "doc", "doc-prd-1"),
        ("https://example.com/post", "url", "https://example.com/post"),
        ("acme/billing", "repo", "acme/billing"),
    ],
)
def test_kind_detection(ref, kind, norm):
    assert research.detect(ref) == (kind, norm)


def test_matching_picks_the_closest_project_and_refuses_a_tie(conn):
    projects.add("billing")
    projects.add_source("billing", "tracker", "PROJ-1")
    projects.add("search")
    projects.add_source("search", "repo", "acme/search")
    assert research.match_project("tracker", "PROJ-9") == "billing"
    projects.add_source("search", "tracker", "PROJ-2")
    with pytest.raises(projects.ProjectError, match="billing, search equally"):
        research.match_project("tracker", "PROJ-9")


def test_disallowed_read_stores_the_ref_only(conn):
    projects.add("billing", memory="work/billing-notes.md")
    projects.add_source("billing", "tracker", "PROJ-1")
    r = research.absorb("https://github.com/acme/billing/pull/41", "billing", read_allowed=False)
    assert not r["read"] and not r["memory"] and "ref and date only" in r["reason"]
    (src,) = [s for s in projects.show("billing")["sources"] if s["ref"] == "https://github.com/acme/billing/pull/41"]
    assert src["checked_at"] and not (conn / "work" / "billing-notes.md").exists()


def test_absorb_reads_and_writes_dated_notes_to_memory(conn):
    projects.add("billing", memory="work/billing-notes.md")
    r = research.absorb("https://docs.google.com/document/d/doc-prd-1/edit", "billing")
    assert r["read"] and r["memory"]
    from careerbrew import store

    notes = store.read_text(conn / "work" / "billing-notes.md")
    assert "Billing migration PRD" in notes and "cut over on 2026-11-15" in notes and "typo fixed" not in notes


def test_refresh_reads_stale_sources_proposes_children_and_unreadable_stays_stale(conn):
    projects.add("billing", memory="work/billing-notes.md")
    projects.add_source("billing", "tracker", "PROJ-7")
    projects.add_source("billing", "url", "https://unknown-host.example.com/x")
    r = research.refresh()
    assert r["refreshed"] == 2 and r["still_stale"] == 1, "memory (empty file) and the tracker refresh; the url cannot be read"
    shown = projects.show("billing")
    bad = next(s for s in shown["sources"] if s["kind"] == "url")
    assert bad["stale"] and bad["stale_reason"]
    assert {p["ref"] for p in shown["proposed"]} >= {"PROJ-101"}, "child issues are proposed, not added"


def test_record_keeps_refs_not_summaries(conn):
    projects.add("billing", memory="work/billing-notes.md")
    research.absorb("https://docs.google.com/document/d/doc-prd-1/edit", "billing")
    from careerbrew import store

    rec = yaml.safe_dump(store.read_yaml(projects.path()))
    assert "cut over" not in rec
