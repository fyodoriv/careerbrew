"""Upstream feedback contract: agents offer a safe, consented report to the author."""

import tomllib
from pathlib import Path

import careerbrew

from .conftest import readme_repo

ROOT = Path(__file__).resolve().parents[2]
# The repo address comes from the README, so this test holds no owner name.
REPO = readme_repo()
# The author name comes from the package metadata for the same reason.
AUTHOR = tomllib.loads((ROOT / "pyproject.toml").read_text())["project"]["authors"][0]["name"].split()[0]


def _section() -> str:
    text = (ROOT / "CONTRIBUTING.md").read_text()
    return text.split("## Report problems and ideas", 1)[1].split("\n## ", 1)[0]


def test_contributing_welcomes_reports_and_names_the_author():
    assert f"{AUTHOR}, the author of careerbrew, is glad to get every report" in _section()


def test_the_readme_address_is_the_one_the_code_uses():
    assert careerbrew.REPO == REPO, "README.md and careerbrew.REPO name different repositories"
    assert careerbrew.CONTACT == "github.com/" + REPO.split("/")[0]


def test_contributing_shows_the_safety_steps_and_the_opt_out():
    text = _section()
    assert "agentbrew report --title" in text and f"--repo {REPO}" in text
    assert "never posts anything on its own" in text
    assert "Only after your yes" in text
    assert "second yes" in text
    assert "agentbrew report --off" in text


def test_reports_never_include_private_data():
    assert "never includes anything from `private/`" in _section()


def test_every_install_path_points_at_the_report_flow():
    assert "CONTRIBUTING.md#report-problems-and-ideas" in (ROOT / "AGENTS.md").read_text()
    command = (ROOT / "commands" / "careerbrew.md").read_text()
    assert "report-upstream" in command and "post only after they say yes" in command
    assert "  - report-upstream" in (ROOT / "Agentfile.yaml").read_text()


def test_contributing_is_tracked_by_the_allowlist():
    assert "!/CONTRIBUTING.md" in (ROOT / ".gitignore").read_text()
