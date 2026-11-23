"""rust-06 native archive, sync and CLI component assertions."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust06.py"
CANARY = b"SYNTHETIC-RUST06-CANARY-7Q9"
CASES = {
    "archive-roundtrip",
    "archive-rejections",
    "archive-paths",
    "archive-corporate",
    "archive-key-guards",
    "archive-targets",
    "sync-roundtrip",
    "sync-rejections",
    "sync-allowlist",
    "cli-handlers",
}


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the cross-engine checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "-p", "careerbrew-vault", "--bin", "vault-sync"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "vault-sync"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_PAIR_KEY_FILE": str(root / "keys" / "pair.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_ENCRYPT": "on",
        "CAREERBREW_NOW": "2026-03-01T09:30:00+00:00",
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }


def case(report: dict, case_id: str) -> dict:
    return next(row for row in report["cases"] if row["id"] == case_id)["result"]


def test_archive_sync_and_handler_contracts_match_real_python(candidate, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=120)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert CANARY not in done.stdout + done.stderr
        report = json.loads(done.stdout)
        ids = [row["id"] for row in report["cases"]]
        assert len(ids) == len(set(ids))
        assert set(ids) == CASES

        roundtrip = case(report, "archive-roundtrip")
        assert roundtrip["verified"] == {"int": "4"}
        assert roundtrip["restored"] == {"int": "4"}
        assert roundtrip["archive_mode"] == "600"
        assert roundtrip["source_link_kept_out"] is True
        assert roundtrip["cache_kept_out"] is True
        assert roundtrip["staging_kept_out"] is True
        assert roundtrip["machine_kept_out"] is True
        assert roundtrip["hardlink_is_file"] is True
        assert case(report, "archive-rejections") == dict.fromkeys(
            ["trailing-junk", "trailing-padding", "truncated", "invalid-base64", "empty", "tampered", "wrong-name", "expected-sha"],
            "RuntimeError",
        )
        assert all("error" in result for result in case(report, "archive-paths").values())
        corporate = case(report, "archive-corporate")
        assert corporate["work"]["role"] == "corporate"
        assert all(corporate[name]["error"] == "RoleError" for name in ("personal-dir", "personal-output", "profile-field", "profile-malformed", "goals"))
        guards = case(report, "archive-key-guards")
        assert guards["missing-key"] == {"error": "VaultError", "key_created": False, "unchanged": True, "archives": {"int": "0"}}
        assert guards["inside-key"]["error"] == "ValueError"
        assert guards["pair-inside"]["error"] == "ValueError"
        assert guards["pair-roundtrip"] == {"key_length": {"int": "44"}, "mode": "600", "roundtrip": True}
        assert guards["pair-invalid"] == "SyncError"
        sync_result = case(report, "sync-roundtrip")
        assert sync_result["imported"] == {"from": "personal", "added": {"int": "2"}, "ignored": {"int": "0"}}
        assert sync_result["settings"] == {"ladder": "generic-swe", "target_level": "staff"}
        assert sync_result["bundle_mode"] == "600"
        rejections = case(report, "sync-rejections")
        assert rejections["corporate-export"]["created"] is False
        assert rejections["wrong-key"] == "SyncError"
        assert rejections["forbidden-payload"]["unchanged"] is True
        handlers = case(report, "cli-handlers")
        assert set(handlers) == {"data-status", "backup-create", "backup-verify", "backup-restore", "pair-join", "sync-export", "sync-import"}
        assert all(row["exit"] == {"int": "0"} for row in handlers.values())
        answers.append(done.stdout)
    assert answers[0] == answers[1]
