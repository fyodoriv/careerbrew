"""The generated agent metadata uses invocation-owned temporary files."""

import os
import stat
import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest
import yaml

from careerbrew import paths, plugins, store, vault

NAMES = ("SYNTHETIC_PRIVATE_ALPHA", "SYNTHETIC_PRIVATE_BETA")


def _inputs(monkeypatch):
    for name in NAMES:
        root = plugins.plugins_dir() / name
        root.mkdir(parents=True)
        (root / "command.md").write_text(f"synthetic {name}\n")
    monkeypatch.setattr(store, "read_yaml", lambda *_args, **_kwargs: {})
    monkeypatch.setattr(plugins, "_command_files", lambda _manifest, root: [root / "command.md"])
    monkeypatch.setattr(plugins, "_run_agentbrew", lambda _folder: None)
    return [{"plugins": {name: {"commands": True}}} for name in NAMES]


def test_concurrent_writers_use_distinct_complete_private_temporaries(home, monkeypatch, capsys):
    locks = _inputs(monkeypatch)
    barrier = threading.Barrier(2)
    seen = []
    original = Path.replace
    fragment = paths.data_root() / "agentbrew" / "Agentfile.yaml"

    def replace(self, target):
        if target == fragment:
            seen.append((self, stat.S_IMODE(self.stat().st_mode), self in vault.files(), yaml.safe_load(self.read_text())))
            barrier.wait(timeout=10)
        return original(self, target)

    monkeypatch.setattr(Path, "replace", replace)
    with ThreadPoolExecutor(max_workers=2) as pool:
        futures = [pool.submit(plugins._sync_command_sources, lock) for lock in locks]
        errors = [future.exception(timeout=20) for future in futures]
    assert errors == [None, None]
    assert len(seen) == 2 and seen[0][0] != seen[1][0]
    assert all(mode == 0o600 and not selected and path.name.endswith(".tmp") for path, mode, selected, _ in seen)
    expected = [{"commands": [str((plugins.plugins_dir() / name / "command.md").resolve())]} for name in NAMES]
    assert all(document in expected for _, _, _, document in seen)
    assert yaml.safe_load(fragment.read_text()) in expected
    assert not list(fragment.parent.glob(".Agentfile.*.tmp"))
    captured = capsys.readouterr()
    assert all(name not in captured.out + captured.err for name in NAMES)


def test_failed_replace_preserves_previous_and_foreign_files(home, monkeypatch):
    lock = _inputs(monkeypatch)[0]
    folder = paths.data_root() / "agentbrew"
    folder.mkdir(parents=True)
    fragment = folder / "Agentfile.yaml"
    fragment.write_text("commands: [previous]\n")
    foreign = folder / ".Agentfile.foreign.tmp"
    foreign.write_text("foreign synthetic file\n")
    original = Path.replace

    def fail(self, target):
        if target == fragment:
            raise OSError("synthetic replacement failure")
        return original(self, target)

    monkeypatch.setattr(Path, "replace", fail)
    with pytest.raises(OSError, match="synthetic replacement failure"):
        plugins._sync_command_sources(lock)
    assert fragment.read_text() == "commands: [previous]\n"
    assert foreign.read_text() == "foreign synthetic file\n"
    assert list(folder.glob(".Agentfile.*.tmp")) == [foreign]


def test_cleanup_leaves_temporary_replaced_before_identity_check(home, monkeypatch):
    lock = _inputs(monkeypatch)[0]
    folder = paths.data_root() / "agentbrew"
    folder.mkdir(parents=True)
    fragment = folder / "Agentfile.yaml"
    fragment.write_text("commands: [previous]\n")
    foreign = folder / "foreign.txt"
    foreign.write_text("foreign synthetic file\n")
    swapped = []

    def swap(self, target):
        assert target == fragment
        self.unlink()
        self.symlink_to(foreign)
        swapped.append(self)
        raise OSError("synthetic replacement failure")

    monkeypatch.setattr(Path, "replace", swap)
    with pytest.raises(OSError, match="synthetic replacement failure"):
        plugins._sync_command_sources(lock)
    assert fragment.read_text() == "commands: [previous]\n"
    assert foreign.read_text() == "foreign synthetic file\n"
    assert swapped[0].is_symlink()


def test_symlinked_metadata_folder_is_refused(home, tmp_path, monkeypatch):
    lock = _inputs(monkeypatch)[0]
    outside = tmp_path / "foreign-metadata"
    outside.mkdir()
    (paths.data_root() / "agentbrew").symlink_to(outside, target_is_directory=True)
    with pytest.raises(plugins.PluginError, match="metadata directory"):
        plugins._sync_command_sources(lock)
    assert not list(outside.iterdir())


def test_failed_identity_check_closes_descriptor_without_unverified_unlink(home, monkeypatch):
    lock = _inputs(monkeypatch)[0]
    folder = paths.data_root() / "agentbrew"
    folder.mkdir(parents=True)
    fragment = folder / "Agentfile.yaml"
    fragment.write_text("commands: [previous]\n")
    created = []
    real_mkstemp = plugins.tempfile.mkstemp
    real_fstat = os.fstat

    def create(*args, **kwargs):
        fd, name = real_mkstemp(*args, **kwargs)
        created.append((fd, Path(name)))
        return fd, name

    def fail(fd):
        if created and fd == created[0][0]:
            raise OSError("synthetic identity failure")
        return real_fstat(fd)

    monkeypatch.setattr(plugins.tempfile, "mkstemp", create)
    monkeypatch.setattr(plugins.os, "fstat", fail)
    with pytest.raises(OSError, match="synthetic identity failure"):
        plugins._sync_command_sources(lock)
    assert fragment.read_text() == "commands: [previous]\n"
    assert created[0][1].exists()
    with pytest.raises(OSError):
        real_fstat(created[0][0])
