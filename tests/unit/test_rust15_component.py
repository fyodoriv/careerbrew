"""rust-15 native work-day record, question, stage and session assertions."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust15.py"
CASES = {"status-schema", "question-records", "step-validation", "stage-readiness", "work-session", "legacy-receipts"}
MARKERS = (b"SYNTHCANARY", b"private-name")


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "careerbrew-workday", "--bin", "workday-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "workday-component"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    return {
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


def case(report: dict, case_id: str) -> dict:
    return next(row for row in report["cases"] if row["id"] == case_id)["result"]


def test_workday_records_questions_stages_and_sessions_match_python(candidate, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=180)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert all(marker not in done.stdout + done.stderr for marker in MARKERS), (engine, [marker for marker in MARKERS if marker in done.stdout + done.stderr])
        report = json.loads(done.stdout)
        ids = [row["id"] for row in report["cases"]]
        assert len(ids) == len(set(ids))
        assert set(ids) == CASES
        assert all("error" not in row["result"] for row in report["cases"])

        status = case(report, "status-schema")
        assert status["blank"]["attempt"] == {"int": "0"} and status["blank"]["seconds"] is None
        assert status["legacy"]["attempt"] == {"int": "0"} and status["legacy"]["started"] == ""
        assert len(status["running"]["summary"]) == 200
        assert status["running"]["attempt"] == {"int": "1"}
        assert status["repeated_attempt"] == {"int": "1"}
        assert status["summary_only"]["ended"] == ""
        assert status["errors"] == ["ValueError"] * 5
        assert status["stale_timeout"] is None and status["noop_writes"] == {"int": "0"}
        assert status["timed"]["status"] == "waiting" and status["timed"]["retry"] is True
        assert status["timed"]["next"] == "run /standup again: attempt 2 of 2, after a timeout"
        assert status["blocked"]["needs_you"] == "Owner picks"
        assert status["handled"]["checkout:~/synthetic"] == "/standup"
        assert status["start_kept"]["preflight"][0]["check"] == "synthetic"
        assert status["sealed"] is True and status["plain_absent"] is True

        question = case(report, "question-records")
        assert question["third_same"] is True and question["repeat_writes"] == {"int": "0"}
        assert question["invalid"] == ["ValueError"] * 8
        assert "text" not in question["public"] and "pick" not in question["public"]
        assert question["public"]["options"] == {"int": "2"} and question["public"]["recheck"] == "command"
        assert question["plugin_public"]["source"] == "plugin" and question["plugin_public"]["recheck"] == "review-pr"
        assert question["close"] == [False, True]
        assert len(question["batch"]["dropped"]) == 1
        assert question["batch"]["dropped"][0]["why"] == "recheck: closed (exit 1)"
        assert sorted(row["state"] for row in question["batch"]["ask"]) == ["agent", "open", "open"]
        assert question["ages"] == ["unknown", "24 h", "3 d"]
        assert question["sealed"] is True and question["plain_absent"] is True

        step = case(report, "step-validation")
        assert step["rich"]["step"] == "/standup" and step["rich"]["timeout"] == {"int": "3"}
        assert step["rich"]["about"] == "Draft today's standup from synthetic work"
        assert step["rich"]["scope"] == ["repo", "repo/web"]
        assert step["defaults"]["about"] == "" and step["defaults"]["timeout"] is None
        assert step["verify_errors"] == ["StepError"] * 6
        assert step["other_errors"] == ["StepError"] * 10
        assert step["resolved"] == {"description": "Draft the standup.", "user_only": True, "present": True, "source_error": ""}
        assert step["plain"]["description"] == "" and step["plain"]["user_only"] is False
        assert step["about"] == ["Own words", "Draft the standup.", ["/a"]]
        assert step["first_sentences"][0] == "One sentence." and len(step["first_sentences"][2]) == 160
        assert step["canonical"]["present"] is True
        assert step["canonical"]["description"].startswith("Run the work day with careerbrew")

        stage = case(report, "stage-readiness")
        assert stage["ready"] == {
            "initial": ["/check", "/other"],
            "running": ["/other"],
            "checked": ["/feed", "/report", "/other"],
            "failed": ["/other"],
            "blocked": ["/feed", "/report", "/other"],
        }
        dispatch = next(row for row in stage["found"] if row["step"] == "/dispatch")
        assert dispatch["ready_when"]["signin_required"] is True
        feed = next(row for row in stage["found"] if row["step"] == "/feed")
        assert feed["ready_when"]["after"] == ["/careerbrew", "/check"]
        assert "finite resource work" in feed["why"]
        assert stage["errors"] == ["StepError"] * 4

        work = case(report, "work-session")
        assert work["opened"]["exit"] == work["ledger"]["exit"] == {"int": "0"}
        assert work["opened"]["value"]["reused"] is False and work["reused"]["value"]["reused"] is True
        assert work["cap"]["exit"] == {"int": "2"} and "2 of 2 sessions are live" in work["cap"]["stdout"]
        assert work["reuse_writes"] == {"int": "0"}
        assert work["raw_claim"]["resource"] == "slack-draft:C0SYN/1.0"
        assert work["public"]["project"] == hashlib.sha256(b"billing").hexdigest()[:8]
        assert work["public"]["claims"] == {"int": "1"} and "title" not in work["public"]["activity"]
        assert work["listed"]["value"]["sessions"][0]["project"] != "billing"
        assert "projects and handoffs hidden" in work["listed_text"]["stdout"]
        assert work["missing"]["exit"] == {"int": "1"} and set(work["missing"]["value"]["missing"]) == {"sess-a", "sess-b"}
        assert work["ended"]["exit"] == {"int": "0"} and set(work["ended"]["value"]["ended"]) == {"sess-a", "sess-b"}
        assert work["claims_after_end"] == []
        assert work["sealed"] is True and work["plain_absent"] is True

        legacy = case(report, "legacy-receipts")
        assert legacy["malformed_status"] == {} and legacy["malformed_questions"] == []
        assert legacy["open_rows"] == []
        assert legacy["review_batch"]["ask"][0]["state"] == "unknown"
        assert legacy["review_batch"]["ask"][0]["why"] == "review recheck: question changed"
        assert len(legacy["review_still_open"]) == 1
        assert legacy["invalid_claim"] == "SessionError" and legacy["unchanged"] is True
        assert legacy["legacy_open"]["value"]["brief_status"] == "legacy-unverified"
        assert "brief" not in legacy["legacy_open"]["value"]
        assert legacy["legacy_end"]["exit"] == {"int": "0"} and legacy["legacy_loops"] == ["new"]
        assert legacy["quiet_open"] == {"id": "resumed", "brief": "prior private handoff", "brief_status": ""}
        assert legacy["quiet_reused"] == "prior private handoff"
        assert legacy["quiet_receipt"] is True and legacy["quiet_reuse_writes"] == {"int": "0"}
        answers.append(done.stdout)
    assert answers[0] == answers[1]
