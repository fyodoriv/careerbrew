"""rust-16 native work-day execution assertions: proofs, timeouts, timing, the watch loop and wrap-up."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust16.py"
CASES = {"gate-check", "timing-history", "loop-records", "loop-pass", "catch-up", "wrap-control"}
MARKERS = (b"SYNTHCANARY", b"private-name")


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "careerbrew-workexec", "--bin", "workexec-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "workexec-component"


def synthetic_env(root: Path, plant: str = "") -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    env = {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_ENCRYPT": "on",
        "CAREERBREW_NOW": "2026-11-02T09:00:00+00:00",
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }
    if plant:
        env["CAREERBREW_RUST16_PLANT_WRITE"] = plant
    return env


def case(report: dict, case_id: str) -> dict:
    return next(row for row in report["cases"] if row["id"] == case_id)["result"]


def n(value: int) -> dict:
    return {"int": str(value)}


def run(command: list[str], root: Path, plant: str = "") -> tuple[bytes, dict]:
    done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root, plant), capture_output=True, timeout=180)
    assert done.returncode == 0, done.stderr.decode(errors="replace")
    assert all(marker not in done.stdout + done.stderr for marker in MARKERS)
    return done.stdout, json.loads(done.stdout)


def test_work_day_execution_matches_python_byte_for_byte(candidate, tmp_path):
    outputs = {}
    for engine in ("python", "rust"):
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        stdout, report = run(command, tmp_path / engine)
        ids = [row["id"] for row in report["cases"]]
        assert len(ids) == len(set(ids)) and set(ids) == CASES
        assert all("error" not in row["result"] for row in report["cases"])

        gate = case(report, "gate-check")
        assert gate["first"]["checked"] == n(3) and gate["first"]["retry"] == ["/standup"]
        assert gate["second"]["failed"] == ["/standup"]
        assert gate["second"]["retry"] == ["/careerbrew", "/fresh"]
        assert gate["third"]["checked"] == n(0) and gate["idle_writes"] == n(0)

        timing = case(report, "timing-history")
        assert timing["medians"] == [n(150), None, n(121), None]
        assert "2 min" in timing["shown"]

        records = case(report, "loop-records")
        assert records["errors"] == ["WatchError"] * 4 + ["StepError"] + ["WatchError"] * 3
        assert records["invalid"] == ["WatchError"] * 9 + ["accepted"]
        assert records["fingerprints"][:2] == ["6bbe27772d2cb36b", "f351f2ec9faf4a58"]
        assert records["fingerprints"][2:4] == [True, True]

        passes = case(report, "loop-pass")
        assert passes["ended_writes"] == n(1)
        assert len(passes["calls"]) == 10
        assert all(call[0] != "acme run" for call in passes["calls"])
        assert passes["skipped"] == {"reason": "previous cycle running", "status": "skipped"}

        wrap = case(report, "wrap-control")
        assert wrap["errors"] == ["ValueError"] * 6
        assert wrap["after_end"] == "ValueError" and wrap["ended_phase"] == "ended"
        outputs[engine] = stdout
    assert outputs["python"] == outputs["rust"]


@pytest.mark.parametrize(
    ("plant", "case_id"),
    [("duplicate-dispatch", "loop-pass"), ("lost-wrap", "wrap-control"), ("false-success", "gate-check")],
)
def test_a_planted_execution_fault_changes_the_native_answer(candidate, tmp_path, plant, case_id):
    _, honest = run([str(candidate), "--case", case_id], tmp_path / "honest")
    _, planted = run([str(candidate), "--case", case_id], tmp_path / "planted", plant)
    assert case(honest, case_id) != case(planted, case_id)
