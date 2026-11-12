"""#58: data that survives a restart, plus an opt-in encrypted iCloud Drive backup."""

from __future__ import annotations

import importlib

import pytest

from careerbrew import backup, config, goals, machine, teacher


@pytest.fixture
def icloud(tmp_path, monkeypatch):
    d = tmp_path / "icloud" / "careerbrew"
    monkeypatch.setenv("CAREERBREW_ICLOUD_DIR", str(d))
    return d


def test_icloud_backup_is_off_by_default(home, icloud):
    assert config.get("backup_icloud") is False
    backup.create()
    assert not icloud.exists(), "nothing goes to iCloud Drive until the user opts in"


def test_icloud_default_folder_is_the_icloud_drive_careerbrew_folder(monkeypatch):
    monkeypatch.delenv("CAREERBREW_ICLOUD_DIR", raising=False)
    assert backup.icloud_dir().parts[-4:] == ("Library", "Mobile Documents", "com~apple~CloudDocs", "careerbrew")


def test_opt_in_icloud_backup_writes_an_encrypted_file_that_restores(home, icloud, tmp_path):
    (home / "notes.md").parent.mkdir(parents=True, exist_ok=True)
    (home / "notes.md").write_text("CANARY-PERSON-7Q shipped the export\n")
    config.set_value("backup_icloud", "true")
    written = backup.create()
    in_cloud = [p for p in written if p.parent == icloud]
    assert len(in_cloud) == 1, "the iCloud folder is a target next to the local ones"
    assert b"CANARY-PERSON-7Q" not in in_cloud[0].read_bytes(), "the cloud copy is encrypted"
    n = backup.restore(in_cloud[0], tmp_path / "restored")
    assert n >= 1 and "CANARY-PERSON-7Q" in (tmp_path / "restored" / "notes.md").read_text()


def test_corporate_machine_refuses_the_icloud_target(home, tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.delenv("CAREERBREW_ICLOUD_DIR", raising=False)
    machine.set_role("corporate", "")
    config.set_value("backup_icloud", "true")
    with pytest.raises(RuntimeError, match="cloud-synced"):
        backup.create()


def test_an_exercise_draft_survives_a_restart(home):
    goals.set_upcoming({"kind": "custom", "title": "Practice", "minutes_per_day": 90})
    s = teacher.complete(teacher.next_step(), {"confirmed": True})["next"]
    teacher.save_draft(s["exercise"]["id"], "def contains_duplicate(nums):\n    return 42\n")
    importlib.reload(teacher)  # a new process: nothing kept in memory
    assert teacher.next_step()["exercise"]["starter"] == "def contains_duplicate(nums):\n    return 42\n"
