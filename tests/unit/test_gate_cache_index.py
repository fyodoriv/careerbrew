"""The Rust gate depends on Git index and revision, not only working files."""

import subprocess
from pathlib import Path

import pytest

from tests.unit.test_gate_cache import GIT, _stub
from tests.unit.test_gate_cache import repo as gate_repo  # noqa: F401


@pytest.fixture
def rust_repo(gate_repo):  # noqa: F811
    repo = gate_repo
    (repo.root / "Cargo.toml").write_text('[package]\nname = "synthetic"\nversion = "0.1.0"\n')
    cargo = Path(repo.env["PATH"].split(":", 1)[0]) / "cargo"
    _stub(cargo, 'if [ "$1" = test ]; then git show :notes.txt | cmp -s - notes.txt || exit 1; fi')
    repo.git("add", "Cargo.toml")
    repo.git("commit", "-qm", "synthetic Rust fixture")
    return repo


def test_unstaging_same_working_bytes_reruns_rust_and_catches_failure(rust_repo):
    (rust_repo.root / "notes.txt").write_text("two\n")
    rust_repo.git("add", "notes.txt")
    assert not rust_repo.gate("fast")["rust-test"]
    assert not rust_repo.gate("fast")["rust-test"]
    rust_repo.git("restore", "--staged", "notes.txt")
    result = rust_repo.run("fast")
    assert result.returncode != 0
    assert "step 'rust-test' failed" in result.stderr
    assert rust_repo.run("fast").returncode != 0


def test_new_revision_invalidates_rust_but_not_unrelated_steps(rust_repo):
    rust_repo.gate("fast")
    rust_repo.git("commit", "--allow-empty", "-qm", "synthetic new revision")
    result = rust_repo.gate("fast")
    assert not result["rust-test"]
    assert result["docs"]
    assert result["leaks"] is False


def test_unmerged_index_cannot_reuse_a_rust_pass(rust_repo):
    rust_repo.gate("fast")
    rust_repo.git("rm", "--cached", "notes.txt")
    blob = subprocess.check_output([*GIT, "hash-object", "-w", "--stdin"], cwd=rust_repo.root, env=rust_repo.env, input=b"synthetic\n").decode().strip()
    rows = f"100644 {blob} 1\tnotes.txt\n100644 {blob} 2\tnotes.txt\n100644 {blob} 3\tnotes.txt\n"
    subprocess.run([*GIT, "update-index", "--index-info"], cwd=rust_repo.root, env=rust_repo.env, input=rows, text=True, check=True, capture_output=True)
    for _ in range(2):
        result = rust_repo.run("fast")
        assert result.returncode != 0
        assert "rust-test cached" not in result.stdout


@pytest.mark.parametrize("change", ["index", "head", "worktree"])
def test_input_change_during_rust_run_never_reuses_a_pass(rust_repo, change):
    (rust_repo.root / "notes.txt").write_text("two\n")
    cargo = Path(rust_repo.env["PATH"].split(":", 1)[0]) / "cargo"
    mutation = {
        "index": "git add notes.txt",
        "head": "git -c core.hooksPath=/dev/null -c user.name=Test -c user.email=test@example.com commit --allow-empty -qm synthetic",
        "worktree": "printf three > notes.txt",
    }[change]
    _stub(cargo, f'if [ "$1" = test ] && [ ! -f .metrics/changed ]; then {mutation}; touch .metrics/changed; fi')
    assert not rust_repo.gate("fast")["rust-test"]
    assert not rust_repo.gate("fast")["rust-test"]
    assert not rust_repo.gate("fast")["rust-test"]


@pytest.mark.parametrize("failure", ["write-tree", "head"])
def test_unavailable_git_identity_never_hits_rust_cache(rust_repo, failure):
    import shlex
    import shutil

    rust_repo.gate("fast")
    real_git = shutil.which("git", path="/usr/bin:/bin:/opt/homebrew/bin")
    assert real_git
    condition = '[ "$1" = write-tree ]' if failure == "write-tree" else '[ "$1" = rev-parse ] && [ "${2:-}" = --verify ]'
    fake_git = Path(rust_repo.env["PATH"].split(":", 1)[0]) / "git"
    _stub(fake_git, f'if {condition}; then exit 1; fi\nexec {shlex.quote(real_git)} "$@"')
    for _ in range(2):
        result = rust_repo.gate("fast")
        assert result["rust-test"] is False
        assert result["leaks"] is False
