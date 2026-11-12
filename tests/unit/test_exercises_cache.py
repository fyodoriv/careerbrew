"""exercises.load() parses each YAML file once until it changes (issue #57: Today timed out after a goal was set)."""

import copy

import yaml

from careerbrew.tutor import exercises


def test_load_parses_each_file_once_until_it_changes(tmp_path, monkeypatch):
    (tmp_path / "a.yaml").write_text("id: a\norder: 1\ntitle: A\n", encoding="utf-8")
    monkeypatch.setenv("CAREERBREW_EXERCISES", str(tmp_path))
    calls = []
    real = yaml.safe_load
    monkeypatch.setattr(exercises.yaml, "safe_load", lambda s: calls.append(1) or real(s))
    first = exercises.load()
    second = exercises.load()
    assert first == second == [{"id": "a", "order": 1, "title": "A"}]
    assert len(calls) == 1
    second[0]["title"] = "mutated"  # a caller's edit never leaks into the cache
    assert exercises.load()[0]["title"] == "A"
    (tmp_path / "b.yaml").write_text("id: b\norder: 0\ntitle: B\n", encoding="utf-8")
    assert [e["id"] for e in exercises.load()] == ["b", "a"]
    assert copy.deepcopy(exercises.load()) == exercises.load()
