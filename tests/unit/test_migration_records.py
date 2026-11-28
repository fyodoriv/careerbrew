"""Batched migration measurement: the workflow, its step list and the records script (docs/ci.md)."""

import json
import os
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/migration-records.sh"
WORKFLOW = ROOT / ".github/workflows/migration-measure.yml"
RECORDS = ("baseline", "verify", "measure", "decide")


def _run(*args: str, root: Path = ROOT) -> subprocess.CompletedProcess[str]:
    env = {**os.environ, "MIGRATION_RECORDS_ROOT": str(root)}
    return subprocess.run(["bash", str(SCRIPT), *args], capture_output=True, text=True, env=env, check=False)


def _registry_steps() -> list[str]:
    """Steps whose baseline entry names a candidate (xtask/src/baseline.rs)."""
    text = (ROOT / "xtask/src/baseline.rs").read_text(encoding="utf-8")
    found = []
    for block in re.split(r"\n    Step \{", text.split("pub const STEPS: &[Step] = &[", 1)[1])[1:]:
        step = re.search(r'id: "(rust-[0-9]{2}[a-z]?)"', block)
        if step and re.search(r"candidate: Some\(", block):
            found.append(step.group(1))
    return found


def test_the_workflow_step_list_is_the_xtask_registry():
    listed = _run("steps")
    assert listed.returncode == 0, listed.stderr
    steps = listed.stdout.split()
    # Every step with a baseline candidate has a measure candidate, and the default matrix is that list.
    assert sorted(steps) == sorted(_registry_steps())
    assert len(steps) == len(set(steps)) and "rust-14" in steps
    matrix = json.loads(_run("matrix", "", "false").stdout)
    assert [job["step"] for job in matrix["include"]] == steps
    assert {job["os"] for job in matrix["include"]} == {"ubuntu-24.04"}
    # The workflow builds its matrix from the script, so a new registry entry joins it with no edit here.
    text = WORKFLOW.read_text(encoding="utf-8")
    assert 'scripts/migration-records.sh matrix "$STEPS" "$MACOS"' in text
    assert "fromJSON(needs.plan.outputs.matrix)" in text
    assert 'scripts/migration-records.sh run "$STEP" "$PAIRS" "$out"' in text


def test_the_matrix_takes_chosen_steps_and_an_optional_macos_job():
    done = _run("matrix", "rust-14 rust-03", "true")
    assert done.returncode == 0, done.stderr
    assert json.loads(done.stdout)["include"] == [
        {"step": "rust-14", "os": "ubuntu-24.04", "artifact": "migration-rust-14"},
        {"step": "rust-14", "os": "macos-15", "artifact": "migration-rust-14-macos-15"},
        {"step": "rust-03", "os": "ubuntu-24.04", "artifact": "migration-rust-03"},
        {"step": "rust-03", "os": "macos-15", "artifact": "migration-rust-03-macos-15"},
    ]


def test_bad_arguments_are_refused_with_exit_2():
    cases = [
        (("matrix", "rust-99", "false"), "no registered Rust candidate"),
        (("matrix", "rust-14;id", "false"), "not a step id"),
        (("matrix", "rust-14", "yes"), "true or false"),
        (("run", "rust-14", "0", "/tmp/x"), "positive number of pairs"),
        (("run", "rust-14", "30", "relative"), "absolute path"),
        (("run", "rust-14", "30", str(ROOT)), "must be new"),
        (("run", "rust-99", "30", "/tmp/x"), "no registered Rust candidate"),
        (("fetch", "abc"), "needs a run id"),
        (("fetch", "--dir", "/no/such/folder"), "no such folder"),
        ((), "Usage:"),
    ]
    for args, message in cases:
        done = _run(*args)
        assert done.returncode == 2, (args, done.stdout, done.stderr)
        assert message in done.stdout + done.stderr, (args, done.stderr)


def _artifact(folder: Path, name: str, step: str, *, passed: bool = True, decision: str = "keep-python") -> Path:
    path = folder / name
    path.mkdir(parents=True)
    for record in RECORDS:
        body = {"schema": f"careerbrew-migration-{record}-v1", "step": step}
        if record == "verify":
            body["passed"] = passed
        if record == "decide":
            body |= {"decision": decision, "reasons": ["noise-floor-missing"] if decision == "keep-python" else []}
        (path / f"{record}.json").write_text(json.dumps(body))
    return path


def test_fetch_writes_each_artifact_into_metrics_and_prints_one_line_per_step(tmp_path):
    downloaded = tmp_path / "downloaded"
    _artifact(downloaded, "migration-rust-14", "rust-14")
    _artifact(downloaded, "migration-rust-14-macos-15", "rust-14", passed=False)
    _artifact(downloaded, "migration-rust-03", "rust-03", decision="activate")
    checkout = tmp_path / "checkout"
    checkout.mkdir()
    done = _run("fetch", "--dir", str(downloaded), root=checkout)
    assert done.returncode == 0, done.stderr
    assert done.stdout.splitlines() == [
        "rust-03 verify=pass decide=activate",
        "rust-14 verify=pass decide=keep-python reasons=noise-floor-missing",
        "rust-14 (macos-15) verify=fail decide=keep-python reasons=noise-floor-missing",
        "migration-records: wrote 3 artifact(s) into .metrics/migration/",
    ]
    for record in RECORDS:
        assert (checkout / f".metrics/migration/rust-14/{record}.json").is_file()
        assert (checkout / f".metrics/migration/rust-14/macos-15/{record}.json").is_file()


