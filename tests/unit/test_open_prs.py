"""scripts/open-prs.sh: checkup always lists open PRs with a next step, and never hides a gh failure."""

import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def _run(tmp_path, gh_body: str) -> subprocess.CompletedProcess[str]:
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    (bin_dir / "gh").write_text("#!/bin/sh\n" + gh_body)
    (bin_dir / "gh").chmod(0o755)
    env = {**os.environ, "GH_BIN": str(bin_dir / "gh")}
    return subprocess.run(["bash", str(ROOT / "scripts" / "open-prs.sh")], capture_output=True, text=True, env=env, check=False)


def test_no_open_prs_passes(tmp_path):
    r = _run(tmp_path, "exit 0\n")
    assert r.returncode == 0 and "no open pull requests" in r.stdout


def test_open_draft_with_conflicts_fails_with_next_step(tmp_path):
    r = _run(tmp_path, "printf '1\\ttrue\\tCONFLICTING\\tdev/x\\t2026-10-19\\tfeat: big work\\n'\n")
    assert r.returncode == 1
    assert "OPEN  #1 feat: big work" in r.stdout and "draft, conflicts with main" in r.stdout
    assert "merge main into dev/x and resolve the conflicts" in r.stdout and "mark it ready" in r.stdout


def test_gh_failure_is_loud_not_silent(tmp_path):
    r = _run(tmp_path, "echo 'tls: failed to verify certificate' >&2; exit 1\n")
    assert r.returncode == 2 and "FAIL  could not list open PRs: tls" in r.stdout
