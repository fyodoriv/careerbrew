"""scripts/migrate-history.sh moves a checkout of the old private history to the public history and loses no work.

Every repository here is a scratch one with synthetic commits.
"""

import os
import subprocess
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "migrate-history.sh"
ENV = {
    "GIT_AUTHOR_NAME": "T",
    "GIT_AUTHOR_EMAIL": "t@example.com",
    "GIT_COMMITTER_NAME": "T",
    "GIT_COMMITTER_EMAIL": "t@example.com",
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_SYSTEM": os.devnull,
    "PATH": os.environ["PATH"],
}


def git(where: Path, *args: str) -> str:
    return subprocess.run(["git", *args], cwd=where, env={**ENV, "HOME": str(where)}, capture_output=True, text=True, check=True).stdout.strip()


def tree(where: Path, note: str) -> None:
    (where / "src" / "careerbrew").mkdir(parents=True, exist_ok=True)
    (where / "Agentfile.yaml").write_text("name: careerbrew\n")
    (where / "src" / "careerbrew" / "note.txt").write_text(note)
    (where / ".gitignore").write_text("private/\n")


@pytest.fixture
def world(tmp_path):
    """An "old" clone with two branches and a stash, whose origin now holds a one-commit public history."""
    old_remote = tmp_path / "old.git"
    git(tmp_path, "init", "-q", "--bare", "-b", "main", str(old_remote))
    clone = tmp_path / "clone"
    git(tmp_path, "clone", "-q", str(old_remote), str(clone))
    tree(clone, "old 1")
    (clone / "journal.md").write_text("old journal\n")
    git(clone, "add", "-A")
    git(clone, "commit", "-q", "-m", "old 1")
    tree(clone, "old 2")
    git(clone, "commit", "-q", "-am", "old 2")
    git(clone, "push", "-q", "origin", "main")
    git(clone, "checkout", "-q", "-b", "feature")
    tree(clone, "feature work")
    git(clone, "commit", "-q", "-am", "feature work")
    git(clone, "checkout", "-q", "main")
    tree(clone, "stashed edit")
    git(clone, "stash", "-q")
    (clone / "private").mkdir()
    (clone / "private" / "data.txt").write_text("owner data\n")

    public = tmp_path / "public"
    git(tmp_path, "init", "-q", "-b", "main", str(public))
    tree(public, "public 1")
    git(public, "add", "-A")
    git(public, "commit", "-q", "-m", "Initial public release")
    new_remote = tmp_path / "new.git"
    git(tmp_path, "clone", "-q", "--bare", str(public), str(new_remote))
    git(clone, "remote", "set-url", "origin", str(new_remote))
    root = git(public, "rev-parse", "HEAD")
    return clone, root, tmp_path / "saved"


def run(clone: Path, root: str, saved: Path) -> subprocess.CompletedProcess[str]:
    env = {**ENV, "HOME": str(clone.parent), "CAREERBREW_PUBLIC_ROOT": root, "CAREERBREW_MIGRATE_SAVE_DIR": str(saved)}
    return subprocess.run(["bash", str(SCRIPT)], cwd=clone, env=env, capture_output=True, text=True, check=False)


def test_an_old_checkout_moves_to_the_public_history_and_keeps_every_piece_of_work(world):
    clone, root, saved = world
    old_main, feature, stash = git(clone, "rev-parse", "main"), git(clone, "rev-parse", "feature"), git(clone, "rev-parse", "refs/stash")

    r = run(clone, root, saved)

    assert r.returncode == 0, r.stdout + r.stderr
    assert git(clone, "rev-parse", "HEAD") == root
    assert git(clone, "symbolic-ref", "--short", "HEAD") == "main"
    assert git(clone, "rev-parse", "--abbrev-ref", "main@{u}") == "origin/main"
    assert (clone / "src" / "careerbrew" / "note.txt").read_text() == "public 1"
    assert not (clone / "journal.md").exists()  # a file only the old history tracked
    assert (clone / "private" / "data.txt").read_text() == "owner data\n"  # ignored data stays
    # No old branch is left where a plain push would send it; each one is kept aside.
    assert git(clone, "for-each-ref", "--format=%(refname)", "refs/heads/") == "refs/heads/main"
    assert git(clone, "rev-parse", "refs/pre-public/main") == old_main
    assert git(clone, "rev-parse", "refs/pre-public/feature") == feature
    # The bundle holds both branches and the stash.
    (bundle,) = saved.glob("pre-public-*.bundle")
    heads = git(clone, "bundle", "list-heads", str(bundle))
    assert old_main in heads and feature in heads and stash in heads


def test_a_second_run_changes_nothing(world):
    clone, root, saved = world
    assert run(clone, root, saved).returncode == 0
    before = git(clone, "for-each-ref")

    r = run(clone, root, saved)

    assert r.returncode == 0
    assert "already on the public history" in r.stdout
    assert git(clone, "for-each-ref") == before
    assert len(list(saved.glob("*.bundle"))) == 1


def test_uncommitted_tracked_changes_stop_the_move_and_nothing_changes(world):
    clone, root, saved = world
    (clone / "src" / "careerbrew" / "note.txt").write_text("unsaved edit")
    before = git(clone, "for-each-ref", "refs/heads", "refs/stash", "refs/pre-public")  # the fetch may move refs/remotes

    r = run(clone, root, saved)

    assert r.returncode == 1
    assert "uncommitted changes" in r.stdout
    assert git(clone, "for-each-ref", "refs/heads", "refs/stash", "refs/pre-public") == before
    assert (clone / "src" / "careerbrew" / "note.txt").read_text() == "unsaved edit"
    assert not saved.exists()


def test_a_linked_worktree_stops_the_move(world):
    clone, root, saved = world
    git(clone, "worktree", "add", "-q", str(clone.parent / "side"), "feature")

    r = run(clone, root, saved)

    assert r.returncode == 1
    assert "linked worktree" in r.stdout
    assert git(clone, "rev-parse", "HEAD") != root


def test_an_origin_that_is_not_the_public_repository_is_refused(world):
    clone, root, saved = world

    r = run(clone, "0" * 40, saved)

    assert r.returncode == 1
    assert "does not start at the public first commit" in r.stdout
    assert git(clone, "rev-parse", "HEAD") != root


def test_a_folder_that_is_not_careerbrew_is_refused(tmp_path):
    git(tmp_path, "init", "-q", "-b", "main", str(tmp_path / "other"))

    r = run(tmp_path / "other", "0" * 40, tmp_path / "saved")

    assert r.returncode == 2
