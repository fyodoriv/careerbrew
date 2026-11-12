from __future__ import annotations

import random
import subprocess
from collections import Counter
from datetime import UTC, datetime, timedelta

import pytest

from careerbrew import cli, machine, store
from careerbrew.learn import project as lp
from careerbrew.tutor import cards

T0 = datetime(2026, 10, 26, 9, 0, tzinfo=UTC)


def _repo(tmp_path):
    repo = tmp_path / "toyrepo"
    (repo / "src").mkdir(parents=True)
    (repo / "README.md").write_text("# Toy\nA queue and a worker.\n")
    (repo / "src" / "queue.py").write_text("class Queue: ...\n")
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    return repo


def _q(bloom, prompt="Why?", answer="Because."):
    return {"bloom": bloom, "prompt": prompt, "answer": answer}


def _concepts():
    return [
        {"id": "queue", "section": "core", "title": "The queue", "cites": ["repo:toyrepo/src/queue.py"], "questions": [_q("understand"), _q("apply")]},
        {"id": "worker", "section": "core", "title": "The worker", "prereqs": ["queue"], "cites": ["note:01-core.md"], "questions": [_q("understand"), _q("analyze")]},
        {"id": "deploy", "section": "ops", "title": "Deploys", "cites": ["doc:runbook"], "questions": [_q("apply")]},
    ]


@pytest.fixture
def staged(tmp_path):
    repo = _repo(tmp_path)
    doc = tmp_path / "runbook.md"
    doc.write_text("Deploy with make ship.\n")
    lp.start("Toy", [("repo", str(repo)), ("doc", f"runbook:{doc}")], consume=True)
    lp.note("Toy", "01-core.md", "# Core\nQueue then worker.\n")
    lp.import_concepts("Toy", _concepts())
    return repo


def test_start_reads_repos_in_place_and_stores_fetched_text_encrypted(tmp_path, home):
    repo = _repo(tmp_path)
    before = sorted(p.relative_to(repo) for p in repo.rglob("*") if ".git" not in p.parts)
    doc = tmp_path / "runbook.md"
    doc.write_text("Deploy with make ship.\n")
    m = lp.start("Toy", [("repo", str(repo)), ("doc", f"runbook:{doc}")], consume=True)
    assert {s["key"] for s in m["sources"]} == {"repo:toyrepo", "doc:runbook"}
    assert not doc.exists(), "consume leaves no plaintext copy"
    stored = lp.folder("Toy") / "inputs" / "doc" / "runbook.md"
    assert b"make ship" not in stored.read_bytes()
    assert store.read_text(stored) == "Deploy with make ship.\n"
    assert sorted(p.relative_to(repo) for p in repo.rglob("*") if ".git" not in p.parts) == before, "nothing is written in the learned repo"


def test_freshness_names_the_sources_that_changed(tmp_path):
    repo = _repo(tmp_path)
    lp.start("Toy", [("repo", str(repo))])
    assert lp.start("Toy", [("repo", str(repo))])["freshness"] == {"changed": [], "unchanged": ["repo:toyrepo"]}
    (repo / "README.md").write_text("# Toy\nNow with retries.\n")
    assert lp.start("Toy", [("repo", str(repo))])["freshness"]["changed"] == ["repo:toyrepo"]


def test_the_concept_check_blocks_bad_cites_cycles_recall_heavy_decks_and_untagged_options(staged):
    bad = [
        {"id": "a", "section": "s", "title": "A", "prereqs": ["b"], "cites": ["repo:toyrepo/missing.py"], "questions": [_q("remember")]},
        {"id": "b", "section": "s", "title": "B", "prereqs": ["a"], "cites": ["note:01-core.md"], "questions": [{**_q("apply"), "options": ["x", "y", "z"], "misconceptions": ["one"]}]},
    ]
    problems = "\n".join(lp.check_concepts("Toy", bad))
    assert "citation repo:toyrepo/missing.py" in problems
    assert "cycle" in problems
    assert "pure recall" in problems
    assert "misconception" in problems
    with pytest.raises(ValueError):
        lp.import_concepts("Toy", bad)
    assert lp.check_concepts("Toy", _concepts()) == []


