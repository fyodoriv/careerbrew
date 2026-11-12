"""Approved GitHub header repair never saves a token or replaces a symlink."""

import json
import stat
import subprocess

import pytest

from careerbrew import cli
from careerbrew.workday import mcp_check


def settings(tmp_path):
    target = tmp_path / "owned-config.json"
    data = {"unrelated": {"keep": True}, "mcpServers": {"fixture": {"type": "http", "url": "https://api.githubcopilot.com/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}", "X-Test": "keep"}}}}
    target.write_text(json.dumps(data))
    target.chmod(0o640)
    link = tmp_path / "claude.json"
    link.symlink_to(target)
    return link, target


def fake_gh(tmp_path, monkeypatch):
    folder = tmp_path / "fake tools"
    folder.mkdir()
    gh = folder / "gh"
    gh.write_text('#!/bin/sh\n[ "$*" = "auth token --hostname github.com" ] || exit 2\nprintf "%s\\n" "synthetic-header-canary-7"\n')
    gh.chmod(0o755)
    monkeypatch.setattr(mcp_check.shutil, "which", lambda name: str(gh) if name == "gh" else None)
    return gh


def test_repair_requires_explicit_owner_confirmation(tmp_path):
    link, target = settings(tmp_path)
    before = target.read_bytes()
    with pytest.raises(ValueError, match="--yes"):
        mcp_check.fix_github_headers(path=link, yes=False, getenv=lambda key: "")
    assert target.read_bytes() == before and link.is_symlink()


def test_failed_launchctl_probe_does_not_authorize_a_repair(tmp_path, monkeypatch, capsys):
    link, target = settings(tmp_path)
    fake_gh(tmp_path, monkeypatch)
    before = target.read_bytes(), target.stat().st_mtime_ns, stat.S_IMODE(target.stat().st_mode), link.readlink()
    calls = []

    def failed_probe(argv, **kwargs):
        calls.append(argv)
        assert argv == ["launchctl", "getenv", "GITHUB_TOKEN"]
        return subprocess.CompletedProcess(argv, 7, stdout="", stderr="synthetic-probe-failure")

    monkeypatch.setattr(mcp_check.subprocess, "run", failed_probe)
    result = mcp_check.fix_github_headers(path=link, yes=True)
    assert result["changed"] == 0
    assert (target.read_bytes(), target.stat().st_mtime_ns, stat.S_IMODE(target.stat().st_mode), link.readlink()) == before
    assert link.is_symlink() and len(calls) == 1
    assert not capsys.readouterr().out


@pytest.mark.parametrize(
    "returncode,stdout,expected",
    [(0, "", ""), (0, "synthetic-probe-present\n", "synthetic-probe-present"), (7, "synthetic-probe-present\n", None)],
    ids=["successful-empty", "successful-present", "failed-with-output"],
)
def test_launchctl_probe_status_controls(monkeypatch, returncode, stdout, expected):
    def probe(argv, **kwargs):
        assert argv == ["launchctl", "getenv", "SYNTHETIC_MCP_PROBE"]
        return subprocess.CompletedProcess(argv, returncode, stdout=stdout, stderr="")

    monkeypatch.setattr(mcp_check.subprocess, "run", probe)
    assert mcp_check._launchctl("SYNTHETIC_MCP_PROBE") == expected


def test_repair_preserves_link_target_mode_and_unrelated_settings(tmp_path, monkeypatch):
    link, target = settings(tmp_path)
    fake_gh(tmp_path, monkeypatch)
    result = mcp_check.fix_github_headers(path=link, yes=True, getenv=lambda key: "")
    assert result["changed"] == 1 and link.is_symlink()
    assert stat.S_IMODE(target.stat().st_mode) == 0o640
    data = json.loads(target.read_text())
    assert data["unrelated"] == {"keep": True}
    server = data["mcpServers"]["fixture"]
    assert server["headers"] == {"X-Test": "keep"}
    assert "headersHelper" in server and "synthetic-header-canary-7" not in target.read_text()
    before = target.read_bytes(), target.stat().st_mtime_ns
    assert mcp_check.fix_github_headers(path=link, yes=True, getenv=lambda key: "")["changed"] == 0
    assert (target.read_bytes(), target.stat().st_mtime_ns) == before


