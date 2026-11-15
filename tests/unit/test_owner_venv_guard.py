"""A unit run never rebuilds the checkout's own .venv (task unit-test-relinks-owner-venv).

On 2026-11-15 the owner's `.venv/pyvenv.cfg` pointed at a Python that uv had installed under a test's
temp HOME; once pytest removed that folder, every `.venv` command failed. Two guards: uv may not
download a Python during unit tests, and the session fails when `pyvenv.cfg` changed during it.
"""

from __future__ import annotations

import os

from .conftest import venv_change


def test_unit_tests_never_let_uv_download_a_python():
    assert os.environ.get("UV_PYTHON_DOWNLOADS") == "never"


def test_the_session_guard_names_a_changed_venv(tmp_path):
    cfg = tmp_path / ".venv" / "pyvenv.cfg"
    cfg.parent.mkdir()
    cfg.write_text("home = /opt/python/bin\n")
    before = cfg.read_bytes()
    assert venv_change(cfg, before) is None
    cfg.write_text("home = /tmp/pytest-1/.test-client-home/.local/share/uv/python/bin\n")
    message = venv_change(cfg, before)
    assert message and "pyvenv.cfg changed during the unit run" in message and "uv sync --locked --reinstall" in message


def test_the_session_guard_names_a_removed_venv(tmp_path):
    cfg = tmp_path / ".venv" / "pyvenv.cfg"
    assert venv_change(cfg, None) is None
    cfg.parent.mkdir()
    cfg.write_text("home = /opt/python/bin\n")
    before = cfg.read_bytes()
    cfg.unlink()
    assert "removed" in (venv_change(cfg, before) or "")
