"""rust-05f component proofs with synthetic homes, PTYs and fake Keychain only."""

from __future__ import annotations

import errno
import json
import os
import select
import shutil
import subprocess
import sys
import termios
import time
from pathlib import Path

import pytest

from careerbrew import approvals, store, vault

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust05.py"
CANARY = "SYNTHETIC-RUST05-CANARY-7Q9"
SECRET = "SYNTHETIC-RUST05-SECRET-NOT-AN-AUTHENTICATION-VALUE"


@pytest.fixture(scope="module")
def store_bin():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the cross-engine checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "-p", "careerbrew-vault", "--bin", "vault-store"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "vault-store"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    env = {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_ENCRYPT": "on",
        "CAREERBREW_NOW": "2026-03-01T09:30:00+00:00",
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }
    return env


def assert_no_synthetic_values(root: Path, output: bytes) -> None:
    for term in (CANARY.encode(), SECRET.encode()):
        assert term not in output
        for path in root.rglob("*"):
            if path.is_file() and not path.is_symlink():
                if path.name == "backup.key":
                    continue
                raw = path.read_bytes()
                if raw.startswith(vault.MAGIC):
                    continue
                assert term not in raw, path.relative_to(root)


def test_stateful_component_workload_matches_real_python(store_bin, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        env = synthetic_env(root)
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(store_bin), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, timeout=120)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        report = json.loads(done.stdout)
        assert report["schema"] == "careerbrew-rust05-component-v1"
        ids = [case["id"] for case in report["cases"]]
        assert len(ids) == len(set(ids))
        assert {"files", "text-noop-identity", "json-noop-identity", "jsonl-noop-identity", "data-edit-rewrites"} <= set(ids)
        assert all(case["result"] is True for case in report["cases"] if case["id"].endswith("identity") or case["id"] == "data-edit-rewrites")
        assert_no_synthetic_values(root, done.stdout + done.stderr)
        answers.append(done.stdout)
    assert answers[0] == answers[1]


def test_invalid_utf8_is_not_a_noop_rewrite_permission(store_bin, tmp_path):
    root = tmp_path / "case"
    env = synthetic_env(root)
    home = Path(env["CAREERBREW_HOME"])
    key = Path(env["CAREERBREW_KEY_FILE"])
    path = home / "invalid.txt"
    path.write_bytes(b"\xff")
    before = (path.read_bytes(), path.stat().st_ino, path.stat().st_mtime_ns)
    done = subprocess.run([store_bin, "write-if-changed", key, home, path, "plain"], input="replacement", text=True, capture_output=True, env=env)
    assert done.returncode == 1
    assert (path.read_bytes(), path.stat().st_ino, path.stat().st_mtime_ns) == before


@pytest.mark.parametrize("stage", ["before-temp", "after-temp", "before-rename", "after-rename"])
def test_hard_interruption_boundaries_and_retry(store_bin, tmp_path, monkeypatch, stage):
    for engine in ("python", "rust"):
        root = tmp_path / stage / engine
        env = synthetic_env(root)
        for name, value in env.items():
            monkeypatch.setenv(name, value)
        home = Path(env["CAREERBREW_HOME"])
        key = Path(env["CAREERBREW_KEY_FILE"])
        target = home / "crash.txt"
        command = [sys.executable, "-I", "-B", str(ORACLE), "--crash", stage]
        if engine == "rust":
            command = [str(store_bin), "crash-write", key, home, target, stage]
        done = subprocess.run(command, cwd=ROOT, env=env, input="synthetic crash", text=True, capture_output=True)
        assert done.returncode == 86
        assert target.exists() == (stage == "after-rename")
        # A hard kill after the temp write may leave its unique sibling. Retry
        # must succeed without assuming impossible cleanup of that sibling.
        if engine == "rust":
            retry = subprocess.run([store_bin, "write", key, home, target, "seal"], input="retry", text=True, capture_output=True, env=env)
            assert retry.returncode == 0, retry.stderr
        else:
            store.write_text(target, "retry")
        assert store.read_text(target) == "retry"


def test_receipt_event_interruption_leaves_readable_state_and_releases_lock(store_bin, tmp_path, monkeypatch):
    for engine in ("python", "rust"):
        root = tmp_path / engine
        env = synthetic_env(root)
        for name, value in env.items():
            monkeypatch.setenv(name, value)
        home = Path(env["CAREERBREW_HOME"])
        key = Path(env["CAREERBREW_KEY_FILE"])
        command = [sys.executable, "-I", "-B", str(ORACLE), "--crash-receipt"]
        if engine == "rust":
            command = [str(store_bin), "crash-receipt", key, home]
        done = subprocess.run(command, cwd=ROOT, env=env, capture_output=True)
        assert done.returncode == 86
        rows = store.read_json(home / "approvals.json")
        assert len(rows) == 1 and rows[0]["status"] == "pending"
        # The next real writer acquires the same lock after the killed owner.
        approvals.request("Synthetic retry")
        assert len(store.read_json(home / "approvals.json")) == 2


