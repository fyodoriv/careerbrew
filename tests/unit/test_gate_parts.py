"""The hosted gate runs in parallel parts (docs/ci.md, "Parallel parts").

`CHECK_STEPS` makes scripts/check.sh run only the steps it names, and
`E2E_SHARD=i/n` makes scripts/e2e.sh run one share of the story files.
These tests prove that the parts of the workflow together run every gate step
and every story file exactly once, so a split can never drop a check.
"""

import os
import re
import shutil
import subprocess
from pathlib import Path

import pytest
import yaml

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/ci.yml"
CHECK = ROOT / "scripts/check.sh"


def _gate_steps() -> list[str]:
    """Every step name in scripts/check.sh, in order, once each."""
    names: list[str] = []
    for name in re.findall(r"^\s*(?:if [^\n]*then )?step ([a-z0-9-]+) ", CHECK.read_text(), flags=re.M):
        if name not in names:
            names.append(name)
    return names


def _parts() -> list[dict]:
    jobs = yaml.safe_load(WORKFLOW.read_text())["jobs"]
    linux, macos = (jobs[os]["strategy"]["matrix"]["part"] for os in ("linux", "macos"))
    assert linux == macos, "both OSes run the same parts"
    return linux


def test_both_oses_run_their_parts_through_the_one_part_workflow():
    jobs = yaml.safe_load(WORKFLOW.read_text())["jobs"]
    assert [jobs[os]["with"]["os"] for os in ("linux", "macos")] == ["ubuntu-24.04", "macos-15"]
    assert {jobs[os]["uses"] for os in ("linux", "macos")} == {"./.github/workflows/gate-part.yml"}


def test_the_parts_run_every_gate_step_exactly_once():
    steps = _gate_steps()
    # The JavaScript-only steps (a root package.json) never run here; evals run only with CHECK_EVALS=1.
    expected = [s for s in steps if s != "evals"]
    seen: list[str] = []
    for part in _parts():
        names = part["steps"].split()
        if "e2e" in names:
            continue  # e2e runs in every e2e shard; the shards split its story files
        seen += names
    seen += ["e2e"]
    assert sorted(seen) == sorted(set(seen)), "a step runs in two parts"
    assert set(seen) == set(expected), {"missing": set(expected) - set(seen), "extra": set(seen) - set(expected)}


def test_the_e2e_parts_name_every_shard_once():
    shards = [p["shard"] for p in _parts() if "e2e" in p["steps"].split()]
    total = {s.split("/")[1] for s in shards}
    assert len(total) == 1, shards
    n = int(total.pop())
    assert sorted(shards) == [f"{i}/{n}" for i in range(1, n + 1)]


def test_each_os_has_one_required_check_name_that_waits_for_all_its_parts():
    jobs = yaml.safe_load(WORKFLOW.read_text())["jobs"]
    names = {job.get("name") for job in jobs.values()}
    # Branch protection on main requires these names (docs/ci.md, "Branch rule on `main`").
    assert {"gate (ubuntu-24.04)", "gate (macos-15)"} <= names
    waits = {job["name"]: job["needs"] for job in jobs.values() if job.get("name", "").startswith("gate (")}
    assert waits == {"gate (ubuntu-24.04)": "linux", "gate (macos-15)": "macos"}
    for job in jobs.values():
        if job.get("name", "").startswith("gate ("):
            # Without always() a failed part would skip the check, and a skipped required check reads as missing.
            assert "always()" in job["if"]
            assert "= success ]" in job["steps"][-1]["run"]


# ---- CHECK_STEPS in a tiny repo with stub step scripts ----


def _check_repo(tmp_path: Path) -> Path:
    root = tmp_path / "repo"
    (root / "scripts").mkdir(parents=True)
    shutil.copy(CHECK, root / "scripts/check.sh")
    (root / "scripts/check-docs.sh").write_text("#!/usr/bin/env bash\necho docs >> ran\n")
    (root / "scripts/input-key.sh").write_text("#!/usr/bin/env bash\necho key\n")
    for f in ("check.sh", "check-docs.sh", "input-key.sh"):
        (root / "scripts" / f).chmod(0o755)
    subprocess.run(["git", "init", "-q"], cwd=root, check=True)
    return root


def _check(root: Path, steps: str, *args: str) -> subprocess.CompletedProcess[str]:
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    env.update({"CHECK_STEPS": steps, "NO_CACHE": "1", "CHECK_OFFLINE": "1", "PATH": "/usr/bin:/bin"})
    return subprocess.run(["bash", "scripts/check.sh", *args], cwd=root, env=env, capture_output=True, text=True, timeout=60, check=False)


