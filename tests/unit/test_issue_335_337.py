"""Keep the reported setup route and shell failures in the agent guides."""

from careerbrew import paths


def test_existing_plugin_route_precedes_creation_steps():
    text = (paths.REPO_ROOT / "docs/corporate-machine.md").read_text()
    opening = text.split("## Rules first")[0]
    assert "#a-second-person-at-the-company" in opening
    assert "already has a plugin" in opening


def test_zsh_equals_trap_has_a_quoted_safe_form():
    text = (paths.REPO_ROOT / "skills/careerbrew-work-day/shell-traps.md").read_text()
    assert "zsh `=word` expansion" in text
    assert "echo '====='" in text


def test_network_guard_warning_covers_the_whole_compound_line():
    text = (paths.REPO_ROOT / "skills/careerbrew-work-day/shell-traps.md").read_text()
    assert "whole line" in text
    assert "`git fetch`" in text
    assert "literal paths" in text
