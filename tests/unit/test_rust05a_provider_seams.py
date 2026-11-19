"""rust-05a: the key, settings and event seams that the native storage port takes over. Synthetic data only."""

from __future__ import annotations

import json
import stat

import pytest

from careerbrew import backup, config, config_io, events, keyfile, store


def test_keyfile_and_backup_name_the_same_key(home):
    assert keyfile.key_path() == backup.key_path()
    made = keyfile.load_key()
    assert stat.S_IMODE(keyfile.key_path().stat().st_mode) == 0o600
    assert backup.load_key(create=False) == made == keyfile.load_key(create=False)


def test_keyfile_reads_the_path_it_is_given(home, tmp_path):
    other = tmp_path / "elsewhere" / "synthetic.key"
    made = keyfile.load_key(True, other)
    assert other.read_bytes().strip() == made
    assert not keyfile.key_path().exists(), "the default key was left alone"


def test_a_missing_key_without_create_names_its_path(home, tmp_path):
    missing = tmp_path / "none" / "synthetic.key"
    with pytest.raises(FileNotFoundError, match="backup key missing"):
        keyfile.load_key(False, missing)


def test_backup_load_key_follows_a_replaced_backup_key_path(home, tmp_path, monkeypatch):
    """Callers that swap `backup.key_path` still reach the key there (the name stays a seam)."""
    swapped = tmp_path / "swapped" / "synthetic.key"
    monkeypatch.setattr(backup, "key_path", lambda: swapped)
    made = backup.load_key()
    assert swapped.read_bytes().strip() == made


def test_settings_io_round_trips_through_encrypted_storage(home):
    assert config_io.read_settings() == {}
    config_io.write_settings({"questions_per_day": 3})
    assert config_io.settings_path().exists()
    assert store.read_yaml(config_io.settings_path(), {}) == {"questions_per_day": 3}
    assert config_io.read_settings() == {"questions_per_day": 3}


def test_config_set_and_unset_go_through_settings_io(home, monkeypatch):
    writes: list[dict] = []
    real = config_io.write_settings
    monkeypatch.setattr(config_io, "write_settings", lambda data: (writes.append(dict(data)), real(data)))
    config.set_value("questions_per_day", "4")
    assert config.get("questions_per_day") == 4
    config.unset("questions_per_day")
    config.unset("questions_per_day")  # nothing left: no second write
    assert writes == [{"questions_per_day": 4}, {}]


def test_event_record_keeps_only_clean_fields(home):
    row = events.record("config.set", status="ok", note="Free text never passes")
    assert row["event"] == "config.set" and row["status"] == "ok"
    assert "note" not in row
    assert set(row) == {"ts", "v", "event", "status"}


def test_emit_is_record_then_write_row(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    events.emit("config.set", status="ok")
    events.write_row({"b": 1, "a": "x"})
    lines = events.log_path().read_text(encoding="utf-8").splitlines()
    assert json.loads(lines[0])["event"] == "config.set"
    assert lines[1] == '{"a": "x", "b": 1}', "one sorted JSON line"


def test_emit_writes_nothing_when_events_are_off(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "off")
    monkeypatch.setattr(events, "write_row", lambda row: pytest.fail("an event was written"))
    events.emit("config.set", status="ok")
