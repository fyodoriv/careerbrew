"""Hot code reload: the watcher swaps to new code between cycles without losing state, pause, lock, or results."""

import os
import signal
import time

from careerbrew.day import results, watch
from careerbrew.work import evidence


def _wait(cond, timeout=20):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if cond():
            return True
        time.sleep(0.1)
    return False


def _code(tmp_path):
    d = tmp_path / "code"
    d.mkdir()
    (d / "mod.py").write_text("x = 1\n")
    return d


def test_code_change_reexecs_same_pid_keeps_state_and_pause(home, tmp_path, monkeypatch):
    code = _code(tmp_path)
    monkeypatch.setenv("CAREERBREW_CODE_DIR", str(code))
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    evidence.note("feat: before reload", ["delivery"])
    st = watch.start(interval=1)
    pid = st["pid"]
    try:
        assert _wait(lambda: results.summary().get("note") == 1)
        watch.pause()
        (code / "mod.py").write_text("x = 2  # updated while running\n")
        assert _wait(lambda: watch.read_state().get("reloads") == 1), watch.read_state()
        assert watch.read_state()["pid"] == pid, "execv keeps the same process"
        assert _wait(watch.running)
        assert watch.read_state()["state"] == "paused", "pause survives a code reload"
        evidence.note("feat: after reload", ["delivery"])
        time.sleep(2)
        assert results.summary().get("note") == 1, "still paused after reload"
        watch.resume()
        assert _wait(lambda: results.summary().get("note") == 2)
        ids = [r["id"] for r in results.load()]
        assert len(ids) == len(set(ids))
    finally:
        watch.stop()
    assert not watch.running()


def test_broken_update_keeps_old_code_running(home, tmp_path, monkeypatch):
    code = _code(tmp_path)
    monkeypatch.setenv("CAREERBREW_CODE_DIR", str(code))
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    st = watch.start(interval=1)
    try:
        assert _wait(watch.running)
        (code / "mod.py").write_text("def broken(:\n")
        assert _wait(lambda: watch.read_state().get("reload_error")), watch.read_state()
        assert watch.read_state().get("reloads", 0) == 0
        assert watch.running() and watch.read_state()["pid"] == st["pid"]
    finally:
        watch.stop()


def test_explicit_reload_signal(home, tmp_path, monkeypatch):
    code = _code(tmp_path)
    monkeypatch.setenv("CAREERBREW_CODE_DIR", str(code))
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-31T23:00:00")
    watch.start()  # The default interval respects off-hours; an explicit interval forces active work.
    try:
        assert _wait(watch.running)
        assert _wait(lambda: bool(watch.read_state().get("started_at")))
        watch.reload()
        assert _wait(lambda: watch.read_state().get("reloads") == 1)
        assert _wait(watch.running)
        assert watch.read_state().get("last_cycle") is None, "the signal-only fixture must not run active work sources"
    finally:
        watch.stop()


def test_old_state_file_is_migrated(home):
    watch.state_path().parent.mkdir(parents=True, exist_ok=True)
    watch.state_path().write_text('{"state": "paused", "pid": null}')
    st = watch.read_state()
    assert st["version"] == watch.STATE_VERSION and st["state"] == "paused" and st["reloads"] == 0


def test_reexec_after_sigkill_mid_reload_recovers(home, tmp_path, monkeypatch):
    code = _code(tmp_path)
    monkeypatch.setenv("CAREERBREW_CODE_DIR", str(code))
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    st = watch.start(interval=1)
    assert _wait(watch.running)
    os.kill(int(st["pid"]), signal.SIGKILL)
    assert _wait(lambda: not watch.running())
    watch.start(interval=1)
    try:
        assert _wait(watch.running)
    finally:
        watch.stop()
