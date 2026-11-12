"""scripts/e2e.sh runs story files in parallel (TASKS e2e-parallel).

Each test runs the real runner in a tiny repo with stub story files, so it is fast and offline.
No test asserts a wall-clock limit: order and overlap come from files the stubs write.
"""

import os
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
# A stub browser story: the runner finds "playwright" in it and checks it calls browser_story (tests/e2e/lib.sh).
WEB = "browser_story() { :; }\nbrowser_story\n# npx playwright test\n"


def _repo(tmp_path: Path, stories: dict[str, str]) -> Path:
    root = tmp_path / "repo"
    (root / "scripts").mkdir(parents=True)
    (root / "tests" / "e2e").mkdir(parents=True)
    (root / "shared").mkdir()
    shutil.copy(ROOT / "scripts" / "e2e.sh", root / "scripts" / "e2e.sh")
    for name, body in stories.items():
        (root / "tests" / "e2e" / name).write_text(f"#!/usr/bin/env bash\nset -euo pipefail\n{body}\n")
    return root


def _run(root: Path, jobs: int = 4) -> subprocess.CompletedProcess[str]:
    env = {**os.environ, "E2E_JOBS": str(jobs), "SHARED": str(root / "shared")}
    return subprocess.run(["bash", "scripts/e2e.sh"], cwd=root, env=env, capture_output=True, text=True, timeout=120, check=False)


def _wait_for(me: str, other: str) -> str:
    """A story that passes only while the story `other` runs beside it (30 s is a failure bound, not a limit)."""
    return f'touch "$SHARED/{me}.up"\nfor _ in $(seq 1 300); do [ -e "$SHARED/{other}.up" ] && exit 0; sleep 0.1; done\necho "{other} never ran beside {me}"; exit 1'


@pytest.mark.parametrize(("first", "second"), [("", ""), (WEB, WEB), (WEB, "")], ids=["cli+cli", "browser+browser", "browser+cli"])
def test_story_files_run_at_the_same_time(tmp_path, first, second):
    # One by one, the first story waits for the second forever and fails.
    root = _repo(tmp_path, {"us-01-a.sh": first + _wait_for("a", "b"), "us-02-b.sh": second + _wait_for("b", "a")})
    r = _run(root)
    assert r.returncode == 0, r.stdout + r.stderr
    assert "e2e: 2 file(s) passed" in r.stdout


def test_browser_stories_start_first(tmp_path):
    stories = {"us-01-cli.sh": 'echo cli >> "$SHARED/order"', "us-02-web.sh": WEB + 'echo web >> "$SHARED/order"'}
    root = _repo(tmp_path, stories)
    r = _run(root, jobs=1)
    assert r.returncode == 0, r.stdout + r.stderr
    assert (root / "shared" / "order").read_text().split() == ["web", "cli"]


def test_a_serial_story_runs_alone_after_the_others(tmp_path):
    def busy(name: str) -> str:
        return f'echo "start {name}" >> "$SHARED/order"; sleep 1; echo "end {name}" >> "$SHARED/order"'

    stories = {
        "us-01-alone.sh": "# e2e: serial: flaked beside the others\n" + WEB + busy("s"),
        "us-02-web.sh": WEB + busy("w"),
        "us-03-cli.sh": busy("c"),
    }
    root = _repo(tmp_path, stories)
    r = _run(root)
    assert r.returncode == 0, r.stdout + r.stderr
    order = (root / "shared" / "order").read_text().splitlines()
    assert order[-2:] == ["start s", "end s"], order
    assert "e2e: 3 file(s) passed" in r.stdout


def test_a_playwright_story_without_its_own_port_fails_with_the_fix(tmp_path):
    root = _repo(tmp_path, {"us-01-web.sh": "# npx playwright test\ntrue"})
    r = _run(root)
    assert r.returncode == 1
    assert "us-01-web.sh runs Playwright without browser_story" in r.stderr
    assert "fix: call browser_story" in r.stderr


