"""Backup and sync bundle safety uses temporary synthetic files only."""

import shutil
from functools import partial

import pytest

from careerbrew import backup, sync, vault


@pytest.mark.parametrize("kind", ["backup", "sync", "sync-no-extension"])
def test_existing_bundle_blocks_bulk_encryption_without_mutation(home, tmp_path, monkeypatch, kind):
    home.mkdir(parents=True)
    record = home / "record.txt"
    record.write_text("synthetic record")
    if kind == "backup":
        source = backup.create()[0]
    else:
        sync.pair_init()
        source = tmp_path / "external.cbsync"
        sync.export(source)
    target = home / ("bundle-without-extension" if kind == "sync-no-extension" else source.name)
    shutil.copyfile(source, target)
    marker = home / vault.MARKER
    marker.write_text("synthetic marker")
    before = {p: p.read_bytes() for p in (record, target, marker)}
    with pytest.raises((RuntimeError, ValueError), match="bundle|backup"):
        vault.encrypt_all()
    assert {p: p.read_bytes() for p in before} == before


@pytest.mark.parametrize("kind", ["backup", "sync"])
@pytest.mark.parametrize("through_link", [False, True])
def test_new_bundle_inside_data_root_is_refused(home, tmp_path, monkeypatch, kind, through_link):
    home.mkdir(parents=True)
    base = home
    if through_link:
        base = tmp_path / "data-link"
        base.symlink_to(home, target_is_directory=True)
    if kind == "backup":
        target = base / "backups"
        monkeypatch.setenv("CAREERBREW_BACKUP_DIRS", str(target))
        operation = backup.create
    else:
        sync.pair_init()
        target = base / "bundle.cbsync"
        operation = partial(sync.export, target)
    with pytest.raises((RuntimeError, ValueError), match="outside.*data"):
        operation()
    assert not target.exists()


def test_external_bundles_remain_readable(home, tmp_path):
    home.mkdir(parents=True)
    (home / "record.txt").write_text("synthetic")
    path = backup.create()[0]
    assert backup.verify(path) > 0
    sync.pair_init()
    bundle = tmp_path / "external.cbsync"
    sync.export(bundle)
    assert sync._fernet().decrypt(bundle.read_bytes()).startswith(b"{")
