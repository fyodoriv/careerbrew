"""Gate step `rust-policy-io` (rust-04d): the policy crate's library does no IO.

`scripts/policy-no-io.py` passes the real crate and refuses each planted IO
use in a scratch copy, naming the file, line and rule. The `src/bin/` driver
is the one IO edge and stays allowed.
"""

import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/policy-no-io.py"
CRATE = ROOT / "crates/policy"


def _run(crate: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, "-I", str(SCRIPT), str(crate)], capture_output=True, text=True, check=False, timeout=60)


def _copy(tmp_path: Path) -> Path:
    dest = tmp_path / "policy"
    shutil.copytree(CRATE, dest, ignore=shutil.ignore_patterns("target"))
    return dest


def test_the_real_policy_crate_is_clean():
    result = _run(CRATE)
    assert result.returncode == 0, result.stdout
    assert result.stdout.strip().endswith("policy-no-io: 0 finding(s)")


@pytest.mark.parametrize(
    ("line", "rule"),
    [
        ("use std::fs;", "std::fs"),
        ('fn f() { let _ = std::fs::read("x"); }', "std::fs"),
        ("use std::{collections::HashMap, fs};", "std::{...} import"),
        ('fn f() -> Option<String> { std::env::var("X").ok() }', "std::env"),
        ("use std::process::Command;", "std::process"),
        ("use std::net::TcpStream;", "std::net"),
        ("fn f() { let _ = std::time::SystemTime::now(); }", "std::time"),
        ("use rand::Rng;", "RNG"),
    ],
)
def test_a_planted_io_use_is_refused_and_named(tmp_path, line, rule):
    crate = _copy(tmp_path)
    target = crate / "src/text.rs"
    lines = target.read_text().splitlines()
    target.write_text("\n".join([*lines, line]) + "\n")
    result = _run(crate)
    assert result.returncode == 1
    assert f"policy/src/text.rs:{len(lines) + 1}: {rule}" in result.stdout


def test_a_mention_in_a_comment_or_string_is_not_a_use(tmp_path):
    crate = _copy(tmp_path)
    target = crate / "src/text.rs"
    target.write_text(target.read_text() + '// std::fs is refused here\nconst NOTE: &str = "std::env";\n')
    assert _run(crate).returncode == 0


def test_the_driver_binary_may_read_its_vector_file(tmp_path):
    crate = _copy(tmp_path)
    assert "std::fs" in (crate / "src/bin/policy-vectors.rs").read_text()
    assert _run(crate).returncode == 0


def test_an_impure_dependency_is_refused(tmp_path):
    crate = _copy(tmp_path)
    manifest = crate / "Cargo.toml"
    manifest.write_text(manifest.read_text().replace("[dependencies]\n", '[dependencies]\nrand = "=0.8.5"\n'))
    result = _run(crate)
    assert result.returncode == 1
    assert "dependencies rand is not on the pure list" in result.stdout


def test_the_gate_runs_the_step():
    check = (ROOT / "scripts/check.sh").read_text()
    assert "step rust-policy-io uv run --quiet python -I scripts/policy-no-io.py" in check
