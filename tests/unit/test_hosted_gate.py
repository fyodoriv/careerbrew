"""The checkup reads the hosted gate's result on main instead of running the gate here (docs/ci.md).

scripts/hosted-gate.sh runs against a fake `gh` on PATH; scripts/checkup.sh runs in a stub repo.
"""

import os
import subprocess
from pathlib import Path

import pytest

from careerbrew import report

from .test_run_pages import _stub_repo

ROOT = Path(__file__).resolve().parents[2]


def _fake_gh(tmp_path: Path, rows: str, code: int = 0) -> dict[str, str]:
    tools = tmp_path / "bin"
    tools.mkdir(exist_ok=True)
    (tools / "gh").write_text(f"#!/bin/sh\necho \"$@\" >> '{tmp_path}/gh-args'\nprintf '{rows}'\nexit {code}\n")
    (tools / "gh").chmod(0o755)
    # No shell startup file: it can put another gh first on PATH (AGENTS.md, "Gotchas").
    env = {k: v for k, v in os.environ.items() if k not in ("BASH_ENV", "ENV")}
    return {**env, "PATH": f"{tools}{os.pathsep}/usr/bin:/bin"}


def _hosted(env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["/bin/bash", str(ROOT / "scripts/hosted-gate.sh")], env=env, capture_output=True, text=True, timeout=30, check=False)


def test_a_green_run_on_main_passes_and_names_its_commit(tmp_path):
    r = _hosted(_fake_gh(tmp_path, "11\\tcompleted\\tsuccess\\tabcdef12\\tpush\\n"))
    assert r.returncode == 0, r.stdout + r.stderr
    assert r.stdout.strip() == "hosted-gate: main abcdef12 passed (run 11, push)"
    assert "--workflow ci.yml --branch main" in (tmp_path / "gh-args").read_text()


def test_a_red_run_fails_with_the_command_that_shows_its_log(tmp_path):
    r = _hosted(_fake_gh(tmp_path, "12\\tcompleted\\tfailure\\tabcdef12\\tpush\\n"))
    assert r.returncode == 1
    assert "-> read it: gh run view 12 --log-failed" in r.stdout


def test_a_run_in_progress_is_named_and_the_newest_finished_run_decides(tmp_path):
    rows = "14\\tin_progress\\t-\\t99999999\\tpush\\n13\\tcompleted\\tcancelled\\t88888888\\tpush\\n12\\tcompleted\\tsuccess\\t77777777\\tpush\\n"
    r = _hosted(_fake_gh(tmp_path, rows))
    assert r.returncode == 0, r.stdout
    assert "run 14 on main 99999999 is still running" in r.stdout
    assert "main 77777777 passed (run 12, push)" in r.stdout, "a cancelled run neither passes nor fails"


@pytest.mark.parametrize(("rows", "code"), [("", 0), ("", 1)], ids=["no finished run", "gh fails"])
def test_an_unknown_result_is_never_green(tmp_path, rows, code):
    r = _hosted(_fake_gh(tmp_path, rows, code))
    assert r.returncode == 1
    assert r.stdout.startswith("hosted-gate: unavailable: ")


def test_without_gh_the_fix_is_the_local_gate(tmp_path):
    env = {k: v for k, v in os.environ.items() if k not in ("BASH_ENV", "ENV")}
    r = _hosted({**env, "PATH": "/usr/bin:/bin"})  # the system folders only: gh comes from Homebrew
    assert r.returncode == 1
    assert r.stdout.rstrip().endswith("-> fix: CHECKUP_LOCAL_GATE=1 scripts/checkup.sh")


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


def test_a_red_hosted_gate_is_a_blocker_with_the_log_command(home, tmp_path):
    repo = _stub_repo(tmp_path)
    (repo / "scripts/hosted-gate.sh").write_text("#!/usr/bin/env bash\necho 'hosted-gate: main 0123abcd failure (run 7, push) -> read it: gh run view 7 --log-failed'\nexit 1\n")
    r = _checkup(repo, tmp_path)
    assert r.returncode == 1
    blockers = {b["label"]: b for b in report.latest("repo-checkup")["blockers"]}
    assert blockers["Gate failed"]["command"] == "gh run view 7 --log-failed"