def test_fetch_refuses_a_foreign_file_or_a_record_of_another_step(tmp_path):
    checkout = tmp_path / "checkout"
    checkout.mkdir()
    extra = _artifact(tmp_path / "a", "migration-rust-14", "rust-14")
    (extra / "trace.zip").write_text("x")
    wrong = _artifact(tmp_path / "b", "migration-rust-14", "rust-13")
    badname = _artifact(tmp_path / "c", "migration-rust-14-windows", "rust-14")
    for folder, message in ((extra.parent, "unexpected file"), (wrong.parent, "not a baseline record of rust-14"), (badname.parent, "refused artifact name")):
        done = _run("fetch", "--dir", str(folder), root=checkout)
        assert done.returncode == 2, done.stdout
        assert message in done.stderr
    assert not (checkout / ".metrics").exists()


def test_the_workflow_uploads_only_the_four_records_and_keeps_the_safety_rules():
    text = WORKFLOW.read_text(encoding="utf-8")
    assert "workflow_dispatch:" in text and "pull_request" not in text
    assert "permissions:\n  contents: read" in text
    assert "secrets." not in text
    assert set(re.findall(r"runs-on: (.+)", text)) == {"ubuntu-24.04", "${{ matrix.os }}"}
    for action in re.findall(r"uses: (\S+)", text):
        assert re.fullmatch(r"[\w.-]+/[\w.-]+@[0-9a-f]{40}", action), action
    paths = re.findall(r"\$\{\{ runner\.temp \}\}/migration/\$\{\{ matrix\.step \}\}/(\S+)", text)
    assert paths == [f"{record}.json" for record in RECORDS]
    # This is the only workflow that may upload an artifact (docs/ci.md, "Safety rules").
    uploads = [p.name for p in (ROOT / ".github/workflows").glob("*.yml") if "upload-artifact" in p.read_text(encoding="utf-8")]
    assert uploads == ["migration-measure.yml"]


def _stub_cargo(folder: Path) -> Path:
    """A fake `cargo` that logs each call and answers `metadata` with one package that has two bins."""
    folder.mkdir(parents=True, exist_ok=True)
    meta = {
        "packages": [
            {
                "name": "careerbrew-exercises",
                "targets": [
                    {"name": "exercises-component", "kind": ["bin"]},
                    {"name": "exercise-fixture-runner", "kind": ["bin"]},
                    {"name": "careerbrew_exercises", "kind": ["lib"]},
                ],
            }
        ]
    }
    (folder / "meta.json").write_text(json.dumps(meta), encoding="utf-8")
    stub = folder / "cargo"
    stub.write_text(
        f'#!/usr/bin/env bash\necho "$*" >> "{folder}/calls.log"\nif [ "$1" = metadata ]; then cat "{folder}/meta.json"; fi\n',
        encoding="utf-8",
    )
    stub.chmod(0o755)
    return folder


def test_run_builds_every_bin_of_the_candidates_package(tmp_path):
    """rust-22's verify needs exercise-fixture-runner next to exercises-component (run 46069607808 missed it)."""
    stubs = _stub_cargo(tmp_path / "bin")
    env = {**os.environ, "PATH": f"{stubs}:{os.environ['PATH']}"}
    done = subprocess.run(["bash", str(SCRIPT), "run", "rust-22", "1", str(tmp_path / "out")], capture_output=True, text=True, env=env, check=False)
    calls = (stubs / "calls.log").read_text(encoding="utf-8").splitlines()
    assert "build --locked --release -p careerbrew-exercises --bins" in calls, calls
    # The stub writes no records, so the step must fail, never pass quietly.
    assert done.returncode == 1, (done.stdout, done.stderr)


def test_the_workflow_step_fails_when_the_records_script_fails(tmp_path):
    """The step pipes the script into tee; without pipefail the job passed with records missing (run 46069607808)."""
    import yaml

    jobs = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))["jobs"]
    steps = [s for job in jobs.values() for s in job.get("steps", []) if s.get("name") == "Baseline, verify, measure and decide"]
    assert len(steps) == 1 and "shell" not in steps[0]
    repo = tmp_path / "repo"
    (repo / "scripts").mkdir(parents=True)
    fake = repo / "scripts/migration-records.sh"
    fake.write_text("#!/usr/bin/env bash\necho 'rust-22 verify=missing decide=missing'\nexit 1\n", encoding="utf-8")
    fake.chmod(0o755)
    script = tmp_path / "step.sh"
    script.write_text(steps[0]["run"], encoding="utf-8")
    env = {**os.environ, "RUNNER_TEMP": str(tmp_path), "GITHUB_STEP_SUMMARY": str(tmp_path / "summary.md"), "STEP": "rust-22", "PAIRS": "1", "OS": "ubuntu-24.04", "GITHUB_RUN_ID": "1", "GITHUB_SHA": "0" * 40}
    # GitHub runs a `run:` block with no `shell:` as `bash -e {0}`.
    done = subprocess.run(["bash", "-e", str(script)], cwd=repo, capture_output=True, text=True, env=env, check=False)
    assert done.returncode == 1, (done.stdout, done.stderr)
    assert "verify=missing" in (tmp_path / "summary.md").read_text(encoding="utf-8")
