"""Pre-push Git context is delivered without weakening the strict leak scan."""

import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
GIT = ["git", "-c", "core.hooksPath=/dev/null", "-c", "user.name=Synthetic", "-c", "user.email=synthetic@example.com", "-c", "commit.gpgsign=false"]
URL = b"https://example.invalid/SYNTHETIC_SECRET"
ZERO = b"0" * 40
ONE = b"1" * 40
TWO = b"2" * 40


def row(local=b"refs/heads/main", new=ONE, remote=b"refs/heads/main", old=ZERO):
    return b" ".join((local, new, remote, old)) + b"\n"


def framed(payload, remote=b"origin", url=URL):
    return remote + b"\0" + url + b"\0" + payload


def run_git(repo, *args, config=True):
    cmd = [*(GIT if config else ["git"]), "-C", str(repo), *args]
    return subprocess.run(cmd, check=True, capture_output=True)


@pytest.fixture
def repo(tmp_path):
    path = tmp_path / "repo"
    path.mkdir()
    run_git(path, "init", "-q", "-b", "main")
    (path / "scripts").mkdir()
    (path / "scripts/leak-check.sh").write_text('#!/bin/sh\necho scan >> .scan-calls\nexit "${SYNTH_SCAN_EXIT:-0}"\n')
    (path / "scripts/leak-check.sh").chmod(0o755)
    return path


def invoke(repo, payload, env=None):
    return subprocess.run([sys.executable, str(ROOT / "scripts/verify-push-context.py")], input=payload, cwd=repo, env=env, capture_output=True, timeout=20)


def test_valid_contexts_reach_the_strict_scan(repo):
    cases = [
        b"",
        row(),
        row(local=b"HEAD~1", remote=b"refs/heads/revision"),
        row(old=TWO),
        row(local=b"HEAD", remote=b"refs/heads/other"),
        row(local=ONE, remote=b"refs/heads/raw"),
        row(local=b"(delete)", new=ZERO, old=TWO),
        row() + row(local=b"refs/heads/other", remote=b"refs/heads/other"),
    ]
    for payload in cases:
        result = invoke(repo, framed(payload))
        assert result.returncode == 0
        assert b"SYNTHETIC_SECRET" not in result.stdout + result.stderr
    assert (repo / ".scan-calls").read_text().splitlines() == ["scan"] * len(cases)


def test_malformed_context_fails_before_the_scan(repo):
    invalid = [
        b"",
        framed(row()[:-1]),
        framed(row(new=b"x" * 40)),
        framed(row(remote=b"not-a-ref")),
        framed(row() + row()),
        framed(row(local=b"(delete)", new=ONE, old=TWO)),
        framed(row() + b"\0"),
        framed(b"x" * (1024 * 1024 + 1)),
    ]
    for payload in invalid:
        result = invoke(repo, payload)
        assert result.returncode != 0
        assert b"SYNTHETIC_SECRET" not in result.stdout + result.stderr
    assert not (repo / ".scan-calls").exists()


def test_strict_scan_failure_is_not_suppressed(repo):
    env = os.environ.copy()
    env["SYNTH_SCAN_EXIT"] = "7"
    result = invoke(repo, framed(row()), env)
    assert result.returncode == 7
    assert (repo / ".scan-calls").read_text().splitlines() == ["scan"]


def test_repo_wrapper_forwards_arguments_and_stdin_to_lefthook(repo, tmp_path):
    (repo / "hooks").mkdir()
    shutil.copy(ROOT / "hooks/pre-push", repo / "hooks/pre-push")
    tools = tmp_path / "bin"
    tools.mkdir()
    fake = tools / "lefthook"
    fake.write_text('#!/bin/sh\nprintf "%s\n" "$@" > .hook-args\ncat > .hook-stdin\n')
    fake.chmod(0o755)
    env = os.environ.copy()
    env["PATH"] = str(tools) + os.pathsep + env["PATH"]
    startup = tmp_path / "bash-startup"
    startup.write_text("set -x\n")
    env["BASH_ENV"] = str(startup)
    result = subprocess.run(["bash", "hooks/pre-push", "origin", URL.decode()], cwd=repo, env=env, input=row(), capture_output=True, timeout=20)
    assert result.returncode == 0
    assert (repo / ".hook-args").read_text().splitlines() == ["run", "pre-push", "origin", URL.decode()]
    assert (repo / ".hook-stdin").read_bytes() == row()
    assert b"SYNTHETIC_SECRET" not in result.stdout + result.stderr


