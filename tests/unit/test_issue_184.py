"""Issue #184: a stale source the agent reads (plugin `agent_reads`) names the agent step, never a connector to set up."""

import pytest

from careerbrew import connectors
from careerbrew.cli import main
from careerbrew.work import projects, research

CHANNEL = "C0SYNTH1"  # a synthetic channel id


@pytest.fixture
def stale_chat(home, monkeypatch, tmp_path):
    root = tmp_path / "p"
    root.mkdir()
    (root / "careerbrew-plugin.yaml").write_text("name: acme\nversion: 0.1.0\nagent_reads: [slack]\n")
    assert main(["plugin", "add", str(root)]) == 0
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-01T09:00+00:00")
    projects.add("billing")
    projects.add_source("billing", "chat", CHANNEL)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-20T09:00+00:00")
    monkeypatch.setattr(connectors, "http_get", lambda *a, **k: pytest.fail("careerbrew read a source the agent reads"))


def test_show_names_the_agent_step_for_an_agent_read_source(stale_chat, capsys):
    research.refresh()
    capsys.readouterr()
    assert main(["work", "project", "show", "billing"]) == 0
    out = capsys.readouterr().out
    assert "read by agent" in out
    assert "careerbrew connect" not in out, out
    assert f'careerbrew work project check billing {CHANNEL} --status "<one line>"' in out, out


def test_a_local_doc_file_is_still_read_by_careerbrew_when_the_agent_reads_gdocs(home, monkeypatch, tmp_path):
    root = tmp_path / "p"
    root.mkdir()
    (root / "careerbrew-plugin.yaml").write_text("name: acme\nversion: 0.1.0\nagent_reads: [gdocs]\n")
    assert main(["plugin", "add", str(root)]) == 0
    notes = tmp_path / "notes.md"
    notes.write_text("# Billing\n\n- decided: ship the export first\n")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-01T09:00+00:00")
    projects.add("billing")
    projects.add_source("billing", "doc", str(notes))
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-20T09:00+00:00")
    assert projects.reader("doc", str(notes)) == "careerbrew"
    assert research.refresh()["refreshed"] == 1