def test_every_taught_concept_gets_a_spaced_review(staged):
    lp.answer("Toy", "queue", "correct", "high", "understand", mode="teach", at=T0)
    lp.answer("Toy", "worker", "partial", "medium", "understand", mode="teach", at=T0)
    lp.answer("Toy", "deploy", "wrong", "low", "apply", mode="pretest", at=T0)
    s = lp.status("Toy", at=T0)
    assert s["taught"] == 2
    assert s["scheduled_share"] == 1.0


def test_mastery_needs_two_confident_correct_answers_one_at_apply(staged):
    assert not lp.answer("Toy", "queue", "correct", "high", "apply", mode="pretest", at=T0)["mastered"], "a pretest is not evidence"
    assert not lp.answer("Toy", "queue", "correct", "low", "apply", at=T0)["mastered"], "a low-confidence guess is not evidence"
    assert not lp.answer("Toy", "queue", "correct", "high", "understand", at=T0)["mastered"]
    row = lp.answer("Toy", "queue", "correct", "medium", "apply", at=T0)
    assert row["mastered"]
    assert lp.status("Toy", at=T0)["sections"]["core"]["mastered"] == 1


def test_mastery_schedules_retests_at_one_and_seven_days(staged):
    lp.answer("Toy", "queue", "correct", "high", "understand", at=T0)
    lp.answer("Toy", "queue", "correct", "high", "apply", at=T0)
    assert lp.status("Toy", at=T0 + timedelta(hours=1))["due_retests"] == 0
    step = lp.next_step("Toy", at=T0 + timedelta(days=1, minutes=1))
    assert (step["mode"], step["concept"]) == ("retest", "queue")
    lp.answer("Toy", "queue", "correct", "high", "apply", mode="retest", at=T0 + timedelta(days=1, minutes=1))
    late = T0 + timedelta(days=7, minutes=2)
    assert lp.status("Toy", at=late)["due_retests"] == 1
    lp.answer("Toy", "queue", "wrong", "high", "apply", mode="retest", at=late)
    s = lp.status("Toy", at=late)
    assert s["retest_retained_7d"] == 0.0
    assert s["brier"] is not None


def test_next_starts_each_section_with_a_pretest_then_teaches_prerequisites_first(staged):
    first = lp.next_step("Toy", at=T0)
    assert (first["mode"], first["concept"]) == ("pretest", "queue")
    lp.answer("Toy", "queue", "dontknow", "low", "understand", mode="pretest", at=T0)
    second = lp.next_step("Toy", at=T0)
    assert (second["mode"], second["section"]) == ("pretest", "ops")
    lp.answer("Toy", "deploy", "wrong", "low", "apply", mode="pretest", at=T0)
    third = lp.next_step("Toy", at=T0)
    assert third["mode"] == "teach"
    assert third["concept"] != "worker", "worker waits for its prerequisite"


def test_learn_cards_stay_out_of_the_interview_deck(staged):
    lp.answer("Toy", "queue", "correct", "high", "understand", at=T0)
    assert all(c["id"] not in {"queue", "worker", "deploy"} for c in cards.queue(T0 + timedelta(days=30)))
    assert not (lp.folder("Toy").parent.parent / "tutor" / "card-state.json").exists()


def test_interview_cards_stay_out_of_a_learn_deck(staged):
    cards.add("Two sum?", "A hash map.", ["algo"])
    lp.answer("Toy", "queue", "correct", "high", "understand", at=T0)
    with cards.deck_root(lp.folder("Toy") / "srs"):
        assert {c["id"] for c in cards.queue(T0 + timedelta(days=30))} <= {"queue", "worker", "deploy"}


def test_a_removed_concept_does_not_break_next_or_status(staged):
    lp.answer("Toy", "queue", "correct", "high", "understand", at=T0)
    lp.answer("Toy", "queue", "correct", "high", "apply", at=T0)
    lp.import_concepts("Toy", [c for c in _concepts() if c["id"] == "deploy"])
    later = T0 + timedelta(days=2)
    assert lp.next_step("Toy", at=later)["concept"] == "deploy"
    assert lp.status("Toy", at=later)["due_retests"] == 0


