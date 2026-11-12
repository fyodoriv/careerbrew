#!/usr/bin/env python3
"""Compare Playwright discovery and execution identities without printing test data."""

from __future__ import annotations

import json
import os
import sys
import tempfile
from collections import Counter
from pathlib import Path
from typing import Any


def _identities(report: Any, *, executed: bool) -> Counter[tuple[str, str]]:
    if not isinstance(report, dict) or report.get("errors") != [] or not isinstance(report.get("suites"), list):
        raise ValueError("invalid report")
    found: Counter[tuple[str, str]] = Counter()

    def walk(suite: Any) -> None:
        if not isinstance(suite, dict) or not isinstance(suite.get("specs", []), list) or not isinstance(suite.get("suites", []), list):
            raise ValueError("invalid suite")
        for spec in suite.get("specs", []):
            if not isinstance(spec, dict) or not isinstance(spec.get("id"), str) or not spec["id"] or not isinstance(spec.get("tests"), list) or not spec["tests"]:
                raise ValueError("invalid spec")
            for test in spec["tests"]:
                if not isinstance(test, dict) or not isinstance(test.get("projectId"), str) or test.get("expectedStatus") != "passed":
                    raise ValueError("invalid test")
                if executed:
                    results = test.get("results")
                    if test.get("status") != "expected" or not isinstance(results, list) or len(results) != 1 or not isinstance(results[0], dict) or results[0].get("status") != "passed" or results[0].get("retry") != 0:
                        raise ValueError("test did not pass once")
                found[(spec["id"], test["projectId"])] += 1
        for child in suite.get("suites", []):
            walk(child)

    for suite in report["suites"]:
        walk(suite)
    if not found or any(count != 1 for count in found.values()):
        raise ValueError("empty or duplicate identities")
    if len(found) > 10000 or any(len(spec) > 256 or len(project) > 256 for spec, project in found):
        raise ValueError("identity limit exceeded")
    return found


def write_receipt(path: str, identities: Counter[tuple[str, str]]) -> None:
    destination = Path(path)
    fd, name = tempfile.mkstemp(prefix=".browser-receipt-", dir=destination.parent)
    try:
        with os.fdopen(fd, "w") as output:
            json.dump({"version": 1, "identities": sorted(identities)}, output)
        os.link(name, destination, follow_symlinks=False)
    finally:
        os.unlink(name)


def matches(discovery: Any, execution: Any) -> bool:
    try:
        return _identities(discovery, executed=False) == _identities(execution, executed=True)
    except (ValueError, RecursionError):
        return False


def main() -> int:
    if len(sys.argv) != 3:
        print("browser test receipts: need discovery and execution reports", file=sys.stderr)
        return 1
    try:
        discovery, execution = (json.loads(Path(path).read_text()) for path in sys.argv[1:])
        identities = _identities(discovery, executed=False)
        if identities != _identities(execution, executed=True):
            raise ValueError("mismatch")
        receipt = os.environ.get("CAREERBREW_BROWSER_RECEIPT")
        if receipt:
            write_receipt(receipt, identities)
    except (OSError, ValueError, RecursionError):
        print("browser test receipts: incomplete or mismatched reports", file=sys.stderr)
        return 1
    print("browser test receipts: all discovered tests passed once")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
