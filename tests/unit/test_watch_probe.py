"""Watcher status probes (`watch.running()`): two at once never see each other as a watcher, and a probe never stops one.

Task watch-running-probe-race: the web app's server answers parallel reads on threads, and each read can probe the
watcher lock (`day/watch.lock`). Probes and watchers here use separate open files, as separate processes would,
so threads in one test process race exactly like the server's request threads.
"""

from __future__ import annotations

import fcntl as real_fcntl
import threading
import time

import pytest

from careerbrew.day import watch


class _FirstLockWaits:
    """Stands in for `fcntl` inside `watch`: the first lock that the named thread takes stays held until `release` is set."""

    def __init__(self, thread_name: str) -> None:
        self.thread_name = thread_name
        self.held = threading.Event()
        self.release = threading.Event()

    def __getattr__(self, name: str):
        return getattr(real_fcntl, name)

    def flock(self, fh, op: int) -> None:
        real_fcntl.flock(fh, op)
        if op != real_fcntl.LOCK_UN and threading.current_thread().name == self.thread_name and not self.held.is_set():
            self.held.set()
            assert self.release.wait(5), "the test never released the held probe"


def _probe_in_flight(monkeypatch) -> tuple[_FirstLockWaits, threading.Thread, dict[str, bool]]:
    """Start a probe on its own thread and stop it while it holds its lock."""
    shim = _FirstLockWaits("probe-in-flight")
    monkeypatch.setattr(watch, "fcntl", shim)
    seen: dict[str, bool] = {}
    thread = threading.Thread(target=lambda: seen.update(first=watch.running()), name="probe-in-flight")
    thread.start()
    assert shim.held.wait(5), "the first probe took the lock"
    return shim, thread, seen


def test_a_probe_in_flight_is_not_a_running_watcher(monkeypatch):
    shim, first, seen = _probe_in_flight(monkeypatch)
    second: dict[str, bool] = {}
    try:
        other = threading.Thread(target=lambda: second.update(running=watch.running()), name="probe-two")
        other.start()
        other.join(5)
        assert second == {"running": False}, "a second probe saw the first probe as a running watcher"
    finally:
        shim.release.set()
        first.join(5)
    assert seen == {"first": False}
    assert watch.read_state().get("pid") is None, "a probe starts no watcher"


def test_many_probes_at_once_never_report_a_watcher():
    seen: list[bool] = []

    def probe() -> None:
        seen.extend(watch.running() for _ in range(200))

    threads = [threading.Thread(target=probe) for _ in range(8)]
    for t in threads:
        t.start()
    for t in threads:
        t.join(30)
    assert len(seen) == 1600
    assert seen.count(True) == 0, f"{seen.count(True)} of 1600 probes saw a watcher that does not exist"


def test_a_watcher_holds_its_lock_alone_and_every_probe_sees_it():
    with watch._hold_lock():
        assert watch.running(), "a probe sees the watcher"
        began = time.monotonic()
        with pytest.raises(RuntimeError, match="already running"), watch._hold_lock():
            pass
        assert time.monotonic() - began < watch.PROBE_WAIT_S / 2, "a second watcher is refused at once, not after the probe wait"
    assert not watch.running()


def test_a_probe_in_flight_does_not_stop_a_watcher_from_starting(monkeypatch):
    shim, first, _seen = _probe_in_flight(monkeypatch)
    timer = threading.Timer(0.2, shim.release.set)
    timer.start()
    try:
        with watch._hold_lock():
            first.join(5)
            assert not first.is_alive(), "the probe finished before the watcher took the lock"
            assert watch.running(), "once started, the watcher holds the lock"
    finally:
        shim.release.set()
        timer.cancel()
        first.join(5)
    assert not watch.running()
