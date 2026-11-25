"""rust-14 native live, health, review and notification component assertions."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust14.py"
CASES = {
    "source-roster",
    "live-cycle",
    "acquisition-health",
    "direct-health",
    "listen-drafts",
    "link-review",
    "review-selection",
    "notify-dedup",
    "notify-time",
    "notify-project-quiet",
    "notify-suggestions",
    "cli-adapters",
}
MARKERS = (b"LIVEZEBRA", b"LISTENZEBRA", b"PROJECTZEBRA", b"CANARYNOTIFY", b"synthetic-not-reported")


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "careerbrew-live", "--bin", "live-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "live-component"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_ENCRYPT": "on",
        "CAREERBREW_NOW": "2026-10-25T16:00:00+00:00",
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }


def case(report: dict, case_id: str) -> dict:
    return next(row for row in report["cases"] if row["id"] == case_id)["result"]


def normalized(data: bytes, root: Path) -> bytes:
    return data.replace(str(root).encode(), b"<owned>").replace(str(root.resolve()).encode(), b"<owned>")


def test_live_health_review_and_notification_contracts_match_python(candidate, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=180)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert all(marker not in done.stdout + done.stderr for marker in MARKERS)
        report = json.loads(done.stdout)
        ids = [row["id"] for row in report["cases"]]
        assert len(ids) == len(set(ids))
        assert set(ids) == CASES
        assert all("error" not in row["result"] for row in report["cases"])

        roster = case(report, "source-roster")
        assert [row["name"] for row in roster["sources"]] == ["chat", "paging"]
        assert roster["sources"][0]["origin"] == "user" and roster["sources"][1]["origin"] == "plugin:acme-live"
        assert roster["team"][0]["name"] == "Sam Lee" and roster["own_roster"] == []
        assert roster["bad"] == ["LiveError"] * 6

        cycle = case(report, "live-cycle")
        assert cycle["first"]["status"] == "ok" and cycle["first"]["verdicts"]["chat:1792929600.000100"] == "answer"
        assert cycle["first"]["verdicts"]["tickets:PROJ-7"] == "evidence"
        assert cycle["second"]["followups"] == [] and cycle["unchanged_state"] == {"seen": True, "followed": True}
        assert cycle["stopped_error"] == cycle["lock_error"] == "LiveError"
        assert cycle["restarted"]["sources"][1]["since"] == "2026-10-21T14:20:00Z"
        assert not any(cycle["clean"].values())

        acquisition = case(report, "acquisition-health")
        assert acquisition["first"]["sources"]["tickets"]["health"] == "healthy"
        assert acquisition["unchanged"]["sources"]["tickets"]["unchanged"] is True
        assert acquisition["inclusive"]["sources"]["tickets"]["age_seconds"] == {"float": "900.0"}
        assert acquisition["expired"]["not_stored"] == ["tickets"]
        assert acquisition["malformed"]["sources"]["tickets"]["health"] == "error"
        assert acquisition["future_second"]["sources"]["tickets"]["health"] == "unknown"
        assert acquisition["reversed"]["sources"]["tickets"]["health"] == "unknown"
        assert acquisition["optional"]["degraded"] == ["optional"]

        direct = case(report, "direct-health")
        assert direct["healthy"]["status"] == "healthy" and direct["healthy"]["mode"] == "direct"
        assert direct["inclusive"]["status"] == "healthy" and direct["stale"]["status"] == "stale"
        assert direct["read_only"] is True and direct["changed"]["status"] != "healthy"
        assert direct["failed"]["status"] == "error" and direct["exact"]["health"] == "healthy"
        assert direct["unknown"]["health"] == "unknown" and direct["invalid_has_path"] is False

        listening = case(report, "listen-drafts")
        assert listening["holding"] is True and all(listening["review_notes"].values())
        assert len(listening["updates"]) == 1 and listening["second"]["followups"] == []
        assert listening["first"]["review_sessions"] == 1 and len(listening["sessions"]) == 1
        assert listening["kept_text"] == "my own words, written earlier"
        assert not any(listening["clean"].values())

        links = case(report, "link-review")
        assert links["research"]["chat:1792936800.000100"][0]["opened"] is True
        assert links["research"]["chat:1792936860.000200"][0]["opened"] is False
        assert links["reference"][0]["opened"] is False
        assert links["re_review"][0]["actions"][-1] == "review session kept: one is already open for this request"

        review = case(report, "review-selection")
        assert review["shop"]["status"] == "ready" and review["shop"]["needed"] == {"int": "2"}
        assert [row["login"] for row in review["shop"]["people"]] == ["ana-dev", "ben-dev"]
        assert review["tools"]["status"] == "short" and review["extra"]["status"] == "ready"
        assert review["patterns"] == [True, True, False, False, True]
        assert review["last"] == ["src-dev", "org/team"] and review["mismatch"] == "ReviewAskError"

        dedup = case(report, "notify-dedup")
        assert dedup["priorities"] == {"pagerduty:P1": {"int": "5"}, "slack:C1:1.0": {"int": "4"}, "github:pr-9": {"int": "4"}, "slack:C3:3.0": {"int": "4"}}
        assert dedup["first"]["reason"] == "sent" and dedup["repeated"]["reason"] == ""
        assert dedup["first_view"]["items"][0]["count"] == {"int": "1"}
        assert dedup["closed_view"]["items"][0]["status"] == "new"
        assert dedup["reopened_view"]["items"][0]["status"] == "new" and dedup["sealed"] is True
        assert dedup["notifier_clean"] is True

        timed = case(report, "notify-time")
        assert timed["held"]["reason"] == "quiet" and timed["released"]["reason"] == "sent"
        assert timed["away_calls"][0][1] == "alerter"
        assert timed["capped"]["reason"] == "cap" and timed["next_day"]["reason"] == "sent"
        assert timed["snoozed"]["items"][0]["status"] == "snoozed"
        assert timed["early"]["reason"] == "" and timed["due"]["reason"] == "sent"

        quiet = case(report, "notify-project-quiet")
        assert quiet["candidates"] == [["slack:C1:1.0", True], ["slack:C1:2.0", False]]
        assert quiet["second_refs"] == ["slack:C1:2.0"] and quiet["third_refs"] == ["slack:C1:3.0"]
        assert quiet["final_refs"] == ["slack:C1:1.0"] and quiet["presented"] == ""
        assert quiet["held"]["items"][0]["popup"] == "held:project" and quiet["returned"]["reason"] == "sent"

        suggestions = case(report, "notify-suggestions")
        assert suggestions["late"] == "" and suggestions["accepted"]["status"] == "done"
        assert suggestions["accepted"]["resolved"] == "accepted" and suggestions["sealed"] is True
        assert not any(suggestions["clean"].values())

        adapters = case(report, "cli-adapters")
        assert adapters["started"]["exit"] == adapters["cycled"]["exit"] == adapters["stopped"]["exit"] == {"int": "0"}
        assert adapters["review"]["exit"] == {"int": "0"} and adapters["short"]["exit"] == {"int": "1"}
        assert "draft hidden" in adapters["review"]["stdout"] and "ana-dev" not in adapters["review"]["stdout"]
        assert adapters["checked"]["exit"] == adapters["listed"]["exit"] == adapters["notify_status"]["exit"] == {"int": "0"}
        answers.append(normalized(done.stdout, root))
    assert answers[0] == answers[1]
