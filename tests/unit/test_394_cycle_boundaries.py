"""Measure the saved review cycle's synthetic deadline boundaries."""

import os
import shlex
import subprocess
import sys
import threading
import time

import pytest

from careerbrew.workday import questions, review_cycle


def test_slow_recheck_exceeds_requested_budget(monkeypatch):
    row = {"recheck": "careerbrew:review-pr https://github.com/example/repo/pull/1", "step": "/review", "source": "settings"}
    monkeypatch.setattr(questions, "open_records", lambda: [row])

    def slow(_row):
        time.sleep(0.2)
        return "open", "synthetic"

    monkeypatch.setattr(questions, "check_one", slow)
    monkeypatch.setattr(review_cycle, "_inventory", lambda _command, _seconds: [])
    start = time.monotonic()
    assert review_cycle.run({"step": "/review", "from": "settings", "review_ask_cycle": "unused"}, 0.05)
    assert time.monotonic() - start >= 0.15


def test_timed_out_inventory_child_can_finish(tmp_path):
    ready, release, marker = (tmp_path / name for name in ("ready", "release", "marker"))
    child = """import os,pathlib,sys,time
ready, release, marker = map(pathlib.Path, sys.argv[1:])
tmp = ready.with_suffix(".tmp")
tmp.write_text(str(os.getpid()))
tmp.replace(ready)
deadline = time.monotonic() + 8
while not release.exists() and time.monotonic() < deadline:
    time.sleep(0.01)
if release.exists(): marker.touch()
"""
    parent = "import subprocess,sys,time; subprocess.Popen([sys.executable, '-c', *sys.argv[1:]], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); time.sleep(6)"
    command = shlex.join([sys.executable, "-c", parent, child, str(ready), str(release), str(marker)])
    outcome = []

    def run():
        try:
            review_cycle._inventory(command, 3)
        except Exception as exc:
            outcome.append(exc)

    worker = threading.Thread(target=run, daemon=True)
    worker.start()
    try:
        deadline = time.monotonic() + 2
        while not ready.exists() and time.monotonic() < deadline:
            time.sleep(0.01)
        assert ready.exists(), "synthetic child did not start in time"
        worker.join(timeout=5)
        assert not worker.is_alive()
        assert len(outcome) == 1 and isinstance(outcome[0], subprocess.TimeoutExpired)
        assert not marker.exists()
        release.touch()
        deadline = time.monotonic() + 2
        while not marker.exists() and time.monotonic() < deadline:
            time.sleep(0.01)
        assert marker.exists()
    finally:
        release.touch()
        worker.join(timeout=7)
        if ready.exists():
            pid = int(ready.read_text())
            deadline = time.monotonic() + 9
            while time.monotonic() < deadline:
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.02)
            else:
                pytest.fail("synthetic child PID did not disappear")
        assert not worker.is_alive()
