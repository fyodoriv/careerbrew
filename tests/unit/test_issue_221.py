"""Issue #221: in a fresh worktree with no web/node_modules, the gate names the fix at once."""

import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def _repo(tmp_path: Path) -> Path:
    repo = tmp_path / "repo"
    (repo / "scripts").mkdir(parents=True)
    (repo / "web").mkdir()
    shutil.copy(ROOT / "scripts" / "check.sh", repo / "scripts" / "check.sh")
    (repo / "web" / "package.json").write_text("{}")
    subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
    return repo


def test_a_missing_web_node_modules_stops_the_gate_with_the_fix(tmp_path):
    r = subprocess.run(["bash", "scripts/check.sh", "fast"], cwd=_repo(tmp_path), capture_output=True, text=True, timeout=60, check=False)
    assert r.returncode == 1
    assert "web/node_modules is missing -> fix: scripts/setup.sh" in r.stderr


def test_with_web_node_modules_the_gate_goes_on_to_its_steps(tmp_path):
    repo = _repo(tmp_path)
    (repo / "web" / "node_modules").mkdir()
    r = subprocess.run(["bash", "scripts/check.sh", "fast"], cwd=repo, capture_output=True, text=True, timeout=60, check=False)
    assert "web/node_modules is missing" not in r.stderr
