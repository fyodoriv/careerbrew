"""An imported project tree asks where notes go once per root project; children use the root's notes (TASKS import-memory-questions)."""

from datetime import date
from pathlib import Path

import pytest

from careerbrew import store
from careerbrew.day import inbox
from careerbrew.work import project_import, projects, research

SHEET = Path(__file__).resolve().parents[1] / "fixtures" / "projects" / "team-goals.csv"
DAY = date(2026, 10, 1)


@pytest.fixture
def tree(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-01T09:00+00:00")
    project_import.run(str(SHEET))  # payments > billing > invoices, payments > ledger; checkout > checkout-analytics
    return home


def _memory_questions() -> dict[str, dict]:
    return {q["data"]["project"]: q for q in inbox.candidates(DAY) if q["kind"] == "memory"}


def test_fixture_import_asks_where_notes_go_once_per_root_project(tree):
    q = _memory_questions()
    assert sorted(q) == ["checkout", "payments"], "6 imported projects, 2 roots: one question per root"
    assert "payments and the 3 projects under it" in q["payments"]["text"]
    assert "checkout and the 1 project under it" in q["checkout"]["text"]


def test_the_root_answer_covers_its_children_and_their_notes_go_to_the_root_file(tree, tmp_path):
    inbox.regenerate(DAY)
    qid = _memory_questions()["payments"]["id"]
    inbox.answer(qid, "work/payments-notes.md")
    assert sorted(_memory_questions()) == ["checkout"], "the root's answer covers billing, invoices, and ledger"
    kickoff = tmp_path / "kickoff-invoices.md"
    kickoff.write_text("# Kickoff\nDecision: freeze the schema on 2026-10-20\n")
    r = research.absorb(str(kickoff), "invoices")
    assert r["read"] and r["memory"], "a grandchild with no notes path writes to its root's notes"
    notes = store.read_text(tree / "work" / "payments-notes.md")
    assert "freeze the schema" in notes and "(project invoices)" in notes, "the shared file names which project the notes are for"


def test_a_child_with_its_own_notes_path_keeps_it(tree, tmp_path):
    projects.set_memory("billing", "work/billing-notes.md")
    assert projects.notes_path("invoices") == "work/billing-notes.md", "the nearest notes path up the tree wins"
    assert projects.notes_path("ledger") == "", "ledger's root has no notes path yet"
    assert "payments and the 1 project under it" in _memory_questions()["payments"]["text"], "only ledger still waits on the root's answer"


def test_a_hand_edited_parent_loop_asks_once_and_ends(home):
    data = {"projects": {n: {**projects.new_record(), "parent": p} for n, p in (("alpha", "beta"), ("beta", "alpha"), ("gamma", "alpha"))}}
    store.write_yaml(projects.path(), data)
    assert {projects.root(n) for n in ("alpha", "beta", "gamma")} == {"alpha"}
    assert sorted(_memory_questions()) == ["alpha"]
