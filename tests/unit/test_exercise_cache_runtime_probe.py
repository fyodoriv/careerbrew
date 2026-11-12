"""Synthetic controls for the exercise validator's saved-pass boundary."""

import pytest

from careerbrew.tutor import exercises


@pytest.mark.parametrize("no_cache", [False, True])
def test_changed_external_input_is_revalidated(tmp_path, monkeypatch, no_cache):
    root = tmp_path / "repo"
    root.mkdir()
    cache = root / ".metrics" / "exercise-check.json"
    cache.parent.mkdir()
    cache.write_text('{"synthetic-old-key": []}')
    original = cache.read_bytes()
    sentinel = tmp_path / "broken"
    example = {"id": "synthetic-exercise", "order": 1}
    calls = []

    def validate(exercise):
        calls.append(exercise["id"])
        return ["synthetic failure"] if sentinel.exists() else []

    monkeypatch.setattr(exercises.paths, "REPO_ROOT", root)
    monkeypatch.setattr(exercises, "load", lambda: [example])
    monkeypatch.setattr(exercises, "validate", validate)
    monkeypatch.delenv("NO_CACHE", raising=False)
    assert exercises.check_all() == []
    assert calls == ["synthetic-exercise"]
    sentinel.touch()
    if no_cache:
        monkeypatch.setenv("NO_CACHE", "1")

    assert exercises.check_all() == ["synthetic-exercise: synthetic failure"]
    assert calls == ["synthetic-exercise", "synthetic-exercise"]
    assert cache.read_bytes() == original


def test_duplicate_and_validation_errors_keep_their_order(tmp_path, monkeypatch):
    monkeypatch.setattr(exercises.paths, "REPO_ROOT", tmp_path)
    monkeypatch.setattr(exercises, "load", lambda: [{"id": "same", "order": 1}, {"id": "same", "order": 1}])
    monkeypatch.setattr(exercises, "validate", lambda _exercise: ["synthetic failure"])
    assert exercises.check_all() == [
        "same: synthetic failure",
        "same: duplicate id",
        "same: duplicate order 1",
        "same: synthetic failure",
    ]
