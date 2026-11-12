from __future__ import annotations

import json
import subprocess
from pathlib import Path

import pytest

from careerbrew import paths, store
from careerbrew.learn import project as lp
from careerbrew.learn import replay, tour

GIT = ["git", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false", "-c", "user.name=Test", "-c", "user.email=test@example.com"]


def _commit(repo: Path, message: str, files: dict[str, str]) -> str:
    for rel, text in files.items():
        (repo / rel).parent.mkdir(parents=True, exist_ok=True)
        (repo / rel).write_text(text)
    subprocess.run([*GIT, "-C", str(repo), "add", "-A"], check=True)
    subprocess.run([*GIT, "-C", str(repo), "commit", "-q", "-m", message], check=True)
    return subprocess.run(["git", "-C", str(repo), "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()


@pytest.fixture
def toy(tmp_path):
    repo = tmp_path / "toyrepo"
    repo.mkdir()
    subprocess.run([*GIT, "init", "-q", "-b", "main", str(repo)], check=True)
    _commit(repo, "chore: start", {"README.md": "# Toy\n", "src/toy/__init__.py": "def add(a, b):\n    return a + b\n"})
    _commit(
        repo,
        "feat: double a number (#7)",
        {"src/toy/__init__.py": "def add(a, b):\n    return a + b\n\n\ndef double(n):\n    return add(n, n)\n", "tests/test_double.py": "from toy import double\n\n\ndef test_double():\n    assert double(4) == 8\n"},
    )
    _commit(repo, "docs: say hello (#8)", {"README.md": "# Toy\nHello.\n"})
    _commit(repo, "a direct push, not a PR", {"README.md": "# Toy\nHi.\n"})
    lp.start("Toy", [("repo", str(repo))])
    return repo


def test_candidates_are_small_squash_merged_prs_with_their_tests(toy):
    rows = replay.candidates(toy)
    assert [r["pr"] for r in rows] == [8, 7]
    assert rows[1]["tests"] == ["tests/test_double.py"]
    assert rows[1]["sources"] == ["src/toy/__init__.py"]


def test_a_replay_task_fails_until_the_change_is_made_and_writes_nothing_in_the_repo(toy):
    before = subprocess.run(["git", "-C", str(toy), "status", "--porcelain", "--ignored"], capture_output=True, text=True).stdout
    refs = sorted(p.name for p in (toy / ".git").iterdir())
    task = replay.start("Toy", 7)
    work = Path(task["dir"])
    assert work.is_relative_to(paths.data_root())
    assert (work / "tests" / "test_double.py").exists()
    assert "double" not in (work / "src" / "toy" / "__init__.py").read_text()
    assert not replay.check("Toy", 7)["passed"]
    (work / "src" / "toy" / "__init__.py").write_text("def add(a, b):\n    return a + b\n\n\ndef double(n):\n    return 2 * n\n")
    assert replay.check("Toy", 7)["passed"]
    assert subprocess.run(["git", "-C", str(toy), "status", "--porcelain", "--ignored"], capture_output=True, text=True).stdout == before
    assert sorted(p.name for p in (toy / ".git").iterdir()) == refs, "no worktree or ref is added to the learned repo"


def test_a_pr_without_tests_cannot_be_a_task(toy):
    with pytest.raises(ValueError, match="no Python test file"):
        replay.start("Toy", 8)


def test_measure_counts_a_pr_runnable_only_when_its_tests_fail_before_and_pass_after(toy):
    r = replay.measure(toy, limit=50)
    assert r["prs"] == 2
    assert r["runnable"] == 1
    assert {row["pr"]: row["why"] for row in r["rows"]} == {7: "fails before, passes after", 8: "no Python test file"}


def _tour(**extra):
    return {
        "repo": "toyrepo",
        "title": "How doubling works",
        "steps": [{"file": "src/toy/__init__.py", "line": 5, "description": "double reuses add.", "concept": "double"}, {"file": "README.md", "line": 1, "description": "The project name."}],
        **extra,
    }


def test_a_tour_is_checked_against_the_staged_repo(toy):
    lp.import_concepts("Toy", [{"id": "double", "section": "core", "title": "Doubling", "cites": ["repo:toyrepo/src/toy/__init__.py"], "questions": [{"bloom": "apply", "prompt": "double(3)?", "answer": "6"}]}])
    bad = {"repo": "toyrepo", "title": "x", "steps": [{"file": "missing.py", "line": 1, "description": "d"}, {"file": "README.md", "line": 99, "description": ""}, {"file": "README.md", "line": 1, "description": "d", "concept": "nope"}]}
    problems = "\n".join(tour.check("Toy", bad))
    assert "no file missing.py" in problems and "line must be 1 to" in problems and "needs a description" in problems and "concept nope" in problems
    assert tour.check("Toy", _tour()) == []


def test_a_tour_fades_mastered_steps_and_flags_review(toy):
    lp.import_concepts("Toy", [{"id": "double", "section": "core", "title": "Doubling", "cites": ["repo:toyrepo/src/toy/__init__.py"], "questions": [{"bloom": "apply", "prompt": "double(3)?", "answer": "6"}]}])
    tour.add("Toy", "doubling", _tour())
    out, info = tour.export("Toy", "doubling")
    data = json.loads(out.read_text())
    assert info == {"state": "unreviewed", "faded": 0, "steps": 2, "changed": []}
    assert data["steps"][0]["description"].startswith("**No maintainer has approved")
    for level in ("understand", "apply"):
        lp.answer("Toy", "double", "correct", "high", level)
    tour.approve("Toy", "doubling", "maintainer")
    out, info = tour.export("Toy", "doubling")
    data = json.loads(out.read_text())
    assert (info["state"], info["faded"]) == ("approved", 1)
    assert data["steps"][0]["description"].startswith("Predict before you read")
    assert data["steps"][1]["description"] == "The project name."
    _commit(toy, "fix: double with a multiply (#9)", {"src/toy/__init__.py": "def add(a, b):\n    return a + b\n\n\ndef double(n):\n    return 2 * n\n"})
    _, info = tour.export("Toy", "doubling")
    assert (info["state"], info["changed"]) == ("stale", ["src/toy/__init__.py"])


def test_the_stored_tour_is_encrypted(toy):
    tour.add("Toy", "doubling", _tour(steps=[_tour()["steps"][1]]))
    raw = (lp.folder("Toy") / "tours" / "doubling.json").read_bytes()
    assert b"The project name" not in raw
    assert store.read_json(lp.folder("Toy") / "tours" / "doubling.json", {})["title"] == "How doubling works"
