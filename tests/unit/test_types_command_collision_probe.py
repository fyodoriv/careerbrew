"""Measure shared type-cache identity with synthetic commands."""

import os
import shutil
import subprocess

from tests.unit.test_gate_cache import GIT, ROOT, _stub


def fixture(tmp_path, *, python):
    root, bin_dir = tmp_path / "repo", tmp_path / "bin"
    (root / "scripts").mkdir(parents=True)
    bin_dir.mkdir()
    for name in ("check.sh", "input-key.sh"):
        shutil.copy(ROOT / "scripts" / name, root / "scripts" / name)
    for name in ("check-docs.sh", "leak-check.sh", "e2e.sh"):
        _stub(root / "scripts" / name)
    _stub(bin_dir / "gitleaks")
    _stub(bin_dir / "npm", 'if [ "$1" = run ] && [ "$3" = typecheck ]; then echo js >> "$PROBE_LOG"; [ ! -f "$PROBE_BAD" ] || exit 17; fi\nexit 0')
    _stub(bin_dir / "uv", 'if [ "$1" = run ] && [ "$2" = --quiet ] && [ "$3" = pyright ]; then echo version >> "$PROBE_VERSION"; fi\nif [ "$1" = run ] && [ "$2" = pyright ]; then echo python >> "$PROBE_LOG"; exit 19; fi\nexit 0')
    (root / ".gitignore").write_text(".metrics/\n")
    (root / "package.json").write_text("{}\n")
    if python:
        (root / "pyproject.toml").write_text("# synthetic\n")
    env = {
        "PATH": f"{bin_dir}{os.pathsep}{os.defpath}",
        "HOME": str(tmp_path),
        "LC_ALL": "C",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_ALLOW_PROTOCOL": "",
        "UNIT_JOBS": "0",
        "PROBE_LOG": str(tmp_path / "calls"),
        "PROBE_BAD": str(tmp_path / "bad"),
        "PROBE_VERSION": str(tmp_path / "version"),
    }

    def run(argv, **overrides):
        return subprocess.run(argv, cwd=root, env=env | overrides, capture_output=True, text=True, timeout=60, check=False)

    for args in (("init", "-q", "-b", "main"), ("add", "-A"), ("commit", "-qm", "fixture")):
        assert run([*GIT, *args]).returncode == 0
    scope = run(["bash", "scripts/check.sh", "inputs", "types"])
    assert scope.returncode == 0 and scope.stdout.startswith("types\t")
    spec = scope.stdout.split("\t", 1)[1].split()

    def key():
        result = run(["bash", "scripts/input-key.sh", *spec])
        assert result.returncode == 0 and result.stdout.strip()
        return result.stdout

    return root, tmp_path, run, key


def test_changed_javascript_checker_cannot_reuse_stale_success(tmp_path):
    root, outside, run, key = fixture(tmp_path, python=False)
    before = key()
    first = run(["bash", "scripts/check.sh", "fast"])
    assert first.returncode == 0 and (outside / "calls").read_text().splitlines() == ["js"]
    cache = root / ".metrics/cache/types"
    cache.write_text(before)
    (outside / "bad").touch()
    cached = run(["bash", "scripts/check.sh", "fast"])
    assert key() == before and cached.returncode != 0 and "step 'types' failed" in cached.stderr
    assert (outside / "calls").read_text().splitlines() == ["js", "js"]
    fresh = run(["bash", "scripts/check.sh", "fast"], NO_CACHE="1")
    assert fresh.returncode != 0 and "step 'types' failed" in fresh.stderr
    assert (outside / "calls").read_text().splitlines() == ["js", "js", "js"]
    assert cache.read_text() == before


def test_javascript_result_cannot_skip_python_with_legacy_cache(tmp_path):
    root, outside, run, key = fixture(tmp_path, python=True)
    before = key()
    cache = root / ".metrics/cache/types"
    cache.parent.mkdir(parents=True, exist_ok=True)
    cache.write_text(before)
    first = run(["bash", "scripts/check.sh", "fast"])
    assert first.returncode != 0 and "step 'types' failed" in first.stderr
    assert (outside / "calls").read_text().splitlines() == ["js", "python"]
    assert (outside / "version").read_text().splitlines() == ["version"]
    assert cache.read_text() == before
    fresh = run(["bash", "scripts/check.sh", "fast"], NO_CACHE="1")
    assert (outside / "version").read_text().splitlines() == ["version", "version"]
    assert key() == before and fresh.returncode != 0 and "step 'types' failed" in fresh.stderr
    assert (outside / "calls").read_text().splitlines() == ["js", "python", "js", "python"]
