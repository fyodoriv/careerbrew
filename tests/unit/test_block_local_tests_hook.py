"""The block-local-tests hook keeps tests on hosted CI (docs/ci.md)."""

import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


@pytest.mark.skipif(shutil.which("perl") is None, reason="the hook needs perl")
def test_the_hook_fixture_passes() -> None:
    run = subprocess.run(
        ["bash", str(ROOT / "hooks" / "block-local-tests.test.sh")],
        capture_output=True,
        text=True,
        timeout=120,
        env={"PATH": "/usr/bin:/bin:/usr/local/bin", "HOME": str(ROOT), "TMPDIR": "/tmp"},
    )
    assert run.returncode == 0, run.stdout + run.stderr
    assert "block-local-tests: all passed" in run.stdout


def test_the_agentfile_registers_the_hook() -> None:
    text = (ROOT / "Agentfile.yaml").read_text()
    assert "hooks/block-local-tests.sh" in text
    assert "event: PreToolUse" in text
