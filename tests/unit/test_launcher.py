"""Issue #304: commands and skills must not hardcode ~/apps/careerbrew. A launcher on PATH finds the real checkout."""

import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "install-launcher.sh"


def _install(tmp_path, root=ROOT, extra=None):
    env = {"PATH": "/usr/bin:/bin", "HOME": str(tmp_path), "CAREERBREW_BIN_DIR": str(tmp_path / "bin"), **(extra or {})}
    return subprocess.run(["bash", str(SCRIPT), str(root)], capture_output=True, text=True, env=env, timeout=30)


def test_the_launcher_prints_its_checkout_and_runs_careerbrew_there(tmp_path):
    r = _install(tmp_path, extra={"CAREERBREW_LAUNCHER_FORCE": "1"})
    assert r.returncode == 0, r.stderr
    launcher = tmp_path / "bin" / "careerbrew"
    assert os.access(launcher, os.X_OK)
    where = subprocess.run([str(launcher), "where"], capture_output=True, text=True, timeout=10)
    assert where.stdout.strip() == str(ROOT)
    assert 'uv run --project "$root" careerbrew' in launcher.read_text()


def test_a_linked_worktree_does_not_take_over_the_launcher(tmp_path):
    other = tmp_path / "wt"
    other.mkdir()
    (other / ".git").write_text("gitdir: /elsewhere/.git/worktrees/wt\n")
    r = _install(tmp_path, root=other)
    assert r.returncode == 0, r.stderr
    assert not (tmp_path / "bin" / "careerbrew").exists()
    assert "linked worktree" in r.stdout


def test_a_careerbrew_file_it_did_not_write_is_kept(tmp_path):
    (tmp_path / "bin").mkdir()
    (tmp_path / "bin" / "careerbrew").write_text("#!/bin/sh\necho mine\n")
    r = _install(tmp_path, extra={"CAREERBREW_LAUNCHER_FORCE": "1"})
    assert r.returncode == 0, r.stderr
    assert (tmp_path / "bin" / "careerbrew").read_text() == "#!/bin/sh\necho mine\n"
    assert "kept" in r.stdout


def test_setup_installs_the_launcher():
    assert "scripts/install-launcher.sh" in (ROOT / "scripts" / "setup.sh").read_text()


def test_no_command_or_skill_hardcodes_the_checkout_path():
    files = [*(ROOT / "commands").glob("*.md"), *(ROOT / "skills").glob("*/SKILL.md")]
    hits = [str(f.relative_to(ROOT)) for f in files if "~/apps/careerbrew" in f.read_text()]
    assert hits == [], f"use `careerbrew …` and `$(careerbrew where)` instead: {hits}"
