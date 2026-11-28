"""The hosted gate runs in parallel parts (docs/ci.md, "Parallel parts").

`CHECK_STEPS` makes scripts/check.sh run only the steps it names, and
`E2E_SHARD=i/n` makes scripts/e2e.sh run one share of the story files.
These tests prove that the parts of the workflow together run every gate step
and every story file exactly once, so a split can never drop a check.
"""

import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

import pytest
import yaml

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/ci.yml"
CHECK = ROOT / "scripts/check.sh"


def _gate_steps() -> list[str]:
    """Every step name in scripts/check.sh, in order, once each."""
    names: list[str] = []
    for name in re.findall(r"(?<![\w#-])step ([a-z0-9-]+) ", "\n".join(line.split("#")[0] for line in CHECK.read_text().splitlines())):
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
            assert names == ["e2e"], "an e2e part runs only e2e, or its other steps would run once per shard"
            continue  # e2e runs in every e2e shard; the shards split its story files
        if "rust_shard" in part:
            # rust-test runs in every Rust share; the shares split its test targets (scripts/rust-test-shard.py).
            assert "rust-test" in names, part
            names = [n for n in names if n != "rust-test"]
        seen += names
    seen += ["e2e", "rust-test"]
    assert sorted(seen) == sorted(set(seen)), "a step runs in two parts"
    assert set(seen) == set(expected), {"missing": set(expected) - set(seen), "extra": set(seen) - set(expected)}


def test_the_e2e_parts_name_every_shard_once():
    shards = [p["shard"] for p in _parts() if "e2e" in p["steps"].split()]
    total = {s.split("/")[1] for s in shards}
    assert len(total) == 1, shards
    n = int(total.pop())
    assert sorted(shards) == [f"{i}/{n}" for i in range(1, n + 1)]


def test_the_rust_test_parts_name_every_share_once():
    parts = [p for p in _parts() if "rust-test" in p["steps"].split()]
    shards = [p.get("rust_shard", "") for p in parts]
    if shards == [""]:
        return  # one part runs all of rust-test
    assert "" not in shards, "every part that runs rust-test names its share, or a share would run all of it"
    n = int(shards[0].split("/")[1])
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


def test_the_real_weight_lines_are_read(tmp_path):
    # The stories carry a note after the number; a weight that does not parse falls back to 10 and unbalances the shards.
    lines = [t.read_text().splitlines()[1] for t in sorted((ROOT / "tests/e2e").glob("us-*.sh")) if "# e2e: weight:" in t.read_text()]
    assert len(lines) >= 10 and len({re.sub(r"\d+", "N", line) for line in lines}) == 1, "every weight line has one shape"
    heavy = (ROOT / "tests/e2e/us-72-every-control.sh").read_text().splitlines()[1]
    # Named to sort first: read as 10, round-robin would put us-01-a beside the heavy story.
    root = _e2e_repo(tmp_path, {"us-01-a.sh": "", "us-02-b.sh": "", "us-72-heavy.sh": heavy})
    assert _e2e(root, "1/2").returncode == 0
    assert (root / "ran-1-2").read_text().split() == ["us-72-heavy.sh"]


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


# ---- RUST_TEST_SHARD with a fake cargo ----

SHARD_SCRIPT = ROOT / "scripts/rust-test-shard.py"
FAKE_TARGETS = {"pkg-a": ["component", "heavy", "a1"], "pkg-b": ["component", "b1", "new_one"], "pkg-c": []}


def _shard_repo(tmp_path: Path, weights: str) -> Path:
    root = tmp_path / "rust"
    (root / "scripts").mkdir(parents=True)
    (root / "config").mkdir()
    (root / "bin").mkdir()
    shutil.copy(SHARD_SCRIPT, root / "scripts/rust-test-shard.py")
    (root / "config/rust-test-weights.txt").write_text(weights)
    packages = [
        {
            "id": f"{name} 0.1.0",
            "targets": [{"name": "lib", "kind": ["lib"]}] + [{"name": t, "kind": ["test"]} for t in tests],
        }
        for name, tests in FAKE_TARGETS.items()
    ]
    # A path dependency outside the workspace: its test targets are not ours to run.
    packages.append({"id": "outside 0.1.0", "targets": [{"name": "outside_test", "kind": ["test"]}]})
    meta = {"packages": packages, "workspace_members": [f"{n} 0.1.0" for n in FAKE_TARGETS]}
    (root / "meta.json").write_text(json.dumps(meta))
    cargo = root / "bin/cargo"
    cargo.write_text('#!/usr/bin/env bash\nif [ "$1" = metadata ]; then cat "$(dirname "$0")/../meta.json"; exit 0; fi\necho "${RUST_TEST_SHARD-unset} $*" >> "$(dirname "$0")/../ran"\nexit "${FAKE_CARGO_EXIT:-0}"\n')
    cargo.chmod(0o755)
    return root


