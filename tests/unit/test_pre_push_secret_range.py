"""The pre-push hook scans every pushed commit for secrets, not only the last 20.

A secret added and removed in an old commit of a long push must stop the push
(task prepush-gitleaks-range). Scratch repositories only; the key is synthetic.
"""

import random
import shutil
import string
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/verify-push-context.py"
GIT = ["git", "-c", "core.hooksPath=/dev/null", "-c", "user.name=Synthetic", "-c", "user.email=synthetic@example.com", "-c", "commit.gpgsign=false"]
ZERO = "0" * 40
# Built at runtime, so no key-shaped literal is in this file.
KEY = "ghp_" + "".join(random.Random(7).choices(string.ascii_letters + string.digits, k=36))

pytestmark = pytest.mark.skipif(shutil.which("gitleaks") is None, reason="gitleaks is not installed")


def git(repo, *args):
    return subprocess.run([*GIT, "-C", str(repo), *args], check=True, capture_output=True, text=True).stdout.strip()


def commit(repo, name, text):
    (repo / name).write_text(text)
    git(repo, "add", name)
    git(repo, "commit", "-q", "-m", f"change {name}")
    return git(repo, "rev-parse", "HEAD")


@pytest.fixture
def repo(tmp_path):
    """A repository with a pushed base commit on a bare `origin`."""
    origin = tmp_path / "origin.git"
    subprocess.run([*GIT, "init", "-q", "--bare", "-b", "main", str(origin)], check=True)
    path = tmp_path / "repo"
    path.mkdir()
    git(path, "init", "-q", "-b", "main")
    (path / "scripts").mkdir()
    (path / "scripts/leak-check.sh").write_text("#!/bin/sh\nexit 0\n")
    (path / "scripts/leak-check.sh").chmod(0o755)
    git(path, "add", "scripts/leak-check.sh")
    base = commit(path, "base.txt", "base\n")
    git(path, "remote", "add", "origin", str(origin))
    git(path, "push", "-q", "origin", "main")
    return path, base


def push(path, root, row):
    payload = f"origin\0https://example.invalid/repo\0{row}\n".encode()
    gitleaks = str(Path(shutil.which("gitleaks")).parent)
    env = {"PATH": f"{gitleaks}:/usr/bin:/bin:/opt/homebrew/bin", "CAREERBREW_PUBLIC_ROOT": root, "HOME": str(path.parent)}
    return subprocess.run([sys.executable, str(SCRIPT)], input=payload, cwd=path, env=env, capture_output=True, timeout=120)


def long_push(path, with_key):
    if with_key:
        commit(path, "notes.txt", f"token = {KEY}\n")
        commit(path, "notes.txt", "token removed\n")
    for index in range(25):
        tip = commit(path, f"file-{index}.txt", f"{index}\n")
    return tip


def test_a_secret_in_a_commit_older_than_the_last_20_stops_the_push_of_an_existing_branch(repo):
    path, base = repo
    tip = long_push(path, with_key=True)
    result = push(path, base, f"refs/heads/main {tip} refs/heads/main {base}")
    assert result.returncode != 0
    assert KEY.encode() not in result.stdout + result.stderr  # the finding is redacted


def test_a_secret_in_an_old_commit_stops_the_push_of_a_new_branch(repo):
    path, base = repo
    tip = long_push(path, with_key=True)
    result = push(path, base, f"refs/heads/topic {tip} refs/heads/topic {ZERO}")
    assert result.returncode != 0


def test_a_long_clean_push_passes(repo):
    path, base = repo
    tip = long_push(path, with_key=False)
    result = push(path, base, f"refs/heads/main {tip} refs/heads/main {base}")
    assert result.returncode == 0, result.stderr


def test_a_missing_gitleaks_stops_the_push(repo):
    path, base = repo
    tip = long_push(path, with_key=False)
    payload = f"origin\0https://example.invalid/repo\0refs/heads/main {tip} refs/heads/main {base}\n".encode()
    env = {"PATH": "/usr/bin:/bin", "CAREERBREW_PUBLIC_ROOT": base, "HOME": str(path.parent)}
    if shutil.which("gitleaks", path=env["PATH"]):
        pytest.skip("gitleaks is on the system path")
    result = subprocess.run([sys.executable, str(SCRIPT)], input=payload, cwd=path, env=env, capture_output=True, timeout=120)
    assert result.returncode != 0
    assert b"gitleaks" in result.stderr
