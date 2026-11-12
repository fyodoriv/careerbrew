"""Issue #183: an empty job-hunt folder that a read left behind does not fail corporate readiness; a real file still does."""

import pytest

from careerbrew import machine, paths, store

CHECK = "no job-hunt folders on this machine"


@pytest.fixture
def corporate(home, monkeypatch, tmp_path):
    monkeypatch.setenv("HOME", str(tmp_path / "userhome"))
    machine.set_role("corporate", "")


def _row():
    return next(r for r in machine.readiness() if r["check"] == CHECK)


def test_an_empty_folder_left_by_a_read_passes(corporate):
    paths.sub("tutor", "readiness.jsonl")  # what a reader such as review.py calls: it makes tutor/ and no file
    assert (paths.data_root() / "tutor").is_dir()
    assert _row()["ok"], _row()


def test_a_job_hunt_file_still_fails(corporate):
    store.write_text(paths.sub("tutor", "readiness.jsonl"), "{}\n")
    assert not _row()["ok"]


def test_finder_and_lock_files_are_not_job_hunt_data(corporate):
    folder = paths.data_root() / "tutor"
    folder.mkdir()
    (folder / ".DS_Store").write_bytes(b"\0")
    (folder / ".readiness.jsonl.lock").write_text("")
    assert _row()["ok"], _row()
    machine.set_role("corporate", "")  # set corporate again: it clears the folder, it does not refuse
    assert not folder.exists()


def test_a_temp_file_left_by_a_crashed_write_still_fails(corporate):
    folder = paths.data_root() / "tutor"
    folder.mkdir()
    (folder / ".readiness.jsonl.123.abc.tmp").write_text("a full copy")
    assert not _row()["ok"]
