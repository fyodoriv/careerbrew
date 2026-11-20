"""Measure the saved review cycle's synthetic deadline boundaries."""

import os
import shlex
import subprocess
import sys
import threading
import time

import pytest

from careerbrew.work import review_ask
from careerbrew.workday import questions, review_cycle


def test_review_recheck_receives_shared_deadline(monkeypatch):
    seen = {}

    def recheck(_url, *, deadline=None):
        seen["deadline"] = deadline
        return "open", "synthetic"

    monkeypatch.setattr(review_ask, "recheck_pr", recheck)
    row = {"recheck": "careerbrew:review-pr https://github.com/example/repo/pull/1", "source": "settings"}
    assert questions.check_one(row, deadline=123.5) == ("open", "synthetic")
    assert seen == {"deadline": 123.5}


def test_slow_recheck_stops_at_requested_budget(monkeypatch):
    row = {"recheck": "careerbrew:review-pr https://github.com/example/repo/pull/1", "step": "/review", "source": "settings"}
    monkeypatch.setattr(questions, "open_records", lambda: [row])
    release = threading.Event()
    finished = threading.Event()

    def slow(_row, **_kwargs):
        release.wait(2)
        finished.set()
        return "open", "synthetic"

    monkeypatch.setattr(questions, "check_one", slow)

    def unexpected(*_args):
        pytest.fail("inventory ran after the shared deadline")

    monkeypatch.setattr(review_cycle, "_inventory", unexpected)
    start = time.monotonic()
    try:
        assert review_cycle.run({"step": "/review", "from": "settings", "review_ask_cycle": "unused"}, 0.05) is False
        assert not finished.is_set()
        assert time.monotonic() - start < 1
    finally:
        release.set()
    assert finished.wait(1)


def test_timed_out_inventory_owns_its_descendants(tmp_path):
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
        pid = int(ready.read_text())
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                break
            time.sleep(0.02)
        else:
            pytest.fail("owned synthetic descendant survived the inventory timeout")
        assert not marker.exists()
    finally:
        release.touch()
        worker.join(timeout=7)
        assert not worker.is_alive()
