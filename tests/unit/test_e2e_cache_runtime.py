"""The end-to-end gate reruns when external inputs cannot be qualified."""

import os
import shutil
import subprocess

from tests.unit.test_gate_cache import GIT, ROOT, _stub


def test_changed_external_input_never_reuses_end_to_end_success(tmp_path):
    root, bin_dir = tmp_path / "repo", tmp_path / "bin"
    (root / "scripts").mkdir(parents=True)
    bin_dir.mkdir()
    for name in ("check.sh", "input-key.sh"):
        shutil.copy(ROOT / "scripts" / name, root / "scripts" / name)
    for name in ("check-docs.sh", "leak-check.sh"):
        _stub(root / "scripts" / name)
    _stub(root / "scripts/e2e.sh", 'echo e2e >> "$PROBE_LOG"; [ ! -f "$PROBE_BAD" ]')
    for name in ("uv", "gitleaks"):
        _stub(bin_dir / name)
    (root / ".gitignore").write_text(".metrics/\n")
    env = {
        "PATH": f"{bin_dir}{os.pathsep}{os.defpath}",
        "HOME": str(tmp_path),
        "LC_ALL": "C",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_ALLOW_PROTOCOL": "",
        "PROBE_LOG": str(tmp_path / "calls"),
        "PROBE_BAD": str(tmp_path / "bad"),
    }

    def run(argv, **overrides):
        return subprocess.run(argv, cwd=root, env=env | overrides, capture_output=True, text=True, timeout=60, check=False)

    for args in (("init", "-q", "-b", "main"), ("add", "-A"), ("commit", "-qm", "fixture")):
        assert run([*GIT, *args]).returncode == 0
    scope = run(["bash", "scripts/check.sh", "inputs", "e2e"])
    assert scope.returncode == 0 and scope.stdout.startswith("e2e\t")
    spec = scope.stdout.split("\t", 1)[1].split()

    def key():
        result = run(["bash", "scripts/input-key.sh", *spec])
        assert result.returncode == 0 and result.stdout.strip()
        return result.stdout

    before = key()
    first = run(["bash", "scripts/check.sh"])
    assert first.returncode == 0 and (tmp_path / "calls").read_text().splitlines() == ["e2e"]
    # A previous version may have left an entry with the same repository key.
    (root / ".metrics/cache/e2e").write_text(before.strip())
    (tmp_path / "bad").touch()
    fresh = run(["bash", "scripts/check.sh"])
    assert key() == before and fresh.returncode != 0 and "step 'e2e' failed" in fresh.stderr
    assert "e2e cached (inputs unchanged)" not in fresh.stdout
    assert (tmp_path / "calls").read_text().splitlines() == ["e2e", "e2e"]
