"""rust-10 native sessions, claims, agents and reply-draft assertions."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust10.py"
CASES = {"session-lifecycle", "claims-and-drafts", "agent-activity", "reply-fit", "prior-selection", "cli-handlers"}


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "-p", "careerbrew-sessions", "--bin", "sessions-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "sessions-component"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_ENCRYPT": "off",
        "CAREERBREW_NOW": "2026-11-02T09:00:00+00:00",
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }


def case(report: dict, case_id: str) -> dict:
    return next(row for row in report["cases"] if row["id"] == case_id)["result"]


def test_session_draft_agent_origin_and_cli_contracts_match_python(candidate, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=120)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert b"SYNTHCANARY" not in done.stdout + done.stderr
        report = json.loads(done.stdout)
        ids = [row["id"] for row in report["cases"]]
        assert len(ids) == len(set(ids))
        assert set(ids) == CASES
        assert all("error" not in row["result"] for row in report["cases"])

        lifecycle = case(report, "session-lifecycle")
        assert lifecycle["identity"] == {"shape": True, "distinct": True}
        assert lifecycle["reused"] == {"id": "sess-a1", "reused": True}
        assert lifecycle["errors"] == {"duplicate": "SessionError", "duplicate_id": "SessionError", "unknown": "SessionError"}
        assert lifecycle["cap"]["exit"] == {"int": "2"}
        assert all(lifecycle["contract"].values())
        assert lifecycle["handoff"] == {"missing": {"int": "2"}, "wrap": ["sess-a1", "sess-b2"], "repeat_wrap": [], "fresh_before": False, "fresh_after": True}
        assert lifecycle["ended"]["count"] == {"int": "2"}
        assert lifecycle["resume"] == {"same_project": True, "other_project": True, "older_day_cleared": True}

        claims = case(report, "claims-and-drafts")
        assert claims["claim"]["canonical"] == "pr:example/tools#17"
        assert claims["claim"]["held"] == "ClaimHeld"
        assert claims["claim"]["renewed"] is True and claims["claim"]["released"] is True
        assert claims["expiry"] == {"before": True, "after": True, "zero": "SessionError"}
        assert claims["save"] == {"same": True, "other": "DraftClaimed", "without_session": "DraftClaimed", "original_kept": True, "after_release": True, "if_empty": True}
        assert claims["failures"]["not_cleared"] == claims["failures"]["mismatch"] == "DraftMismatch"
        assert claims["privacy"] == {"sealed": True, "plain_text_absent": True, "digest_name": True}
        assert claims["interleavings"] == {
            "operations": {"int": "1000"},
            "contentions": {"int": "250"},
            "draft_saves": {"int": "250"},
            "cross_session_writes": {"int": "0"},
            "stale_claims": {"int": "0"},
            "conflicting_target_saves": {"int": "0"},
        }

        activity = case(report, "agent-activity")
        assert activity["counts"] == {"running": {"int": "2"}, "quiet": {"int": "0"}, "done": {"int": "1"}, "failed": {"int": "0"}}
        assert activity["groups"] == ["billing", "ledger"]
        assert activity["quiet"] is True
        assert activity["restart"] == {"events": {"int": "1"}, "started_is_last": True}
        assert activity["invalid"] == ["AgentError"] * 6
        assert activity["expired"] == [] and activity["retained"] == ["new-bot"]

        fit = case(report, "reply-fit")
        assert fit["started"]["kind"] == "ack" and fit["started"]["deploy_started"] is True
        assert fit["approve"] == {"kind": "ack", "keep_draft": True, "text": None}
        assert fit["major"]["kind"] == "stop" and fit["major"]["long_error"] == "DraftError"
        assert all(kind == "reply" for _, kind in fit["planned"])
        assert fit["rollback"] == "reply" and fit["restarted"] == "ack"
        assert fit["stale"] == {"kind": "ack", "newer": {"int": "2"}, "stale": True, "newest_ts": "1700000002.000100"}
        assert fit["ordered"] == "ack" and fit["mine"] == "none"

        prior = case(report, "prior-selection")
        assert prior["results"][0] == [{"index": {"int": "0"}, "outcome": "candidate", "reason": "prior_day"}]
        assert prior["errors"] == ["ValueError"] * 5
        assert prior["repeat"] is True

        handlers = case(report, "cli-handlers")
        assert handlers["agent"]["event"]["exit"] == handlers["claim"]["take"]["exit"] == handlers["draft"]["replace"]["exit"] == {"int": "0"}
        assert handlers["claim"]["held"]["exit"] == handlers["draft"]["check_bad"]["exit"] == {"int": "1"}
        assert "titles and links hidden" in handlers["agent"]["show"]["stdout"]
        assert "pr:(hidden)" in handlers["claim"]["list"]["stdout"]
        assert '"chat_tool_saved": false' in handlers["draft"]["replace_json"]["stdout"]
        assert "stored locally, not saved in the chat tool" in handlers["draft"]["replace"]["stdout"]
        assert "differs at line 1" in handlers["draft"]["check_bad"]["stdout"]
        answers.append(done.stdout)
    assert answers[0] == answers[1]