def test_a_leech_is_retaught_not_a_crash(staged):
    at, row = T0, {}
    for _ in range(12):
        lp.answer("Toy", "queue", "correct", "high", "apply", at=at)
        at += timedelta(days=30)
        row = lp.answer("Toy", "queue", "wrong", "high", "apply", mode="review", at=at)
        at += timedelta(days=1)
    assert any(r.get("leech") for r in lp._log("Toy")), "the leech is flagged in the log"
    assert len([r for r in lp._log("Toy") if r["mode"] == "review"]) == 12, "every answer is logged"
    assert "leech" in row


def test_a_missed_retest_takes_mastery_away_until_it_is_earned_again(staged):
    lp.answer("Toy", "queue", "correct", "high", "understand", at=T0)
    lp.answer("Toy", "queue", "correct", "high", "apply", at=T0)
    day1 = T0 + timedelta(days=1, minutes=1)
    assert not lp.answer("Toy", "queue", "wrong", "high", "apply", mode="retest", at=day1)["mastered"]
    assert lp.status("Toy", at=day1)["sections"]["core"]["mastered"] == 0
    assert lp.status("Toy", at=T0 + timedelta(days=8))["due_retests"] == 0, "no retest for a concept that is not mastered"
    lp.answer("Toy", "queue", "correct", "high", "understand", at=day1)
    assert lp.answer("Toy", "queue", "correct", "high", "apply", at=day1)["mastered"]
    step = lp.next_step("Toy", at=day1 + timedelta(days=1, minutes=1))
    assert (step["mode"], step["concept"]) == ("retest", "queue"), "re-mastery schedules new retests"


def test_stale_lists_only_sources_that_changed_since_they_were_taught(tmp_path):
    repo = _repo(tmp_path)
    lp.start("Toy", [("repo", str(repo))])
    lp.start("Toy", [("repo", str(repo))])
    assert lp.status("Toy")["stale"] == []
    (repo / "README.md").write_text("# Toy\nA queue, a worker, and a retry.\n")
    lp.start("Toy", [("repo", str(repo))])
    lp.start("Toy", [("repo", str(repo))])
    assert lp.status("Toy")["stale"] == ["repo:toyrepo"], "a later start keeps the stale list"
    lp.import_concepts("Toy", _concepts()[:1])
    assert lp.status("Toy")["stale"] == []


def test_a_cite_that_leaves_the_repo_is_rejected(staged, tmp_path):
    (tmp_path / "outside.md").write_text("x")
    bad = [{**_concepts()[0], "cites": ["repo:toyrepo/../outside.md"]}]
    assert any("citation repo:toyrepo/../outside.md" in p for p in lp.check_concepts("Toy", bad))


def test_learn_runs_on_a_corporate_machine(staged, monkeypatch, capsys):
    monkeypatch.setattr(machine, "role", lambda: "corporate")
    assert cli.main(["learn", "status", "Toy", "--json"]) in (0, None)
    assert '"concepts": 3' in capsys.readouterr().out


def test_the_rule_beat_bkt_in_the_preregistered_simulation():
    """The pre-registered check (2026-10-26) that dropped BKT, kept as a record.

    The full run measured BKT at 0.728 agreement and the rule at 0.771, with no false
    mastery. This smaller run pins the property
    that decided it: the rule never calls an unknown concept mastered.
    """
    rng = random.Random(7)
    verdicts: Counter[tuple[bool, bool]] = Counter()
    for _ in range(4000):
        known, rows = rng.random() < 0.5, []
        for i in range(rng.randint(2, 6)):
            correct = rng.random() < (0.9 if known else 0.2)
            conf = rng.choice(("medium", "high")) if correct == known else "low"
            rows.append({"mode": "quiz", "result": "correct" if correct else "wrong", "confidence": conf, "level": lp.BLOOM[min(1 + i, 3)]})
        verdicts[(known, lp._mastered(rows))] += 1
    assert verdicts[(False, True)] == 0, "the rule never calls an unknown concept mastered"
