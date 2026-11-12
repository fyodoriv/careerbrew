"""Observe the lint cache against a changed synthetic external runner."""

import os
import shutil
import subprocess

from tests.unit.test_gate_cache import GIT, ROOT, _stub


def test_changed_lint_runner_cannot_reuse_a_stale_lint_pass(tmp_path):
    root, bin_dir = tmp_path / "repo", tmp_path / "bin"
    (root / "scripts").mkdir(parents=True)
    bin_dir.mkdir()
    for name in ("check.sh", "input-key.sh"):
        shutil.copy(ROOT / "scripts" / name, root / "scripts" / name)
    for name in ("check-docs.sh", "leak-check.sh", "e2e.sh"):
        _stub(root / "scripts" / name)
    for name in ("uv", "gitleaks"):
        _stub(bin_dir / name)
    (root / ".gitignore").write_text(".metrics/\n")
    (root / "pyproject.toml").write_text("# synthetic gate fixture\n")
    env = {"PATH": f"{bin_dir}{os.pathsep}{os.defpath}", "HOME": str(tmp_path), "LC_ALL": "C", "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull, "GIT_TERMINAL_PROMPT": "0", "GIT_ALLOW_PROTOCOL": "", "UNIT_JOBS": "0"}

    def run(argv, **overrides):
        return subprocess.run(argv, cwd=root, env=env | overrides, capture_output=True, text=True, timeout=60, check=False)

    for args in (("init", "-q", "-b", "main"), ("add", "-A"), ("commit", "-qm", "fixture")):
        assert run([*GIT, *args]).returncode == 0
    assert run(["bash", "scripts/check.sh", "fast"]).returncode == 0
    before = run(["bash", "scripts/input-key.sh"])
    scope = run(["bash", "scripts/check.sh", "inputs", "lint"])
    assert scope.returncode == 0 and scope.stdout.startswith("lint\t")
    lint_key = run(["bash", "scripts/input-key.sh", *scope.stdout.split("\t", 1)[1].split()])
    assert lint_key.returncode == 0 and lint_key.stdout
    old_lint_key = lint_key.stdout
    (root / ".metrics/cache/lint").write_text(old_lint_key)
    _stub(bin_dir / "uv", 'if [ "$1" = run ] && [ "$2" = ruff ] && [ "$3" = check ]; then exit 17; fi\nexit 0')
    after = run(["bash", "scripts/input-key.sh"])
    cached = run(["bash", "scripts/check.sh", "fast"])
    fresh = run(["bash", "scripts/check.sh", "fast"], NO_CACHE="1")
    assert before.returncode == after.returncode == 0
    assert before.stdout and before.stdout == after.stdout
    assert cached.returncode != 0 and "step 'lint' failed" in cached.stderr
    assert (root / ".metrics/cache/lint").read_text() == old_lint_key
    assert fresh.returncode != 0 and "step 'lint' failed" in fresh.stderr
