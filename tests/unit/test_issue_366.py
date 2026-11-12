"""Frozen path-alias histories: registration120ab000, four primaries and eleven controls."""

import socket
import subprocess
from pathlib import Path

import pytest

from careerbrew import paths, secrets, store, vault
from careerbrew.cli import main, may_show_business

VALUE = "synthetic-outline-value-366"


def _snapshot(root):
    return {str(p.relative_to(root)): (p.read_bytes(), p.stat().st_mtime_ns) for p in root.rglob("*") if p.is_file()}


@pytest.fixture
def alias_world(home, tmp_path, monkeypatch):
    assert home.name != "private"
    store.write_yaml(home / "profile.yaml", {"current_level": 3})
    store.write_yaml(home / "cache/evidence/index.yaml", {"count": 7, "note": VALUE})
    store.write_yaml(home / "private/nested.yaml", {"nested_only": 11})
    store.write_yaml(home / "privateish/literal.yaml", {"literal_only": 13})
    outside = tmp_path / "outside.yaml"
    outside.write_text(f"outside: {VALUE}\n")
    (home / "escape.yaml").symlink_to(outside)
    assert paths.data_root().resolve() == home.resolve()
    assert Path.home().resolve().is_relative_to(tmp_path.resolve())
    assert vault.is_sealed((home / "cache/evidence/index.yaml").read_bytes())
    assert not (home / "private/private/nested.yaml").exists()
    assert not (home / "nested.yaml").exists()
    assert not may_show_business(), "BLOCKED: fixture must use the restricted agent output path"
    attempts = []

    def deny(*_args, **_kwargs):
        attempts.append("forbidden host operation")
        raise AssertionError("BLOCKED: a host transport or secret resolver was called")

    monkeypatch.setattr(subprocess, "run", deny)
    monkeypatch.setattr(subprocess, "Popen", deny)
    monkeypatch.setattr(socket, "create_connection", deny)
    monkeypatch.setattr(socket.socket, "connect", deny)
    monkeypatch.setattr(socket.socket, "connect_ex", deny)
    monkeypatch.setattr(socket.socket, "sendto", deny)
    monkeypatch.setattr(secrets, "resolve", deny)
    before = _snapshot(tmp_path)
    yield home, outside
    assert attempts == []
    assert _snapshot(tmp_path) == before, "show must preserve all synthetic file bytes and mtimes"


def _show(capsys, name, *, keys=False):
    capsys.readouterr()
    rc = main(["data", "show", str(name), *(["--keys"] if keys else [])])
    captured = capsys.readouterr()
    assert captured.err == ""
    return rc, captured.out


@pytest.mark.parametrize("case", ["P01-personal", "P02-cache-keys", "P03-cache-refusal", "P04-exact-once"])
def test_private_alias_primary(alias_world, capsys, case):
    home, _outside = alias_world
    if case == "P01-personal":
        reference = _show(capsys, "profile.yaml")
        assert reference == (0, "current_level: 3\n")
        assert _show(capsys, "private/profile.yaml") == reference
    elif case == "P02-cache-keys":
        reference = _show(capsys, "cache/evidence/index.yaml", keys=True)
        assert reference[0] == 0 and reference[1].splitlines()[1:] == ["count: int", "note: str"]
        assert VALUE not in reference[1]
        assert _show(capsys, "private/cache/evidence/index.yaml", keys=True) == reference
    elif case == "P03-cache-refusal":
        reference = _show(capsys, "cache/evidence/index.yaml")
        assert reference[0] == 3 and "--keys" in reference[1] and VALUE not in reference[1]
        assert _show(capsys, "private/cache/evidence/index.yaml") == reference
    else:
        assert (home / "private/nested.yaml").is_file()
        rc, out = _show(capsys, "private/private/nested.yaml", keys=True)
        assert rc == 0
        assert out.splitlines()[0].startswith("private/nested.yaml: yaml, encrypted on disk")
        assert out.splitlines()[1:] == ["nested_only: int"]
        assert "11" not in out


@pytest.mark.parametrize(
    "case",
    ["C01-personal", "C02-cache-keys", "C03-cache-refusal", "C04-missing", "C05-directory", "C06-traversal", "C07-symlink", "C08-absolute-inside", "C09-absolute-outside", "C11-lookalike", "C12-root-directory"],
)
def test_private_alias_control(alias_world, capsys, case):
    home, outside = alias_world
    if case in {"C01-personal", "C08-absolute-inside"}:
        name = home / "profile.yaml" if case == "C08-absolute-inside" else "profile.yaml"
        assert _show(capsys, name) == (0, "current_level: 3\n")
    elif case == "C02-cache-keys":
        rc, out = _show(capsys, "cache/evidence/index.yaml", keys=True)
        assert rc == 0 and out.splitlines()[1:] == ["count: int", "note: str"]
        assert VALUE not in out
    elif case == "C03-cache-refusal":
        rc, out = _show(capsys, "cache/evidence/index.yaml")
        assert rc == 3 and "--keys" in out and VALUE not in out
    elif case == "C11-lookalike":
        rc, out = _show(capsys, "privateish/literal.yaml", keys=True)
        assert rc == 0 and out.splitlines()[1:] == ["literal_only: int"]
        assert "13" not in out
        rc, out = _show(capsys, "privateish/literal.yaml")
        assert rc == 3 and "--keys" in out and "literal_only" not in out
    else:
        names = {
            "C04-missing": "private/missing.yaml",
            "C05-directory": "private/cache",
            "C06-traversal": "private/../outside.yaml",
            "C07-symlink": "private/escape.yaml",
            "C09-absolute-outside": outside,
            "C12-root-directory": "private/",
        }
        rc, out = _show(capsys, names[case])
        assert rc == 1 and "no such data file" in out
        assert VALUE not in out
