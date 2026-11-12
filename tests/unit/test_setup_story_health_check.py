"""The setup story must not lose a successful health match to SIGPIPE."""

import subprocess
from pathlib import Path

import pytest

STORY = Path(__file__).resolve().parents[1] / "e2e/us-01-setup.sh"
NEEDLE = "doctor: all checks passed"


def assertion() -> str:
    lines = [line for line in STORY.read_text().splitlines() if "FAIL: setup did not end healthy" in line]
    assert len(lines) == 1
    return lines[0]


def run_check(output: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["bash", "-o", "pipefail", "-c", "out=$(cat)\n" + assertion()], input=output, text=True, capture_output=True, timeout=10, check=False)


@pytest.mark.parametrize("prefix", ["", "synthetic line\n" * 100000], ids=["beginning", "middle"])
def test_matching_health_line_is_accepted_with_large_output(prefix):
    result = run_check(prefix + NEEDLE + "\n" + "synthetic line\n" * 200000)
    assert result.returncode == 0, "a present health line must be accepted"


def test_missing_health_line_is_refused():
    result = run_check("synthetic line\n" * 200000)
    assert result.returncode != 0
    assert "FAIL: setup did not end healthy" in result.stdout
