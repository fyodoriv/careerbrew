"""Issue #26: the gate checks repo content, so it passes on a corporate-role machine."""

import os
import subprocess
from pathlib import Path

from careerbrew import machine
from careerbrew.cli import main

ROOT = Path(__file__).resolve().parents[2]


def test_public_cards_check_runs_on_corporate_role(home, capsys):
    machine.set_role("corporate", "")
    assert main(["cards", "public-check"]) == 0
    assert "public cards:" in capsys.readouterr().out


def test_other_cards_commands_stay_personal_only(home, capsys):
    machine.set_role("corporate", "")
    assert main(["cards", "stats"]) != 0
    assert "personal-only" in capsys.readouterr().out


def test_repo_content_gate_steps_pass_with_corporate_role(home):
    machine.set_role("corporate", "")
    env = {**os.environ, "CAREERBREW_HOME": str(home)}
    for cmd in (["careerbrew", "cards", "public-check"], ["careerbrew", "exercises", "check"]):
        r = subprocess.run([str(ROOT / ".venv" / "bin" / cmd[0]), *cmd[1:]], capture_output=True, text=True, env=env, check=False)
        assert r.returncode == 0, (cmd, r.stdout[-300:])
