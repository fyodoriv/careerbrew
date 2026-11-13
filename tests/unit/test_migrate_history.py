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
    """An "old" clone with three branches, a tag and two stashes, whose origin now holds a one-commit public history."""
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
    git(clone, "branch", "topic/with-slash")
    git(clone, "tag", "v-old")
    git(clone, "checkout", "-q", "main")
    tree(clone, "stashed edit 1")
    git(clone, "stash", "-q")
    tree(clone, "stashed edit 2")
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
    older_stash = git(clone, "rev-parse", "stash@{1}")

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
    assert git(clone, "rev-parse", "refs/pre-public/topic/with-slash") == feature
    # No old tag is left where `git push --tags` would send it.
    assert git(clone, "for-each-ref", "refs/tags/") == ""
    assert git(clone, "rev-parse", "refs/pre-public-tags/v-old") == feature
    # The stashes are still usable, and the bundle holds the branches and every stash.
    assert git(clone, "rev-parse", "refs/stash") == stash
    (bundle,) = saved.glob("pre-public-*.bundle")
    heads = git(clone, "bundle", "list-heads", str(bundle))
    assert old_main in heads and feature in heads and stash in heads and older_stash in heads


def old_heads(clone: Path, root: str) -> list[str]:
    """Branches and tags a plain push could send that hold any commit outside the public history."""
    out = []
    for ref in git(clone, "for-each-ref", "--format=%(refname)", "refs/heads/", "refs/tags/").split():
        if git(clone, "rev-list", "--max-parents=0", ref) != root:
            out.append(ref)
    return out


def test_a_failing_checkout_hook_leaves_no_old_branch_behind(world):
    clone, root, saved = world
    git(clone, "checkout", "-q", "feature")
    hook = clone / ".git" / "hooks" / "post-checkout"
    hook.write_text("#!/bin/sh\nexit 1\n")
    hook.chmod(0o755)

    r = run(clone, root, saved)

    assert r.returncode == 0, r.stdout + r.stderr
    assert git(clone, "rev-parse", "HEAD") == root
    assert old_heads(clone, root) == []


def test_a_rerun_sweeps_old_branches_a_manual_reset_left(world):
    clone, root, saved = world
    old_main = git(clone, "rev-parse", "main")
    git(clone, "stash", "clear")
    git(clone, "fetch", "-q", "origin", "main")
    git(clone, "reset", "-q", "--hard", "origin/main")  # what the docs forbid, done before the script ran
    git(clone, "branch", "kept-old", old_main)

    r = run(clone, root, saved)

    assert r.returncode == 0, r.stdout + r.stderr
    assert old_heads(clone, root) == []
    assert git(clone, "rev-parse", "refs/pre-public/kept-old") == old_main


def test_a_branch_that_merged_both_histories_counts_as_old(world):
    clone, root, saved = world
    git(clone, "fetch", "-q", "origin", "main")
    git(clone, "checkout", "-q", "-b", "mixed")
    git(clone, "merge", "-q", "--allow-unrelated-histories", "-X", "ours", "-m", "mix", "origin/main")
    mixed = git(clone, "rev-parse", "mixed")

    r = run(clone, root, saved)

    assert r.returncode == 0, r.stdout + r.stderr
    assert old_heads(clone, root) == []
    assert git(clone, "rev-parse", "refs/pre-public/mixed") == mixed


def test_an_ignored_file_in_the_way_stops_the_move_and_keeps_the_file(world):
    clone, root, saved = world
    public = clone.parent / "public"
    (public / "local.cfg").write_text("public default\n")
    git(public, "add", "local.cfg")
    git(public, "commit", "-q", "--amend", "-m", "Initial public release")
    git(public, "push", "-q", "--force", str(clone.parent / "new.git"), "main")
    root = git(public, "rev-parse", "HEAD")
    (clone / ".git" / "info" / "exclude").write_text("local.cfg\n")
    (clone / "local.cfg").write_text("the owner's own settings\n")

    r = run(clone, root, saved)

    assert r.returncode == 1
    assert "in the way" in r.stdout and "local.cfg" in r.stdout
    assert (clone / "local.cfg").read_text() == "the owner's own settings\n"
    assert git(clone, "rev-parse", "HEAD") != root


