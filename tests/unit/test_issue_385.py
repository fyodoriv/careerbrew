"""Measure repeated Git discovery without replacing its authority."""

import json
import os
import shutil
import socket
import subprocess
import sys
import webbrowser
from pathlib import Path

import pytest

from careerbrew import cli, config, models, paths
from careerbrew.workday import start, status

TOP = ("rev-parse", "--show-toplevel")
COMMIT = ("rev-parse", "--short=12", "HEAD")
REMOTE = ("config", "--get", "remote.origin.url")
PRIVATE_CHECKOUT = "SYNTHETIC-CHECKOUT-CANARY-385"
REMOTE_CANARY = "synthetic-remote-canary-385"


@pytest.fixture
def own_git(tmp_path, monkeypatch):
    git = shutil.which("git")
    assert git is not None
    for name in tuple(os.environ):
        if name.startswith("GIT_"):
            monkeypatch.delenv(name)
    monkeypatch.setenv("GIT_AUTHOR_DATE", "2026-11-03T09:00:00-04:00")
    monkeypatch.setenv("GIT_COMMITTER_DATE", "2026-11-03T09:00:00-04:00")
    monkeypatch.setenv("PATH", os.pathsep.join((str(Path(git).parent), "/usr/bin", "/bin")))
    monkeypatch.setenv("GIT_CONFIG_GLOBAL", os.devnull)
    monkeypatch.setenv("GIT_CONFIG_NOSYSTEM", "1")
    monkeypatch.setenv("GIT_ATTR_NOSYSTEM", "1")
    monkeypatch.setenv("GIT_TERMINAL_PROMPT", "0")
    monkeypatch.setenv("GIT_ALLOW_PROTOCOL", "")
    monkeypatch.setenv("GIT_CEILING_DIRECTORIES", str(tmp_path.resolve()))
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-03T09:00:00-04:00")

    def run(repo, *args):
        result = subprocess.run(
            [git, "-C", str(repo), "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgSign=false", "-c", "user.name=Synthetic Fixture", "-c", "user.email=fixture@example.com", *args],
            capture_output=True,
            check=True,
            text=True,
            timeout=15,
        )
        return result.stdout.strip()

    return run


def _seed_repo(repo, git, remote):
    repo.mkdir(parents=True, exist_ok=True)
    git(repo, "init", "--quiet", "--template=")
    git(repo, "config", "core.hooksPath", os.devnull)
    git(repo, "config", "remote.origin.url", str(repo.parent / remote))
    git(repo, "add", ".")
    git(repo, "commit", "--quiet", "--no-gpg-sign", "-m", "synthetic source")
    return git(repo, *COMMIT)


def test_git_fixture_commit_inputs_repeat(tmp_path, own_git):
    records = []
    for name in ("first", "second"):
        repo = tmp_path / name
        repo.mkdir()
        (repo / "source.md").write_text("synthetic identity input\n", encoding="utf-8")
        _seed_repo(repo, own_git, "unused-origin.git")
        records.append(
            {
                "commit": own_git(repo, "rev-parse", "HEAD"),
                "metadata": own_git(repo, "show", "-s", "--format=%an%n%ae%n%cn%n%ce%n%aI%n%cI", "HEAD").splitlines(),
            }
        )
    (tmp_path / "git-fixture-identities.json").write_text(json.dumps(records, indent=2) + "\n", encoding="utf-8")
    for record in records:
        assert record["metadata"] == [
            "Synthetic Fixture",
            "fixture@example.com",
            "Synthetic Fixture",
            "fixture@example.com",
            "2026-11-03T09:00:00-04:00",
            "2026-11-03T09:00:00-04:00",
        ]
    assert records[0]["commit"] == records[1]["commit"]


def _workflow_inputs(tmp_path, monkeypatch, git, *, foreign_git):
    own = tmp_path.resolve() / "self"
    foreign = tmp_path.resolve() / PRIVATE_CHECKOUT
    for folder in (own / "commands", foreign / "commands"):
        folder.mkdir(parents=True)
    (own / "Agentfile.yaml").write_text("commands: [./commands]\nskillDirectories: []\n", encoding="utf-8")
    (own / "commands" / "careerbrew.md").write_text("---\ndescription: Start the synthetic plan.\n---\n", encoding="utf-8")
    aliases = Path.home() / ".agents" / "commands"
    aliases.mkdir(parents=True)
    for name in ("alpha", "beta"):
        command = foreign / "commands" / f"{name}.md"
        command.write_text("---\ndescription: Synthetic settings step.\n---\n", encoding="utf-8")
        (aliases / command.name).symlink_to(command)
    self_commit = _seed_repo(own, git, "self-origin.git")
    foreign_commit = _seed_repo(foreign, git, REMOTE_CANARY + ".git") if foreign_git else ""
    monkeypatch.setattr(paths, "REPO_ROOT", own)
    config.set_value("work_steps", "alpha, beta")
    config.set_value("work_checkouts", "careerbrew")
    status.save_start({"preflight": [{"check": "synthetic", "ok": True, "value": "saved fixture", "safe": "no host check"}]})
    return own, foreign, self_commit, foreign_commit


