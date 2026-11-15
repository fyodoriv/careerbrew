"""The checkup reads the hosted gate's result on main instead of running the gate here (docs/ci.md).

scripts/hosted-gate.sh runs against a fake `gh` on PATH; scripts/checkup.sh runs in a stub repo.
"""

import os
import shutil
import subprocess
from pathlib import Path

import pytest

from careerbrew import report

from .test_run_pages import _stub_repo

ROOT = Path(__file__).resolve().parents[2]


TIP = "abcdef1234567890abcdef1234567890abcdef12"


def _fake_gh(tmp_path: Path, ci: str, readiness: str = "21\\tcompleted\\tsuccess\\n", code: int = 0) -> dict[str, str]:
    """A gh that answers `run list` for each merge workflow with the given rows (id, status, conclusion)."""
    tools = tmp_path / "bin"
    tools.mkdir(exist_ok=True)
    (tools / "gh").write_text(f"#!/bin/sh\necho \"$@\" >> '{tmp_path}/gh-args'\ncase \"$*\" in *ci.yml*) printf '{ci}' ;; *public-readiness.yml*) printf '{readiness}' ;; esac\nexit {code}\n")
    (tools / "gh").chmod(0o755)
    # No shell startup file: it can put another gh first on PATH (AGENTS.md, "Gotchas").
    env = {k: v for k, v in os.environ.items() if k not in ("BASH_ENV", "ENV")}
    return {**env, "PATH": f"{tools}{os.pathsep}/usr/bin:/bin", "HOSTED_GATE_TIP": TIP}


def _hosted(env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["/bin/bash", str(ROOT / "scripts/hosted-gate.sh")], env=env, capture_output=True, text=True, timeout=30, check=False)


def test_green_needs_both_merge_workflows_on_the_tip(tmp_path):
    r = _hosted(_fake_gh(tmp_path, "11\\tcompleted\\tsuccess\\n"))
    assert r.returncode == 0, r.stdout + r.stderr
    assert r.stdout.strip() == "hosted-gate: main abcdef12 passed (ci.yml run 11, public-readiness.yml run 21)"
    asked = (tmp_path / "gh-args").read_text().splitlines()
    # The tip's own push runs only: an older commit's run, or a fork's pull request from its main, never counts.
    assert [line.split("--workflow ")[1].split()[0] for line in asked] == ["ci.yml", "public-readiness.yml"]
    assert all(f"--branch main --event push --commit {TIP}" in line for line in asked), asked


def test_a_red_run_fails_with_the_command_that_shows_its_log(tmp_path):
    r = _hosted(_fake_gh(tmp_path, "11\\tcompleted\\tsuccess\\n", readiness="22\\tcompleted\\tfailure\\n"))
    assert r.returncode == 1
    assert "public-readiness.yml failure (run 22) -> read it: gh run view 22 --log-failed" in r.stdout


def test_a_tip_still_running_is_pending_not_green(tmp_path):
    r = _hosted(_fake_gh(tmp_path, "14\\tin_progress\\t-\\n"))
    assert r.returncode == 1
    assert r.stdout.strip() == "hosted-gate: main abcdef12 pending: ci.yml run 14 is in_progress -> wait: gh run watch 14"


@pytest.mark.parametrize("rows", ["", "13\\tcompleted\\tcancelled\\n12\\tcompleted\\tskipped\\n"], ids=["no run yet", "all cancelled"])
def test_a_tip_with_no_finished_run_is_pending(tmp_path, rows):
    r = _hosted(_fake_gh(tmp_path, rows))
    assert r.returncode == 1
    assert "pending: ci.yml has no run for abcdef12 yet" in r.stdout


def test_a_cancelled_rerun_does_not_hide_the_real_result(tmp_path):
    r = _hosted(_fake_gh(tmp_path, "13\\tcompleted\\tcancelled\\n12\\tcompleted\\tfailure\\n"))
    assert r.returncode == 1 and "ci.yml failure (run 12)" in r.stdout


def test_when_gh_fails_the_result_is_unavailable(tmp_path):
    r = _hosted(_fake_gh(tmp_path, "", code=1))
    assert r.returncode == 1
    assert r.stdout.startswith("hosted-gate: unavailable: gh could not read the runs")


def test_without_gh_the_fix_is_the_local_gate(tmp_path):
    # Only the tools the script needs: a Linux runner ships gh in /usr/bin, so the system folders will not do.
    tools = tmp_path / "bin"
    tools.mkdir()
    for tool in ("dirname", "git", "cut"):
        (tools / tool).symlink_to(shutil.which(tool))
    env = {k: v for k, v in os.environ.items() if k not in ("BASH_ENV", "ENV")}
    r = _hosted({**env, "PATH": str(tools), "HOSTED_GATE_TIP": TIP})
    assert r.returncode == 1
    assert r.stdout.strip() == "hosted-gate: unavailable: gh is not installed (brew install gh), so run the gate here -> fix: CHECKUP_LOCAL_GATE=1 scripts/checkup.sh"


def _checkup(repo: Path, tmp_path: Path, **extra: str) -> subprocess.CompletedProcess[str]:
    env = {**os.environ, "WRAP_UP_SKILL": str(tmp_path / "none"), "NEW_PROJECT_SKILL": str(tmp_path / "none"), **extra}
    return subprocess.run(["bash", str(repo / "scripts/checkup.sh")], capture_output=True, text=True, env=env, check=False, timeout=120)


def test_the_checkup_runs_no_local_gate_by_default(home, tmp_path):
    repo = _stub_repo(tmp_path)
    (repo / "scripts/check.sh").write_text(f"#!/usr/bin/env bash\ntouch '{tmp_path}/local-gate-ran'\n")
    _checkup(repo, tmp_path)
    assert not (tmp_path / "local-gate-ran").exists()
    done = {d["label"]: d for d in report.latest("repo-checkup")["done"]}
    assert done["Gate passed"]["command"] == "scripts/hosted-gate.sh"


def test_checkup_local_gate_runs_the_gate_here(home, tmp_path):
    repo = _stub_repo(tmp_path)
    _checkup(repo, tmp_path, CHECKUP_LOCAL_GATE="1")
    done = {d["label"]: d for d in report.latest("repo-checkup")["done"]}
    assert done["Gate passed"]["command"] == "scripts/check.sh"


def test_a_pending_hosted_gate_is_a_blocker_that_says_to_wait(home, tmp_path):
    repo = _stub_repo(tmp_path)
    (repo / "scripts/hosted-gate.sh").write_text("#!/usr/bin/env bash\necho 'hosted-gate: main 0123abcd pending: ci.yml run 9 is in_progress -> wait: gh run watch 9'\nexit 1\n")
    _checkup(repo, tmp_path)
    blockers = {b["label"]: b for b in report.latest("repo-checkup")["blockers"]}
    assert blockers["Gate pending on main's tip"]["command"] == "gh run watch 9"


def test_a_red_hosted_gate_is_a_blocker_with_the_log_command(home, tmp_path):
    repo = _stub_repo(tmp_path)
    (repo / "scripts/hosted-gate.sh").write_text("#!/usr/bin/env bash\necho 'hosted-gate: main 0123abcd failure (run 7, push) -> read it: gh run view 7 --log-failed'\nexit 1\n")
    r = _checkup(repo, tmp_path)
    assert r.returncode == 1
    blockers = {b["label"]: b for b in report.latest("repo-checkup")["blockers"]}
    assert blockers["Gate failed"]["command"] == "gh run view 7 --log-failed"
