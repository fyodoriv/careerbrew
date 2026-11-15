"""One version source: pyproject.toml. careerbrew.__version__, uv.lock and the release script follow it."""

from __future__ import annotations

import re
import subprocess
import tomllib
from pathlib import Path

import careerbrew
from careerbrew import telemetry

ROOT = Path(__file__).resolve().parents[2]
PYPROJECT = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))["project"]["version"]


def test_the_package_version_is_read_from_pyproject_not_written_twice():
    assert not re.search(r'^__version__ = "', (ROOT / "src/careerbrew/__init__.py").read_text(encoding="utf-8"), re.M)
    assert careerbrew.__version__ == PYPROJECT


def test_the_lock_file_carries_the_same_version():
    lock = tomllib.loads((ROOT / "uv.lock").read_text(encoding="utf-8"))
    assert [p["version"] for p in lock["package"] if p["name"] == "careerbrew"] == [PYPROJECT]


def test_the_version_is_plain_so_telemetry_sends_it():
    assert telemetry._VERSION.match(PYPROJECT)


def test_the_release_script_refuses_a_version_that_is_not_plain():
    for bad in ("1.2", "v1.2.3", "1.2.3-rc1", ""):
        run = subprocess.run(["bash", "scripts/release.sh", bad], cwd=ROOT, capture_output=True, text=True, check=False)
        assert run.returncode == 2 and "usage: scripts/release.sh" in run.stderr, bad


def test_the_release_script_refuses_a_version_that_does_not_move_up():
    run = subprocess.run(["bash", "scripts/release.sh", PYPROJECT], cwd=ROOT, capture_output=True, text=True, check=False)
    assert run.returncode == 2 and "must be higher than" in run.stderr
