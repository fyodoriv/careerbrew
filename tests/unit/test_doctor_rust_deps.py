"""Doctor refuses an unavailable or incompatible direct native dependency."""

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest


@pytest.mark.parametrize("case", ["good", "wrong-version", "missing-derive", "malformed", "cargo-error"])
def test_doctor_checks_locked_offline_serde_metadata(tmp_path, case):
    root = Path(__file__).resolve().parents[2]
    scripts = tmp_path / "scripts"
    scripts.mkdir()
    shutil.copy2(root / "scripts/doctor.sh", scripts / "doctor.sh")
    shutil.copy2(root / "scripts/rust-readiness.sh", scripts / "rust-readiness.sh")
    shutil.copy2(root / "rust-toolchain.toml", tmp_path / "rust-toolchain.toml")
    (scripts / "check-deployed-commands.py").write_text("print('ok    synthetic deployed commands')\n")
    for directory in [".metrics", "private", "config", "web/dist", "bin"]:
        (tmp_path / directory).mkdir(parents=True, exist_ok=True)
    for name in ["Cargo.toml", "pyproject.toml", "config/models.yaml", "web/dist/index.html", ".git"]:
        (tmp_path / name).touch()
    bin_dir = tmp_path / "bin"
    (bin_dir / "python3").symlink_to(sys.executable)
    for command in ["git", "node", "gitleaks", "uv"]:
        script = bin_dir / command
        script.write_text("#!/bin/sh\nexit 0\n")
        script.chmod(0o755)
    curl = bin_dir / "curl"
    curl.write_text("#!/bin/sh\nexit 1\n")
    curl.chmod(0o755)
    for command, version in {
        "rustc": "rustc 1.99.1\nrelease: 1.99.1",
        "rustfmt": "rustfmt 1.10.0",
        "clippy-driver": "clippy 0.1.99",
    }.items():
        script = bin_dir / command
        script.write_text(f"#!/bin/sh\nprintf '%s\\n' '{version}'\n")
        script.chmod(0o755)
    dependency = {"name": "serde", "req": "=1.0.229", "features": ["derive"], "kind": None}
    if case == "wrong-version":
        dependency["req"] = "^1.0"
    if case == "missing-derive":
        dependency["features"] = []
    metadata = json.dumps({"packages": [{"name": "careerbrew-xtask", "dependencies": [dependency]}]})
    cargo = bin_dir / "cargo"
    cargo.write_text(
        f"#!{sys.executable}\nimport json, pathlib, sys\nif sys.argv[1:] == ['--version']:\n    print('cargo 1.99.1 (fixture)')\n    sys.exit(0)\npathlib.Path('cargo-args.json').write_text(json.dumps(sys.argv[1:]))\nprint({('not-json' if case == 'malformed' else metadata)!r})\nsys.exit({7 if case == 'cargo-error' else 0})\n"
    )
    cargo.chmod(0o755)
    env = {key: value for key, value in os.environ.items() if key not in {"BASH_ENV", "ENV", "RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTUP_TOOLCHAIN"}}
    env.update(PATH=f"{bin_dir}:/usr/bin:/bin", HOME=str(tmp_path), CAREERBREW_KEY_FILE=str(tmp_path / "absent-key"))
    result = subprocess.run(["bash", "scripts/doctor.sh"], cwd=tmp_path, env=env, capture_output=True, text=True, check=False)
    if case == "good":
        assert "ok    native Serde derive dependency (locked, offline)" in result.stdout, result.stdout
        assert result.returncode == 0, result.stdout
    else:
        assert "FAIL  native Serde derive dependency unavailable or incompatible -> fix:" in result.stdout, result.stdout
        assert "cargo fetch --locked" in result.stdout
        assert "doctor: 1 check(s) failed" in result.stdout, result.stdout
        assert result.returncode == 1
    assert json.loads((tmp_path / "cargo-args.json").read_text()) == ["metadata", "--locked", "--offline", "--no-deps", "--format-version", "1"]
