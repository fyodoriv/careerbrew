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
        assert manifest["refusals"]["requires-bool"] == "PluginError"
        assert manifest["refusals"]["work-step-field"] == "PluginError"
        assert manifest["real_version_refusal"] == "PluginError"
        assert manifest["command_semantics"] == {
            "verify-env": "PluginError",
            "verify-shell": "PluginError",
            "verify-managed": "accepted",
            "cycle-python": "accepted",
            "review-python": "accepted",
            "cycle-unclosed": "PluginError",
        }
        assert manifest["merged"]["allowed_routes"] == ["local"]
        assert manifest["merged"]["backup_targets"] == [str(Path("/") / "home" / "u" / "Team")]
        assert manifest["merged"]["agent_may_read_business"] is True
        assert manifest["denied_agent_read"] is False
        assert manifest["order"] == ["acme", "cycle-b", "cycle-a", "team", "unknown"]
        assert manifest["order"].index("acme") < manifest["order"].index("team")

        lifecycle = case(report, "plugin-lifecycle")
        assert lifecycle["verified"] == [["acme", True]]
        assert lifecycle["tampered"] == [["acme", False], ["team", True]]
        assert lifecycle["verified_after"] == [["acme", True], ["team", True]]
        assert lifecycle["dependent_refusal"] == "PluginError"
        assert lifecycle["removed"][1]["purged_records"] == {"int": "2"}
        assert lifecycle["purged_left"] == ["b"]
        assert lifecycle["inbox_left"] == ["inbox-b"]
        assert lifecycle["inbox_lock"] is True
        assert lifecycle["home_source"] is True
        assert lifecycle["git_locked"] is True
        assert lifecycle["git_refreshed"] is True
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
        assert readiness["output_cleanup"] == {
            "out/interview-notes.md": False,
            "out/interview-notes.txt": True,
            "out/strategy.md": False,
            "out/progress.html": False,
            "out/other.md": True,
            "out/reports/run.html": False,
        }
        assert readiness["temp_refusal"] == "RoleError"
        assert readiness["repos"] == ["a", "b"]
        assert readiness["per_user"] == [True, True, False, False]
        assert readiness["goal_counts"] == [{"int": "0"}, {"int": "1"}, {"int": "1"}, {"int": "1"}, {"int": "1"}, {"int": "1"}, {"int": "0"}]
        assert readiness["goal_refusals"] == {"history": "RoleError", "upcoming": "RoleError", "malformed": "RoleError", "safe": "accepted"}

        oauth = case(report, "oauth-refs")
        assert oauth["poll"][:3] == ["done", "wait", "slow"]
        assert oauth["refresh"][:3] == ["token", "reconnect", "reconnect"]
        assert oauth["settings"] == [["setting-client", "setting-tenant"], ["flag-client", "flag-tenant"]]
        assert oauth["secret_refs"] == {"keychain": "keychain", "op": "op", "env": "env", "literal": "SecretError"}
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
            "connectors-list-invalid",
            "pair-init",
            "split-evidence",
            "split-evidence-live",
            "split_fields_preserved",
            "plugin-refresh",
            "plugin-remove",
            "pair_key_not_printed",
        }
        nonzero_handlers = {name: row["exit"] for name, row in handlers.items() if isinstance(row, dict) and row["exit"] != {"int": "0"}}
        assert nonzero_handlers == {"connectors-list-invalid": {"int": "1"}}
        assert handlers["pair_key_not_printed"] is True
        assert handlers["split_fields_preserved"] is True
        assert "secret=op allowed_by_policy=True" in handlers["connectors-list"]["stdout"]
        assert handlers["connectors-list-invalid"] == {"exit": {"int": "1"}, "stdout": "SecretError"}
        assert "evidence sources work collect reads: 7 -> 6\n" in handlers["split-evidence"]["stdout"]
        assert handlers["split-evidence-live"]["stdout"] == (
            "plugin acme evidence.yaml: 3 evidence entries -> 2 shared (moved 1 source(s), 0 connector(s))\n"
            "your work/sources.yaml: 1 -> 1 source(s); work/connectors.yaml: 3 -> 3 connector(s)\n"
            "evidence sources work collect reads: 7 -> 6\n"
            "ok    the company plugin evidence file holds no per-person values (emails, logins, home paths)\n"
            "next: commit the change in the plugin's own repo, so a coworker's copy holds shared entries only\n"
        )
        answers.append(done.stdout)
    assert answers[0] == answers[1]
