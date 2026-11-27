"""rust-19 native job search component: postings, criteria, Want and the corporate guard."""

from __future__ import annotations

import json
import math
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust19.py"
CASES = ["helpers", "ats-scan", "criteria-filter", "want-learn", "linkedin-alerts", "corporate-guard"]
DEFERRED = ["platforms", "strategy", "interview", "prep", "research"]
MARKERS = (b"RUST19CANARYPERSON", b"RUST19CANARYSECRET")
# The autouse home fixture changes HOME between tests. Keep Cargo's real registry.
CARGO_HOME = os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))
FLOAT_ABS_TOLERANCE = 0.00001
FLOAT_REL_TOLERANCE = 0.000001


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "careerbrew-jobs", "--bin", "jobs-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        env={**os.environ, "CARGO_HOME": CARGO_HOME},
    )
    return ROOT / "target" / "debug" / "jobs-component"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_ENCRYPT": "on",
        "CAREERBREW_NOW": "2026-10-19T13:00:00+00:00",
        "CAREERBREW_RUST19_REPO": str(ROOT),
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }


def case(report: dict, case_id: str) -> dict:
    return next(row for row in report["cases"] if row["id"] == case_id)["result"]


def assert_same(actual, expected, path: str = "report", floats: list[str] | None = None):
    """Compare typed reports, with numeric tolerance only at actual float leaves."""
    if floats is None:
        floats = []
    if isinstance(expected, dict) and set(expected) == {"float"}:
        assert isinstance(actual, dict) and set(actual) == {"float"}, path
        left, right = float(actual["float"]), float(expected["float"])
        assert math.isfinite(left) and math.isfinite(right), path
        assert math.isclose(left, right, abs_tol=FLOAT_ABS_TOLERANCE, rel_tol=FLOAT_REL_TOLERANCE), (path, left, right)
        floats.append(path)
        return floats
    assert type(actual) is type(expected), (path, type(actual), type(expected))
    if isinstance(expected, dict):
        assert set(actual) == set(expected), path
        for key in expected:
            assert_same(actual[key], expected[key], f"{path}.{key}", floats)
    elif isinstance(expected, list):
        assert len(actual) == len(expected), path
        for index, (left, right) in enumerate(zip(actual, expected, strict=True)):
            assert_same(left, right, f"{path}[{index}]", floats)
    else:
        assert actual == expected, (path, actual, expected)
    return floats


def test_float_wrapper_rejects_extra_fields():
    with pytest.raises(AssertionError):
        assert_same({"float": "1.0", "extra": True}, {"float": "1.0"})


def test_a_changed_reason_fails_the_comparison():
    with pytest.raises(AssertionError):
        assert_same({"p1": "salary"}, {"p1": None})


def test_job_search_contracts_match_python(candidate, tmp_path):
    reports = []
    for engine in ("python", "rust"):
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(tmp_path / engine), capture_output=True, timeout=180)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert all(marker not in done.stdout + done.stderr for marker in MARKERS)
        report = json.loads(done.stdout)
        assert [row["id"] for row in report["cases"]] == CASES
        assert all("error" not in row["result"] for row in report["cases"])
        reports.append(report)

        helpers = case(report, "helpers")
        assert helpers["strip"][1] == "a b 'c' \"d\" A"
        assert helpers["regions"][6] == "", "Australian Central Time is not a US zone"
        assert helpers["custom"] == [["examplia", ["examplia", "examplian"]]]

        ats = case(report, "ats-scan")
        assert ats["loaded_ids"][0] == "ashby:fabrikam:f-1"
        assert {p["region"] for p in ats["scanned"]} >= {"us", "global", "sampleland"}

        criteria = case(report, "criteria-filter")
        assert criteria["kept"] == ["p1", "p5"]
        assert criteria["reasons"]["p10"] == "company-excluded"

        want = case(report, "want-learn")
        assert want["few"]["status"] == "need-more-ratings"
        assert want["tags"] == [["go", "remote"], ["kafka"]]
        assert want["bad_rating"] == ["ValueError", "rating must be 1..5"]

        alerts = case(report, "linkedin-alerts")
        assert alerts["added"] == {"int": "3"} and alerts["again"] == {"int": "0"}

        guard = case(report, "corporate-guard")
        assert guard["refused"][0] == "RoleError"
        assert guard["kept"]["role"] == "personal"
        assert guard["corporate"]["jobs"][0] == "RoleError"
        assert guard["corporate"]["day"] == ["accepted", ""]

    floats = assert_same(reports[1], reports[0])
    assert len(floats) == 140, "every Want score, feature and weight was compared"


def test_deferred_vectors_stay_out_of_the_all_report(tmp_path):
    done = subprocess.run([sys.executable, "-I", "-B", str(ORACLE), "--case", "nope"], cwd=ROOT, env=synthetic_env(tmp_path), capture_output=True, timeout=60)
    assert done.returncode == 2
    source = ORACLE.read_text(encoding="utf-8")
    for name in DEFERRED:
        assert f'"{name}"' in source