def test_a_failure_prints_only_its_log_and_every_file_still_runs(tmp_path):
    root = _repo(
        tmp_path,
        {
            "us-01-ok.sh": 'echo "noise from a passing story"',
            "us-02-bad.sh": 'echo "why it broke"; exit 1',
            "us-03-after.sh": 'touch "$SHARED/after-ran"',
        },
    )
    r = _run(root)
    assert r.returncode == 1
    out = r.stdout + r.stderr
    assert "why it broke" in out
    assert "noise from a passing story" not in out
    assert (root / "shared" / "after-ran").exists(), "a later story still runs after a failure"
    assert "e2e: 1 of 3 file(s) failed: tests/e2e/us-02-bad.sh" in r.stderr


def test_one_job_runs_every_file_in_order(tmp_path):
    stories = {f"us-0{i}-s.sh": f'echo {i} >> "$SHARED/order"' for i in range(1, 4)}
    root = _repo(tmp_path, stories)
    r = _run(root, jobs=1)
    assert r.returncode == 0, r.stdout + r.stderr
    assert (root / "shared" / "order").read_text().split() == ["1", "2", "3"]
    assert "e2e: 3 file(s) passed" in r.stdout


def test_silent_failure_reports_child_exit_status(tmp_path):
    root = _repo(tmp_path, {"us-01-silent.sh": "exit 37"})
    result = _run(root)
    assert result.returncode == 1
    assert "story exit status: 37" in result.stderr


def test_failure_statuses_preserve_logs_and_are_attributed(tmp_path):
    root = _repo(
        tmp_path,
        {
            "us-01-silent.sh": "exit 37",
            "us-02-noisy.sh": 'echo "synthetic failure"; exit 19',
            "us-03-serial.sh": "# e2e: serial: synthetic control\nexit 23",
            "us-04-success.sh": 'echo "passing output"',
        },
    )
    result = _run(root)
    assert result.returncode == 1
    assert "---- log of tests/e2e/us-01-silent.sh\nstory exit status: 37\n" in result.stderr
    assert "---- log of tests/e2e/us-02-noisy.sh\nstory exit status: 19\nsynthetic failure\n" in result.stderr
    assert "---- log of tests/e2e/us-03-serial.sh\nstory exit status: 23\n" in result.stderr
    assert "passing output" not in result.stderr
    assert "log of tests/e2e/us-04-success.sh" not in result.stderr


def test_missing_worker_receipt_reports_unavailable(tmp_path):
    root = _repo(tmp_path, {"us-01-unstarted.sh": "exit 0"})
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    fake_xargs = fake_bin / "xargs"
    fake_xargs.write_text("#!/bin/sh\nexit 0\n")
    fake_xargs.chmod(0o755)
    env = {**os.environ, "PATH": f"{fake_bin}{os.pathsep}{os.environ['PATH']}", "E2E_JOBS": "1"}
    result = subprocess.run(["bash", "scripts/e2e.sh"], cwd=root, env=env, capture_output=True, text=True, timeout=120, check=False)
    assert result.returncode == 1
    assert "story exit status: unavailable" in result.stderr


def test_malformed_worker_receipt_does_not_print_its_content(tmp_path):
    root = _repo(tmp_path, {"us-01-unstarted.sh": "exit 0"})
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    fake_xargs = fake_bin / "xargs"
    fake_xargs.write_text('#!/bin/sh\nprintf "synthetic-private-canary\\n" > "$E2E_LOGS/us-01-unstarted.status"\nprintf "FAIL\\n" > "$E2E_LOGS/us-01-unstarted.result"\n')
    fake_xargs.chmod(0o755)
    env = {**os.environ, "PATH": f"{fake_bin}{os.pathsep}{os.environ['PATH']}", "E2E_JOBS": "1"}
    result = subprocess.run(["bash", "scripts/e2e.sh"], cwd=root, env=env, capture_output=True, text=True, timeout=120, check=False)
    assert result.returncode == 1
    assert "story exit status: unavailable" in result.stderr
    assert "synthetic-private-canary" not in result.stderr