def test_helper_reads_gh_only_when_client_runs_it(tmp_path, monkeypatch):
    link, target = settings(tmp_path)
    fake_gh(tmp_path, monkeypatch)
    mcp_check.fix_github_headers(path=link, yes=True, getenv=lambda key: "")
    command = json.loads(target.read_text())["mcpServers"]["fixture"]["headersHelper"]
    got = subprocess.run(command, shell=True, capture_output=True, text=True, check=True)
    assert json.loads(got.stdout) == {"Authorization": "Bearer synthetic-header-canary-7"}
    assert not got.stderr


@pytest.mark.parametrize("case", ["foreign-host", "already-helper", "disabled", "launchd-unknown", "token-present", "ambiguous-auth"])
def test_unsupported_or_healthy_configuration_stays_unchanged(tmp_path, monkeypatch, case):
    link, target = settings(tmp_path)
    fake_gh(tmp_path, monkeypatch)
    data = json.loads(target.read_text())
    server = data["mcpServers"]["fixture"]
    if case == "foreign-host":
        server["url"] = "https://github.com.example.test/mcp"
    elif case == "already-helper":
        server["headersHelper"] = "/owner/helper"
    elif case == "disabled":
        server["enabled"] = False
    elif case == "ambiguous-auth":
        server["headers"]["authorization"] = "Bearer synthetic-static-canary-7"
    target.write_text(json.dumps(data))
    before = target.read_bytes()

    def env(key):
        return None if case == "launchd-unknown" else "already-set" if case == "token-present" else ""

    assert mcp_check.fix_github_headers(path=link, yes=True, getenv=env)["changed"] == 0
    assert target.read_bytes() == before


def test_cli_fix_without_yes_refuses_before_any_owner_config_change(tmp_path, monkeypatch, capsys):
    link, target = settings(tmp_path)
    monkeypatch.setenv("CAREERBREW_CLAUDE_CONFIG", str(link))
    before = target.read_bytes()
    assert cli.main(["day", "start", "--fix"]) == 2
    assert "--yes" in capsys.readouterr().out
    assert target.read_bytes() == before


def test_concurrent_owner_edit_is_kept_and_temporary_file_is_removed(tmp_path, monkeypatch):
    link, target = settings(tmp_path)
    fake_gh(tmp_path, monkeypatch)
    original_fsync = mcp_check.os.fsync

    def edit_during_prepare(fd):
        data = json.loads(target.read_text())
        data["unrelated"]["owner_edit"] = True
        target.write_text(json.dumps(data))
        return original_fsync(fd)

    monkeypatch.setattr(mcp_check.os, "fsync", edit_during_prepare)
    with pytest.raises(ValueError, match="changed during repair"):
        mcp_check.fix_github_headers(path=link, yes=True, getenv=lambda key: "")
    assert json.loads(target.read_text())["unrelated"]["owner_edit"] is True
    assert "headersHelper" not in target.read_text()
    assert not list(tmp_path.glob(".careerbrew-mcp-*.tmp"))


def test_helper_failure_is_quiet_and_never_returns_a_partial_header(tmp_path, monkeypatch):
    link, target = settings(tmp_path)
    gh = fake_gh(tmp_path, monkeypatch)
    gh.write_text('#!/bin/sh\nprintf "%s\n" "synthetic-error-canary" >&2\nexit 1\n')
    mcp_check.fix_github_headers(path=link, yes=True, getenv=lambda key: "")
    command = json.loads(target.read_text())["mcpServers"]["fixture"]["headersHelper"]
    got = subprocess.run(command, shell=True, capture_output=True, text=True)
    assert got.returncode != 0 and not got.stdout and not got.stderr


def test_repair_is_discoverable_and_confirmation_survives_the_ui_handler(tmp_path, monkeypatch):
    from careerbrew import actions

    link, target = settings(tmp_path)
    monkeypatch.setenv("CAREERBREW_CLAUDE_CONFIG", str(link))
    before = target.read_bytes()
    action = next(a for a in actions.catalog() if a["name"] == "day.start")
    assert {"fix", "yes"} <= {a["name"] for a in action["args"]}
    with pytest.raises(actions.ActionError, match="confirm"):
        actions.run("day.start", {"fix": True})
    assert target.read_bytes() == before
    fake_gh(tmp_path, monkeypatch)
    monkeypatch.setattr(mcp_check, "_launchctl", lambda key: "")
    result = actions.run("day.start", {"fix": True, "json": True}, confirm=True)
    assert result["exit"] == 0 and json.loads(result["output"])["changed"] == 1
    assert "--yes" in result["argv"]


