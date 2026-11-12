"""One GitHub item never becomes a repository-wide project source."""

import pytest

from careerbrew import store
from careerbrew.work import projects, research


def test_source_lookup_scope_keeps_reference_url_target_exact(home):
    projects.add("billing")
    ref = "https://github.com/acme/billing/pull/41#discussion_r1"
    projects.add_source("billing", "url", ref)
    every = projects.records()
    every["billing"]["sources"][0].update(ref=ref, reference=True)
    store.write_yaml(home / "work" / "projects.yaml", {"projects": every})
    with pytest.raises(projects.ProjectError, match="has no source"):
        projects.set_reference("billing", ref.replace("r1", "r2"), False)
    assert projects.records() == every


def test_source_lookup_scope_prefers_an_exact_saved_url(home):
    projects.add("billing")
    ref = "https://docs.google.com/document/d/doc-a/edit"
    projects.add_source("billing", "url", ref)
    projects.set_reference("billing", ref)
    projects.add_source("billing", "doc", "doc-a")
    assert projects.check("billing", ref, "Reviewed exact reference") == 1
    source = next(s for s in projects.records()["billing"]["sources"] if s["kind"] == "doc")
    assert source["checked_at"] is None and source["status"] == ""


def test_alias_selector_refuses_ambiguity_between_memory_and_doc(home, tmp_path):
    target = tmp_path / "notes.md"
    target.write_text("# Synthetic project notes\n")
    projects.add("billing", memory=str(target))
    projects.add_source("billing", "doc", str(target))
    before = projects.records()
    with pytest.raises(projects.ProjectError, match="multiple source kinds"):
        projects.check("billing", f" {target} ")
    assert projects.records() == before


@pytest.mark.parametrize("action", ["check", "since", "reference", "unreadable"])
def test_source_lookup_alias_uses_the_saved_canonical_source(home, tmp_path, action):
    folder = tmp_path / "documents"
    folder.mkdir()
    target = folder / "kickoff.md"
    target.write_text("# Synthetic kickoff\n")
    alias = tmp_path / "shortcut"
    alias.symlink_to(folder, target_is_directory=True)
    ref = str(alias / target.name)
    projects.add("billing")
    projects.add_source("billing", "doc", ref)
    assert projects.records()["billing"]["sources"][0]["ref"] == str(target.resolve())
    if action == "check":
        assert projects.check("billing", ref, "Reviewed") == 1
    elif action == "since":
        rows = research.changes("billing", ref, read_allowed=False)
        assert len(rows) == 1 and rows[0]["ref"] == str(target.resolve())
    elif action == "reference":
        assert projects.set_reference("billing", ref) == 1
    else:
        projects.mark_unreadable("billing", ref, "Synthetic unavailable")
    source = projects.records()["billing"]["sources"][0]
    assert len(projects.records()["billing"]["sources"]) == 1
    if action == "check":
        assert source["checked_at"] and source["status"] == "Reviewed"
    elif action == "since":
        assert source["checked_at"] is None
    elif action == "reference":
        assert source["reference"] is True
    else:
        assert source["stale_reason"] == "Synthetic unavailable"


@pytest.mark.parametrize("action", ["check", "since", "reference", "unreadable"])
def test_missing_target_alias_selects_the_saved_document(home, tmp_path, monkeypatch, action):
    folder = tmp_path / "documents"
    folder.mkdir()
    target = folder / "kickoff.md"
    target.write_text("# Synthetic kickoff\n")
    alias = tmp_path / "shortcut"
    alias.symlink_to(folder, target_is_directory=True)
    ref = str(alias / target.name)
    canonical = str(target.resolve())
    projects.add("billing")
    projects.add_source("billing", "doc", ref)
    assert projects.records()["billing"]["sources"][0]["ref"] == canonical
    target.unlink()
    assert folder.is_dir() and alias.is_symlink() and not target.exists()
    before = projects.records()
    before_bytes = projects.path().read_bytes()
    before_mtime = projects.path().stat().st_mtime_ns
    if action == "check":
        assert projects.check("billing", ref, "Reviewed missing target") == 1
    elif action == "since":
        monkeypatch.setattr(research, "read", lambda *args: pytest.fail("business reading is off"))
        monkeypatch.setattr(research.connectors, "http_json", lambda *args: pytest.fail("no network request"))
        monkeypatch.setattr(research.connectors, "http_get", lambda *args: pytest.fail("no network request"))
        rows = research.changes("billing", ref, read_allowed=False)
        assert rows == [{"kind": "doc", "ref": canonical, "since": None, "count": None, "reason": "", "state": "never read"}]
        assert projects.records() == before
        assert projects.path().read_bytes() == before_bytes and projects.path().stat().st_mtime_ns == before_mtime
    elif action == "reference":
        assert projects.set_reference("billing", ref) == 1
    else:
        projects.mark_unreadable("billing", ref, "Synthetic missing target")
    sources = projects.records()["billing"]["sources"]
    assert len(sources) == 1 and sources[0]["ref"] == canonical
    source = sources[0]
    if action == "check":
        assert source["checked_at"] and source["status"] == "Reviewed missing target"
    elif action == "reference":
        assert source["reference"] is True and source["checked_at"] is None and source["status"] == ""
    elif action == "unreadable":
        assert source["stale_reason"] == "Synthetic missing target" and source["checked_at"] is None and source["status"] == ""


