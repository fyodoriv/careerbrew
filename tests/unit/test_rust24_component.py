"""rust-24 native events, telemetry, repo-health, time-saved, chart and checklist assertions."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust24.py"
CASES = {"event-clean", "telemetry-allowlist", "repo-health", "time-saved", "field-totals", "charts", "checklist"}
MARKERS = (b"SYNTHCANARY", b"free text", b"a title")


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "careerbrew-reports", "--bin", "reports-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "reports-component"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    (root / "tmp").mkdir()
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_NOW": "2026-10-19T09:00:00",
        "TMPDIR": str(root / "tmp"),
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }


def case(report: dict, case_id: str) -> dict:
    return next(row for row in report["cases"] if row["id"] == case_id)["result"]


def test_reports_metrics_and_exports_match_python(candidate, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=180)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert all(marker not in done.stdout + done.stderr for marker in MARKERS), (engine, [marker for marker in MARKERS if marker in done.stdout + done.stderr])
        text = done.stdout.decode().replace(str(root.resolve()), "<owned>").replace(str(root), "<owned>")
        report = json.loads(text)
        ids = [row["id"] for row in report["cases"]]
        assert len(ids) == len(set(ids))
        assert set(ids) == CASES

        events = case(report, "event-clean")["cleaned"]
        assert events[0] == {"command": "day.log", "event": "cli.run", "ms": {"int": "1200"}, "status": "ok"}

        telemetry = case(report, "telemetry-allowlist")
        assert "approvals.decide" in telemetry["command_names"]
        sent = [row for row in telemetry["events"] if row is not None]
        assert sent and len(sent) < len(telemetry["events"])  # the allowlist drops unknown events
        assert all(row["properties"]["$geoip_disable"] is True for row in sent)

        health = case(report, "repo-health")
        assert health["valid"] == "accepted"
        assert health["written"] == ["summary.json", "steps.json", "daily.json"]
        assert health["bad_days"] == ["--days must be 1 to 366"] * 2
        assert health["inside"].startswith("--out must be outside <owned>")

        saved = case(report, "time-saved")
        assert saved["command_action"]["day.log"] == "day_log"
        assert case(report, "field-totals")["empty"]["action_runs"] == {"int": "0"}
        assert case(report, "charts")["html"].startswith("<!doctype html>")
        assert case(report, "checklist")["results"][0]["steps"][0]["command"] == "careerbrew connect"
        answers.append(text)
    assert answers[0] == answers[1]
