import subprocess
from datetime import UTC, date, datetime

import pytest

from careerbrew import review
from careerbrew.work import evidence, priorities, promo


def _repo(tmp_path, commits):
    repo = tmp_path / "svc"
    repo.mkdir()
    subprocess.run(["git", "init", "-q", "-b", "main", str(repo)], check=True)
    for i, (email, msg) in enumerate(commits):
        (repo / f"f{i}.txt").write_text("x\n" * (i + 1))
        subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
        subprocess.run(
            ["git", "-C", str(repo), "-c", f"user.email={email}", "-c", "user.name=T", "-c", "commit.gpgsign=false", "commit", "-q", "--no-verify", "-m", msg],
            check=True,
        )
    return repo


def test_git_evidence_filters_author_dedups_and_caches(home, tmp_path):
    repo = _repo(tmp_path, [("me@example.com", "feat: launch export"), ("other@example.com", "fix: theirs"), ("me@example.com", "fix: flaky test")])
    (home / "work").mkdir(parents=True, exist_ok=True)
    (home / "work" / "sources.yaml").write_text(f"sources:\n  - {{kind: git, path: '{repo}', authors: [me@example.com]}}\n")
    res = evidence.collect()
    assert (res["new"], res["total"], res["errors"], res["reasons"]) == (2, 2, 0, [])
    assert evidence.collect()["new"] == 0
    tags = {e["title"]: e["tags"] for e in evidence.load()}
    assert tags["feat: launch export"] == ["delivery"]
    assert "quality" in tags["fix: flaky test"]


def test_bad_source_is_counted_not_fatal(home):
    (home / "work").mkdir(parents=True, exist_ok=True)
    (home / "work" / "sources.yaml").write_text("sources:\n  - {kind: git, path: /nonexistent/repo}\n")
    assert evidence.collect()["errors"] == 1


def test_promo_assess_doc_and_gap_order(home):
    for i in range(30):
        evidence.note(f"feat: shipped thing {i}", ["delivery"])
    evidence.note("Wrote design doc for queue", ["design"])
    asm = promo.assess("generic-swe", "senior")
    cov = {r["id"]: r["coverage"] for r in asm["competencies"]}
    assert cov["delivery"] == 1.0 and cov["design"] == pytest.approx(1 / 3, abs=0.01)
    gaps = promo.gaps(asm)
    assert "delivery" not in [g["id"] for g in gaps]
    pr = [g["priority"] for g in gaps]
    assert pr == sorted(pr, reverse=True)
    doc = promo.write_doc(asm).read_text()
    assert "## What is missing" in doc and "shipped thing" in doc


def test_old_evidence_counts_half(home):
    evidence.note("Mentored a new hire", ["leadership"], at="2020-01-01T00:00:00+00:00")
    asm = promo.assess("generic-swe", "senior", at=datetime(2026, 10, 1, tzinfo=UTC))
    lead = next(r for r in asm["competencies"] if r["id"] == "leadership")
    assert lead["credit"] == 0.5


def test_ladder_validation():
    with pytest.raises(promo.LadderError):
        promo.validate_ladder({"id": "x", "levels": ["a"], "competencies": [{"id": "c", "name": "C", "weight": 1, "signals": {}, "expect": {}}]})


def test_priorities_promo_bonus_and_due(home):
    priorities.add("routine bug", 3, 3, 2, [])
    priorities.add("write design doc", 3, 3, 2, ["design"])
    priorities.add("deadline thing", 3, 4, 2, [], due="2026-10-19")  # 3 + (4+3) = 10 > 3 + 3 + 3 promo
    ranked = priorities.rank({"design": 1.0}, today=date(2026, 10, 18))
    assert ranked[0]["title"] == "deadline thing"
    assert ranked[1]["title"] == "write design doc"
    assert priorities.plan({"design": 1.0}, capacity_h=4)["planned_h"] == 4


def test_review_keep_watch_pivot(home):
    priorities.add("a", 3, 3, 2, [])
    priorities.done("t001", 6)  # ratio 3.0 > pivot 2.0
    rows = {r["metric"]: r for r in review.run()}
    assert rows["estimate_ratio"]["status"] == "PIVOT" and rows["estimate_ratio"]["action"]
    assert rows["want_agreement"]["status"] == "no-data"
    assert review.status(0.9, {"success": 0.8, "pivot": 0.4}) == "KEEP"
    assert review.status(0.5, {"success": 0.8, "pivot": 0.4}) == "WATCH"
