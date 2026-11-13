"""The hosted gate workflow keeps the pins the gate's frozen fixtures need."""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/ci.yml"
REFERENCE = ROOT / "xtask/tests/fixtures/capture/http-reference-rules-promise-v3.json"


def test_the_runner_python_is_the_one_in_the_frozen_http_reference():
    frozen = set(re.findall(r"Python/(\d+\.\d+\.\d+)", REFERENCE.read_text()))
    assert len(frozen) == 1, frozen
    pinned = re.findall(r'python-version: "([^"]+)"', WORKFLOW.read_text())
    assert pinned == [frozen.pop()]