def test_check_steps_runs_only_the_named_steps(tmp_path):
    root = _check_repo(tmp_path)
    r = _check(root, "docs")
    assert r.returncode == 0, r.stdout + r.stderr
    assert (root / "ran").read_text().split() == ["docs"]
    assert "check.sh: steps docs passed" in r.stdout
    # learnings, leaks, sources and stories would run uv or scripts the stub repo does not have.
    assert "learnings" not in r.stdout + r.stderr


def test_check_steps_refuses_a_name_that_is_not_a_step(tmp_path):
    root = _check_repo(tmp_path)
    r = _check(root, "docs unti")
    assert r.returncode == 1
    assert "unti" in r.stderr and "did not run" in r.stderr


def test_a_step_does_not_pass_the_filter_on_to_its_children(tmp_path):
    # The hosted checks part ran unit tests that run check.sh in a scratch repo; they saw the part's filter and failed.
    root = _check_repo(tmp_path)
    (root / "scripts/check-docs.sh").write_text('#!/usr/bin/env bash\necho "${CHECK_STEPS-unset}" >> ran\n')
    r = _check(root, "docs")
    assert r.returncode == 0, r.stdout + r.stderr
    assert (root / "ran").read_text().split() == ["unset"]


# ---- E2E_SHARD in a tiny repo with stub stories ----


def _e2e_repo(tmp_path: Path, stories: dict[str, str]) -> Path:
    root = tmp_path / "e2e"
    (root / "scripts").mkdir(parents=True)
    (root / "tests/e2e").mkdir(parents=True)
    shutil.copy(ROOT / "scripts/e2e.sh", root / "scripts/e2e.sh")
    for name, head in stories.items():
        (root / "tests/e2e" / name).write_text(f'#!/usr/bin/env bash\n{head}\necho {name} >> "$RAN"\n')
    return root


def _e2e(root: Path, shard: str) -> subprocess.CompletedProcess[str]:
    env = {**os.environ, "E2E_JOBS": "2", "E2E_SHARD": shard, "RAN": str(root / f"ran-{shard.replace('/', '-')}")}
    return subprocess.run(["bash", "scripts/e2e.sh"], cwd=root, env=env, capture_output=True, text=True, timeout=60, check=False)


STORIES = {
    "us-01-heavy.sh": "# e2e: weight: 600",
    "us-02-a.sh": "",
    "us-03-b.sh": "# e2e: weight: 40",
    "us-04-c.sh": "",
    "us-05-d.sh": "# e2e: weight: 90",
}


@pytest.mark.parametrize("n", [1, 2, 3])
def test_the_shards_run_every_story_exactly_once(tmp_path, n):
    root = _e2e_repo(tmp_path, STORIES)
    ran: list[str] = []
    for i in range(1, n + 1):
        r = _e2e(root, f"{i}/{n}")
        assert r.returncode == 0, r.stdout + r.stderr
        ran += (root / f"ran-{i}-{n}").read_text().split() if (root / f"ran-{i}-{n}").exists() else []
    assert sorted(ran) == sorted(STORIES)


def test_the_heaviest_story_gets_a_shard_of_its_own_weight(tmp_path):
    root = _e2e_repo(tmp_path, STORIES)
    assert _e2e(root, "1/2").returncode == 0
    # 600 is more than all the others together (40 + 90 + 2 x 10), so shard 1 holds it alone.
    assert (root / "ran-1-2").read_text().split() == ["us-01-heavy.sh"]
    assert _e2e(root, "2/2").returncode == 0
    assert sorted((root / "ran-2-2").read_text().split()) == ["us-02-a.sh", "us-03-b.sh", "us-04-c.sh", "us-05-d.sh"]


@pytest.mark.parametrize("bad", ["0/2", "3/2", "2", "a/b", "1/0"])
def test_a_wrong_shard_fails_with_the_format(tmp_path, bad):
    root = _e2e_repo(tmp_path, STORIES)
    r = _e2e(root, bad)
    assert r.returncode == 2
    assert "E2E_SHARD" in r.stderr and "i/n" in r.stderr


def test_a_story_does_not_inherit_the_shard(tmp_path):
    root = _e2e_repo(tmp_path, {"us-01-a.sh": 'echo "${E2E_SHARD-unset}" >> "$RAN.env"'})
    assert _e2e(root, "1/1").returncode == 0
    assert (root / "ran-1-1.env").read_text().split() == ["unset"]


def test_a_shard_run_does_not_write_the_whole_suite_results(tmp_path):
    root = _e2e_repo(tmp_path, STORIES)
    assert _e2e(root, "1/2").returncode == 0
    assert not (root / ".metrics/e2e-results.json").exists()