def _shard(root: Path, shard: str, *args: str, **env: str) -> subprocess.CompletedProcess[str]:
    full = {**os.environ, "PATH": f"{root / 'bin'}:{os.environ['PATH']}", "RUST_TEST_SHARD": shard, **env}
    return subprocess.run([sys.executable, "-I", str(root / "scripts/rust-test-shard.py"), *args], env=full, capture_output=True, text=True, timeout=60, check=False)


WEIGHTS_TEXT = "# test weights\n@share-1-extra 50\nheavy 100\ncomponent 40\na1 5\nb1 5\n"


@pytest.mark.parametrize("n", [1, 2, 3])
def test_the_rust_shares_run_every_test_target_exactly_once(tmp_path, n):
    root = _shard_repo(tmp_path, WEIGHTS_TEXT)
    ran: list[str] = []
    for i in range(1, n + 1):
        r = _shard(root, f"{i}/{n}")
        assert r.returncode == 0, r.stdout + r.stderr
    lines = (root / "ran").read_text().splitlines()
    for line in lines:
        ran += [a.removeprefix("--test=") for a in line.split() if a.startswith("--test=")]
    # Same-name targets of two crates are one entry; the outside package's target never runs.
    assert sorted(ran) == ["a1", "b1", "component", "heavy", "new_one"]
    assert sum("--lib --bins" in line for line in lines) == 1, "lib and bin unit tests run in share 1 only"
    assert sum(line.endswith("test --workspace --doc") for line in lines) == 1, "doc tests run in share 1 only"
    assert all(line.startswith("unset ") for line in lines), "cargo must not inherit the share"
    # Tests run the workspace's executables from target/debug, which plain `cargo test --all` built as a side effect.
    builds = [k for k, line in enumerate(lines) if line == "unset build --workspace --bins"]
    assert len(builds) == n and all(lines[k + 1].startswith("unset test ") for k in builds), lines


def test_the_heaviest_target_goes_first_and_a_new_target_still_runs(tmp_path):
    root = _shard_repo(tmp_path, WEIGHTS_TEXT)
    r = _shard(root, "1/2", "--plan")
    assert r.returncode == 0, r.stderr
    shares = json.loads(r.stdout)["shares"]
    # Share 1 starts at 50 (its part's other steps), so heavy (100) goes to share 2.
    # new_one has no weight line, weighs 60, and goes to share 1; component (40) then joins heavy.
    assert shares == [["a1", "b1", "new_one"], ["component", "heavy"]]


@pytest.mark.parametrize(
    "odd",
    [{"name": "demo", "kind": ["example"]}, {"name": "speed", "kind": ["bench"]}, {"name": "skipped", "kind": ["test"], "test": False}],
)
def test_a_target_the_split_does_not_handle_fails_the_share(tmp_path, odd):
    root = _shard_repo(tmp_path, WEIGHTS_TEXT)
    meta = json.loads((root / "meta.json").read_text())
    meta["packages"][0]["targets"].append(odd)
    (root / "meta.json").write_text(json.dumps(meta))
    r = _shard(root, "1/2")
    assert r.returncode == 1 and odd["name"] in r.stderr
    assert not (root / "ran").exists(), "nothing runs when the split could drop a target"


def test_a_failed_cargo_fails_the_share(tmp_path):
    root = _shard_repo(tmp_path, WEIGHTS_TEXT)
    r = _shard(root, "1/2", FAKE_CARGO_EXIT="101")
    assert r.returncode == 101
    assert len((root / "ran").read_text().splitlines()) == 1, "a failed first command stops the share"


@pytest.mark.parametrize("bad", ["", "0/2", "3/2", "2", "a/b", "1/0"])
def test_a_wrong_rust_shard_fails_with_the_format(tmp_path, bad):
    root = _shard_repo(tmp_path, WEIGHTS_TEXT)
    r = _shard(root, bad)
    assert r.returncode == 2
    assert "RUST_TEST_SHARD" in r.stderr and "i/n" in r.stderr


def test_a_bad_weight_line_fails_and_names_its_line(tmp_path):
    root = _shard_repo(tmp_path, "heavy lots\n")
    r = _shard(root, "1/2")
    assert r.returncode != 0 and "rust-test-weights.txt:1" in r.stderr


def test_the_real_weights_file_parses():
    for number, line in enumerate((ROOT / "config/rust-test-weights.txt").read_text().splitlines(), 1):
        line = line.split("#", 1)[0].strip()
        assert not line or re.fullmatch(r"@?[a-z0-9_-]+ [0-9]+", line), f"line {number}: {line!r}"


def test_check_sh_passes_the_share_to_the_script_and_hides_it_from_other_steps():
    text = CHECK.read_text()
    assert "export -n RUST_TEST_SHARD" in text
    assert "step rust-test rust_test" in text
    # With no share, rust-test is unchanged: the frozen tool contract in test_gate_cache.py.
    assert "else cargo test --all; fi" in text