def test_mixed_receipt_writers_and_lock_free_reads(store_bin, tmp_path, monkeypatch):
    root = tmp_path / "mixed-receipts"
    env = synthetic_env(root)
    for name, value in env.items():
        monkeypatch.setenv(name, value)
    home = Path(env["CAREERBREW_HOME"])
    key = Path(env["CAREERBREW_KEY_FILE"])
    worker = "from careerbrew import approvals; approvals.request('Synthetic Python writer')"
    children = [subprocess.Popen([sys.executable, "-c", worker], cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE) for _ in range(4)]
    children += [
        subprocess.Popen(
            [store_bin, "receipt-request", key, home, f"rust{i:04d}"],
            cwd=ROOT,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        for i in range(4)
    ]
    for child in children:
        _, error = child.communicate(timeout=60)
        assert child.returncode == 0, error.decode(errors="replace")
    rows = approvals.validate_records(store.read_json(home / "approvals.json"))
    assert len(rows) == 8
    assert len({row["id"] for row in rows}) == 8
    # A reader does not wait for the writer's flock; atomic replacement is its
    # consistency boundary. The synthetic read runs while this process holds it.
    with approvals._locked():
        done = subprocess.run([store_bin, "read", key, home, home / "approvals.json"], env=env, capture_output=True, timeout=5)
        assert done.returncode == 0
        assert len(json.loads(done.stdout)) == 8


def read_pty(master: int, child: subprocess.Popen, *, secret: str | None = None) -> bytes:
    output = bytearray()
    sent = False
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        ready, _, _ = select.select([master], [], [], 0.1)
        if ready:
            try:
                chunk = os.read(master, 4096)
            except OSError as error:
                if error.errno == errno.EIO:
                    break
                raise
            if not chunk:
                break
            output.extend(chunk)
        if secret is not None and not sent and b"input hidden" in output:
            attributes = termios.tcgetattr(master)
            if not attributes[3] & termios.ECHO:
                os.write(master, secret.encode() + b"\n")
                sent = True
        if child.poll() is not None and not ready:
            break
    assert child.wait(timeout=5) == 0
    assert secret is None or sent
    return bytes(output)


def fake_security(root: Path) -> Path:
    bin_dir = root / "bin"
    bin_dir.mkdir()
    security = bin_dir / "security"
    security.write_text("#!/bin/sh\ncat >/dev/null\nexit 0\n", encoding="utf-8")
    security.chmod(0o755)
    return bin_dir


@pytest.mark.parametrize("engine", ["python", "rust"])
def test_secret_set_uses_a_real_pty_fake_keychain_and_restores_echo(store_bin, tmp_path, engine):
    root = tmp_path / engine
    env = synthetic_env(root)
    env["PATH"] = str(fake_security(root)) + os.pathsep + "/usr/bin:/bin"
    command = [sys.executable, "-B", "-m", "careerbrew", "secret", "set", "synthetic.service"]
    if engine == "rust":
        command = [str(store_bin), "secret", "set", "synthetic.service"]
    master, slave = os.openpty()
    try:
        child = subprocess.Popen(command, cwd=ROOT, env=env, stdin=slave, stdout=slave, stderr=slave, close_fds=True)
        os.close(slave)
        slave = -1
        output = read_pty(master, child, secret=SECRET)
        assert b"stored as keychain:synthetic.service" in output
        assert SECRET.encode() not in output
        assert termios.tcgetattr(master)[3] & termios.ECHO
        assert_no_synthetic_values(root, output)
    finally:
        os.close(master)
        if slave >= 0:
            os.close(slave)


@pytest.mark.parametrize("engine", ["python", "rust"])
def test_data_edit_real_pty_fake_editor_and_full_rewrite(store_bin, tmp_path, monkeypatch, engine):
    root = tmp_path / engine
    env = synthetic_env(root)
    for name, value in env.items():
        monkeypatch.setenv(name, value)
    home = Path(env["CAREERBREW_HOME"])
    target = home / "record.yaml"
    store.write_yaml(target, {"count": 1})
    before = target.stat().st_ino
    editor = root / "editor.sh"
    editor.write_text("#!/bin/sh\nprintf 'count: 1\\n' > \"$1\"\n", encoding="utf-8")
    editor.chmod(0o755)
    env["EDITOR"] = str(editor)
    command = [sys.executable, "-B", "-m", "careerbrew", "data", "edit", "record.yaml"]
    if engine == "rust":
        command = [str(store_bin), "data", "edit", "record.yaml"]
    master, slave = os.openpty()
    try:
        child = subprocess.Popen(command, cwd=ROOT, env=env, stdin=slave, stdout=slave, stderr=slave, close_fds=True)
        os.close(slave)
        slave = -1
        output = read_pty(master, child)
        assert b"saved record.yaml (encrypted)" in output
        assert store.read_yaml(target) == {"count": 1}
        assert target.stat().st_ino != before
        assert not (home / ".edit" / "record.yaml").exists()
        assert_no_synthetic_values(root, output)
    finally:
        os.close(master)
        if slave >= 0:
            os.close(slave)