def _read_plan(capsys, output=None):
    assert not cli.may_show_business()
    assert cli.main(["day", "run", "--json"]) == 0
    captured = capsys.readouterr()
    assert captured.err == ""
    assert PRIVATE_CHECKOUT not in captured.out
    assert REMOTE_CANARY not in captured.out
    if output is not None:
        output.append(captured.out)
    return json.loads(captured.out)


def test_day_plan_git_lookup_primary(tmp_path, monkeypatch, capsys, own_git):
    own, foreign, self_commit, foreign_commit = _workflow_inputs(tmp_path, monkeypatch, own_git, foreign_git=True)
    folders = {own: "self-root", own / "commands": "self-commands", foreign: "other-root", foreign / "commands": "other-commands"}
    names = {TOP: "toplevel", COMMIT: "commit", REMOTE: "remote"}
    actual = []
    real_git, real_popen = start._git, subprocess.Popen

    def git_only(argv, *args, **kwargs):
        assert len(argv) >= 4 and argv[0:2] == ["git", "-C"]
        assert Path(argv[2]) in folders and tuple(argv[3:]) in names
        return real_popen(argv, *args, **kwargs)

    def no_external(*_args, **_kwargs):
        pytest.fail("forbidden browser, model or network attempt")

    def observe(where, *argv, **kwargs):
        frame = sys._getframe(1)
        phase = "build"
        while frame is not None:
            if frame.f_code in (start.view.__code__, start.labels.__code__):
                phase = frame.f_code.co_name
                break
            frame = frame.f_back
        assert Path(where) in folders and argv in names
        code, out = real_git(where, *argv, **kwargs)
        actual.append({"phase": phase, "directory": folders[Path(where)], "kind": names[argv], "exit": code})
        assert code == 0 and out
        return code, out

    with monkeypatch.context() as guard:
        guard.setattr(start, "_git", observe)
        guard.setattr(subprocess, "Popen", git_only)
        guard.setattr(socket, "create_connection", no_external)
        guard.setattr(socket.socket, "connect", no_external)
        guard.setattr(socket.socket, "connect_ex", no_external)
        guard.setattr(socket.socket, "sendto", no_external)
        guard.setattr(socket, "getaddrinfo", no_external)
        guard.setattr(webbrowser, "open", no_external)
        guard.setattr(models, "_http_transport", no_external)
        output = []
        result = _read_plan(capsys, output)
    evidence = {"plan": result, "git": actual, "stdout": output[0]}
    (tmp_path / "day-plan-git-observations.json").write_text(json.dumps(evidence, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    assert [(step["step"], step["source"]["commit"]) for step in result["steps"]] == [
        ("/careerbrew", self_commit),
        ("/alpha", foreign_commit),
        ("/beta", foreign_commit),
    ]
    assert len(result["start_of_day"]["checkouts"]) == 2
    top = [record for record in actual if record["kind"] == "toplevel"]
    other = [record for record in actual if record["kind"] != "toplevel"]
    assert len(other) == 11
    assert {record["directory"] for record in top} == set(folders.values())
    assert (len(top), len(actual)) == (4, 15)


def test_git_authority_control_c01_linked_worktree_and_symlink(tmp_path, own_git):
    repo = tmp_path / "main-source"
    repo.mkdir()
    (repo / "source.md").write_text("synthetic\n", encoding="utf-8")
    _seed_repo(repo, own_git, "unused-origin.git")
    linked = tmp_path / "linked"
    own_git(repo, "worktree", "add", "--quiet", "--detach", str(linked))
    nested = linked / "nested"
    nested.mkdir()
    command = nested / "command.md"
    command.write_text("synthetic command\n", encoding="utf-8")
    alias = tmp_path / "alias.md"
    alias.symlink_to(command)
    assert (linked / ".git").is_file()
    assert start.toplevel(nested) == start.toplevel(alias) == str(linked.resolve())


@pytest.mark.usefixtures("own_git")
def test_git_authority_control_c02_malformed_marker_is_not_a_checkout(tmp_path):
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / ".git").write_text("gitdir: nonexistent\n", encoding="utf-8")
    assert start.toplevel(outside) is None


def test_git_authority_control_c03_next_invocation_discovers_new_repo(tmp_path, monkeypatch, capsys, own_git):
    _, foreign, _, _ = _workflow_inputs(tmp_path, monkeypatch, own_git, foreign_git=False)
    before = _read_plan(capsys)
    assert [step["source"]["commit"] for step in before["steps"][1:]] == ["", ""]
    assert len(before["start_of_day"]["checkouts"]) == 1
    now = _seed_repo(foreign, own_git, REMOTE_CANARY + ".git")
    after = _read_plan(capsys)
    assert [step["source"]["commit"] for step in after["steps"][1:]] == [now, now]
    assert len(after["start_of_day"]["checkouts"]) == 2


def test_git_authority_control_c04_git_ceiling_still_blocks_parent(tmp_path, monkeypatch, own_git):
    repo = tmp_path / "main-source"
    repo.mkdir()
    (repo / "source.md").write_text("synthetic\n", encoding="utf-8")
    _seed_repo(repo, own_git, "unused-origin.git")
    ceiling = repo / "ceiling"
    nested = ceiling / "nested"
    nested.mkdir(parents=True)
    assert start.toplevel(nested) == str(repo.resolve())
    monkeypatch.setenv("GIT_CEILING_DIRECTORIES", str(ceiling.resolve()))
    assert start.toplevel(nested) is None