def test_repo_wrapper_without_lefthook_keeps_context_scan_and_secret_check_and_runs_no_gate(repo, tmp_path):
    (repo / "hooks/lefthook/pre-push").mkdir(parents=True)
    shutil.copy(ROOT / "hooks/pre-push", repo / "hooks/pre-push")
    shutil.copy(ROOT / "hooks/lefthook/pre-push/privacy.sh", repo / "hooks/lefthook/pre-push/privacy.sh")
    shutil.copy(ROOT / "scripts/verify-push-context.py", repo / "scripts/verify-push-context.py")
    (repo / "scripts/check.sh").write_text("#!/bin/sh\necho gate >> .gate-calls\n")
    (repo / "scripts/check.sh").chmod(0o755)
    tools = tmp_path / "bin"
    tools.mkdir()
    system_git = shutil.which("git", path="/usr/bin:/bin:/opt/homebrew/bin")
    assert system_git
    for name, executable in (("git", system_git), ("python3", sys.executable), ("cat", "/bin/cat"), ("bash", "/bin/bash")):
        (tools / name).symlink_to(executable)
    (tools / "gitleaks").write_text("#!/bin/bash\necho secrets >> .secret-calls\n")
    (tools / "gitleaks").chmod(0o755)
    env = os.environ.copy()
    env["PATH"] = str(tools)
    startup = tmp_path / "bash-startup"
    startup.write_text("set -x\n")
    env["BASH_ENV"] = str(startup)
    result = subprocess.run(["/bin/bash", "hooks/pre-push", "origin", URL.decode()], cwd=repo, env=env, input=row(), capture_output=True, timeout=20)
    assert result.returncode == 0, result.stderr
    assert (repo / ".scan-calls").read_text().splitlines() == ["scan"]
    assert (repo / ".secret-calls").read_text().splitlines() == ["secrets"]
    assert not (repo / ".gate-calls").exists(), "a push runs no test gate: the hosted checks do (docs/ci.md)"
    assert b"SYNTHETIC_SECRET" not in result.stdout + result.stderr


def test_native_lefthook_receives_real_synthetic_push_shapes(repo, tmp_path):
    executable = shutil.which("lefthook")
    if not executable:
        pytest.skip("lefthook is not installed")
    (repo / "hooks/lefthook/pre-push").mkdir(parents=True)
    shutil.copy(ROOT / "hooks/lefthook/pre-push/privacy.sh", repo / "hooks/lefthook/pre-push/privacy.sh")
    shutil.copy(ROOT / "scripts/verify-push-context.py", repo / "scripts/verify-push-context.py")
    shutil.copy(ROOT / "lefthook.yml", repo / "lefthook.yml")
    (repo / "scripts/check.sh").write_text("#!/bin/sh\necho gate >> .gate-calls\n")
    (repo / "scripts/check.sh").chmod(0o755)
    (repo / "file.txt").write_text("synthetic\n")
    run_git(repo, "add", ".")
    run_git(repo, "commit", "-qm", "initial")
    run_git(repo, "config", "user.name", "Synthetic")
    run_git(repo, "config", "user.email", "synthetic@example.com")
    run_git(repo, "config", "commit.gpgsign", "false")
    run_git(repo, "config", "core.hooksPath", ".git/hooks")
    remote = tmp_path / "remote.git"
    subprocess.run(["git", "init", "--bare", "-q", str(remote)], check=True, capture_output=True)
    run_git(repo, "remote", "add", "origin", str(remote))
    subprocess.run([executable, "install", "--force"], cwd=repo, check=True, capture_output=True, timeout=20)
    run_git(repo, "push", "-q", "origin", "main", config=False)
    run_git(repo, "push", "-q", "origin", "main", config=False)
    run_git(repo, "commit", "--allow-empty", "-qm", "empty")
    run_git(repo, "push", "-q", "origin", "main", config=False)
    run_git(repo, "push", "-q", "origin", "HEAD~1:refs/heads/revision", config=False)
    run_git(repo, "branch", "one")
    run_git(repo, "branch", "two")
    run_git(repo, "push", "-q", "origin", "one", "two", config=False)
    run_git(repo, "tag", "-a", "v1", "-m", "synthetic tag")
    run_git(repo, "push", "-q", "origin", "v1", config=False)
    run_git(repo, "push", "-q", "origin", "--delete", "one", config=False)
    shutil.copy(ROOT / "hooks/pre-push", repo / ".git/hooks/pre-push")
    (repo / ".git/hooks/pre-push").chmod(0o755)
    run_git(repo, "commit", "--allow-empty", "-qm", "wrapper")
    run_git(repo, "push", "-q", "origin", "main", config=False)
    assert (repo / ".scan-calls").read_text().splitlines() == ["scan"] * 8
    assert not (repo / ".gate-calls").exists(), "a push runs no test gate: the hosted checks do (docs/ci.md)"
