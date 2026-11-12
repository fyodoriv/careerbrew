"""Web type checks rerun when external inputs cannot be qualified."""

import hashlib

from tests.unit.test_gate_cache import _stub
from tests.unit.test_types_command_collision_probe import fixture


def test_changed_web_checker_input_never_reuses_stale_success(tmp_path):
    root, outside, run, _ = fixture(tmp_path, python=False)
    (root / "package.json").unlink()
    (root / "web/node_modules").mkdir(parents=True)
    (root / "web/package.json").write_text("{}\n")
    checker = outside / "bin/npm"
    _stub(checker, 'if [ "$1" = run ] && [ "$2" = --silent ] && [ "$3" = typecheck ]; then echo web >> "$PROBE_LOG"; [ ! -f "$PROBE_BAD" ] || exit 17; fi\nexit 0')
    binary_hash = hashlib.sha256(checker.read_bytes()).hexdigest()
    scope = run(["bash", "scripts/check.sh", "inputs", "web-types"])
    assert scope.returncode == 0 and scope.stdout.startswith("web-types\t")
    spec = scope.stdout.split("\t", 1)[1].split()

    def key():
        value = run(["bash", "scripts/input-key.sh", *spec])
        assert value.returncode == 0 and value.stdout.strip()
        return value.stdout

    before = key()
    first = run(["bash", "scripts/check.sh", "fast"])
    assert first.returncode == 0 and (outside / "calls").read_text().splitlines() == ["web"]
    # A previous version may have left an entry with the same repository key.
    (root / ".metrics/cache/web-types").write_text(before.strip())
    (outside / "bad").touch()
    fresh = run(["bash", "scripts/check.sh", "fast"])
    assert hashlib.sha256(checker.read_bytes()).hexdigest() == binary_hash
    assert key() == before and fresh.returncode != 0 and "step 'web-types' failed" in fresh.stderr
    assert "web-types cached (inputs unchanged)" not in fresh.stdout
    assert (outside / "calls").read_text().splitlines() == ["web", "web"]
