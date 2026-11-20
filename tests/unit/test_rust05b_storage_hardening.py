"""rust-05b: Python storage hardening the Rust port must then match. Synthetic data and temp folders only."""

from __future__ import annotations

import fcntl
import os
import stat
import threading
from pathlib import Path

import pytest

from careerbrew import backup, config, paths, store, vault


def _blocked_while_locked(lock_path: Path, action) -> bool:
    """Hold `lock_path` from another open file, run `action` in a thread; True when it waited for the lock."""
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    with lock_path.open("a") as held:
        fcntl.flock(held, fcntl.LOCK_EX)
        worker = threading.Thread(target=action)
        worker.start()
        worker.join(0.3)
        waited = worker.is_alive()
        fcntl.flock(held, fcntl.LOCK_UN)
    worker.join(10)
    assert not worker.is_alive()
    return waited


def test_vault_rewrite_uses_a_new_temp_name_each_time(home, monkeypatch):
    home.mkdir(parents=True, exist_ok=True)
    target = home / "synthetic.txt"
    target.write_bytes(b"one")
    names: list[str] = []
    real_replace = Path.replace

    def spy(self, dst):
        names.append(self.name)
        return real_replace(self, dst)

    monkeypatch.setattr(Path, "replace", spy)
    vault._rewrite(target, b"two")
    vault._rewrite(target, b"three")
    assert len(names) == 2 and names[0] != names[1]
    assert target.read_bytes() == b"three"


def test_vault_rewrite_leaves_no_temp_file_when_the_swap_fails(home, monkeypatch):
    home.mkdir(parents=True, exist_ok=True)
    target = home / "synthetic.txt"
    target.write_bytes(b"one")

    def fail(self, dst):
        raise OSError("synthetic replace failure")

    monkeypatch.setattr(Path, "replace", fail)
    with pytest.raises(OSError):
        vault._rewrite(target, b"two")
    assert sorted(p.name for p in home.iterdir()) == ["synthetic.txt"]
    assert target.read_bytes() == b"one"


def test_store_write_text_leaves_no_temp_file_when_the_swap_fails(home, monkeypatch):
    folder = home / "notes"
    folder.mkdir(parents=True)

    def fail(self, dst):
        raise OSError("synthetic replace failure")

    monkeypatch.setattr(Path, "replace", fail)
    with pytest.raises(OSError):
        store.write_text(folder / "a.txt", "synthetic")
    assert list(folder.iterdir()) == []


def test_a_new_key_is_created_at_mode_0600_without_a_later_chmod(monkeypatch):
    monkeypatch.setattr(Path, "chmod", lambda self, mode: None)
    monkeypatch.setattr(os, "chmod", lambda path, mode, **kw: None)
    old = os.umask(0o022)
    try:
        backup.load_key()
    finally:
        os.umask(old)
    assert stat.S_IMODE(backup.key_path().stat().st_mode) == 0o600


def test_a_first_run_that_loses_the_create_race_reads_the_winners_key(monkeypatch):
    path = backup.key_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    winner = backup.Fernet.generate_key()
    path.write_bytes(winner)
    real_exists = Path.exists
    calls = {"n": 0}

    def late_exists(self, *a, **kw):
        if self == path and calls["n"] == 0:
            calls["n"] += 1
            return False  # this process checked before the other writer created the key
        return real_exists(self, *a, **kw)

    monkeypatch.setattr(Path, "exists", late_exists)
    assert backup.load_key() == winner
    assert path.read_bytes() == winner


def test_setting_a_value_waits_for_the_settings_lock(home):
    home.mkdir(parents=True, exist_ok=True)
    assert _blocked_while_locked(home / ".settings.yaml.lock", lambda: config.set_value("work_start", "08:00"))
    assert config.get("work_start") == "08:00"


def test_unsetting_a_value_waits_for_the_settings_lock(home):
    home.mkdir(parents=True, exist_ok=True)
    config.set_value("work_start", "08:00")
    assert _blocked_while_locked(home / ".settings.yaml.lock", lambda: config.unset("work_start"))
    assert config.get("work_start") == config.DEFAULTS["work_start"][0]


def test_a_plaintext_append_takes_the_lock_upsert_takes(home):
    target = paths.data_root() / "logs" / "synthetic.jsonl"
    assert not vault.sealable(target)
    assert _blocked_while_locked(target.with_name(f".{target.name}.lock"), lambda: store.append_jsonl(target, [{"id": "a"}]))
    assert store.read_jsonl(target) == [{"id": "a"}]
