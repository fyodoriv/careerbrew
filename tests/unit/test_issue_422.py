"""Stale connector detection uses synthetic credentials only."""

import os
import sys

import pytest

from careerbrew.workday import mcp_check

SERVER = {"url": "https://api.githubcopilot.com/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}"}}


@pytest.mark.parametrize("launchd,github,stale", [("synthetic-old", "synthetic-new", True), ("synthetic-same", "synthetic-same", False), ("synthetic-old", None, False)])
def test_github_connector_mismatch_is_reported_without_values(monkeypatch, launchd, github, stale):
    monkeypatch.setattr(mcp_check, "_github_cli_token", lambda: github, raising=False)
    rows = mcp_check.check({"fixture": SERVER}, getenv=lambda key: launchd if key == "GITHUB_TOKEN" else "/usr/bin", repair_host="claude")
    assert any("mismatch" in row["value"] for row in rows) is stale
    assert "synthetic-old" not in str(rows)
    assert "synthetic-new" not in str(rows)
    assert "synthetic-same" not in str(rows)


def test_unrelated_connector_does_not_look_up_github_token(monkeypatch):
    monkeypatch.setattr(mcp_check, "_github_cli_token", lambda: pytest.fail("unexpected credential lookup"), raising=False)
    other = {"url": "https://example.com/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}"}}
    rows = mcp_check.check({"fixture": other}, getenv=lambda key: "synthetic-value", repair_host="claude")
    assert all(row["ok"] for row in rows)


def test_unknown_launchd_probe_does_not_claim_a_mismatch(monkeypatch):
    monkeypatch.setattr(mcp_check, "_github_cli_token", lambda: pytest.fail("unexpected credential lookup"), raising=False)
    rows = mcp_check.check({"fixture": SERVER}, getenv=lambda key: None, repair_host="claude")
    assert not any("mismatch" in row["value"] for row in rows)


def fake_gh(tmp_path, monkeypatch, body):
    script = tmp_path / "gh"
    script.write_text(f"#!{sys.executable}\nimport os, sys, time\n" + body)
    script.chmod(0o700)
    monkeypatch.setattr(mcp_check.shutil, "which", lambda name: str(script))


def test_github_cli_probe_scrubs_token_environment_and_stdin(monkeypatch, tmp_path):
    monkeypatch.setenv("GITHUB_TOKEN", "synthetic-ambient-one")
    monkeypatch.setenv("GH_TOKEN", "synthetic-ambient-two")
    fake_gh(
        tmp_path, monkeypatch, "assert sys.argv[1:] == ['auth', 'token', '--hostname', 'github.com']\nassert 'GITHUB_TOKEN' not in os.environ and 'GH_TOKEN' not in os.environ\nassert sys.stdin.read() == ''\nprint('synthetic-current')\n"
    )
    assert mcp_check._github_cli_token() == "synthetic-current"


@pytest.mark.parametrize("output,code", [("", 0), ("synthetic one\nsynthetic two", 0), ("synthetic-current", 1)])
def test_github_cli_probe_rejects_invalid_output(monkeypatch, tmp_path, output, code):
    fake_gh(tmp_path, monkeypatch, f"sys.stdout.write({output!r})\nsys.exit({code})\n")
    assert mcp_check._github_cli_token() is None


def test_github_cli_probe_rejects_oversized_output(monkeypatch, tmp_path):
    pid_file = tmp_path / "pid"
    fake_gh(tmp_path, monkeypatch, f"open({str(pid_file)!r}, 'w').write(str(os.getpid()))\nsys.stdout.write('x' * 4097)\nsys.stdout.flush()\ntime.sleep(30)\n")
    assert mcp_check._github_cli_token() is None
    with pytest.raises(ProcessLookupError):
        os.kill(int(pid_file.read_text()), 0)


def test_github_cli_probe_accepts_limit_sized_output(monkeypatch, tmp_path):
    fake_gh(tmp_path, monkeypatch, "sys.stdout.write('x' * 4096)\n")
    assert mcp_check._github_cli_token() == "x" * 4096


def test_github_cli_probe_times_out_and_reaps_child(monkeypatch, tmp_path):
    pid_file = tmp_path / "pid"
    fake_gh(tmp_path, monkeypatch, f"open({str(pid_file)!r}, 'w').write(str(os.getpid()))\ntime.sleep(30)\n")
    monkeypatch.setattr(mcp_check, "GITHUB_TOKEN_TIMEOUT", 3.0)
    assert mcp_check._github_cli_token() is None
    with pytest.raises(ProcessLookupError):
        os.kill(int(pid_file.read_text()), 0)
