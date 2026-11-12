"""A backup must not replace a missing key for sealed synthetic data."""

import pytest

from careerbrew import backup, vault


@pytest.mark.parametrize("kind", ["record", "replay", "plugin"])
def test_backup_does_not_replace_missing_key(home, tmp_path, kind):
    if kind == "replay":
        path = home / "learn" / "synthetic" / "replay" / "7" / ".git" / "HEAD"
    elif kind == "plugin":
        path = home / "plugins" / "synthetic" / "config.yaml"
    else:
        path = home / "work" / "synthetic.txt"
    path.parent.mkdir(parents=True)
    path.write_bytes(vault.seal_bytes(b"synthetic sealed data"))
    before = path.read_bytes()
    key = backup.key_path()
    key.unlink()
    vault._fernet.cache_clear()
    with pytest.raises(vault.VaultError, match="missing"):
        backup.create()
    assert not key.exists()
    assert path.read_bytes() == before
    assert not list((tmp_path / "backups").glob("*.cbk"))


def test_backup_still_creates_key_on_clean_data_root(home):
    home.mkdir(parents=True)
    key = backup.load_key()
    assert key == backup.load_key(create=False)
    assert backup.create()
