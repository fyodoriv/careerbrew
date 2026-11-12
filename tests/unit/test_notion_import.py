"""Notion bundle import: every item lands in its feature, re-import is idempotent."""

import json
import shutil

import pytest

from careerbrew import notion_import, paths, prep, store
from careerbrew.jobs import strategy
from careerbrew.tutor import cards
from careerbrew.work import evidence

from .conftest import FIXTURES

BUNDLE = FIXTURES / "notion-bundle"


def _run(home):
    home.mkdir(parents=True, exist_ok=True)
    (home / "profile.yaml").write_text("name: Test\nskills:\n  python: 4\n")
    return notion_import.run(BUNDLE)


def test_every_kind_lands_in_its_feature(home):
    r = _run(home)
    assert r["counts"] == {"pages": 3, "jobs": 4, "flashcards": 4, "plan": 2, "skills": 3, "stories": 1, "past_job": 2}
    assert len(cards.all_cards()) == 4, "flashcards become practice cards"
    lib = notion_import.library()
    assert {p["section"] for p in lib} >= {"career", "how-to", "plan", "past-job"}
    assert (paths.data_root() / "library" / "notion").is_dir()
    board = notion_import.jobs_board()
    assert {j["company"] for j in board} == {"Zzalpha", "Zzbeta", "Zzgamma", "Zzdelta"}
    apps = {a["company"]: a["stage"] for a in store.read_jsonl(strategy.applications_path())}
    assert apps == {"Zzalpha": "rejected", "Zzgamma": "offer", "Zzdelta": "applied"}, "not-started jobs are not applications"
    from datetime import UTC, datetime

    assert "zzalpha" in strategy.used_companies(at=datetime(2025, 1, 1, tzinfo=UTC)), "a real loop counts for the season strategy"
    assert "zzalpha" not in strategy.used_companies(at=datetime(2026, 1, 1, tzinfo=UTC)), "after the cooldown the company is back"
    assert [s["title"] for s in prep.stories()] == ["Sample story one"]
    ev = [e for e in evidence.load() if e.get("origin") == "notion"]
    assert len(ev) == 2 and all("leadership" in e["tags"] for e in ev)


def test_skills_merge_without_overwriting(home):
    _run(home)
    skills = store.read_yaml(home / "profile.yaml", {})["skills"]
    assert skills["python"] == 4, "an existing rating wins"
    assert skills["typescript"] == 3 and skills["graphql"] == 4


def test_people_and_past_employer_become_leak_terms(home):
    _run(home)
    terms = store.read_text(home / "sensitive-terms.txt")
    assert "Qqperson" in terms and "Zzlegacy" in terms


def test_reimport_is_idempotent_and_keeps_practice_history(home):
    _run(home)
    cards.review("notion-f1", "good")
    before = {k: len(store.read_jsonl(p)) if p.suffix == ".jsonl" else None for k, p in {"apps": strategy.applications_path(), "ev": evidence.evidence_path()}.items()}
    notion_import.run(BUNDLE)
    after = {k: len(store.read_jsonl(p)) for k, p in {"apps": strategy.applications_path(), "ev": evidence.evidence_path()}.items()}
    assert before == after
    assert len(prep.stories()) == 1 and len(cards.all_cards()) == 4
    assert cards.stats()["reviewed_today"] == 1


def test_check_compares_bundle_with_the_stores(home):
    _run(home)
    chk = notion_import.check(BUNDLE)
    assert chk["ok"] and chk["imported"] == chk["expected"] == 19
    shutil.rmtree(paths.data_root() / "tutor" / "decks")
    assert not notion_import.check(BUNDLE)["ok"]


def test_manifest_counts_must_match_the_files(home, tmp_path):
    bad = tmp_path / "b"
    shutil.copytree(BUNDLE, bad)
    m = json.loads((bad / "manifest.json").read_text())
    m["counts"]["jobs"] = 99
    (bad / "manifest.json").write_text(json.dumps(m))
    with pytest.raises(notion_import.ImportError_, match="jobs"):
        notion_import.run(bad)


def test_corporate_machine_refuses_the_import(home):
    from careerbrew import machine

    home.mkdir(parents=True, exist_ok=True)
    machine.set_role("corporate")
    with pytest.raises(machine.RoleError):
        notion_import.run(BUNDLE)


def test_imported_dates_do_not_break_promo_review_or_today(home):
    from careerbrew import review, server
    from careerbrew.work import promo

    _run(home)
    promo.assess("generic-swe", "senior")
    review.run()
    server.today()
    assert all("+" in e["at"] or e["at"].endswith("Z") for e in evidence.load() if e.get("origin") == "notion")


def test_blank_notion_cards_are_reported_not_counted_as_missing(home, tmp_path):
    bundle = tmp_path / "b"
    shutil.copytree(BUNDLE, bundle)
    rows = json.loads((bundle / "flashcards.json").read_text())
    rows.append({"id": "f-blank", "question": "", "answer": "", "tags": []})
    (bundle / "flashcards.json").write_text(json.dumps(rows))
    m = json.loads((bundle / "manifest.json").read_text())
    m["counts"]["flashcards"] = 5
    (bundle / "manifest.json").write_text(json.dumps(m))
    home.mkdir(parents=True, exist_ok=True)
    r = notion_import.run(bundle)
    assert r["skipped_empty"] == 1 and r["cards"] == 4
    chk = notion_import.check(bundle)
    assert chk["ok"] and chk["skipped_empty"] == 1


def test_import_into_an_uninitialised_root_starts_from_the_example_profile(home):
    home.mkdir(parents=True, exist_ok=True)
    notion_import.run(BUNDLE)
    prof = store.read_yaml(home / "profile.yaml", {})
    assert {"name", "current_level", "sensitive"} <= set(prof), "template fields exist for the user to fill"
    assert prof["skills"]["graphql"] == 4, "a Notion-only skill is added"


def test_card_questions_lose_notion_markdown(home):
    assert notion_import.plain("**What is a \\^heap?**") == "What is a ^heap?"
    assert notion_import.plain("__Why__ `useMemo`?") == "Why useMemo?"


def test_stories_and_job_notes_are_readable_in_the_library(home):
    notion_import.run(FIXTURES / "notion-bundle")
    lib = {e["id"]: e for e in notion_import.library()}
    assert lib["st1"]["section"] == "story" and "sample step" in notion_import.page("st1")["markdown"]
    jobs = [e for e in lib.values() if e["section"] == "job"]
    assert sorted(e["title"] for e in jobs) == ["Zzalpha: Sample role A", "Zzgamma: Sample role C"], "only jobs with notes become pages"
    assert all(e["status"] for e in jobs), "the stage shows as the page status"
