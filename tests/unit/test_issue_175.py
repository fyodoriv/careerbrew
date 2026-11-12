"""install.sh asks for the role on a terminal, never waits in a setup chain, and ends with the feedback step (issue #175)."""

import errno
import json
import os
import pty
import select
import signal
import subprocess
import time
from pathlib import Path

import pytest

from .conftest import readme_repo

ROOT = Path(__file__).resolve().parents[2]
# The repo address comes from the README, so this test holds no owner name.
REPO = readme_repo()


def _env(tmp_path, current="personal"):
    log = tmp_path / "calls.log"
    stub = tmp_path / "careerbrew"
    stub.write_text(f'#!/bin/sh\necho "$*" >> {log}\n[ "$1 $2" = "machine show" ] && echo "role: {current} label: -"\nexit 0\n')
    stub.chmod(0o755)
    env = {**os.environ, "CAREERBREW_BIN": str(stub), "SETUP_SKIP_DEPS": "1", "HW_ARCH": "x86_64"}
    return env, log


def _on_a_terminal(env, answer: str) -> tuple[str, int]:
    """Run install.sh with a pseudo-terminal for stdin and stdout, answer the prompt; return what it printed and its exit code."""
    try:
        pid, fd = pty.fork()
    except OSError as exc:  # the agent sandbox blocks pseudo-terminals ("out of pty devices")
        pytest.skip(f"no pseudo-terminal here: {exc}")
    if pid == 0:  # the child: install.sh on the terminal
        os.chdir(ROOT)
        os.execve("/bin/bash", ["bash", "scripts/install.sh"], env)
    out, sent, end = b"", False, time.monotonic() + 30
    try:
        while time.monotonic() < end:
            ready, _, _ = select.select([fd], [], [], 0.2)
            if ready:
                try:
                    chunk = os.read(fd, 1024)
                except OSError:
                    break
                if not chunk:
                    break
                out += chunk
            if not sent and b"Which machine is this?" in out:
                os.write(fd, answer.encode() + b"\n")
                sent = True
        else:
            os.kill(pid, signal.SIGKILL)  # a prompt that never came or a step that hangs must not hang the suite
    finally:
        os.close(fd)
    _, status = os.waitpid(pid, 0)
    return out.decode(errors="replace"), os.waitstatus_to_exitcode(status)


def test_on_a_terminal_it_asks_for_the_role_and_sets_the_answer(tmp_path):
    env, log = _env(tmp_path)
    out, rc = _on_a_terminal(env, "Corporate")  # any case
    assert rc == 0, out
    assert "Which machine is this? personal" in out and "[personal]" in out
    assert "machine set corporate" in log.read_text()


def test_enter_keeps_the_current_role(tmp_path):
    env, log = _env(tmp_path, current="corporate")
    out, rc = _on_a_terminal(env, "")
    assert rc == 0 and "[corporate]" in out, out
    assert "installed (corporate)" in out
    assert "machine set corporate" in log.read_text(), "Enter never switches a corporate machine to personal"


def test_a_bad_answer_exits_2_with_the_fix(tmp_path):
    env, log = _env(tmp_path)
    out, rc = _on_a_terminal(env, "work")
    assert rc == 2, out
    assert "unknown role work: use personal or corporate (or pass --role)" in out
    assert "machine set" not in log.read_text()


def test_with_no_terminal_it_never_waits_and_ends_with_the_feedback_step(tmp_path):
    env, log = _env(tmp_path)
    r = subprocess.run(["bash", "scripts/install.sh"], cwd=ROOT, env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60)
    assert r.returncode == 0, r.stderr
    assert "Which machine is this?" not in r.stdout and "machine set" not in log.read_text()
    assert f"agentbrew report --repo {REPO}" in r.stdout and "agentbrew contribute" in r.stdout


def test_a_bad_option_exits_2():
    r = subprocess.run(["bash", "scripts/install.sh", "--nope"], cwd=ROOT, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60)
    assert r.returncode == 2 and "unknown option --nope" in r.stderr


def test_install_md_is_an_agent_playbook_with_the_question_round_and_the_feedback_step():
    text = (ROOT / "INSTALL.md").read_text()
    for part in ("## 2. Explain the plan", "## 3. Ask for preferences", "--role personal|corporate", f"agentbrew report --repo {REPO}", "agentbrew contribute"):
        assert part in text, part


def _terminal_master_case(monkeypatch, tmp_path, read_error):
    real_read, real_write, real_close, real_fstat = os.read, os.write, os.close, os.fstat
    fd, writer = os.pipe()
    foreign_read, foreign_write = os.pipe()
    real_write(writer, b"synthetic terminal output\n")
    real_close(writer)
    token, reads, closes, waits = 424242, [], [], []
    owned_closed = False

    def is_open(descriptor):
        try:
            real_fstat(descriptor)
        except OSError as exc:
            assert exc.errno == errno.EBADF
            return False
        return True

    def read(descriptor, size):
        if descriptor == fd:
            reads.append(descriptor)
            if read_error and len(reads) == 2:
                raise OSError(errno.EIO, "synthetic terminal read failure")
        return real_read(descriptor, size)

    def close(descriptor):
        nonlocal owned_closed
        result = real_close(descriptor)
        closes.append(descriptor)
        if descriptor == fd:
            owned_closed = True
        return result

    def waitpid(pid, options):
        assert (pid, options) == (token, 0)
        waits.append({"owned_open": int(is_open(fd))})
        return token, (17 << 8) if read_error else 0

    def forbid_signal(*_args):
        raise AssertionError("terminal FD controls must not signal any PID")

    try:
        with monkeypatch.context() as patch:
            patch.setattr(pty, "fork", lambda: (token, fd))
            patch.setattr(os, "read", read)
            patch.setattr(os, "close", close)
            patch.setattr(os, "waitpid", waitpid)
            patch.setattr(os, "kill", forbid_signal)
            output, status = _on_a_terminal({}, "unused")
        owned_open = int(is_open(fd))
        real_write(foreign_write, b"foreign sentinel")
        foreign_ok = real_read(foreign_read, 64) == b"foreign sentinel"
        evidence = {
            "case": "EIO" if read_error else "EOF",
            "owned_open_before_cleanup": owned_open,
            "waits": waits,
            "owned_closes": closes.count(fd),
            "foreign_closes": sum(descriptor in (foreign_read, foreign_write) for descriptor in closes),
            "foreign_usable": foreign_ok,
            "read_count": len(reads),
            "status": status,
            "output_preserved": output == "synthetic terminal output\n",
        }
        (tmp_path / "terminal-fd-counts.json").write_text(json.dumps(evidence, sort_keys=True) + "\n")
        assert evidence["output_preserved"] and status == (17 if read_error else 0)
        assert len(reads) == 2 and len(waits) == 1
        assert foreign_ok and evidence["foreign_closes"] == 0
        assert owned_open == 0 and closes.count(fd) == 1
        assert waits == [{"owned_open": 0}]
    finally:
        if not owned_closed and is_open(fd):
            real_close(fd)
        for descriptor in (foreign_read, foreign_write):
            if is_open(descriptor):
                real_close(descriptor)


def test_terminal_master_fd_closes_on_eof(monkeypatch, tmp_path):
    _terminal_master_case(monkeypatch, tmp_path, read_error=False)


def test_terminal_master_fd_closes_on_read_error(monkeypatch, tmp_path):
    _terminal_master_case(monkeypatch, tmp_path, read_error=True)
