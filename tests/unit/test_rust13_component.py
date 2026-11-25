"""rust-13 native feed ingestion and transcript-normalization assertions."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust13.py"
CASES = {
    "source-ingestion",
    "edited-upsert",
    "raw-replies",
    "claude-transcript",
    "codex-transcript",
    "containment-since",
    "asks-cycle",
    "team-feed",
    "local-time-and-nan",
    "source-failures",
}
MARKERS = (b"PROJECTZEBRA", b"CANARYCHAT", b"CANARYTICKET", b"CANARYMAIL", b"CANARY-NATIVE-ERROR", b"CANARYSOURCE", b"synthetic-not-reported")


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "careerbrew-feed", "--bin", "feed-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "feed-component"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_ENCRYPT": "on",
        "CAREERBREW_NOW": "2026-10-21T16:00:00+00:00",
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }


def case(report: dict, case_id: str) -> dict:
    return next(row for row in report["cases"] if row["id"] == case_id)["result"]


def normalized(data: bytes, root: Path) -> bytes:
    return data.replace(str(root).encode(), b"<owned>").replace(str(root.resolve()).encode(), b"<owned>")


def test_feed_transcript_write_and_cli_contracts_match_python(candidate, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=120)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert all(marker not in done.stdout + done.stderr for marker in MARKERS)
        report = json.loads(done.stdout)
        ids = [row["id"] for row in report["cases"]]
        assert len(ids) == len(set(ids))
        assert set(ids) == CASES
        assert all("error" not in row["result"] for row in report["cases"])

        source = case(report, "source-ingestion")
        assert source["first"]["new"] == {"int": "16"}
        assert source["repeated"]["new"] == source["repeated"]["updated"] == {"int": "0"}
        assert source["count"] == {"int": "16"}
        assert source["dm"]["kind"] == "dm" and source["dm"]["channel"] == ""
        assert source["dm"]["thread"] == "slack:D0ALEX" and source["dm"]["to_me"] is True
        assert source["sealed"] is True and source["original_kept"] is True
        assert source["analysis_separated"] is True and source["lock"] is True

        edited = case(report, "edited-upsert")
        assert edited["first"]["new"] == {"int": "1"}
        assert edited["second"]["updated"] == {"int": "1"}
        assert edited["repeated"]["new"] == edited["repeated"]["updated"] == {"int": "0"}
        assert edited["title_edited"] is True
        assert edited["row"]["ask_mark"] == "done"
        assert edited["research_kept"] is True and edited["original_edited"] is True
        assert edited["one_row"] is True and edited["open"] == []

        raw = case(report, "raw-replies")
        assert raw["tickets"]["new"] == {"int": "2"} and raw["tickets"]["skipped"] == {"int": "1"}
        assert raw["mail"]["new"] == {"int": "2"}
        assert raw["calendar"]["new"] == {"int": "2"} and raw["calendar"]["skipped"] == {"int": "2"}
        assert raw["quiet"]["new"] == {"int": "0"}
        assert "jira:PROJ-7:2026-10-21T12:00:00Z:In Progress" in raw["rows"]
        assert "calendar:allday:2026-10-30" in raw["rows"]
        assert raw["ticket_original"] is True and raw["mail_original"] is True
        assert raw["analysis_separated"] is True and raw["not_calendar"] is True
        assert raw["bad_source"] == "FeedError"

        claude = case(report, "claude-transcript")
        assert claude["first"]["new"] == {"int": "4"} and claude["first"]["skipped"] == {"int": "1"}
        assert claude["first"]["gaps"] == ["mail"]
        assert claude["first"]["pointers"] == {"followed": {"int": "1"}, "refused": {"int": "0"}}
        assert claude["repeated"]["already"] == {"int": "4"}
        assert claude["repeated"]["new"] == claude["repeated"]["updated"] == {"int": "0"}
        assert claude["cli"]["exit"] == {"int": "1"}
        assert "gap  mail:" in claude["cli"]["stdout"]
        assert claude["original_padded"] is True and claude["sealed"] is True

        codex = case(report, "codex-transcript")
        assert codex["same_facts"] is True
        assert codex["response_items"]["new"] == {"int": "0"}
        assert codex["response_items"]["already"] == {"int": "4"}
        assert codex["end_events"]["errors"] == {"int": "1"}
        assert codex["end_events"]["gaps"] == ["chat", "tickets"]
        assert codex["unsupported"] == "FeedError"

        containment = case(report, "containment-since")
        assert containment["refused"]["pointers"]["refused"] == {"int": "1"}
        assert containment["outside_stored"] is False
        assert containment["since"]["replies"]["chat"] == {"int": "1"}
        assert containment["since"]["gaps"] == ["tickets", "mail"]
        assert containment["duration"]["gaps"] == ["chat", "tickets", "mail"]
        assert containment["bad"] == "FeedError"

        asks = case(report, "asks-cycle")
        assert asks["idle"]["new"] == asks["idle"]["updated"] == {"int": "0"}
        assert asks["open_first"] == ["rr-12"]
        assert asks["marked_closed"] is True and asks["reply_closed"] is True
        assert asks["legacy_sibling"] == ["valid"]
        assert asks["cycle"]["not_stored"] == ["jira", "mail"]
        assert asks["next_cycle"]["not_stored"] == ["github", "jira", "mail"]
        assert asks["cli_cycle"]["exit"] == {"int": "1"}
        assert asks["lock"] is True

        team = case(report, "team-feed")
        assert team["pulled"]["new"] == {"int": "4"}
        assert len(team["rows"]) == 4
        assert team["rows"]["slack:C0ENG:1792591260.000300"]["team"] == ["Sam Lee"]
        assert team["rows"]["slack:C0ENG:1792591260.000300"]["triage"] == "fyi"
        assert team["without_scope"] == ["slack:C0ENG:1792591200.000100", "slack:D0SAM:1792591500.000200"]
        assert "titles hidden" in team["cli_feed"]["stdout"]

        local = case(report, "local-time-and-nan")
        assert local["now"] == "2026-10-21T16:00:00-04:00"
        assert local["naive"] == local["aware"] == "2026-10-21T16:00:00Z"
        assert local["work_minutes"] == {"int": "60"}
        assert [row["skipped"] for row in local["raw"]] == [{"int": "1"}, {"int": "1"}]
        assert local["transcript"] == ["skipped", "OverflowError"]

        failures = case(report, "source-failures")["result"]
        assert failures["sources"] == failures["errors"] == {"int": "3"}
        assert failures["new"] == {"int": "0"}
        assert all("[REDACTED]" in reason for reason in failures["reasons"])
        assert case(report, "source-failures")["lengths"] == [{"int": "207"}, {"int": "211"}, {"int": "209"}]
        answers.append(normalized(done.stdout, root))
    assert answers[0] == answers[1]
