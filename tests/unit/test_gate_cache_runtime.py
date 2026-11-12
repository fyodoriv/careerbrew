"""Unqualified external runtimes must not produce cached Rust passes."""

import shlex
from pathlib import Path

import pytest

from tests.unit.test_gate_cache import _stub
from tests.unit.test_gate_cache import repo as gate_repo  # noqa: F401


@pytest.fixture
def rust_repo(gate_repo):  # noqa: F811
    repo = gate_repo
    (repo.root / "Cargo.toml").write_text('[package]\nname = "synthetic"\nversion = "0.1.0"\n')
    cargo = Path(repo.env["PATH"].split(":", 1)[0]) / "cargo"
    _stub(cargo)
    repo.git("add", "Cargo.toml")
    repo.git("commit", "-qm", "synthetic Rust fixture")
    return repo, cargo


@pytest.mark.parametrize(("command", "step"), [("fmt", "rust-fmt"), ("clippy", "rust-clippy"), ("test", "rust-test")])
def test_changed_external_cargo_cannot_reuse_a_pass(rust_repo, command, step):
    repo, cargo = rust_repo
    assert repo.run("fast").returncode == 0
    assert repo.run("fast").returncode == 0
    _stub(cargo, f'if [ "$1" = {shlex.quote(command)} ]; then exit 1; fi')
    result = repo.run("fast")
    assert result.returncode != 0
    assert f"step '{step}' failed" in result.stderr


def test_unqualified_runtime_reruns_rust_and_preserves_other_cache_behavior(rust_repo):
    repo, _ = rust_repo
    repo.gate("fast")
    result = repo.gate("fast")
    assert all(result[name] is False for name in ("rust-fmt", "rust-clippy", "rust-test"))
    assert result["docs"]
    assert result["leaks"] is False


def test_changed_external_runtime_file_cannot_reuse_a_pass(rust_repo, tmp_path):
    repo, cargo = rust_repo
    runtime = tmp_path / "synthetic-runtime"
    runtime.write_text("ok\n")
    _stub(cargo, f'if [ "$1" = test ]; then test "$(cat {shlex.quote(str(runtime))})" = ok || exit 1; fi')
    assert repo.run("fast").returncode == 0
    runtime.write_text("bad\n")
    result = repo.run("fast")
    assert result.returncode != 0
    assert "step 'rust-test' failed" in result.stderr


def test_gate_rejects_staged_unassigned_rust_source_after_pass(rust_repo):
    repo, cargo = rust_repo
    _stub(cargo, 'if [ "$1" = xtask ] && git ls-files --error-unmatch unassigned.rs >/dev/null 2>&1; then exit 1; fi')
    assert repo.run("fast").returncode == 0
    (repo.root / "unassigned.rs").write_text("// synthetic source\n")
    repo.git("add", "unassigned.rs")
    result = repo.run("fast")
    assert result.returncode != 0
    assert "step 'rust-inventory' failed" in result.stderr


def test_inventory_cannot_reuse_pass_after_cargo_changes(rust_repo):
    repo, cargo = rust_repo
    assert repo.run("fast").returncode == 0
    _stub(cargo, 'if [ "$1" = xtask ]; then exit 1; fi')
    result = repo.run("fast")
    assert result.returncode != 0
    assert "step 'rust-inventory' failed" in result.stderr
