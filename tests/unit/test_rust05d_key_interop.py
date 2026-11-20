"""rust-05d: cross-engine key and atomic-write checks with synthetic data only.

A key Python's `keyfile.load_key` makes opens Rust's reads, and a key Rust's
`keyfile::load_key` makes opens Python's reads. Rust's atomic write leaves no
temp file and keeps write-if-changed. The `vault-store` binary never prints
the key.
"""

import concurrent.futures
import os
import shutil
import stat
import subprocess
import threading
from pathlib import Path

import pytest
from cryptography.fernet import Fernet

from careerbrew import keyfile, vault

ROOT = Path(__file__).resolve().parents[2]


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
    return ROOT / "target/debug/vault-store"


@pytest.fixture(autouse=True)
def data_root(home):
    home.mkdir(parents=True, exist_ok=True)
    return home


def run(store_bin, *args, text=None):
    return subprocess.run([store_bin, *map(str, args)], input=text, capture_output=True, text=True, timeout=10)


def seal(key: bytes, text: str) -> bytes:
    return vault.MAGIC + Fernet(key).encrypt(text.encode())


def unseal(key: bytes, data: bytes) -> str:
    assert data.startswith(vault.MAGIC)
    return Fernet(key).decrypt(data[len(vault.MAGIC) :]).decode()


def test_a_python_made_key_opens_in_rust(store_bin, home, tmp_path):
    key_file = tmp_path / "keys" / "backup.key"
    key = keyfile.load_key(create=True, path=key_file)
    data = home / "a.yaml"
    data.write_bytes(seal(key, "synthetic: python\n"))
    result = run(store_bin, "read", key_file, home, data)
    assert result.returncode == 0, result.stderr
    assert result.stdout == "synthetic: python\n"
    assert key.decode() not in result.stdout + result.stderr


def test_a_rust_made_key_opens_in_python(store_bin, home, tmp_path):
    key_file = tmp_path / "keys" / "backup.key"
    data = home / "b.json"
    result = run(store_bin, "write", key_file, home, data, "seal", text='{"synthetic": "rust"}')
    assert result.returncode == 0, result.stderr
    assert stat.S_IMODE(key_file.stat().st_mode) == 0o600
    key = keyfile.load_key(create=False, path=key_file)
    assert key.decode() not in result.stdout + result.stderr
    assert unseal(key, data.read_bytes()) == '{"synthetic": "rust"}'


def test_rust_refuses_a_missing_key_and_a_key_inside_the_data_root(store_bin, home, tmp_path):
    data = home / "c.md"
    data.write_bytes(b"plain")
    missing = run(store_bin, "read", tmp_path / "nokey", home, data)
    assert missing.returncode == 1 and "missing-key" in missing.stderr and not missing.stdout
    inside = run(store_bin, "write", home / "backup.key", home, data, "seal", text="x")
    assert inside.returncode == 1 and "inside-data-root" in inside.stderr
    assert not (home / "backup.key").exists()


def test_rust_write_if_changed_keeps_an_equal_file_and_leaves_no_temp(store_bin, home, tmp_path):
    key_file = tmp_path / "keys" / "backup.key"
    key = keyfile.load_key(create=True, path=key_file)
    data = home / "day" / "log.md"
    data.parent.mkdir(parents=True)
    data.write_bytes(seal(key, "same"))
    before = (data.read_bytes(), data.stat().st_mtime_ns)
    same = run(store_bin, "write-if-changed", key_file, home, data, "seal", text="same")
    assert same.returncode == 0 and same.stdout == "unchanged"
    assert (data.read_bytes(), data.stat().st_mtime_ns) == before
    changed = run(store_bin, "write-if-changed", key_file, home, data, "seal", text="new")
    assert changed.returncode == 0 and changed.stdout == "wrote"
    assert unseal(key, data.read_bytes()) == "new"
    assert [p.name for p in data.parent.iterdir() if p.name.endswith(".tmp")] == []


def test_concurrent_rust_writers_leave_one_whole_file_and_no_temp(store_bin, home, tmp_path):
    key_file = tmp_path / "keys" / "backup.key"
    key = keyfile.load_key(create=True, path=key_file)
    data = home / "mixed.json"
    texts = {f"rust-{i}" for i in range(20)}
    procs = [subprocess.Popen([store_bin, "write", key_file, home, data, "seal"], stdin=subprocess.PIPE, text=True) for _ in range(20)]
    barrier = threading.Barrier(len(procs))

    def feed(proc, text):
        # The binary writes only after stdin EOF. Release every communicate call
        # together so no earlier process can finish while a later one waits.
        barrier.wait(timeout=10)
        proc.communicate(text, timeout=10)

    with concurrent.futures.ThreadPoolExecutor(max_workers=len(procs)) as pool:
        list(pool.map(feed, procs, sorted(texts)))
    assert all(proc.returncode == 0 for proc in procs)
    assert unseal(key, data.read_bytes()) in texts
    assert [p for p in os.listdir(home) if p.endswith(".tmp")] == []
