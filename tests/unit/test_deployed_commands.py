"""scripts/check-deployed-commands.py: a deployed /careerbrew-* command older than its source is a doctor failure (task checkup-command-drift).

agentbrew copies `commands/` from the checkout it records (`commandSourceDirs` in its state file) into each agent's
commands folder on `agentbrew sync`, and records every copy in its manifest. A copy older than that source, or a
source on main that is behind origin/main, makes an agent follow old steps, so the doctor (and the checkup's Health
section) names the fix that clears it.
"""

from __future__ import annotations

import hashlib
import importlib.util
import json
import subprocess
import sys
from pathlib import Path

import pytest
import yaml

SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "check-deployed-commands.py"
GIT = ["git", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false", "-c", "user.name=Test", "-c", "user.email=test@example.com"]
NAME = "careerbrew-checkup.md"
NEW = "# checkup\n\n1. Run the gate.\n2. Refresh the Dashboard.\n"


def _module():
    spec = importlib.util.spec_from_file_location("check_deployed_commands", SCRIPT)
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


@pytest.mark.parametrize("agent,binary", [("claude-code", "claude"), ("codex", "codex")])
@pytest.mark.parametrize("state", ["ready", "signed-out", "missing", "timeout", "unknown", "os-error", "optional", "opted-out"])
def test_runtime_doctor_reports_safe_native_status(tmp_path, monkeypatch, capsys, agent, binary, state):
    from types import SimpleNamespace

    from careerbrew import agent_workflows

    canary = "CANARY-AUTH-ACCOUNT-OUTPUT"
    client = tmp_path / "client"
    other = "codex" if agent == "claude-code" else "claude-code"
    records = [{"name": other, "detected": False}]
    if state != "optional":
        records.append({"name": agent, "detected": state != "opted-out", "skillsDir": str(client / "skills")})
    if state == "opted-out":
        client.mkdir()
    config = tmp_path / "state.yaml"
    config.write_text(yaml.safe_dump({"agents": records}))
    executable = str(tmp_path / binary)
    calls = []

    def probe(argv, *, capture_output, text, timeout, stdin, check):
        calls.append(argv)
        assert capture_output is True and text is True and timeout == 2 and stdin == subprocess.DEVNULL and check is False
        if state == "timeout":
            raise subprocess.TimeoutExpired(argv, 2, output=canary, stderr=canary)
        if state == "os-error":
            raise OSError(canary)
        if state == "unknown":
            return subprocess.CompletedProcess(argv, 0 if binary == "claude" else 2, canary, canary)
        signed_in = state == "ready"
        body = json.dumps({"loggedIn": signed_in, "email": canary, "token": canary}) if binary == "claude" else "Logged in using ChatGPT" if signed_in else "Not logged in"
        return subprocess.CompletedProcess(argv, 0 if signed_in else 1, body, "")

    monkeypatch.setattr(agent_workflows, "subprocess", SimpleNamespace(run=probe, DEVNULL=subprocess.DEVNULL, TimeoutExpired=subprocess.TimeoutExpired), raising=False)
    monkeypatch.setattr(agent_workflows, "shutil", SimpleNamespace(which=lambda name: executable if name == binary and state not in {"missing", "optional"} else None), raising=False)
    mod = _module()
    monkeypatch.setattr(mod, "rows", lambda *args: [])
    code = mod.main([str(SCRIPT), str(tmp_path), str(tmp_path / "manifest.json"), str(config)])
    out = capsys.readouterr().out
    expected = {
        "ready": f"{binary} CLI installed; local auth: signed in",
        "signed-out": f"{binary} CLI installed; local auth: needs login",
        "missing": f"{binary} CLI missing for configured agent",
        "timeout": f"{binary} CLI installed; local auth: unknown",
        "unknown": f"{binary} CLI installed; local auth: unknown",
        "os-error": f"{binary} CLI installed; local auth: unknown",
        "optional": f"{binary} CLI not installed (optional; agent not configured)",
        "opted-out": f"{binary} CLI installed; auth not checked (agent not configured)",
    }
    assert expected[state] in out
    assert code == (0 if state in {"ready", "optional", "opted-out"} else 1)
    assert canary not in out
    if state in {"timeout", "unknown", "os-error"}:
        assert "local auth: needs login" not in out
    if state in {"missing", "optional", "opted-out"}:
        assert calls == []
    else:
        assert calls == [[executable, *(["auth", "status", "--json"] if binary == "claude" else ["login", "status"])]]


def _git(cwd: Path, *args: str) -> None:
    subprocess.run([*GIT, *args], cwd=cwd, check=True, capture_output=True)


def _commit(repo: Path, text: str, msg: str) -> None:
    (repo / "commands" / NAME).write_text(text)
    _git(repo, "add", ".")
    _git(repo, "commit", "-q", "-m", msg)


def _deploy(env, text: str | None = None) -> None:
    """What `agentbrew sync` does: copy the source file to every agent and record each copy's hash."""
    body = text if text is not None else (env["repo"] / "commands" / NAME).read_text()
    hashes = {}
    for c in env["copies"]:
        c.parent.mkdir(parents=True, exist_ok=True)
        c.write_text(body)
        hashes[str(c)] = hashlib.sha256(body.encode()).hexdigest()
    env["manifest"].write_text(json.dumps({"hashes": hashes}))


@pytest.fixture(autouse=True)
def isolated_agent_home(tmp_path, monkeypatch):
    """CLI subprocesses must never inspect the owner's configured agent directories."""
    agent_home = tmp_path / "agent-home"
    agent_home.mkdir(exist_ok=True)
    monkeypatch.setenv("HOME", str(agent_home))


@pytest.fixture
def env(tmp_path):
    """A source checkout of a repo with an origin, agentbrew state that records it, and two deployed copies."""
    remote = tmp_path / "remote.git"
    _git(tmp_path, "init", "-q", "--bare", "-b", "main", str(remote))
    repo = tmp_path / "repo"
    (repo / "commands").mkdir(parents=True)
    _git(repo, "init", "-q", "-b", "main")
    _commit(repo, NEW, "init")
    _git(repo, "remote", "add", "origin", str(remote))
    _git(repo, "push", "-q", "-u", "origin", "main")
    state = tmp_path / "state.yaml"
    state.write_text(yaml.safe_dump({"commandSourceDirs": [{"label": "other", "path": str(tmp_path / "x" / "commands")}, {"label": "careerbrew", "path": str(repo / "commands")}]}))
    e = {
        "repo": repo,
        "remote": remote,
        "state": state,
        "manifest": tmp_path / "manifest.json",
        "copies": [tmp_path / "agentbrew" / "commands" / NAME, tmp_path / "claude" / "commands" / NAME],
    }
    _deploy(e)
    return e


def _rows(env, repo: Path | None = None):
    return _module().rows(repo or env["repo"], env["manifest"], env["state"])


def _bad(env, repo: Path | None = None):
    return [r for r in _rows(env, repo) if not r[0]]


def test_synced_commands_pass(env):
    rows = _rows(env)
    assert rows and all(r[0] for r in rows), rows


def test_a_copy_older_than_its_source_fails_with_the_sync_fix(env):
    _deploy(env, "# checkup\n\n1. Run the gate.\n")  # synced before the Dashboard step was added
    bad = _bad(env)
    assert len(bad) == 1 and "older than its source" in bad[0][1] and "careerbrew-checkup" in bad[0][1], bad
    assert bad[0][2] == "agentbrew sync"


def test_a_hand_edited_copy_names_the_fix_that_clears_it(env):
    """agentbrew sync skips a copy whose content no longer matches its manifest hash, so sync alone never fixes it."""
    env["copies"][1].write_text(NEW + "my own note\n")
    bad = _bad(env)
    assert len(bad) == 1 and "edited by hand" in bad[0][1], bad
    assert "rm " not in bad[0][2]
    assert bad[0][2] == "agentbrew status --verbose"


def test_a_deleted_copy_says_missing(env):
    env["copies"][0].unlink()
    bad = _bad(env)
    assert len(bad) == 1 and "missing" in bad[0][1] and bad[0][2] == "agentbrew sync", bad


def test_a_source_on_main_behind_origin_fails_with_the_pull_fix(env, tmp_path):
    """The 2026-10-25 shape: a command changed on origin/main, and the source checkout was never pulled."""
    other = tmp_path / "other"
    _git(tmp_path, "clone", "-q", str(env["remote"]), str(other))
    _commit(other, NEW + "3. A new step.\n", "new step")
    _git(other, "push", "-q", "origin", "main")
    _git(env["repo"], "fetch", "-q", "origin")
    bad = _bad(env)
    assert len(bad) == 1 and "behind origin/main" in bad[0][1], bad
    assert bad[0][2] == f"git -C {env['repo']} pull --ff-only && agentbrew sync"


def test_a_source_on_main_ahead_of_origin_passes(env):
    """An unpushed commit that edits a command, synced, is not "behind": a pull would not change it."""
    _commit(env["repo"], NEW + "3. A local step.\n", "local step")
    _deploy(env)
    assert all(r[0] for r in _rows(env)), _rows(env)


def test_a_new_command_on_origin_main_fails_with_the_pull_fix(env, tmp_path):
    other = tmp_path / "other"
    _git(tmp_path, "clone", "-q", str(env["remote"]), str(other))
    (other / "commands" / "careerbrew-new.md").write_text("# new\n")
    _git(other, "add", ".")
    _git(other, "commit", "-q", "-m", "new command")
    _git(other, "push", "-q", "origin", "main")
    _git(env["repo"], "fetch", "-q", "origin")
    bad = _bad(env)
    assert len(bad) == 1 and "/careerbrew-new" in bad[0][1] and "pull --ff-only" in bad[0][2], bad


def test_a_command_deleted_on_local_main_is_not_new_on_origin(env):
    """An unpushed delete is not something a pull brings back."""
    (env["repo"] / "commands" / "careerbrew-old.md").write_text("# old\n")
    _git(env["repo"], "add", ".")
    _git(env["repo"], "commit", "-q", "-m", "old command")
    _git(env["repo"], "push", "-q", "origin", "main")
    _git(env["repo"], "rm", "-q", "commands/careerbrew-old.md")
    _git(env["repo"], "commit", "-q", "-m", "drop old command")
    assert all(r[0] for r in _rows(env)), _rows(env)


def test_a_deleted_commands_folder_of_a_live_agent_says_missing(env):
    """agentbrew recreates commands/ when the agent's own folder exists, so sync clears it."""
    import shutil

    shutil.rmtree(env["copies"][1].parent)
    bad = _bad(env)
    assert len(bad) == 1 and "missing" in bad[0][1] and bad[0][2] == "agentbrew sync", bad


def test_a_copy_for_a_removed_agent_is_not_a_failure(env):
    """agentbrew writes a copy only when the agent's folder exists, so sync cannot clear a removed agent's entry."""
    import shutil

    shutil.rmtree(env["copies"][1].parent.parent)
    rows = _rows(env)
    assert all(r[0] for r in rows) and any("(1 copies)" in r[1] for r in rows), rows


def test_a_missing_yaml_library_is_a_crash_not_a_quiet_pass(env, tmp_path):
    """Without PyYAML the state file cannot be read, so the check must not say "not deployed"."""
    fake = tmp_path / "noyaml"
    fake.mkdir()
    (fake / "yaml.py").write_text("raise ImportError('no yaml')\n")
    out = subprocess.run([sys.executable, str(SCRIPT), str(env["repo"]), str(env["manifest"]), str(env["state"])], capture_output=True, text=True, check=False, env={"PYTHONPATH": str(fake), "PATH": "/usr/bin:/bin", "HOME": str(tmp_path)})
    assert out.returncode != 0 and "Traceback" in out.stderr and "not deployed" not in out.stdout, out
    assert "crashed" in _doctor_block(tmp_path, "import sys\nprint('Traceback (most recent call last):', file=sys.stderr)\nsys.exit(1)\n")


def test_a_source_on_a_branch_that_edits_a_command_is_a_note(env):
    """A sync from a branch is on purpose; any worktree's doctor must not fail on it."""
    _git(env["repo"], "switch", "-q", "-c", "feat/x")
    _commit(env["repo"], NEW + "3. A step this branch adds.\n", "branch step")
    _deploy(env)
    rows = _rows(env)
    assert all(r[0] for r in rows) and any("feat/x" in r[1] for r in rows), rows


def test_another_worktree_checks_the_recorded_source_not_its_own_files(env, tmp_path):
    wt = tmp_path / "wt"
    _git(env["repo"], "worktree", "add", "-q", "-b", "feat/y", str(wt))
    (wt / "commands" / NAME).write_text(NEW + "3. An edit not yet synced.\n")
    assert all(r[0] for r in _rows(env, wt))
    _deploy(env, "old\n")
    assert len(_bad(env, wt)) == 1


def test_no_recorded_source_is_a_note_not_a_failure(env):
    """A contributor whose agentbrew does not deploy careerbrew's commands still gets a healthy setup."""
    env["state"].write_text(yaml.safe_dump({"commandSourceDirs": [{"label": "other", "path": "/nowhere/commands"}]}))
    rows = _rows(env)
    assert all(r[0] for r in rows) and "not deployed by agentbrew" in rows[0][1], rows


@pytest.mark.parametrize("body", ["[]", '"x"', '{"hashes": []}', "{"])
def test_an_odd_manifest_is_a_note_not_a_crash(env, body):
    env["manifest"].write_text(body)
    rows = _rows(env)
    assert all(r[0] for r in rows) and "not checked" in rows[0][1], rows


@pytest.mark.parametrize("body", ["[]", "commandSourceDirs: 3", ": : :"])
def test_an_odd_state_file_is_a_note_not_a_crash(env, body):
    env["state"].write_text(body)
    rows = _rows(env)
    assert all(r[0] for r in rows), rows


def test_the_script_exits_1_and_prints_doctor_lines(env):
    _deploy(env, "old\n")
    out = subprocess.run([sys.executable, str(SCRIPT), str(env["repo"]), str(env["manifest"]), str(env["state"])], capture_output=True, text=True, check=False)
    assert out.returncode == 1, out
    assert any(line.startswith("FAIL  ") and "-> fix: " in line for line in out.stdout.splitlines()), out.stdout


def test_the_checkup_page_shows_a_stale_command_as_a_blocker_with_the_fix(env, tmp_path):
    from careerbrew import runs

    _deploy(env, "old\n")
    out = subprocess.run([sys.executable, str(SCRIPT), str(env["repo"]), str(env["manifest"]), str(env["state"])], capture_output=True, text=True, check=False).stdout
    logs = tmp_path / "logs"
    logs.mkdir()
    for ext, text in {"title": "Health", "cmd": "scripts/doctor.sh", "log": out, "rc": "1"}.items():
        (logs / f"01-health.{ext}").write_text(text)
    r = runs.repo_checkup(logs, 1)
    assert [b.command for b in r.blockers] == ["agentbrew sync"], r.blockers


def _doctor_block(tmp_path: Path, script: str) -> str:
    root = Path(__file__).resolve().parents[2]
    (tmp_path / "scripts").mkdir(exist_ok=True)
    (tmp_path / "scripts" / "check-deployed-commands.py").write_text(script)
    doctor = (root / "scripts" / "doctor.sh").read_text()
    block = doctor[doctor.index("# A deployed /careerbrew-* command") : doctor.index("if git rev-parse --git-dir")]
    run = f'cd {tmp_path}\nfails=0\nfail() {{ echo "FAIL  $1 -> fix: $2"; fails=$((fails + 1)); }}\n{block}\necho "fails=$fails"\n'
    return subprocess.run(["bash", "-c", run], capture_output=True, text=True, check=False).stdout


@pytest.mark.parametrize(
    "script",
    [
        "raise SystemExit('boom')\n",
        "print('ok    /careerbrew deployed and current (1 copies)')\nraise RuntimeError('late crash')\n",
    ],
)
def test_the_doctor_counts_a_crash_of_the_check_as_a_failure(tmp_path, script):
    out = _doctor_block(tmp_path, script)
    assert "fails=1" in out and "FAIL  deployed commands check crashed" in out, out


def test_the_doctor_counts_each_fail_line(tmp_path):
    out = _doctor_block(tmp_path, "import sys\nprint('ok    a')\nprint('FAIL  b -> fix: c')\nsys.exit(1)\n")
    assert "fails=1" in out and "FAIL  b -> fix: c" in out and "crashed" not in out, out


def test_native_prompt_transform_and_bridge_are_checked(env):
    source = env["repo"] / "commands" / NAME
    source.write_text("---\ndescription: Fixture\nallowed-tools: Bash\n---\nBody\n")
    prompt = env["copies"][0].parents[1] / ".codex" / "prompts" / NAME
    root = prompt.parents[1] / "skills" / "agentbrew-command-careerbrew-checkup"
    state = yaml.safe_load(env["state"].read_text())
    state["agents"] = [{"name": "codex", "detected": True, "skillsDir": str(root.parent)}]
    env["state"].write_text(yaml.safe_dump(state))
    files = {
        prompt: "---\ndescription: Fixture\n---\nBody\n",
        root / "references" / "command.md": source.read_text(),
        root / "SKILL.md": "---\nname: agentbrew-command-careerbrew-checkup\ndescription: Fixture\n---\nRead references/command.md. Keep the current approval rules. Do not use shell expansion.\n",
        root / "agents" / "openai.yaml": "policy:\n  allow_implicit_invocation: false\n",
    }
    for f, text in files.items():
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(text)
    _deploy(env, source.read_text())
    hashes = json.loads(env["manifest"].read_text())["hashes"]
    hashes.update({str(f): hashlib.sha256(f.read_bytes()).hexdigest() for f in files})
    env["manifest"].write_text(json.dumps({"hashes": hashes}))
    assert _bad(env) == []
    prompt.write_text("stale")
    assert any("native" in r[1] for r in _bad(env))
    prompt.write_text(files[prompt])
    (root / "references" / "command.md").unlink()
    assert any("native" in r[1] for r in _bad(env))


def test_native_bridge_policy_and_wrapper_guards_are_required(env):
    root = env["copies"][0].parents[1] / ".codex" / "skills" / "agentbrew-command-careerbrew-checkup"
    state = yaml.safe_load(env["state"].read_text())
    state["agents"] = [{"name": "codex", "detected": True, "skillsDir": str(root.parent)}]
    env["state"].write_text(yaml.safe_dump(state))
    files = {root / "references" / "command.md": NEW, root / "SKILL.md": "unsafe wrapper", root / "agents" / "openai.yaml": "policy: {allow_implicit_invocation: true}"}
    hashes = json.loads(env["manifest"].read_text())["hashes"]
    for f, text in files.items():
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(text)
        hashes[str(f)] = hashlib.sha256(f.read_bytes()).hexdigest()
    env["manifest"].write_text(json.dumps({"hashes": hashes}))
    assert any("native" in r[1] for r in _bad(env))


def test_detected_codex_requires_bridge_even_without_manifest_leaves(env):
    state = yaml.safe_load(env["state"].read_text())
    state["agents"] = [{"name": "codex", "detected": True, "skillsDir": str(env["repo"] / ".codex" / "skills")}]
    env["state"].write_text(yaml.safe_dump(state))
    assert any("native" in r[1] for r in _bad(env))


@pytest.mark.parametrize("host", [".claude", ".codex"])
@pytest.mark.parametrize("metadata", ["missing", "empty"])
def test_a_present_client_never_calls_missing_entrypoints_optional(tmp_path, monkeypatch, host, metadata):
    agent_home = tmp_path / "agent-home"
    (agent_home / host).mkdir(parents=True)
    monkeypatch.setattr(Path, "home", lambda: agent_home)
    manifest, state = tmp_path / "manifest.json", tmp_path / "state.yaml"
    if metadata == "empty":
        manifest.write_text('{"hashes": {}}')
        state.write_text("commandSourceDirs: []\n")
    root = Path(__file__).resolve().parents[2]
    result = _module().rows(root, manifest, state)
    assert any(not row[0] for row in result)
    assert not any("optional" in row[1] for row in result)
    assert any("agentbrew sync" in row[2] for row in result if not row[0])


@pytest.mark.parametrize("host", [".claude", ".codex"])
def test_declared_skills_are_required_on_each_present_client(tmp_path, monkeypatch, host):
    root = Path(__file__).resolve().parents[2]
    agent_home = tmp_path / "agent-home"
    client = agent_home / host
    client.mkdir(parents=True)
    monkeypatch.setattr(Path, "home", lambda: agent_home)
    files = {}
    for src in (root / "commands").glob("*.md"):
        if host == ".claude":
            files[client / "commands" / src.name] = src.read_text()
        else:
            bridge = client / "skills" / ("agentbrew-command-" + src.stem)
            files[bridge / "references" / "command.md"] = src.read_text()
            files[bridge / "SKILL.md"] = "references/command.md Keep the current approval rules Do not use shell expansion"
            files[bridge / "agents" / "openai.yaml"] = "policy: {allow_implicit_invocation: false}"
    for path, text in files.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    sources = list((root / "skills").glob("*/SKILL.md"))
    for source in sources:
        target = client / "skills" / source.parent.name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.symlink_to(source.parent, target_is_directory=True)
    manifest, state = tmp_path / "manifest.json", tmp_path / "state.yaml"
    manifest.write_text(json.dumps({"hashes": {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in files}}))
    state.write_text(yaml.safe_dump({"commandSourceDirs": [{"path": str(root / "commands")}]}))
    assert not [row for row in _module().rows(root, manifest, state) if not row[0]]
    (client / "skills" / sources[0].parent.name).unlink()
    assert any(not row[0] and "skill" in row[1] for row in _module().rows(root, manifest, state))


def test_excluded_codex_old_artifacts_do_not_fail_required_claude(env, tmp_path):
    state = yaml.safe_load(env["state"].read_text())
    state["agents"] = [
        {"name": "claude-code", "detected": True, "skillsDir": str(env["copies"][1].parent.parent / "skills")},
        {"name": "codex", "detected": False, "skillsDir": str(tmp_path / "old-native" / "skills")},
    ]
    env["state"].write_text(yaml.safe_dump(state))
    hashes = json.loads(env["manifest"].read_text())["hashes"]
    hashes[str(tmp_path / "old-native/skills/agentbrew-command-careerbrew-checkup/references/command.md")] = "old"
    env["manifest"].write_text(json.dumps({"hashes": hashes}))
    assert _bad(env) == []


def _custom_client(env, tmp_path, agent):
    client = tmp_path / ("configured-" + agent)
    client.mkdir()
    state = yaml.safe_load(env["state"].read_text())
    state["agents"] = [{"name": agent, "detected": True, "skillsDir": str(client / "skills"), "commandsDir": str(client / ("prompts" if agent == "codex" else "commands"))}]
    env["state"].write_text(yaml.safe_dump(state))
    _custom_command_copy(env, env["repo"] / "commands" / NAME, client, agent)
    return client


def _custom_command_copy(env, source, client, agent):
    hashes = json.loads(env["manifest"].read_text())["hashes"]
    if agent == "claude-code":
        files = {client / "commands" / source.name: source.read_text()}
    else:
        bridge = client / "skills" / ("agentbrew-command-" + source.stem)
        files = {
            bridge / "references/command.md": source.read_text(),
            bridge / "SKILL.md": "Read references/command.md. Keep the current approval rules. Do not use shell expansion.\n",
            bridge / "agents/openai.yaml": "policy:\n  allow_implicit_invocation: false\n",
        }
    for path, content in files.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        hashes[str(path)] = hashlib.sha256(path.read_bytes()).hexdigest()
    env["manifest"].write_text(json.dumps({"hashes": hashes}))


def _declare_doctor_sources(env, commands, skills):
    (env["repo"] / "Agentfile.yaml").write_text(yaml.safe_dump({"commands": commands, "skillDirectories": skills}))
    state = yaml.safe_load(env["state"].read_text())
    state["commandSourceDirs"] = [{"path": str(env["repo"] / p), "origin": "agentfile", "agentfile": str(env["repo"])} for p in commands]
    env["state"].write_text(yaml.safe_dump(state))


@pytest.mark.parametrize("agent", ["claude-code", "codex"])
def test_deployed_custom_skill_is_required(env, tmp_path, agent):
    _custom_client(env, tmp_path, agent)
    source = env["repo"] / "published-skills/careerbrew-extra/SKILL.md"
    source.parent.mkdir(parents=True)
    source.write_text("---\nname: careerbrew-extra\ndescription: Synthetic fixture\n---\n")
    nested = env["repo"] / "nested/entrypoints"
    nested.mkdir(parents=True)
    selected = nested / "selected.md"
    selected.write_text("Synthetic selected workflow.\n")
    client = tmp_path / ("configured-" + agent)
    _custom_command_copy(env, selected, client, agent)
    _declare_doctor_sources(env, ["nested/entrypoints"], ["published-skills"])
    assert any("careerbrew-extra" in row[1] for row in _bad(env))


@pytest.mark.parametrize("agent", ["claude-code", "codex"])
def test_deployed_selected_command_is_required(env, tmp_path, agent):
    _custom_client(env, tmp_path, agent)
    selected = env["repo"] / "entrypoints/selected.md"
    selected.parent.mkdir()
    selected.write_text("Synthetic selected workflow.\n")
    _declare_doctor_sources(env, ["commands", "entrypoints/selected.md"], [])
    assert any("/selected" in row[1] for row in _bad(env))


@pytest.mark.parametrize("agent", ["claude-code", "codex"])
def test_deployed_custom_only_sources_are_healthy(env, tmp_path, agent):
    client = _custom_client(env, tmp_path, agent)
    selected = env["repo"] / "nested/entrypoints/selected.md"
    selected.parent.mkdir(parents=True)
    selected.write_text("Synthetic selected workflow.\n")
    _custom_command_copy(env, selected, client, agent)
    alias = selected.with_name("selected-alias.md")
    alias.symlink_to(selected.name)
    _custom_command_copy(env, alias, client, agent)
    skill = env["repo"] / "published-skills/careerbrew-extra/SKILL.md"
    skill.parent.mkdir(parents=True)
    skill.write_text("---\nname: careerbrew-extra\ndescription: Synthetic fixture\n---\n")
    target = client / "skills/careerbrew-extra/SKILL.md"
    target.parent.mkdir(parents=True)
    target.write_bytes(skill.read_bytes())
    for commands in (["nested/entrypoints/selected.md"], ["nested/entrypoints"], ["nested/entrypoints/../entrypoints/selected-alias.md"]):
        _declare_doctor_sources(env, commands, ["published-skills"])
        assert _bad(env) == []


def test_deployed_custom_outside_alias_is_reported(env, tmp_path, monkeypatch):
    _custom_client(env, tmp_path, "claude-code")
    folder = env["repo"] / "entrypoints"
    folder.mkdir()
    selected = folder / "selected.md"
    selected.write_text("Protected synthetic source.\n")
    alias = tmp_path / "outside-alias"
    alias.symlink_to(folder, target_is_directory=True)
    _declare_doctor_sources(env, [str(alias / selected.name)], [])
    text, binary = Path.read_text, Path.read_bytes
    seen = []

    def guard(read):
        def guarded(path, *args, **kwargs):
            if path.resolve() == selected:
                seen.append(path)
                raise AssertionError("unsupported declaration body read")
            return read(path, *args, **kwargs)

        return guarded

    monkeypatch.setattr(Path, "read_text", guard(text))
    monkeypatch.setattr(Path, "read_bytes", guard(binary))
    assert any("outside" in row[1] for row in _bad(env))
    assert seen == []
