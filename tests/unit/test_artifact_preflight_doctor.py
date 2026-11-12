"""Optional source-parser tooling readiness never imports parser code."""

import importlib.util
import json
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


def checker():
    spec = importlib.util.spec_from_file_location("artifact_preflight_doctor", ROOT / "scripts/check-artifact-preflight.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def fixture_tools(tmp_path):
    source = ROOT / "evals/fixtures/artifact-proof/source-preflight-v3-tools"
    tools = tmp_path / "evals/fixtures/artifact-proof/source-preflight-v3-tools"
    tools.mkdir(parents=True)
    for name in ("package.json", "package-lock.json"):
        (tools / name).write_bytes((source / name).read_bytes())
    lock = json.loads((tools / "package-lock.json").read_text())
    for name, record in lock["packages"].items():
        if name:
            package = tools / name
            package.mkdir(parents=True)
            (package / "package.json").write_text(json.dumps({"name": name.removeprefix("node_modules/"), "version": record["version"]}))
    return tools


@pytest.mark.parametrize("enabled", [None, "0", "1"])
def test_optional_doctor_block_calls_checker_only_when_enabled(tmp_path, enabled):
    text = (ROOT / "scripts/doctor.sh").read_text()
    block = text.split("# Optional artifact source-preflight tooling.", 1)[1].split("# End optional artifact source-preflight tooling.", 1)[0]
    binary = tmp_path / "bin"
    binary.mkdir()
    python = binary / "python3"
    python.write_text('#!/bin/bash\nprintf "%s\\n" "$*" >> checker-calls\n')
    python.chmod(0o700)
    env = {"PATH": f"{binary}:/usr/bin:/bin"}
    if enabled is not None:
        env["CAREERBREW_CHECK_ARTIFACT_PREFLIGHT"] = enabled
    result = subprocess.run(["/bin/bash", "-c", "fails=0\n" + block + '\nexit "$fails"'], cwd=tmp_path, env=env, capture_output=True, text=True)
    assert result.returncode == 0
    calls = tmp_path / "checker-calls"
    if enabled == "1":
        assert calls.read_text().splitlines() == ["scripts/check-artifact-preflight.py"]
    else:
        assert not calls.exists()
    assert "Node.js 20+" in text


@pytest.mark.parametrize("version", [None, "v20.19.0", "v24.0.0", "v25.0.0", "not-a-version"])
def test_opt_in_rejects_missing_or_unsupported_node(tmp_path, version):
    fixture_tools(tmp_path)
    assert checker().check(tmp_path, version) == "node-version"


@pytest.mark.parametrize("version", ["v24.15.0", "v24.21.0"])
def test_opt_in_accepts_exact_isolated_manifests(tmp_path, version):
    fixture_tools(tmp_path)
    assert checker().check(tmp_path, version) is None


@pytest.mark.parametrize("fault", ["missing", "wrong-version", "symlink-modules", "symlink-package", "lock-mismatch", "unexpected-dependency"])
def test_opt_in_refuses_incomplete_or_shared_tools(tmp_path, fault):
    tools = fixture_tools(tmp_path)
    package = tools / "node_modules/parse5/package.json"
    if fault == "missing":
        package.unlink()
    elif fault == "wrong-version":
        package.write_text('{"version":"7.3.0"}')
    elif fault in ("symlink-modules", "symlink-package"):
        target = tools / "node_modules" if fault == "symlink-modules" else package.parent
        moved = tmp_path / "shared-modules"
        target.rename(moved)
        target.symlink_to(moved, target_is_directory=True)
    elif fault == "lock-mismatch":
        lock = json.loads((tools / "package-lock.json").read_text())
        lock["packages"]["node_modules/parse5"]["version"] = "7.3.0"
        (tools / "package-lock.json").write_text(json.dumps(lock))
    else:
        manifest = json.loads((tools / "package.json").read_text())
        manifest["devDependencies"]["unexpected"] = "1.0.0"
        (tools / "package.json").write_text(json.dumps(manifest))
    assert checker().check(tmp_path, "v24.21.0") is not None
