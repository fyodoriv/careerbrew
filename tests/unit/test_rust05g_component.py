"""rust-05g settings and general-event parity in synthetic homes only."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

from careerbrew import vault

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust05.py"
CANARY = "SYNTHETIC-RUST05-CANARY-7Q9"
SECRET = "SYNTHETIC-RUST05-SECRET-NOT-AN-AUTHENTICATION-VALUE"


@pytest.fixture(scope="module")
def store_bin():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the cross-engine checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "-p", "careerbrew-vault", "--bin", "vault-store"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "vault-store"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "on",
        "CAREERBREW_ENCRYPT": "on",
        "CAREERBREW_NOW": "2026-03-01T09:30:00+00:00",
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }


def case(report: dict, case_id: str):
    return next(row for row in report["cases"] if row["id"] == case_id)


def assert_no_synthetic_values(root: Path, output: bytes) -> None:
    for term in (CANARY.encode(), SECRET.encode()):
        assert term not in output
        for path in root.rglob("*"):
            if path.is_file() and not path.is_symlink() and path.name != "backup.key":
                raw = path.read_bytes()
                if not raw.startswith(vault.MAGIC):
                    assert term not in raw, path.relative_to(root)


def test_settings_and_general_events_match_real_python(store_bin, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(store_bin), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=120)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        report = json.loads(done.stdout)
        ids = {row["id"] for row in report["cases"]}
        assert {
            "settings-path",
            "settings-missing",
            "settings-empty",
            "settings-write",
            "settings-read",
            "settings-write-rewrites",
            "event-clean",
            "event-record",
            "event-write-row",
            "event-write-row-line",
            "event-emit-config",
            "event-emit-error",
            "event-lines",
            "event-write-float-row",
            "event-write-float-line",
            "event-off",
            "event-off-identity",
        } <= ids
        assert case(report, "settings-write-rewrites")["result"] is True
        assert case(report, "event-off-identity")["result"] is True
        assert case(report, "event-write-row-line")["result"] == '{"a": "x", "b": 1}'
        cleaned = case(report, "event-clean")["result"]
        assert cleaned["status"] == "a" * 48
        assert "command" not in cleaned
        assert cleaned["route"] == "ok\n"
        assert "action" not in cleaned
        assert case(report, "event-write-float-line")["result"] == (
            '{"exponent": 1e-07, "high_fixed": 1000000000000000.0, "high_scientific": 1e+16, "infinity": Infinity, "low_fixed": 0.0001, "low_scientific": 1e-05, "nan": NaN, "negative_infinity": -Infinity, "negative_zero": -0.0}'
        )
        assert case(report, "receipt-events")["result"] == [
            {"event": "approvals.request", "status": "pending"},
            {"event": "approvals.decide", "status": "approved"},
        ]
        settings = next(row for row in case(report, "files")["result"] if row["path"] == "settings.yaml")
        assert settings["sealed"] is True
        assert_no_synthetic_values(root, done.stdout + done.stderr)
        answers.append(done.stdout)
    assert answers[0] == answers[1]
