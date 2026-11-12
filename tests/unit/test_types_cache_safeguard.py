"""Exercise unqualified type check inputs and the shared legacy cache key."""

import os

from tests.unit.test_gate_cache import _stub
from tests.unit.test_types_command_collision_probe import fixture


def seed(root, key):
    target = root / ".metrics/cache/types"
    target.parent.mkdir(parents=True, exist_ok=True)
    value = key().strip()
    target.write_text(value + "\n")
    os.utime(target, (1, 1))
    return target, value


def test_changed_javascript_ignores_legacy_cache(tmp_path):
    root, outside, run, key = fixture(tmp_path, python=False)
    target, value = seed(root, key)
    (outside / "bad").touch()
    result = run(["bash", "scripts/check.sh", "fast"])
    assert result.returncode != 0 and "step 'types' failed" in result.stderr
    assert (outside / "calls").read_text().splitlines() == ["js"]
    assert target.read_text().strip() == value
    assert target.stat().st_mtime_ns == 1_000_000_000


def test_python_only_ignores_legacy_cache(tmp_path):
    root, outside, run, key = fixture(tmp_path, python=True)
    (root / "package.json").unlink()
    target, value = seed(root, key)
    result = run(["bash", "scripts/check.sh", "fast"])
    assert result.returncode != 0 and "step 'types' failed" in result.stderr
    assert (outside / "calls").read_text().splitlines() == ["python"]
    assert target.read_text().strip() == value
    assert target.stat().st_mtime_ns == 1_000_000_000


def test_combined_runs_both_in_order_and_keeps_other_cache(tmp_path):
    root, outside, run, key = fixture(tmp_path, python=True)
    _stub(outside / "bin/uv", 'if [ "$1" = run ] && [ "$2" = pyright ]; then echo python >> "$PROBE_LOG"; fi\nexit 0')
    target, value = seed(root, key)
    for _ in range(2):
        result = run(["bash", "scripts/check.sh", "fast"])
        assert result.returncode == 0
    assert (outside / "calls").read_text().splitlines() == ["js", "python", "js", "python"]
    assert "docs cached (inputs unchanged)" in result.stdout
    assert target.read_text().strip() == value
    assert target.stat().st_mtime_ns == 1_000_000_000


def test_combined_python_failure_follows_javascript(tmp_path):
    root, outside, run, key = fixture(tmp_path, python=True)
    seed(root, key)
    result = run(["bash", "scripts/check.sh", "fast"])
    assert result.returncode != 0 and "step 'types' failed" in result.stderr
    assert (outside / "calls").read_text().splitlines() == ["js", "python"]


def test_javascript_failure_stops_before_python(tmp_path):
    root, outside, run, key = fixture(tmp_path, python=True)
    seed(root, key)
    (outside / "bad").touch()
    result = run(["bash", "scripts/check.sh", "fast"])
    assert result.returncode != 0 and "step 'types' failed" in result.stderr
    assert (outside / "calls").read_text().splitlines() == ["js"]
    assert not (outside / "version").exists()


def test_unavailable_python_checker_keeps_existing_skip(tmp_path):
    root, outside, run, key = fixture(tmp_path, python=True)
    _stub(outside / "bin/uv", 'if [ "$1" = run ] && [ "$2" = --quiet ] && [ "$3" = pyright ]; then echo version >> "$PROBE_VERSION"; exit 5; fi\nif [ "$1" = run ] && [ "$2" = pyright ]; then echo python >> "$PROBE_LOG"; fi\nexit 0')
    seed(root, key)
    result = run(["bash", "scripts/check.sh", "fast"])
    assert result.returncode == 0
    assert (outside / "calls").read_text().splitlines() == ["js"]
    assert (outside / "version").read_text().splitlines() == ["version"]


def test_passing_combined_checks_do_not_create_a_type_cache(tmp_path):
    root, outside, run, _ = fixture(tmp_path, python=True)
    _stub(outside / "bin/uv", 'if [ "$1" = run ] && [ "$2" = pyright ]; then echo python >> "$PROBE_LOG"; fi\nexit 0')
    assert run(["bash", "scripts/check.sh", "fast"]).returncode == 0
    assert (outside / "calls").read_text().splitlines() == ["js", "python"]
    assert not (root / ".metrics/cache/types").exists()


def test_missing_javascript_typecheck_remains_optional(tmp_path):
    root, outside, run, key = fixture(tmp_path, python=False)
    _stub(outside / "bin/npm", 'if [ "$1" = run ] && [ "$2" = --if-present ] && [ "$3" = typecheck ]; then echo optional >> "$PROBE_LOG"; fi\nexit 0')
    target, value = seed(root, key)
    result = run(["bash", "scripts/check.sh", "fast"])
    assert result.returncode == 0
    assert (outside / "calls").read_text().splitlines() == ["optional"]
    assert target.read_text().strip() == value
    assert target.stat().st_mtime_ns == 1_000_000_000