def republish(clone: Path, change) -> str:
    """Change the one public commit, publish it again, and return the new first commit."""
    public = clone.parent / "public"
    change(public)
    git(public, "add", "-A")
    git(public, "commit", "-q", "--amend", "-m", "Initial public release")
    git(public, "push", "-q", "--force", str(clone.parent / "new.git"), "main")
    return git(public, "rev-parse", "HEAD")


def test_an_ignored_file_where_the_public_tree_has_a_folder_stops_the_move(world):
    clone, root, saved = world

    def change(public: Path) -> None:
        (public / "cfg").mkdir()
        (public / "cfg" / "a.txt").write_text("public\n")

    root = republish(clone, change)
    (clone / ".git" / "info" / "exclude").write_text("cfg\n")
    (clone / "cfg").write_text("the owner's own file\n")

    r = run(clone, root, saved)

    assert r.returncode == 1
    assert "in the way" in r.stdout
    assert (clone / "cfg").read_text() == "the owner's own file\n"


def test_an_ignored_file_with_a_quote_in_its_name_stops_the_move(world):
    clone, root, saved = world
    name = 'q"uote\ttab.txt'
    root = republish(clone, lambda public: (public / name).write_text("public\n"))
    (clone / ".git" / "info" / "exclude").write_text("q*\n")
    (clone / name).write_text("the owner's own file\n")

    r = run(clone, root, saved)

    assert r.returncode == 1
    assert "in the way" in r.stdout
    assert (clone / name).read_text() == "the owner's own file\n"


def test_an_ignored_file_in_the_way_of_a_local_public_main_stops_the_move(world):
    clone, root, saved = world
    git(clone, "fetch", "-q", "origin", "main")
    git(clone, "checkout", "-q", "-b", "old-main")
    git(clone, "update-ref", "refs/heads/main", "origin/main")
    git(clone, "stash", "clear")
    git(clone, "checkout", "-q", "main")
    (clone / "local.cfg").write_text("committed on the local main\n")
    git(clone, "add", "local.cfg")
    git(clone, "commit", "-q", "-m", "local work on the public history")
    git(clone, "checkout", "-q", "old-main")
    (clone / ".git" / "info" / "exclude").write_text("local.cfg\n")
    (clone / "local.cfg").write_text("the owner's own settings\n")

    r = run(clone, root, saved)

    assert r.returncode == 1
    assert "in the way" in r.stdout
    assert (clone / "local.cfg").read_text() == "the owner's own settings\n"


def test_a_rerun_after_a_stop_does_not_copy_the_stashes_twice(world):
    clone, root, saved = world
    lock = clone / ".git" / "index.lock"
    lock.write_text("")
    assert run(clone, root, saved).returncode == 1
    lock.unlink()

    r = run(clone, root, saved)

    assert r.returncode == 0, r.stdout + r.stderr
    assert len(git(clone, "for-each-ref", "--format=%(refname)", "refs/pre-public-stash/").split()) == 2


def test_a_public_main_with_an_unpushed_commit_is_kept(world):
    clone, root, saved = world
    git(clone, "fetch", "-q", "origin", "main")
    git(clone, "branch", "-m", "main", "old-main")
    git(clone, "stash", "clear")
    git(clone, "checkout", "-q", "-b", "main", "origin/main")
    tree(clone, "new local work")
    git(clone, "commit", "-q", "-am", "local work on the public history")
    local = git(clone, "rev-parse", "main")
    git(clone, "checkout", "-q", "old-main")

    r = run(clone, root, saved)

    assert r.returncode == 0, r.stdout + r.stderr
    assert git(clone, "symbolic-ref", "--short", "HEAD") == "main"
    assert git(clone, "rev-parse", "main") == local
    assert old_heads(clone, root) == []


def test_a_detached_head_stops_the_move(world):
    clone, root, saved = world
    git(clone, "checkout", "-q", "--detach")

    r = run(clone, root, saved)

    assert r.returncode == 1
    assert "detached" in r.stdout
    assert not saved.exists()


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
    assert "not a careerbrew checkout" in r.stdout
