"""Key locations use temporary synthetic files only."""

import pytest

from careerbrew import backup, sync


@pytest.mark.parametrize("kind", ["backup", "pair"])
@pytest.mark.parametrize("through_link", [False, True])
def test_new_key_inside_data_root_is_refused(home, tmp_path, monkeypatch, kind, through_link):
    home.mkdir(parents=True)
    base = home
    if through_link:
        base = tmp_path / "data-link"
        base.symlink_to(home, target_is_directory=True)
    path = base / "keys" / f"{kind}.key"
    monkeypatch.setenv("CAREERBREW_KEY_FILE" if kind == "backup" else "CAREERBREW_PAIR_KEY_FILE", str(path))
    with pytest.raises((RuntimeError, ValueError), match="outside.*data"):
        backup.load_key() if kind == "backup" else sync.pair_init()
    assert not path.exists()


@pytest.mark.parametrize("kind", ["backup", "pair"])
def test_existing_unsafe_key_is_not_changed(home, monkeypatch, kind):
    home.mkdir(parents=True)
    path = home / f"{kind}.key"
    path.write_bytes(b"synthetic-existing-key")
    monkeypatch.setenv("CAREERBREW_KEY_FILE" if kind == "backup" else "CAREERBREW_PAIR_KEY_FILE", str(path))
    with pytest.raises((RuntimeError, ValueError), match="outside.*data"):
        backup.load_key() if kind == "backup" else sync.pair_init()
    assert path.read_bytes() == b"synthetic-existing-key"


def test_external_keys_still_work(home):
    assert backup.load_key() == backup.load_key(create=False)
    key = sync.pair_init()
    assert sync._fernet().decrypt(sync._fernet().encrypt(b"synthetic")) == b"synthetic"
    assert key
