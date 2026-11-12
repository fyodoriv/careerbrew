"""The browser suite's one-process seed (tests/e2e/web_seed.py, task web-seed-one-process)."""

from __future__ import annotations

import importlib.util
import os
import sys
from pathlib import Path

import pytest

from careerbrew import cli

ROOT = Path(__file__).resolve().parents[2]


def _seed():
    spec = importlib.util.spec_from_file_location("web_seed", ROOT / "tests" / "e2e" / "web_seed.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def test_a_failing_step_stops_the_seed(monkeypatch):
    seed = _seed()
    monkeypatch.setattr(cli, "main", lambda argv: 2)
    with pytest.raises(SystemExit, match="careerbrew jobs scan` exited 2"):
        seed.cb("jobs", "scan")


def test_a_step_gets_its_stdin_and_env_and_neither_leaks_into_the_next(monkeypatch):
    seed = _seed()
    seen = []
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    monkeypatch.setattr(cli, "main", lambda argv: seen.append((argv, sys.stdin.read(), os.environ["CAREERBREW_NOW"])) or 0)
    seed.cb("work", "feed", "add", "-", stdin='{"id": "x"}\n', CAREERBREW_NOW="2026-10-18T17:00")
    seed.cb("day", "log")
    assert seen == [(["work", "feed", "add", "-"], '{"id": "x"}\n', "2026-10-18T17:00"), (["day", "log"], "", "2026-10-19T09:00")]
    assert sys.stdin is sys.__stdin__


def test_a_step_prints_nothing(monkeypatch, capsys):
    seed = _seed()
    monkeypatch.setattr(cli, "main", lambda argv: print("seeded 3 tasks") or 0)
    seed.cb("work", "task", "x")
    assert capsys.readouterr().out == ""


def test_the_moved_asks_keep_their_workday_distance(tmp_path):
    # Thursday 16:30 before the fixture's Monday is the second workday back; on a Wednesday it is the Monday before.
    fixture = tmp_path / "asks.jsonl"
    fixture.write_text('{"id": "a", "at": "2026-10-15T16:30:00"}\n{"id": "b", "at": "2026-10-19T08:15:00"}\n')
    moved = _seed().moved_asks(fixture, _seed().date(2026, 10, 21))
    assert moved == '{"id": "a", "at": "2026-10-19T16:30:00"}\n{"id": "b", "at": "2026-10-21T08:15:00"}\n'
