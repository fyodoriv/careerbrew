"""rust-12 native learning, replay, tour and explainer assertions."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust12.py"
CASES = {"source-staging", "concept-validation", "learning-schedule", "replay", "tours", "explainers-cli"}


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "-p", "careerbrew-learn", "--bin", "learn-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "learn-component"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_ENCRYPT": "off",
        "CAREERBREW_LLM": "off",
        "CAREERBREW_NOW": "2026-10-26T09:00:00+00:00",
        "CAREERBREW_RUST12_PYTHON": sys.executable,
        "CAREERBREW_RUST12_REPO": str(ROOT),
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }


def case(report: dict, case_id: str) -> dict:
    return next(row for row in report["cases"] if row["id"] == case_id)["result"]


def test_learning_replay_tour_explainer_and_cli_contracts_match_python(candidate, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=180)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert b"LEARNCANARY" not in done.stdout + done.stderr
        report = json.loads(done.stdout)
        ids = [row["id"] for row in report["cases"]]
        assert len(ids) == len(set(ids))
        assert set(ids) == CASES
        assert all("error" not in row["result"] for row in report["cases"])

        source = case(report, "source-staging")
        assert source["first"]["keys"] == ["doc:runbook", "repo:toyrepo"]
        assert source["privacy"] == {"consumed": True, "sealed": True, "plain_absent": True, "repo_unchanged": True}
        assert source["identity"] == {"strasse": "strasse", "a7d0": "\ua7d1"}
        assert source["repeat"] == {"changed": [], "unchanged": ["repo:toyrepo"]}
        assert source["changed"]["freshness"]["changed"] == ["repo:toyrepo"]
        assert source["note"]["bad_path"] == "ValueError"

        concepts = case(report, "concept-validation")
        assert any("cycle" in p for p in concepts["bad"])
        assert any("pure recall" in p for p in concepts["bad"])
        assert any("misconception" in p for p in concepts["bad"])
        assert concepts["refused"] == "ValueError"
        assert concepts["imported"] == {"concepts": {"int": "3"}, "sections": {"int": "2"}}
        assert concepts["removed"]["status_due"] == {"int": "0"}
        assert concepts["decks"] == {"learn_only": True, "interview_separate": True}
        assert concepts["cli"]["bad"]["exit"] == {"int": "1"}

        learning = case(report, "learning-schedule")
        assert [(s["mode"], s.get("concept")) for s in learning["steps"]] == [("pretest", "queue"), ("pretest", "deploy"), ("teach", "queue"), ("retest", "queue"), ("retest", "queue")]
        assert learning["evidence"] == [False, False, False]
        assert learning["mastered"]["value"] is True
        assert learning["early"] == {"due_retests": {"int": "0"}, "scheduled_share": {"float": "1.0"}}
        assert learning["missed"]["mastered"] is False
        assert learning["final"]["retest_retained_7d"] == {"float": "0.0"}
        assert learning["leech"] == {"flag": True, "suspended": True, "reviews": {"int": "8"}}
        assert "misconception" in learning["cli"]["answer"]["stdout"]

        replayed = case(report, "replay")
        assert [r["pr"] for r in replayed["candidates"]] == [{"int": "8"}, {"int": "7"}]
        assert replayed["candidates"][1]["tests"] == ["tests/test_double.py"]
        assert replayed["task"]["inside_data"] is True
        assert [r["passed"] for r in replayed["checks"]] == [False, True]
        assert replayed["repo_untouched"] is True
        assert replayed["no_tests"] == "ValueError"
        assert replayed["measure"]["runnable"] == {"int": "1"}
        assert replayed["cli"]["check"]["exit"] == {"int": "0"}

        toured = case(report, "tours")
        assert len(toured["bad"]) == 4
        assert toured["added"]["plain_absent"] is True
        assert toured["unreviewed"]["info"]["state"] == "unreviewed"
        assert toured["approved"]["info"]["faded"] == {"int": "1"}
        assert toured["approved"]["faded_text"].startswith("Predict before you read")
        assert toured["stale"] == {"state": "stale", "faded": {"int": "1"}, "steps": {"int": "2"}, "changed": ["src/toy/__init__.py"]}
        assert toured["cli"]["export"]["exit"] == {"int": "0"}

        explained = case(report, "explainers-cli")
        assert explained["full"] == explained["short"] == []
        assert explained["deep"]["pass"] == []
        assert any("How it runs" in p for p in explained["deep"]["missing"])
        assert explained["prd"]["pass"] == explained["prd"]["fits"] == []
        assert explained["scaffolds"]["deep"] is True and explained["scaffolds"]["prd"] is True
        assert explained["cli"]["explain_check"]["exit"] == explained["cli"]["prd_check"]["exit"] == {"int": "0"}
        assert explained["cli"]["deep_check"]["exit"] == {"int": "1"}
        answers.append(done.stdout)
    assert answers[0] == answers[1]
