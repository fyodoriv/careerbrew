from __future__ import annotations

import re
import shutil
from pathlib import Path

import pytest

FIXTURES = Path(__file__).resolve().parents[1] / "fixtures"
CANARIES = ["CANARY-PERSON-7Q", "CANARY-EMPLOYER-3X", "PROJECTZEBRA"]
# The checkout's own environment. A unit run must leave it as it found it (task unit-test-relinks-owner-venv).
OWNER_VENV_CFG = Path(__file__).resolve().parents[2] / ".venv" / "pyvenv.cfg"


def venv_change(cfg: Path, before: bytes | None) -> str | None:
    """Why `cfg` differs from its bytes at session start, or None when it is unchanged."""
    if before is None:
        return None
    if not cfg.exists():
        return f"{cfg} was removed during the unit run (fix: uv sync --locked --reinstall)"
    if cfg.read_bytes() != before:
        return f"{cfg.name} changed during the unit run: a test rebuilt the checkout's .venv (fix: uv sync --locked --reinstall)"
    return None


def pytest_sessionstart(session):
    session.config.stash[_OWNER_VENV] = OWNER_VENV_CFG.read_bytes() if OWNER_VENV_CFG.exists() else None


def pytest_sessionfinish(session, exitstatus):
    if hasattr(session.config, "workerinput"):  # an xdist worker: the controller checks once
        return
    message = venv_change(OWNER_VENV_CFG, session.config.stash.get(_OWNER_VENV, None))
    if message:
        session.config.get_terminal_writer().line(f"FAIL  {message}", red=True)
        session.exitstatus = pytest.ExitCode.TESTS_FAILED


_OWNER_VENV = pytest.StashKey[bytes | None]()


@pytest.fixture(autouse=True)
def home(tmp_path, monkeypatch):
    from careerbrew import plugins

    # Subprocess-based CLI tests must not deploy into the owner's agent configuration.
    client_home = tmp_path / ".test-client-home"
    client_home.mkdir()
    monkeypatch.setenv("HOME", str(client_home))
    bin_dir = tmp_path / ".agentbrew-test-bin"
    bin_dir.mkdir()
    helper = bin_dir / "agentbrew"
    helper.write_text('#!/bin/sh\n[ "$*" = "sync --only commands" ] || exit 2\n[ -f Agentfile.yaml ] || exit 3\n')
    helper.chmod(0o755)
    import os

    monkeypatch.setenv("PATH", str(bin_dir) + os.pathsep + os.environ.get("PATH", ""))
    monkeypatch.setattr(plugins, "_run_agentbrew", lambda folder: None)  # external deployment has its own scratch fixture
    monkeypatch.setenv("CAREERBREW_CODEX_CONFIG", str(tmp_path / "no-codex.toml"))
    root = tmp_path / "data"
    monkeypatch.setenv("CAREERBREW_HOME", str(root))
    monkeypatch.setenv("CAREERBREW_EVENTS", "off")
    monkeypatch.setenv("CAREERBREW_EXTRA_KNOWLEDGE", "0")  # only the base knowledge set: the same result with or without the optional batch files
    monkeypatch.setenv("TZ", "Etc/GMT+4")  # the zone an empty `timezone` setting falls back to: fixed UTC-4, no place name
    monkeypatch.setenv("CAREERBREW_POSTHOG_KEY", "")  # never the Keychain key: no test sends real telemetry
    monkeypatch.setenv("CAREERBREW_TELEMETRY_URL", "http://127.0.0.1:9")
    monkeypatch.setenv("CAREERBREW_COMMANDS_DIR", str(tmp_path / "commands"))
    monkeypatch.setenv("CAREERBREW_LLM", "off")
    monkeypatch.setenv("CAREERBREW_CLAUDE_CONFIG", str(tmp_path / "no-claude.json"))  # never the real agent config (#312)
    monkeypatch.setenv("CAREERBREW_KEY_FILE", str(tmp_path / "key" / "backup.key"))
    monkeypatch.setenv("CAREERBREW_BACKUP_DIRS", str(tmp_path / "backups"))
    monkeypatch.setenv("CAREERBREW_PAIR_KEY_FILE", str(tmp_path / "key" / "pair.key"))
    monkeypatch.setenv("CAREERBREW_PUBLIC_CARDS", str(tmp_path / "public-cards"))
    monkeypatch.setenv("CAREERBREW_EXERCISES", str(FIXTURES / "exercises"))
    monkeypatch.setenv("XDG_STATE_HOME", str(tmp_path / "state"))  # checkup runs (issue #232): never the owner's ~/.local/state
    monkeypatch.setenv("UV_PYTHON_DOWNLOADS", "never")  # uv never installs a Python under the temp HOME, so it cannot relink a venv to one
    return root


@pytest.fixture
def ats_sources(home):
    (home / "jobs").mkdir(parents=True, exist_ok=True)
    ats = FIXTURES / "ats"
    (home / "jobs" / "sources.yaml").write_text(
        "sources:\n"
        f"  - {{kind: greenhouse, board: northwind, company: Northwind, url: 'file://{ats}/greenhouse-northwind.json'}}\n"
        f"  - {{kind: lever, board: contoso, company: Contoso, url: 'file://{ats}/lever-contoso.json'}}\n"
        f"  - {{kind: ashby, board: fabrikam, company: Fabrikam, url: 'file://{ats}/ashby-fabrikam.json'}}\n"
    )
    (home / "profile.yaml").write_text(f"name: {CANARIES[0]}\nemployer: {CANARIES[1]}\ncurrent_level: 3\ntarget_salary: 180000\nremote_preference: remote\nskills: {{python: 4, typescript: 3, sql: 3}}\nwant_to_learn: [llm, rust]\n")
    (home / "jobs" / "criteria.yaml").write_text("min_seniority: 3\nlocations_any: [Remote, Example City]\nmin_salary: 150000\n")
    return home


@pytest.fixture
def acme_plugin(tmp_path):
    dest = tmp_path / "plugin-acme"
    shutil.copytree(FIXTURES / "plugin-acme", dest)
    return dest


def readme_repo() -> str:
    """The project's repository address, read from the README link, so no test holds an owner name."""
    readme = Path(__file__).resolve().parents[2] / "README.md"
    found = re.findall(r"github\.com/([\w-]+/careerbrew)", readme.read_text())
    assert found, "README.md has no github.com/<owner>/careerbrew link"
    return found[0]
