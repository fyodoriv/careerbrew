"""End-to-end results and capability reports use isolated synthetic stories.

Reports run each distinct story once per invocation. A saved result does not
bind external runtime inputs, so it cannot replace a fresh story execution.
"""

import json
import os
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
GIT = ["git", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false", "-c", "user.name=Test", "-c", "user.email=test@example.com"]
METRICS_MD = """# Metrics

| # | Capability | E2E test |
|---|---|---|
| 1 | Story A | `tests/e2e/us-01-a.sh` |
| 2 | Story A again | `tests/e2e/us-01-a.sh` |
| 3 | Story B | `tests/e2e/us-02-b.sh` |
"""
# Story B fails while the file "broken" exists beside the run log.
STORIES = {
    "us-01-a.sh": 'echo a >> "$SHARED/runs"',
    "us-02-b.sh": 'echo b >> "$SHARED/runs"\n[ ! -e "$SHARED/broken" ]',
}


class Repo:
    def __init__(self, root: Path, shared: Path):
        self.root = root
        self.shared = shared
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_") and k not in {"NO_CACHE", "E2E_JOBS"}}
        self.env.update(SHARED=str(shared), E2E_JOBS="2")

    def git(self, *args: str) -> None:
        subprocess.run([*GIT, *args], cwd=self.root, env=self.env, check=True, capture_output=True, text=True)

    def script(self, *args: str, **env: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["bash", *args], cwd=self.root, env={**self.env, **env}, capture_output=True, text=True, timeout=120, check=False)

    def e2e(self) -> subprocess.CompletedProcess[str]:
        return self.script("scripts/e2e.sh")

    def coverage(self, **env: str) -> subprocess.CompletedProcess[str]:
        return self.script("scripts/metrics.sh", "capability-coverage", **env)

    def key(self) -> str:
        r = self.script("scripts/input-key.sh")
        assert r.returncode == 0, r.stderr
        return r.stdout.strip()

    def runs(self) -> list[str]:
        log = self.shared / "runs"
        return log.read_text().split() if log.exists() else []

    def saved(self) -> dict:
        return json.loads((self.root / ".metrics" / "e2e-results.json").read_text())


def _repo(tmp_path: Path, *, git: bool = True) -> Repo:
    root = tmp_path / "repo"
    shared = tmp_path / "shared"  # outside the repo: a story's run log is not an input
    for d in (root / "scripts", root / "tests" / "e2e", root / "docs", shared):
        d.mkdir(parents=True)
    for name in ("e2e.sh", "metrics.sh", "input-key.sh"):
        shutil.copy(ROOT / "scripts" / name, root / "scripts" / name)
    for name, body in STORIES.items():
        (root / "tests" / "e2e" / name).write_text(f"#!/usr/bin/env bash\nset -euo pipefail\n{body}\n")
    (root / "docs" / "metrics.md").write_text(METRICS_MD)
    (root / ".gitignore").write_text(".metrics/\n")
    (root / "notes.txt").write_text("one\n")
    repo = Repo(root, shared)
    if git:
        repo.git("init", "-q", "-b", "main")
        repo.git("add", "-A")
        repo.git("commit", "-qm", "init")
    return repo


@pytest.fixture
def repo(tmp_path):
    return _repo(tmp_path)


def test_e2e_records_each_file_under_the_input_hash(repo):
    (repo.shared / "broken").touch()
    r = repo.e2e()
    assert r.returncode == 1, r.stdout + r.stderr
    assert repo.saved() == {"key": repo.key(), "results": {"tests/e2e/us-01-a.sh": "ok", "tests/e2e/us-02-b.sh": "fail"}}


def test_coverage_after_a_green_e2e_run_runs_each_distinct_story(repo):
    assert repo.e2e().returncode == 0
    before = repo.runs()
    r = repo.coverage()
    assert r.returncode == 0, r.stdout + r.stderr
    assert r.stdout.splitlines()[-1] == "capability-coverage: 3/3"
    assert "ran 2 distinct story file(s)" in r.stdout
    assert sorted(repo.runs()[len(before) :]) == ["a", "b"]


def test_no_cache_runs_every_story_and_counts_the_same(repo):
    assert repo.e2e().returncode == 0
    hit = repo.coverage()
    before = repo.runs()
    fresh = repo.coverage(NO_CACHE="1")
    assert fresh.returncode == 0, fresh.stdout + fresh.stderr
    assert sorted(repo.runs()[len(before) :]) == ["a", "b"], "a fresh run runs each story file once"
    assert fresh.stdout.splitlines()[-1] == hit.stdout.splitlines()[-1] == "capability-coverage: 3/3"


@pytest.mark.parametrize("change", ["edit", "untracked", "story"])
def test_changed_inputs_run_the_stories_again(repo, change):
    assert repo.e2e().returncode == 0
    if change == "edit":
        (repo.root / "notes.txt").write_text("two\n")
    elif change == "untracked":
        (repo.root / "extra.txt").write_text("new\n")
    else:
        story = repo.root / "tests" / "e2e" / "us-01-a.sh"
        story.write_text(story.read_text() + "true\n")
    before = repo.runs()
    r = repo.coverage()
    assert r.returncode == 0, r.stdout + r.stderr
    assert sorted(repo.runs()[len(before) :]) == ["a", "b"], f"{change}: the saved results are for other inputs"


def test_a_recorded_failure_runs_again(repo):
    (repo.shared / "broken").touch()
    assert repo.e2e().returncode == 1
    (repo.shared / "broken").unlink()  # fixed outside the repo: same inputs
    before = repo.runs()
    r = repo.coverage()
    assert r.returncode == 0, r.stdout + r.stderr
    assert sorted(repo.runs()[len(before) :]) == ["a", "b"], "each distinct story runs again"
    assert r.stdout.splitlines()[-1] == "capability-coverage: 3/3"


def test_inputs_changed_during_the_run_are_not_recorded(repo):
    story = repo.root / "tests" / "e2e" / "us-01-a.sh"
    story.write_text(story.read_text() + "echo during >> notes.txt\n")
    repo.git("commit", "-qam", "a story that edits a tracked file")
    assert repo.e2e().returncode == 0
    assert not (repo.root / ".metrics" / "e2e-results.json").exists(), "results of a run whose inputs changed under it"


def test_outside_a_git_checkout_nothing_is_recorded(tmp_path):
    repo = _repo(tmp_path, git=False)
    r = repo.e2e()
    assert r.returncode == 0, r.stdout + r.stderr
    assert not (repo.root / ".metrics" / "e2e-results.json").exists()
    assert repo.coverage().stdout.splitlines()[-1] == "capability-coverage: 3/3"


def test_external_failure_is_rechecked_for_coverage(repo):
    initial = repo.e2e()
    assert initial.returncode == 0, initial.stdout + initial.stderr
    key = repo.key()
    assert repo.saved() == {"key": key, "results": {"tests/e2e/us-01-a.sh": "ok", "tests/e2e/us-02-b.sh": "ok"}}
    before = repo.runs()
    sentinel = repo.shared / "broken"
    sentinel.touch()
    assert sentinel.is_file() and not sentinel.is_relative_to(repo.root)
    assert repo.key() == key

    current = repo.coverage()
    assert current.returncode == 1, current.stdout + current.stderr
    assert current.stdout.splitlines()[-1] == "capability-coverage: 2/3"
    assert "FAIL  capability 3 (tests/e2e/us-02-b.sh)" in current.stdout
    assert sorted(repo.runs()[len(before) :]) == ["a", "b"]
    assert repo.key() == key


def test_duplicate_failed_story_runs_once_per_report(repo):
    metrics = repo.root / "docs" / "metrics.md"
    metrics.write_text(METRICS_MD + "| 4 | Story B again | `tests/e2e/us-02-b.sh` |\n")
    (repo.shared / "broken").touch()
    result = repo.coverage()
    assert result.returncode == 1, result.stdout + result.stderr
    assert result.stdout.splitlines()[-1] == "capability-coverage: 2/4"
    assert sorted(repo.runs()) == ["a", "b"]
