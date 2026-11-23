"""rust-09 native projects, priorities, promises, focus and team assertions."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust09.py"
CASES = {"project-lifecycle", "readiness-gates", "priorities-tickets", "promise-lifecycle", "lock-and-team", "cli-handlers"}


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "-p", "careerbrew-projects", "--bin", "projects-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "projects-component"


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


def test_project_priority_promise_lock_and_cli_contracts_match_python(candidate, tmp_path):
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

        lifecycle = case(report, "project-lifecycle")
        assert lifecycle["identity"] == {"shape": True, "stable": True, "migrated": False}
        assert lifecycle["approval"] == {"kind": "approval", "restricted_has_text": False, "stored": True}
        assert lifecycle["errors"] == {"duplicate": "ProjectError", "kind": "ProjectError", "graph": "ProjectError", "remove": "ProjectError"}
        assert lifecycle["relations"]["parent"] == "search"
        assert lifecycle["relations"]["root"] == "search"
        assert lifecycle["tree"] == [["search", {"int": "0"}], ["search-ranking", {"int": "1"}], ["checkout", {"int": "0"}]]

        gates = case(report, "readiness-gates")
        assert gates["initial"] == {"done": {"int": "0"}, "total": {"int": "4"}, "ready": False, "event_on": "2026-11-03"}
        links_after_check = next(row for row in gates["after_check"] if row["gate"] == "links")
        assert links_after_check["done"] is False
        assert links_after_check["note"] == "0 of 2 read by you"
        assert gates["ready"]["ready"] is True
        assert gates["ready"]["quizzes"] == {"int": "3"}
        assert gates["short_requiz"] == "ProjectError"
        assert "not ready: 1 of 4 gate(s) open (links)" in gates["archive_refused"]
        assert [row["id"] for row in gates["blocks_before"]] == ["gate:demo:links"]
        assert gates["blocks_after"] == []

        priority = case(report, "priorities-tickets")
        assert priority["bad_hours"] == ["ValueError"] * 5
        assert priority["estimates"][:5] == [{"float": "1.0"}] * 5
        assert priority["configured_keys"] == ["bill-42", "bill", "fix utf-8 and iso-8601; bill-42"]
        assert priority["generic_keys"] == ["abc-123", "abc", "fix utf-8, iso-8601 and abc-123"]
        assert priority["linked_keys"] == ["ops-3", "ops", "see the ticket"]
        assert priority["ticket_projects"] == ["BILL", "SRCH"]
        assert priority["bonus"] == [{"float": "0.0"}, {"float": "3.0"}, {"float": "1.5"}, {"float": "0.0"}]
        assert [row["id"] for row in priority["ranked"]] == ["t001", "t002"]
        assert priority["done"]["status"] == "done"
        assert priority["already_done"] == "ValueError"

        promise = case(report, "promise-lifecycle")
        assert promise["initial"][0]["title"] == "Send the rollout notes tomorrow"
        assert promise["initial"][0]["to"] == "Sam"
        assert promise["initial"][0]["parent"] == "billing"
        assert promise["initial"][0]["due"] == "2026-11-03"
        assert "title" not in promise["restricted"][0] and "to" not in promise["restricted"][0]
        assert promise["after_research"] == "draft" and promise["after_draft"] == "status"
        assert promise["rescan"] == {"events": {"int": "1"}, "new": {"int": "0"}}
        assert len(promise["after_rescan"]) == 1
        assert promise["after_rescan"][0]["closed"] == "done"
        assert promise["manual_distinct"] is True and promise["manual_reopened"] is True
        assert all(row is not None for row in promise["positive"])
        assert promise["negative"] == [None] * 7
        assert promise["stale_sources"] == []

        locked = case(report, "lock-and-team")
        assert locked["nested"] is True
        assert locked["lock_rows"]["billing"]["updates"] == ["nested update", "update billing"]
        assert locked["lock_rows"]["search"]["updates"] == ["update search"]
        assert locked["lock_rows"]["billing"]["status"] == "seen billing"
        assert locked["lock_rows"]["search"]["status"] == "seen search"
        assert locked["owner_closure"] == {"closed": "done", "owner_kept": True, "archived": True}
        assert locked["team_added"]["changed"] is True
        assert locked["team_unchanged"]["changed"] is False
        assert locked["matches"] == [["Sam Lee"], ["Sam Lee"], ["Sam Lee"], ["Sam Lee"], []]
        assert locked["bad_chat"] == locked["unknown_remove"] == "TeamError"
        assert locked["team_after_remove"] == []

        handlers = case(report, "cli-handlers")
        assert handlers["priority_set"]["exit"] == handlers["task_set"]["exit"] == handlers["impact"]["exit"] == handlers["done"]["exit"] == {"int": "0"}
        assert handlers["task_set_empty"]["exit"] == handlers["impact_empty"]["exit"] == {"int": "2"}
        assert "1.5h" in handlers["task_set"]["stdout"]
        assert "done t001 (1.0h vs 1.5h planned)" in handlers["done"]["stdout"]
        assert "nothing to change" in handlers["task_set_empty"]["stdout"]
        assert "nothing to record" in handlers["impact_empty"]["stdout"]
        answers.append(done.stdout)
    assert answers[0] == answers[1]
