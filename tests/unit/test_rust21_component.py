"""rust-21 native teacher, goals, practice and mistake-tracking assertions."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust21.py"
CASES = (
    "goals-queue",
    "goals-focus-blocks",
    "teacher-local-day",
    "teacher-flow",
    "teacher-cards-validate-resume",
    "teacher-improvements",
    "teacher-drafts",
)
MARKER = b"RUST21CANARYCODE"
# The autouse home fixture changes HOME between tests. Keep Cargo's real registry.
CARGO_HOME = os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "careerbrew-teacher", "--bin", "teacher-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        env={**os.environ, "CARGO_HOME": CARGO_HOME},
    )
    return ROOT / "target" / "debug" / "teacher-component"


def synthetic_env(root: Path, **extra: str) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_ENCRYPT": "on",
        "CAREERBREW_NOW": "2026-10-19T13:00:00+00:00",
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
        **extra,
    }


def run(command: list[str], root: Path, **extra: str) -> bytes:
    done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root, **extra), capture_output=True, timeout=300)
    assert done.returncode == 0, done.stderr.decode(errors="replace")
    assert MARKER not in done.stdout + done.stderr
    return done.stdout


def results(stdout: bytes) -> dict:
    return {row["id"]: row["result"] for row in json.loads(stdout)["cases"]}


@pytest.fixture(scope="module")
def reports(candidate, tmp_path_factory):
    base = tmp_path_factory.mktemp("rust21")
    python = run([sys.executable, "-I", "-B", str(ORACLE), "--all"], base / "python")
    rust = run([str(candidate), "--all"], base / "rust")
    return python, rust


def test_the_rust_component_prints_the_oracles_bytes(reports):
    python, rust = reports
    assert [row["id"] for row in json.loads(python)["cases"]] == list(CASES)
    assert all("error" not in row["result"] for row in json.loads(python)["cases"])
    assert rust == python


def test_study_steps_count_by_the_settings_zones_local_day(reports):
    day = results(reports[0])["teacher-local-day"]
    # 03:30 UTC is still Oct 19 in New York: two of the four rows, 30 whole minutes.
    assert day["summary"]["steps"] == {"int": "2"}
    assert day["summary"]["minutes"] == {"int": "30"}
    assert day["done"]["kind"] == "done"
    assert day["under_budget"]["kind"] == "lesson"
    assert day["after_midnight"]["steps"] == {"int": "1"}


def test_the_planted_utc_day_defect_fails_parity(candidate, reports, tmp_path):
    planted = run([str(candidate), "--case", "teacher-local-day"], tmp_path, CAREERBREW_RUST21_PLANT_UTC="1")
    expected = results(reports[0])["teacher-local-day"]
    actual = results(planted)["teacher-local-day"]
    assert actual != expected
    assert actual["done"]["kind"] != "done"


def test_the_flow_reaches_every_kind_and_the_capped_loop_ends(reports):
    flow = results(reports[0])["teacher-flow"]
    assert flow["approve"]["kind"] == "approve"
    assert flow["mistake"]["kind"] == "mistake"
    assert flow["retry"]["retry"] is True
    assert flow["exercise_fail-error"]["result"]["solution"]
    cards = results(reports[0])["teacher-cards-validate-resume"]
    assert cards["resume"]["kind"] == "resume"
    assert cards["resume_sent"]["next"]["job"]["company"] == "OtherCo"
    assert cards["resume_skip"]["next"]["kind"] == "done"


def test_improvements_name_each_signal_with_its_automatic_fix(reports):
    improve = results(reports[0])["teacher-improvements"]
    signals = [row["signal"] for row in improve["improvements"]]
    for signal in ("slow-step", "stuck-exercise", "repeat-mistake", "skipped", "low-confirm", "card-leech", "written"):
        assert signal in signals
    assert improve["low"] == ["card"]
    assert all("auto" in row for row in improve["improvements"])
