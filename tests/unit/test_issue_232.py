"""The checkup says when it last ran on this machine and at which commit, and records each run (issue #232)."""

import json
import os
import subprocess
from pathlib import Path

import pytest

from careerbrew import checkup_runs, report

from .test_run_pages import _checkup_sh, _stub_repo


def _git(repo: Path, *args: str) -> None:
    subprocess.run(["git", "-C", str(repo), "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@example.com", *args], check=True, capture_output=True)


def _commit(repo: Path, name: str) -> None:
    (repo / name).write_text(name)
    _git(repo, "add", name)
    _git(repo, "commit", "-q", "-m", name)


def test_tests_never_write_the_owners_state_folder(tmp_path):
    assert checkup_runs.folder() == tmp_path / "state" / "checkups"
    assert Path.home() not in checkup_runs.folder().parents


def test_the_first_checkup_says_none_ran_and_records_its_run(home, tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00+00:00")
    repo = _stub_repo(tmp_path)
    _git(repo, "init", "-q")
    _commit(repo, "a")
    r = _checkup_sh(repo, tmp_path)
    assert r.returncode == 1, r.stderr
    assert "no earlier careerbrew checkup on this machine" in r.stderr, "it prints at the start"
    row = json.loads((tmp_path / "state" / "checkups" / "careerbrew.json").read_text())
    sha = subprocess.run(["git", "-C", str(repo), "rev-parse", "--short", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()
    assert row == {"finished_at": "2026-10-19T09:00:00+00:00", "result": "blocked", "commits": {"repo": sha}}


def test_the_next_checkup_shows_the_last_run_and_the_commits_since(home, tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00+00:00")
    repo = _stub_repo(tmp_path)
    _git(repo, "init", "-q")
    _commit(repo, "a")
    _checkup_sh(repo, tmp_path)
    old = json.loads((tmp_path / "state" / "checkups" / "careerbrew.json").read_text())["commits"]["repo"]
    _commit(repo, "b")
    _commit(repo, "c")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-20T10:30+00:00")
    r = _checkup_sh(repo, tmp_path)
    assert "last careerbrew checkup: Mon Oct 19 " in r.stderr and "(blocked)" in r.stderr, r.stderr
    assert f"repo at {old}: 2 commit(s) since" in r.stderr
    done = {d["id"]: d for d in report.latest("repo-checkup")["done"]}
    assert done["last-run"]["label"].startswith("Last careerbrew checkup: Mon Oct 19"), "the page shows it too"
    assert json.loads((tmp_path / "state" / "checkups" / "careerbrew.json").read_text())["finished_at"] == "2026-10-20T10:30:00+00:00"


def test_a_green_checkup_records_green(home, tmp_path):
    checkup_runs.record("careerbrew", "green", [str(tmp_path)])
    assert checkup_runs.read("careerbrew")["result"] == "green"
    assert checkup_runs.read("careerbrew")["commits"] == {}, "a folder that is not a git repo has no commit"


@pytest.mark.parametrize("bad", [("../x", "green"), ("careerbrew", "ok")])
def test_a_bad_name_or_result_is_refused(home, bad):
    with pytest.raises(ValueError):
        checkup_runs.record(*bad, ["."])


def test_a_plugin_checkup_writes_its_own_file_in_the_same_folder(home, tmp_path):
    env = {**os.environ}
    r = subprocess.run([str(Path(__file__).resolve().parents[2] / ".venv" / "bin" / "python"), "-m", "careerbrew.checkup_runs", "record", "acme-plugin", "degraded", str(tmp_path)], env=env, capture_output=True, text=True, check=False)
    assert r.returncode == 0, r.stderr
    assert sorted(p.name for p in checkup_runs.folder().iterdir()) == ["acme-plugin.json"]
    assert checkup_runs.last_lines("acme-plugin", [str(tmp_path)])[0].endswith("(degraded)")
