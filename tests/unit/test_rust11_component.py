"""rust-11 native project import, source identity and memory-sync assertions."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust11.py"
CASES = {"source-identity", "project-import", "memory-in", "memory-out", "memory-removal", "research-cli"}


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "-p", "careerbrew-memory", "--bin", "memory-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "memory-component"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_ENCRYPT": "off",
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


def test_import_identity_sync_lock_and_cli_contracts_match_python(candidate, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=120)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert b"PROJECTZEBRA" not in done.stdout + done.stderr
        report = json.loads(done.stdout)
        ids = [row["id"] for row in report["cases"]]
        assert len(ids) == len(set(ids))
        assert set(ids) == CASES
        assert all("error" not in row["result"] for row in report["cases"])

        identity = case(report, "source-identity")
        assert identity["closest"] == "billing"
        assert identity["tied"] == identity["bad"] == "ProjectError" or identity["bad"] == "ReadError"
        assert identity["detected"][5] == ["url", "https://github.com/acme/billing/pull/41"]
        assert identity["detected"][6] == ["url", "https://github.example.com/acme/billing/issues/7"]
        assert identity["detected"][7] == ["repo", "acme/billing"]
        assert identity["items"][5] == ["acme", "billing", "pull", "41"]
        assert identity["after"] == [True, False, True, True, False, True]
        assert identity["detected"][-1] == ["url", "https://github.com/acme/billing/pull/٤١"]
        assert identity["items"][-1] == ["acme", "billing", "pull", "٤١"]
        assert len(identity["unicode_digest"]) == 24

        imported = case(report, "project-import")
        assert imported["dry_empty"] is True
        assert imported["first"]["rows"] == {"int": "6"}
        assert imported["first"]["sources"] == {"int": "5"}
        assert imported["first"]["ignored"] == ["owner"]
        assert imported["initial"]["billing"]["parent"] == "payments"
        assert imported["initial"]["billing"]["links"] == ["ledger", "invoices"]
        assert imported["initial"]["checkout-analytics"]["parent"] == "checkout"
        assert imported["second"]["updated"] == ["billing"]
        assert imported["second"]["unchanged"] == ["payments"]
        assert imported["checked"]["title"] == "Billing revamp phase 2"
        assert imported["checked"]["sources"][0][2:] == [True, "epic 3 of 5"]
        assert imported["cutline"]["added"] == ["search", "search-ranking"]
        assert imported["remote"]["added"] == ["reports"]
        assert imported["remote_bom"]["rows"] == {"int": "2"}
        assert imported["remote_bom"]["unchanged"] == ["billing", "reports"]
        assert imported["bad_cli"]["exit"] == {"int": "2"}
        assert imported["unchanged"] is True
        assert imported["sealed"] is True and imported["sealed_readback"] is True and imported["canary_hidden"] is True
        assert "nothing written" in imported["cli"]["stdout"]

        incoming = case(report, "memory-in")
        assert incoming["changed"]["files"] == {"int": "1"}
        assert incoming["changed"]["sources"] == {"int": "5"}
        assert incoming["repeat"]["files"] == incoming["touched"]["files"] == {"int": "0"}
        assert incoming["same_mtime"]["files"] == {"int": "1"}
        assert incoming["own_write"]["files"] == {"int": "0"}
        assert incoming["owner_after"]["status"] == {"int": "1"}
        assert incoming["status"] == "owner final"
        assert incoming["links"] == ["search"]
        refs = {tuple(row[:2]) for row in incoming["resumed_sources"]}
        assert ("url", "https://github.com/acme/billing/pull/41") in refs
        assert ("repo", "acme/billing") not in refs
        assert ("tracker", "PROJ-10") in refs
        assert ("url", "https://example.com/off") not in refs
        assert not {("tracker", "UTF-8"), ("tracker", "ISO-8601")} & refs

        outgoing = case(report, "memory-out")
        assert outgoing["denied"]["notes"] == {"int": "0"}
        assert "business reading is off" in outgoing["denied"]["reason"]
        assert outgoing["first"]["notes"] == {"int": "7"}
        assert outgoing["second"]["notes"] == outgoing["idle"]["notes"] == {"int": "0"}
        assert outgoing["idle_state"] is True and outgoing["rebuilt"]["notes"] == {"int": "0"}
        assert outgoing["other_unmatched"] is True
        assert outgoing["child"]["notes"] == {"int": "1"}
        assert outgoing["child_header"] is True
        assert outgoing["own_write_reread"]["files"] == {"int": "0"}
        assert len(outgoing["lines"]) == 7
        assert outgoing["lines"][0].endswith("<https://github.com/acme/billing/pull/41>")
        assert "PROJ-12: earlier instant" in outgoing["lines"][2]
        assert "PROJ-11: later instant" in outgoing["lines"][3]
        assert outgoing["lines"][6].startswith("- 2026-10-22 jira: PROJ-13: local date")

        removal = case(report, "memory-removal")
        assert removal["seeded"]["notes"] == {"int": "1"}
        assert removal["removed"]["removed"] == {"int": "1"}
        assert removal["kept_append"] is True and removal["append_source"] is True
        assert removal["mode"] == {"int": str(0o600)}
        assert removal["refused"]["removed"] == {"int": "0"}
        assert removal["owner_kept"] is True
        assert removal["retry"]["removed"] == {"int": "1"}
        assert removal["final_has_note"] is False

        research_cli = case(report, "research-cli")
        assert research_cli["stamps_unchanged"] is True
        assert research_cli["absorbed"]["ref"] == "https://github.com/acme/billing/pull/41"
        assert research_cli["absorbed"]["read"] is False
        assert research_cli["cli_absorb"]["exit"] == {"int": "0"}
        assert research_cli["bad_since"]["exit"] == {"int": "2"}
        assert "nothing stamped" in research_cli["cli_since"]["stdout"]
        assert len(research_cli["alias"]) == 1
        assert research_cli["alias"][0]["ref"] == "https://github.com/acme/billing/pull/41"
        assert research_cli["cli_alias"]["exit"] == {"int": "0"}
        answers.append(normalized(done.stdout, root))
    assert answers[0] == answers[1]