def test_preflight_offers_only_the_approved_repair_for_known_github_auth():
    server = {"url": "https://api.githubcopilot.com/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}"}}
    rows = mcp_check.check({"fixture": server}, getenv=lambda key: "", which=lambda *args, **kwargs: None, repair_host="claude")
    assert not rows[0]["ok"] and "--fix --yes" in rows[0]["safe"] and "owner approves" in rows[0]["safe"]


def test_update_and_config_repair_modes_cannot_run_together(monkeypatch, capsys):
    from careerbrew.workday import self_update

    monkeypatch.setattr(self_update, "run", lambda: (_ for _ in ()).throw(AssertionError("source update ran")))
    monkeypatch.setattr(mcp_check, "fix_github_headers", lambda **kwargs: (_ for _ in ()).throw(AssertionError("config changed")))
    assert cli.main(["day", "start", "--update-only", "--fix", "--yes"]) == 2
    assert "separately" in capsys.readouterr().out


@pytest.mark.parametrize("invalid", [False, True])
def test_registered_source_ownership_is_not_overwritten(tmp_path, monkeypatch, invalid):
    from pathlib import Path

    link, target = settings(tmp_path)
    fake_gh(tmp_path, monkeypatch)
    state = Path.home() / ".config/agentbrew/state.yaml"
    state.parent.mkdir(parents=True)
    state.write_text("mcpServers: [" if invalid else "mcpServers:\n  - name: fixture\n    source: user\n")
    before = target.read_bytes()
    with pytest.raises(ValueError, match="source|ownership"):
        mcp_check.fix_github_headers(path=link, yes=True, getenv=lambda key: "")
    assert target.read_bytes() == before


def test_native_client_never_offers_claude_config_repair():
    server = {"url": "https://api.githubcopilot.com/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}"}}
    rows = mcp_check.check({"fixture": server}, getenv=lambda key: "", repair_host="codex")
    assert "--fix" not in rows[0]["safe"]


def test_source_registration_during_prepare_stops_the_repair(tmp_path, monkeypatch):
    from pathlib import Path

    link, target = settings(tmp_path)
    fake_gh(tmp_path, monkeypatch)
    before = target.read_bytes()
    real_sync = mcp_check.os.fsync

    def register_during_prepare(fd):
        real_sync(fd)
        state = Path.home() / ".config/agentbrew/state.yaml"
        state.parent.mkdir(parents=True, exist_ok=True)
        state.write_text("mcpServers:\n  - name: fixture\n    source: user\n")

    monkeypatch.setattr(mcp_check.os, "fsync", register_during_prepare)
    with pytest.raises(ValueError, match="source"):
        mcp_check.fix_github_headers(path=link, yes=True, getenv=lambda key: "")
    assert target.read_bytes() == before
    assert not list(target.parent.glob(".careerbrew-mcp-*.tmp"))


def test_helper_survives_removing_the_invoking_venv_launcher(tmp_path, monkeypatch):
    from pathlib import Path

    link, target = settings(tmp_path)
    fake_gh(tmp_path, monkeypatch)
    launcher = tmp_path / "temporary venv python"
    launcher.symlink_to(Path(mcp_check.sys.executable).resolve())
    monkeypatch.setattr(mcp_check.sys, "executable", str(launcher))
    mcp_check.fix_github_headers(path=link, yes=True, getenv=lambda key: "")
    command = json.loads(target.read_text())["mcpServers"]["fixture"]["headersHelper"]
    launcher.unlink()
    got = subprocess.run(command, shell=True, capture_output=True, text=True)
    assert got.returncode == 0 and json.loads(got.stdout)["Authorization"].startswith("Bearer ")


def test_helper_keeps_the_stable_gh_entry_after_its_target_is_updated(tmp_path, monkeypatch):
    link, target = settings(tmp_path)
    original = fake_gh(tmp_path, monkeypatch)
    stable_bin = tmp_path / "stable bin"
    stable_bin.mkdir()
    stable = stable_bin / "gh"
    stable.symlink_to(original)
    monkeypatch.setattr(mcp_check.shutil, "which", lambda name: str(stable) if name == "gh" else None)
    mcp_check.fix_github_headers(path=link, yes=True, getenv=lambda key: "")
    command = json.loads(target.read_text())["mcpServers"]["fixture"]["headersHelper"]
    updated = original.with_name("gh-updated")
    updated.write_bytes(original.read_bytes())
    updated.chmod(original.stat().st_mode)
    stable.unlink()
    stable.symlink_to(updated)
    original.unlink()
    got = subprocess.run(command, shell=True, capture_output=True, text=True)
    assert got.returncode == 0 and json.loads(got.stdout)["Authorization"].startswith("Bearer ")
