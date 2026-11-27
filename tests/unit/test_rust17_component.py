"""rust-17 native day results, handoff, sent-PM receipt and PM lesson assertions."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust17.py"
CASES = {"difflib-vectors", "results-records", "handoff-records", "pm-sent-receipts", "pm-lessons"}
MARKERS = (b"SYNTHCANARY", b"private-name")


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "careerbrew-day", "--bin", "day-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "day-component"


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


def i(value: int) -> dict:
    return {"int": str(value)}


def test_day_records_handoff_sent_pm_and_lessons_match_python(candidate, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=180)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert all(marker not in done.stdout + done.stderr for marker in MARKERS), engine
        report = json.loads(done.stdout)
        ids = [row["id"] for row in report["cases"]]
        assert len(ids) == len(set(ids)) and set(ids) == CASES
        assert all("error" not in row["result"] for row in report["cases"])

        vectors = case(report, "difflib-vectors")
        assert vectors["ratios"][:5] == [{"float": "1.0"}, {"float": "0.0"}, {"float": "0.9259259259259259"}, {"float": "0.7555555555555555"}, {"float": "0.75"}]
        assert vectors["ratios"][6] == vectors["ratios"][7]
        assert [op[0] for op in vectors["opcodes_b"]] == ["insert", "equal", "replace"]

        results = case(report, "results-records")
        assert results["added"] == i(2) and results["repeat"] == i(0) and results["repeat_writes"] == i(0)
        assert results["moved_at"] == i(0) and results["corrected"] == i(2) and results["draft"] == i(1)
        assert results["titles_plain"] == ["Fix login flow <canary>", "Review search v2", "Wrote notes"]
        assert results["summary"] == {"note": i(1), "pr": i(1), "review": i(1)} and results["empty_summary"] == {}
        assert [row["id"] for row in results["evidence"]] == ["e1", "e3", "e6"]
        assert results["sealed"] is True and results["plain_absent"] is True

        handoff = case(report, "handoff-records")
        assert handoff["errors"] == ["ValueError"] * 3 and handoff["edge_len"] == i(200)
        assert handoff["extra"]["text"] == "Ask the team" and "empty" not in handoff["extra"] and "fixed" not in handoff["extra"]
        assert handoff["again_same"] is True and handoff["repeat_writes"] == i(0) and handoff["owned_repeat_writes"] == i(0)
        assert [row["owner"] for row in handoff["after_replace"]] == ["", "wrap", "wrap"]
        assert handoff["done"] == [True, False]
        assert handoff["latest"][0] == "2026-10-30" and handoff["latest_none"] == [None, []]
        assert handoff["sealed"] is True and handoff["plain_absent"] is True

        sent = case(report, "pm-sent-receipts")
        assert sent["no_target"] == [None, None] and sent["no_target_writes"] == i(0)
        assert sent["manual"] == sent["malformed"] == "Manual <canary> update"
        assert sent["receipt_id"] == {"id": "slack:C0SYN:200.5"}
        assert "(edited <canary>)" in sent["receipt_text"]
        assert sent["learned"] == [True, False, True, True, False]
        assert sent["idle_writes"] == i(0) and sent["revision_ok"] is True
        assert sent["channel"] == "Billing: shipped invoices today.\nSearch: tuned ranking more."
        assert sent["dm"] == "Billing: shipped invoices.\nSearch: DM to the right person."
        assert sent["none"] is None and sent["cleared"] is None
        assert sent["after_manual"] == "Billing: manual correction."
        assert sent["files"]["2026-10-27"] == [True, False, False]
        assert [row[0] for row in sent["history"]] == ["2026-10-27", "2026-10-29", "2026-10-30"]
        assert len(sent["locks"]) == 6
        assert sent["sealed"] is True and sent["plain_absent"] is True

        lessons = case(report, "pm-lessons")
        assert lessons["after_one"]["cut"] == [] and lessons["after_two"]["cut"] == ["Ops"]
        assert lessons["after_two"]["added"] == {"meeting": i(2), "other": i(2)}
        assert lessons["unchanged_writes"] == i(0) and lessons["web_after_sent_writes"] == i(0)
        assert lessons["applied"][0].startswith("Ledger:") and "Ops: status check: no action needed." not in lessons["applied"]
        assert lessons["all_cut"] == ["Ops: status check: no action needed."]
        assert lessons["left_out"] == ["Ops"]
        assert lessons["projection"][0] == ["Ops"] and lessons["projection_present"][0] == []
        assert lessons["hints"] == ["announcement", "meeting", "own post", "other"]
        assert lessons["reset_had"] == i(3) and lessons["reset_missing"] == "ValueError"
        assert lessons["after_reset_cut"] == []
        assert len(lessons["kept_days"]) == 14
        assert lessons["sealed"] is True and lessons["plain_absent"] is True
        answers.append(done.stdout)
    assert answers[0] == answers[1]


def test_the_component_refuses_an_unknown_case(candidate, tmp_path):
    done = subprocess.run([str(candidate), "--case", "nope"], cwd=ROOT, env=synthetic_env(tmp_path), capture_output=True, timeout=60)
    assert done.returncode == 2
    assert done.stdout == b""
