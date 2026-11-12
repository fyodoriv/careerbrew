"""Gate flake 2026-10-25: at load ~125, six server tests failed "the web app did not start on port N: another
program may use it". A cold `careerbrew serve` took 1.5 to 2.4 s alone at load 84, and the start waited only
50 tries of 0.1 s (about 5 s). Made deterministic here: a fake clock, and a server that answers after 8 s.
"""

from __future__ import annotations

import time as real_time

import pytest

from careerbrew import server


class _Clock:
    def __init__(self) -> None:
        self.now = 0.0

    def sleep(self, s: float) -> None:
        self.now += s

    def monotonic(self) -> float:
        return self.now

    def __getattr__(self, name: str):
        return getattr(real_time, name)


class _Proc:
    pid = 999_999

    def __init__(self, exits: bool = False) -> None:
        self.exits = exits
        self.killed = False

    def poll(self):
        return 1 if self.exits else None

    def kill(self) -> None:
        self.killed = True


@pytest.fixture
def fake(home, monkeypatch):
    clock, procs = _Clock(), []
    monkeypatch.setattr(server, "time", clock)
    monkeypatch.setattr(server, "stop_untracked", lambda *a, **k: 0)
    monkeypatch.setattr(server, "listening_pids", lambda port: set())

    def popen(*a, exits=False, **k):
        procs.append(_Proc(exits))
        return procs[-1]

    monkeypatch.setattr(server.subprocess, "Popen", popen)
    return clock, procs, monkeypatch


def test_a_server_that_starts_slowly_on_a_busy_machine_is_still_used(fake):
    clock, procs, monkeypatch = fake
    monkeypatch.setattr(server, "_ping", lambda port, token, timeout=1: clock.now >= 8)
    url = server.start_background(50_123)
    assert url.startswith("http://127.0.0.1:50123/#token=") and not procs[0].killed


def test_a_server_that_exits_at_once_fails_at_once(fake):
    clock, procs, monkeypatch = fake
    monkeypatch.setattr(server, "_ping", lambda port, token, timeout=1: False)
    monkeypatch.setattr(server.subprocess, "Popen", lambda *a, **k: procs.append(_Proc(exits=True)) or procs[-1])
    with pytest.raises(RuntimeError, match="another program may use it"):
        server.start_background(50_124)
    assert clock.now < 1, "a server that could not bind the port is not waited for"


def test_a_server_that_never_answers_is_stopped_after_the_wait(fake):
    clock, procs, monkeypatch = fake
    monkeypatch.setattr(server, "_ping", lambda port, token, timeout=1: False)
    with pytest.raises(RuntimeError):
        server.start_background(50_125)
    assert procs[0].killed and clock.now <= server.START_WAIT_SECONDS + 1
