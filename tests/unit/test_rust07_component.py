"""rust-07 native plugin, machine, account, preflight and CLI assertions."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust07.py"
CASES = {
    "manifest-policy",
    "plugin-lifecycle",
    "command-ownership",
    "machine-readiness",
    "oauth-refs",
    "mcp-preflight",
    "managed-repair",
    "update-workflow",
    "cli-handlers",
}


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "-p", "careerbrew-setup", "--bin", "setup-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "setup-component"


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


def test_plugin_machine_account_preflight_and_handler_contracts_match_python(candidate, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=120)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert b"not-reported-old" not in done.stdout + done.stderr
        assert b"not-reported-new" not in done.stdout + done.stderr
        assert b"rotated-not-reported" not in done.stdout + done.stderr
        report = json.loads(done.stdout)
        ids = [row["id"] for row in report["cases"]]
        assert len(ids) == len(set(ids))
        assert set(ids) == CASES

        manifest = case(report, "manifest-policy")
        assert manifest["valid"] == "acme"
        assert set(manifest["refusals"].values()) <= {"PluginError", "accepted"}
        assert manifest["refusals"]["mapping"] == "PluginError"
        assert manifest["merged"]["allowed_routes"] == ["local"]
        assert manifest["merged"]["backup_targets"] == [str(Path("/") / "home" / "u" / "Team")]
        assert manifest["merged"]["agent_may_read_business"] is False
        assert manifest["order"][:2] == ["acme", "team"]

        lifecycle = case(report, "plugin-lifecycle")
        assert lifecycle["verified"] == [["acme", True]]
        assert lifecycle["tampered"] == [["acme", False], ["team", True]]
        assert lifecycle["verified_after"] == [["acme", True], ["team", True]]
        assert lifecycle["dependent_refusal"] == "PluginError"
        assert lifecycle["lock_empty"] is True

        commands = case(report, "command-ownership")
        assert commands["lifecycle"] == {
            "selected": True,
            "legacy_removed": True,
            "archived": True,
            "archive_sealed": True,
            "fragment_mode": "600",
            "empty_after_remove": True,
        }
        assert commands["collision"] == {"error": "PluginError", "kept": True, "installed": False}
        assert commands["symlink"] == {"error": "PluginError", "staging_removed": True}
        assert commands["metadata_symlink"] == {"error": "PluginError", "outside_empty": True}

        readiness = case(report, "machine-readiness")
        assert readiness["ready_count"] == [{"int": "14"}, {"int": "14"}]
        assert readiness["cleared"] is True
        assert readiness["temp_refusal"] == "RoleError"
        assert readiness["repos"] == ["a", "b"]
        assert readiness["per_user"] == [True, True, False, False]

        oauth = case(report, "oauth-refs")
        assert oauth["poll"][:3] == ["done", "wait", "slow"]
        assert oauth["refresh"][:3] == ["token", "reconnect", "reconnect"]
        assert oauth["settings"] == [["setting-client", "setting-tenant"], ["flag-client", "flag-tenant"]]
        assert oauth["bearer_is_value"] is True
        assert oauth["rotated"] is True
        assert oauth["reconnect_cleared"] is True

        preflight = case(report, "mcp-preflight")
        assert [row["value"] for row in preflight["rows"]] == ["command not on launchd PATH", "GitHub credential mismatch", "port 8765 closed"]
        assert "--fix --yes" in preflight["repair"][0]["safe"]
        assert "--fix" not in preflight["native"][0]["safe"]
        assert preflight["strict"] == ["ValueError"] * 3
        assert preflight["auth"] == ["Authorization", None, None]

        repair = case(report, "managed-repair")
        assert repair["confirmation"] == {"error": "ValueError", "unchanged": True}
        assert repair["success"]["result"]["changed"] == {"int": "1"}
        assert repair["success"]["link"] is True
        assert repair["success"]["mode"] == "640"
        assert repair["success"]["unrelated"] is True
        assert repair["success"]["headers"] is True
        assert repair["success"]["helper"] is True
        assert repair["success"]["temporary_left"] is False
        assert repair["success"]["second"]["changed"] == {"int": "0"}
        assert repair["managed"] == {"error": "ValueError", "unchanged": True, "link": True}
        assert repair["foreign"]["changed"] == {"int": "0"}
        assert repair["foreign"]["unchanged"] is True

        update = case(report, "update-workflow")
        assert update["launcher"] == [True, False]
        assert update["repairs"] == [True, False, False]
        assert len(update["workflow"]) == 4
        assert update["workflow"][1]["ok"] is True
        assert update["workflow"][2]["ok"] is False

        handlers = case(report, "cli-handlers")
        assert set(handlers) == {
            "init",
            "plugin-add",
            "plugin-list",
            "plugin-verify",
            "machine-set",
            "machine-show",
            "connectors-list",
            "pair-init",
            "split-evidence",
            "plugin-refresh",
            "plugin-remove",
            "pair_key_not_printed",
        }
        assert all(row["exit"] == {"int": "0"} for row in handlers.values() if isinstance(row, dict))
        assert handlers["pair_key_not_printed"] is True
        answers.append(done.stdout)
    assert answers[0] == answers[1]
