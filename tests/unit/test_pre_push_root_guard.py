"""The pre-push hook refuses a commit that does not descend from the public first commit.

A checkout made before the public release holds the old private history. The hook
travels with every clone, so a push of that history stops on every machine
(task pre-push-root-guard). Scratch repositories only.
"""

import importlib.util
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/verify-push-context.py"
GIT = ["git", "-c", "core.hooksPath=/dev/null", "-c", "user.name=Synthetic", "-c", "user.email=synthetic@example.com", "-c", "commit.gpgsign=false"]
ZERO = "0" * 40
# The hook also runs gitleaks on the pushed commits (test_pre_push_secret_range.py).
PATH = os.pathsep.join(filter(None, [os.path.dirname(shutil.which("gitleaks") or ""), "/usr/bin:/bin:/opt/homebrew/bin"]))


def git(repo, *args):
    return subprocess.run([*GIT, "-C", str(repo), *args], check=True, capture_output=True, text=True).stdout.strip()


def commit(repo, name):
    (repo / name).write_text(name)
    git(repo, "add", name)
    git(repo, "commit", "-q", "-m", name)
    return git(repo, "rev-parse", "HEAD")


@pytest.fixture
def repo(tmp_path):
    """One repository with two unrelated histories: `main` (public) and `old` (private)."""
    path = tmp_path / "repo"
    path.mkdir()
    git(path, "init", "-q", "-b", "main")
    (path / "scripts").mkdir()
    (path / "scripts/leak-check.sh").write_text("#!/bin/sh\necho scan >> .scan-calls\n")
    (path / "scripts/leak-check.sh").chmod(0o755)
    public_root = commit(path, "public-first")
    public_tip = commit(path, "public-second")
    git(path, "checkout", "-q", "--orphan", "old")
    git(path, "rm", "-rq", "--cached", ".")
    old_tip = commit(path, "old-first")
    return path, public_root, public_tip, old_tip


def push(path, public_root, new, remote_ref="refs/heads/topic", local="refs/heads/topic"):
    payload = f"origin\0https://example.invalid/repo\0{local} {new} {remote_ref} {ZERO}\n".encode()
    env = {"PATH": PATH, "CAREERBREW_PUBLIC_ROOT": public_root}
    return subprocess.run([sys.executable, str(SCRIPT)], input=payload, cwd=path, env=env, capture_output=True, timeout=30)


def test_a_push_of_a_new_root_branch_passes_and_reaches_the_scan(repo):
    path, public_root, public_tip, _ = repo
    result = push(path, public_root, public_tip)
    assert result.returncode == 0, result.stderr
    assert (path / ".scan-calls").exists()


def test_a_push_of_an_old_root_branch_stops_before_the_scan_and_names_the_guide(repo):
    path, public_root, _, old_tip = repo
    result = push(path, public_root, old_tip)
    assert result.returncode != 0
    assert b"docs/migrate-to-public-history.md" in result.stderr
    assert not (path / ".scan-calls").exists()
    assert old_tip.encode() not in result.stderr + result.stdout  # no object ID in the message


def test_one_old_row_among_new_rows_stops_the_whole_push(repo):
    path, public_root, public_tip, old_tip = repo
    payload = (f"origin\0https://example.invalid/repo\0refs/heads/a {public_tip} refs/heads/a {ZERO}\nrefs/heads/b {old_tip} refs/heads/b {ZERO}\n").encode()
    env = {"PATH": PATH, "CAREERBREW_PUBLIC_ROOT": public_root}
    result = subprocess.run([sys.executable, str(SCRIPT)], input=payload, cwd=path, env=env, capture_output=True, timeout=30)
    assert result.returncode != 0


def test_a_checkout_without_the_public_first_commit_cannot_push_a_commit(repo):
    path, _, public_tip, _ = repo
    result = push(path, "3" * 40, public_tip)
    assert result.returncode != 0
    assert b"docs/migrate-to-public-history.md" in result.stderr


def test_a_deletion_passes_the_guard(repo):
    path, public_root, _, old_tip = repo
    payload = f"origin\0https://example.invalid/repo\0(delete) {ZERO} refs/heads/gone {old_tip}\n".encode()
    env = {"PATH": PATH, "CAREERBREW_PUBLIC_ROOT": public_root}
    result = subprocess.run([sys.executable, str(SCRIPT)], input=payload, cwd=path, env=env, capture_output=True, timeout=30)
    assert result.returncode == 0, result.stderr


def test_the_default_root_is_the_first_commit_of_this_repository():
    spec = importlib.util.spec_from_file_location("verify_push_context", SCRIPT)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert module.PUBLIC_ROOT == "22e3e591b70dd8a5f8a1111355201b85c8f68ed5"
    roots = subprocess.run(["git", "-C", str(ROOT), "rev-list", "--max-parents=0", "HEAD"], capture_output=True, text=True, check=False)
    if roots.returncode == 0 and (ROOT / ".git").exists() and len(roots.stdout.split()) > 1:
        pytest.fail("this checkout holds more than one first commit")
    if roots.returncode == 0 and module.PUBLIC_ROOT not in roots.stdout.split():
        # Only a release-tree copy may differ: it holds one new commit and no remote.
        assert subprocess.run(["git", "-C", str(ROOT), "remote"], capture_output=True, text=True, check=False).stdout.strip() == ""


def test_a_merge_of_the_old_history_into_the_public_history_stops(repo):
    # `git pull --allow-unrelated-histories` in an old checkout makes such a merge: it descends from both first commits.
    path, public_root, public_tip, old_tip = repo
    git(path, "checkout", "-q", "-f", "main")  # the fixture left the public files untracked
    git(path, "merge", "-q", "--allow-unrelated-histories", "-m", "mixed", old_tip)
    mixed = git(path, "rev-parse", "HEAD")
    assert git(path, "merge-base", "--is-ancestor", public_root, mixed) == ""
    result = push(path, public_root, mixed)
    assert result.returncode != 0
    assert b"docs/migrate-to-public-history.md" in result.stderr
    assert not (path / ".scan-calls").exists()


def test_a_tag_that_points_at_a_tree_stops(repo):
    # Git does push a tag of a tree or a file, so the guard cannot skip an object that is not a commit.
    path, public_root, _, old_tip = repo
    tree = git(path, "rev-parse", f"{old_tip}^{{tree}}")
    result = push(path, public_root, tree, remote_ref="refs/tags/content", local="refs/tags/content")
    assert result.returncode != 0
    assert not (path / ".scan-calls").exists()
