"""Compare synthetic Playwright discovery and execution receipts."""

import copy
import importlib.util
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "browser-test-receipts.py"
spec = importlib.util.spec_from_file_location("browser_test_receipts", SCRIPT)
assert spec and spec.loader
receipts = importlib.util.module_from_spec(spec)
spec.loader.exec_module(receipts)


def report(*, discovery):
    def item(ident):
        test = {"projectId": "", "expectedStatus": "passed", "status": "skipped" if discovery else "expected", "results": [] if discovery else [{"status": "passed", "retry": 0}]}
        return {"id": ident, "tests": [test]}

    return {"suites": [{"specs": [item("a")], "suites": [{"specs": [item("b")]}]}], "errors": []}


def test_matching_discovery_and_execution_is_accepted():
    assert receipts.matches(report(discovery=True), report(discovery=False))


@pytest.mark.parametrize("case", ["missing", "duplicate", "skipped", "failed"])
def test_incomplete_or_unsuccessful_execution_is_rejected(case):
    discovery, executed = report(discovery=True), report(discovery=False)
    first = executed["suites"][0]["specs"]
    if case == "missing":
        first.clear()
    elif case == "duplicate":
        first.append(copy.deepcopy(first[0]))
    elif case == "skipped":
        first[0]["tests"][0]["results"][0]["status"] = "skipped"
    else:
        first[0]["tests"][0]["results"][0]["status"] = "failed"
    assert not receipts.matches(discovery, executed)


@pytest.mark.parametrize("case", ["unexpected", "discovery_duplicate", "expected_skip", "outcome", "retry", "multiple", "errors", "empty"])
def test_other_invalid_reports_are_rejected(case):
    discovery, executed = report(discovery=True), report(discovery=False)
    item = executed["suites"][0]["specs"][0]
    test = item["tests"][0]
    if case == "unexpected":
        item["id"] = "unexpected"
    elif case == "discovery_duplicate":
        discovery["suites"][0]["specs"].append(copy.deepcopy(discovery["suites"][0]["specs"][0]))
    elif case == "expected_skip":
        test["expectedStatus"] = "skipped"
    elif case == "outcome":
        test["status"] = "unexpected"
    elif case == "retry":
        test["results"][0]["retry"] = 1
    elif case == "multiple":
        test["results"].append(copy.deepcopy(test["results"][0]))
    elif case == "errors":
        executed["errors"].append({"message": "synthetic"})
    else:
        discovery["suites"] = []
    assert not receipts.matches(discovery, executed)


def test_private_receipt_does_not_replace_existing_file(tmp_path):
    path = tmp_path / "receipt.json"
    identities = receipts._identities(report(discovery=True), executed=False)
    receipts.write_receipt(str(path), identities)
    assert path.stat().st_mode & 0o777 == 0o600
    with pytest.raises(FileExistsError):
        receipts.write_receipt(str(path), identities)


def test_failed_receipt_write_leaves_no_final_or_temporary_file(tmp_path, monkeypatch):
    tmp_path = tmp_path / "receipts"
    tmp_path.mkdir()
    path = tmp_path / "receipt.json"
    identities = receipts._identities(report(discovery=True), executed=False)

    def fail_write(*args, **kwargs):
        raise OSError("synthetic write failure")

    monkeypatch.setattr(receipts.json, "dump", fail_write)
    with pytest.raises(OSError):
        receipts.write_receipt(str(path), identities)
    assert list(tmp_path.iterdir()) == []


def test_receipt_refuses_existing_symlink(tmp_path):
    target = tmp_path / "target"
    target.write_text("unchanged")
    path = tmp_path / "receipt.json"
    path.symlink_to(target)
    with pytest.raises(FileExistsError):
        receipts.write_receipt(str(path), receipts._identities(report(discovery=True), executed=False))
    assert target.read_text() == "unchanged"


def test_cli_writes_only_a_matching_private_receipt(tmp_path):
    import json
    import os
    import subprocess
    import sys

    discovery = tmp_path / "discovery.json"
    execution = tmp_path / "execution.json"
    destination = tmp_path / "receipt.json"
    discovery.write_text(json.dumps(report(discovery=True)))
    execution.write_text(json.dumps(report(discovery=False)))
    result = subprocess.run(
        [sys.executable, str(SCRIPT), str(discovery), str(execution)],
        env={**os.environ, "CAREERBREW_BROWSER_RECEIPT": str(destination)},
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0
    assert json.loads(destination.read_text())["version"] == 1
    assert destination.stat().st_mode & 0o777 == 0o600
    assert not list(tmp_path.glob(".browser-receipt-*"))
