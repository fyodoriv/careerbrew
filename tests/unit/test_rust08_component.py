"""rust-08 native HTTP-source, calendar, mail and connect assertions."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust08.py"
CASES = {"http-policy", "connector-evidence", "calendar-recurrence", "mail-readers", "source-isolation", "cli-connect"}


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "-p", "careerbrew-sources", "--bin", "sources-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "sources-component"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_ENCRYPT": "off",
        "CAREERBREW_NOW": "2026-10-19T10:00:00+00:00",
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }


def case(report: dict, case_id: str) -> dict:
    return next(row for row in report["cases"] if row["id"] == case_id)["result"]


def test_http_source_calendar_mail_and_connect_contracts_match_python(candidate, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=120)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        streams = done.stdout + done.stderr
        for marker in (b"synthetic-not-reported", b"synthetic-jira-not-reported", b"synthetic-access-token", b"synthetic-refresh-token"):
            assert marker not in streams
        report = json.loads(done.stdout)
        ids = [row["id"] for row in report["cases"]]
        assert len(ids) == len(set(ids))
        assert set(ids) == CASES

        http = case(report, "http-policy")
        assert http["denied"] == "ConnectorError"
        assert http["redirects"] == [
            "https://api.example.com/b",
            "ConnectorError",
            "ConnectorError",
            "https://api.example.org/b",
            "ConnectorError",
        ]
        assert http["limits"] == {"jira_pages": {"int": "50"}, "pagerduty_pages": {"int": "10"}, "reply_pages": {"int": "3"}}
        assert http["missing"] == "FileNotFoundError"
        assert http["status"] == "HTTPError"

        source = case(report, "connector-evidence")
        assert [row["title"] for row in source["evidence"]["outlook"]] == ["Organized: Design review"]
        assert source["evidence"]["outlook"][0]["at"] == "2026-10-19T19:00:00Z"
        assert [row["tags"] for row in source["evidence"]["jira"]] == [["quality"], ["impact"], ["collaboration", "ownership"]]
        assert source["jira_pages"] == {"count": {"int": "101"}, "first": "jira:PROJ-0", "last": "jira:PROJ-100", "calls": {"int": "2"}, "cursor": True}
        assert source["jira_rejected"] == "ConnectorError"
        assert source["pagerduty"]["calls"] == {"int": "2"}
        assert source["pagerduty"]["second_offset"] is True
        assert source["personal_slack"] == []
        assert source["status_missing"]["ready"] is False
        assert source["status_ready"]["ready"] is True

        cal = case(report, "calendar-recurrence")
        assert [row["title"] for row in cal["work"]] == ["Team standup", "1:1 with manager", "Company holiday in another office", "Optional talk"]
        assert cal["series_lengths"] == [{"int": "1"}, {"int": "0"}, {"int": "1"}]
        occurrence = cal["series"][2][0]
        assert occurrence["personal"] is True and occurrence["led"] is True and occurrence["recurring"] is True
        assert occurrence["start"] == "2026-10-21T09:00:00-04:00"
        assert [row["title"] for row in cal["graph"]] == ["Design review", "Focus time"]
        assert cal["graph"][1]["busy"] is False
        assert cal["exception_lengths"] == [{"int": "1"}, {"int": "1"}]
        moved = cal["exceptions"][0][0]
        assert moved["title"] == "Moved DST review"
        assert moved["start"] == "2026-11-01T10:30:00-05:00"
        assert moved["end"] == "2026-11-01T11:30:00-05:00"
        assert [row["title"] for row in cal["exceptions"][1]] == ["Cancelled series slot"]

        mail = case(report, "mail-readers")
        assert [row["id"] for row in mail["graph"]] == ["<a1@mail.example.com>", "<a2@mail.example.com>"]
        assert mail["classes"]["linkedin_alert"] == ["<m1@linkedin.com>"]
        assert mail["classes"]["linkedin_application"] == ["<m2@linkedin.com>"]
        assert mail["classes"]["interview"] == ["<a1@mail.example.com>", "<imap@synthetic.example>"]
        assert mail["near_miss_classes"]["other"] == ["assessments", "offering"]
        assert mail["near_miss_classes"]["interview"] == ["assessment", "offer", "take-home", "take home", "next steps"]
        assert mail["corporate_accounts"] == ["work"]
        assert mail["imap_calls"][-1] == "logout"
        assert "BODY.PEEK[HEADER.FIELDS (MESSAGE-ID FROM SUBJECT DATE)]" in mail["imap_calls"][-2]
        assert mail["wrong_host"] == "ConnectorError"

        isolated = case(report, "source-isolation")
        assert isolated["calendar_titles"] == ["Focus time", "Design review"]
        assert isolated["calendar_errors"] == ["ics: FileNotFoundError"]
        assert isolated["mail_ids"] == ["<a1@mail.example.com>", "<a2@mail.example.com>"]
        assert isolated["mail_errors"] == ["bad: FileNotFoundError"]
        assert isolated["literal_ics"] == isolated["literal_token"] == "SecretError"
        restricted = isolated["restrictive"]
        assert restricted["source"] == restricted["calendar"] == restricted["mail"] == "ConnectorError"
        assert restricted["connect"]["exit"] == {"int": "1"}
        assert restricted["connect"]["saved"] == []
        assert "host api.github.com is not in the plugin policy allowed_hosts" in restricted["connect"]["stdout"]

        handlers = case(report, "cli-connect")
        assert {name: row["exit"] for name, row in handlers.items() if isinstance(row, dict) and "exit" in row} == {
            "scoped": {"int": "0"},
            "bad": {"int": "1"},
            "missing": {"int": "2"},
            "interactive": {"int": "0"},
            "denied": {"int": "3"},
            "outlook": {"int": "0"},
            "declined": {"int": "1"},
            "no_client": {"int": "2"},
            "manual": {"int": "0"},
            "piped": {"int": "3"},
        }
        assert handlers["scoped"]["saved"][0]["base_url"] == "https://api.atlassian.com/ex/jira/synthetic-cloud-1"
        assert handlers["bad"]["saved"] == handlers["denied"]["saved"] == handlers["declined"]["saved"] == []
        assert handlers["outlook"]["rotated"] is True
        assert handlers["outlook"]["saved"][0]["token"] == "keychain:careerbrew.outlook.refresh"
        assert "SYNTH123" in handlers["outlook"]["stdout"]
        assert "SYNTH123" not in handlers["piped"]["stdout"]
        answers.append(done.stdout)
    assert answers[0] == answers[1]