def test_unrelated_missing_document_selector_leaves_saved_source_unchanged(home, tmp_path):
    target = tmp_path / "kickoff.md"
    target.write_text("# Synthetic kickoff\n")
    projects.add("billing")
    projects.add_source("billing", "doc", str(target))
    target.unlink()
    before = projects.records()
    before_bytes = projects.path().read_bytes()
    before_mtime = projects.path().stat().st_mtime_ns
    with pytest.raises(projects.ProjectError, match="has no source"):
        projects.check("billing", str(tmp_path / "unrelated.md"), "Must not stamp")
    assert projects.records() == before
    assert projects.path().read_bytes() == before_bytes and projects.path().stat().st_mtime_ns == before_mtime


@pytest.mark.parametrize("part", ["pull/41", "issues/42"])
@pytest.mark.parametrize("host", ["github.com", "github.example.com"])
def test_item_detection_keeps_the_item_url(host, part):
    ref = f"https://{host}/acme/billing/{part}"
    assert research.detect(ref) == ("url", ref)


def test_absorb_without_reading_never_adds_the_repository(home, monkeypatch):
    projects.add("billing")
    monkeypatch.setattr(research, "read", lambda *args: pytest.fail("reading is off"))
    ref = "https://github.com/acme/billing/pull/41"
    got = research.absorb(ref, "billing", read_allowed=False)
    assert not got["read"]
    assert [(s["kind"], s["ref"]) for s in projects.records()["billing"]["sources"]] == [("url", ref)]


@pytest.mark.parametrize(("part", "label", "merged"), [("pull/41", "PR #41 merged", True), ("issues/42", "issue #42 closed", False)])
def test_absorb_reads_one_item_and_writes_its_title_and_state(home, monkeypatch, part, label, merged):
    monkeypatch.setenv("CB_T", "synthetic")
    store.write_yaml(home / "work" / "connectors.yaml", {"connectors": [{"kind": "github", "token": "env:CB_T"}]})
    calls = []

    def read_one(url, headers):
        calls.append(url)
        return {"number": 41 if merged else 42, "title": "Ship billing ledger", "state": "closed", "merged_at": "2026-10-01T10:00:00Z" if merged else None, "updated_at": "2026-10-01T10:00:00Z"}

    monkeypatch.setattr(research.connectors, "http_json", read_one)
    monkeypatch.setattr(research.connectors, "http_get", lambda *args: pytest.fail("must use the item API"))
    projects.add("billing", memory="work/notes.md")
    got = research.absorb(f"https://github.com/acme/billing/{part}", "billing")
    assert got["read"] and got["memory"]
    assert calls == [f"https://api.github.com/repos/acme/billing/{part.replace('pull/', 'pulls/')}"]
    notes = store.read_text(home / "work" / "notes.md")
    assert "Ship billing ledger" in notes and label in notes
    assert all(s["kind"] != "repo" for s in projects.records()["billing"]["sources"])


def test_item_reader_refuses_a_connector_for_a_different_host(home, monkeypatch):
    store.write_yaml(home / "work" / "connectors.yaml", {"connectors": [{"kind": "github", "token": "env:CB_T", "api": "https://github.other.example.com/api/v3"}]})
    monkeypatch.setattr(research.connectors, "http_json", lambda *args: pytest.fail("wrong connector host"))
    with pytest.raises(research.ReadError, match="host"):
        research.read("url", "https://github.example.com/acme/billing/pull/41", None)


def test_absorb_item_without_project_name_still_matches_its_repository(home):
    projects.add("invoices")
    projects.add_source("invoices", "repo", "acme/billing")
    got = research.absorb("https://github.com/acme/billing/pull/41", read_allowed=False)
    assert got["project"] == "invoices" and got["kind"] == "url"


@pytest.mark.parametrize("host", ["github.com", "GitHub.com"])
def test_memory_sync_does_not_match_another_item_with_the_same_number_prefix(home, monkeypatch, host):
    from careerbrew.work import evidence, memory_sync

    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T09:00:00Z")
    projects.add("invoices", memory="work/notes.md")
    projects.add_source("invoices", "url", f"https://{host}/acme/billing/pull/41")
    store.append_jsonl(
        evidence.evidence_path(),
        [
            {"id": "pr:410", "title": "Unrelated item", "at": "2026-10-20T09:00:00Z", "url": f"https://{host}/acme/billing/pull/410"},
            {"id": "pr:41", "title": "This item", "at": "2026-10-20T09:00:00Z", "url": f"https://{host}/acme/billing/pull/41#discussion"},
        ],
    )
    got = memory_sync.sync_out(allowed=True)
    assert got["notes"] == 1
    notes = store.read_text(home / "work" / "notes.md")
    assert "This item" in notes and "Unrelated item" not in notes
