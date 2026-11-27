"""rust-22 native exercise runner core: decision-level parity with the Python runner."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust22.py"
CASES = ["decisions", "comparison-modes", "grading", "child-lifecycle", "scaling-runs", "reference-validation"]


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(["cargo", "build", "--locked", "-p", "careerbrew-exercises", "--bins"], cwd=ROOT, check=True, capture_output=True)
    return ROOT / "target" / "debug" / "exercises-component"


def synthetic_env(root: Path, runner: Path, plant: str | None = None) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    env = {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_LLM": "off",
        "CAREERBREW_RUST22_RUNNER": str(runner),
        "TZ": "UTC",
        "PATH": os.environ.get("PATH", ""),
    }
    if plant:
        env["CAREERBREW_RUST22_PLANT"] = plant
    return env


def report(command: list[str], env: dict[str, str]) -> tuple[bytes, dict]:
    done = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, timeout=300)
    assert done.returncode == 0, done.stderr.decode(errors="replace")
    assert b"EXERCISECANARY" not in done.stdout + done.stderr
    return done.stdout, {row["id"]: row["result"] for row in json.loads(done.stdout)["cases"]}


def test_runner_core_matches_python_and_rejects_the_wrong_class(candidate, tmp_path):
    runner = candidate.with_name("exercise-fixture-runner")
    py_bytes, py = report([sys.executable, "-I", "-B", str(ORACLE), "--all"], synthetic_env(tmp_path / "py", runner))
    rs_bytes, rs = report([str(candidate), "--all"], synthetic_env(tmp_path / "rs", runner))
    assert list(py) == CASES
    assert all(not (isinstance(py[case], dict) and "error" in py[case]) for case in CASES)

    decisions = py["decisions"]
    cubic = decisions["complexity"][1]
    assert cubic["ok"] is False and cubic["expected"] == "O(n)"
    assert decisions["complexity"][2]["ok"] is True  # too small to judge
    assert decisions["scale_sizes"][0] == [10, 20, 40]

    child = {row["label"]: row["result"] for row in py["child-lifecycle"]}
    assert child["cpu-limit"]["diagnostic"]["category"] == "signal_exit"
    assert child["cpu-limit"]["timeout"] is True
    assert child["wall-limit"]["diagnostic"]["category"] == "wall_timeout"
    assert child["crash-with-private-stderr"]["error"] == "RuntimeError: synthetic crash"
    probe = child["sandbox-probe"]["results"][0]["got"]
    assert probe.startswith("cwd_prefix=true;cwd_empty=true;env=PATH,PYTHONHASHSEED;hashseed=0;cpu=6/6")

    runs = {(row["exercise"], row["code"]): row["result"] for row in py["scaling-runs"]}
    assert "diagnostic" not in runs[("linear", "linear")]
    assert runs[("linear", "cubic")]["diagnostic"]["category"] == "complexity_rejected"

    problems = {row["exercise"]: row["problems"] for row in py["reference-validation"]}
    assert problems["linear"] == problems["quadratic"] == []
    assert problems["cubic-reference"] == ["reference solution misses its own complexity (O(n), growth n^<timed>) [complexity_rejected]"]

    assert py_bytes == rs_bytes


def test_a_planted_wrong_class_acceptance_is_visible(candidate, tmp_path):
    runner = candidate.with_name("exercise-fixture-runner")
    py_bytes, _ = report([sys.executable, "-I", "-B", str(ORACLE), "--case", "reference-validation"], synthetic_env(tmp_path / "py", runner))
    rs_bytes, rs = report([str(candidate), "--case", "reference-validation"], synthetic_env(tmp_path / "rs", runner, "accept-wrong-class"))
    problems = {row["exercise"]: row["problems"] for row in rs["reference-validation"]}
    assert problems["cubic-reference"] == []
    assert py_bytes != rs_bytes
