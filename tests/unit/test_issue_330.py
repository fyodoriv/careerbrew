"""Self-update runs before the old plan can be read (#330)."""

import subprocess

import pytest

from careerbrew import cli, config, paths, plugins
from careerbrew.workday import start


@pytest.fixture(autouse=True)
def fake_launcher(monkeypatch):
    from careerbrew.workday import self_update

    monkeypatch.setattr(self_update, "launcher_ready", lambda: True)


def test_update_only_runs_before_building_a_plan(monkeypatch, capsys):
    from careerbrew.workday import plan, self_update

    monkeypatch.setattr(plan, "build", lambda: (_ for _ in ()).throw(AssertionError("old plan read")))
    monkeypatch.setattr(self_update, "run", lambda: {"updated": True, "changed_files": ["commands/work.md"], "refresh": [], "repairs": [], "ok": True})
    assert cli.main(["day", "start", "--update-only", "--json"]) == 0
    assert '"updated": true' in capsys.readouterr().out


def test_update_repairs_deps_launcher_plugins_and_commands_in_order(monkeypatch, tmp_path):
    from careerbrew.workday import self_update

    (tmp_path / "Agentfile.yaml").write_text("commands: [./commands]\n")
    monkeypatch.setattr(paths, "REPO_ROOT", tmp_path)
    monkeypatch.setattr(start, "checkouts", lambda declared, **kwargs: [{"top": str(tmp_path), "for": ["careerbrew"], "duplicate_of": ""}])
    monkeypatch.setattr(start, "refresh_one", lambda top: {"status": "current", "before": "a", "after": "a", "path": top})
    monkeypatch.setattr(self_update, "declared_steps", lambda: [])
    monkeypatch.setattr(self_update, "primary_agent_roots", lambda: {"codex": tmp_path / "custom-agent"})
    calls = []
    monkeypatch.setattr(self_update, "repair", lambda argv, cwd: calls.append((argv, cwd)) or True)
    monkeypatch.setattr(self_update.shutil, "which", lambda command: "/fake/agentbrew")
    got = self_update.run()
    assert got["ok"]
    assert [argv[:2] for argv, _ in calls] == [["uv", "sync"], ["bash", "scripts/install-launcher.sh"], ["uv", "run"], ["/fake/agentbrew", "sync"]]
    assert calls[2][0][-3:] == ["careerbrew", "plugin", "refresh"]
    assert all(cwd == tmp_path for _, cwd in calls)


def test_refused_self_checkout_is_not_installed(monkeypatch, tmp_path):
    from careerbrew.workday import self_update

    monkeypatch.setattr(paths, "REPO_ROOT", tmp_path)
    monkeypatch.setattr(start, "checkouts", lambda declared, **kwargs: [{"top": str(tmp_path), "for": ["careerbrew"], "duplicate_of": ""}])
    monkeypatch.setattr(start, "refresh_one", lambda top: {"status": "local changes", "before": "a", "after": "a", "path": top})
    monkeypatch.setattr(self_update, "declared_steps", lambda: [])
    monkeypatch.setattr(self_update, "repair", lambda *args: (_ for _ in ()).throw(AssertionError("dirty checkout installed")))
    assert not self_update.run()["ok"]


def test_shell_only_update_does_not_deploy_agent_configuration(monkeypatch, tmp_path):
    from careerbrew.workday import self_update

    (tmp_path / "Agentfile.yaml").write_text("commands: [./commands]\n")
    monkeypatch.setattr(paths, "REPO_ROOT", tmp_path)
    monkeypatch.setattr(start, "checkouts", lambda declared, **kwargs: [{"top": str(tmp_path), "for": ["careerbrew"], "duplicate_of": ""}])
    monkeypatch.setattr(start, "refresh_one", lambda top: {"status": "current", "path": top})
    monkeypatch.setattr(self_update, "declared_steps", lambda: [])
    monkeypatch.setattr(self_update, "primary_agent_roots", lambda: {})
    monkeypatch.setattr(self_update.shutil, "which", lambda command: "/fake/agentbrew")
    calls = []
    monkeypatch.setattr(self_update, "repair", lambda argv, cwd: calls.append(argv) or True)
    assert self_update.run()["ok"]
    assert all(argv[0] != "/fake/agentbrew" for argv in calls)


def test_stale_step_declarations_do_not_prevent_source_refresh(monkeypatch, tmp_path):
    from careerbrew.workday import self_update, steps

    monkeypatch.setattr(paths, "REPO_ROOT", tmp_path)
    monkeypatch.setattr(self_update, "declared_steps", lambda: (_ for _ in ()).throw(steps.StepError("stale step")))
    monkeypatch.setattr(start, "checkouts", lambda declared, **kwargs: [{"top": str(tmp_path), "for": ["careerbrew"], "duplicate_of": ""}])
    calls = []
    monkeypatch.setattr(start, "refresh_one", lambda top: calls.append(top) or {"status": "current", "path": top})
    monkeypatch.setattr(self_update, "primary_agent_roots", lambda: {})
    monkeypatch.setattr(self_update, "repair", lambda argv, cwd: True)
    assert self_update.run()["ok"]
    assert calls == [str(tmp_path)]


def test_preserved_foreign_launcher_is_not_reported_repaired(monkeypatch, tmp_path):
    from careerbrew.workday import self_update

    monkeypatch.setattr(paths, "REPO_ROOT", tmp_path)
    monkeypatch.setattr(start, "checkouts", lambda declared, **kwargs: [{"top": str(tmp_path), "for": ["careerbrew"], "duplicate_of": ""}])
    monkeypatch.setattr(start, "refresh_one", lambda top: {"status": "current", "path": top})
    monkeypatch.setattr(self_update, "declared_steps", lambda: [])
    monkeypatch.setattr(self_update, "primary_agent_roots", lambda: {})
    monkeypatch.setattr(self_update, "repair", lambda argv, cwd: True)
    monkeypatch.setattr(self_update, "launcher_ready", lambda: False)
    got = self_update.run()
    assert not got["ok"]
    assert got["repairs"][-1] == {"check": "launcher", "ok": False}


def test_fast_forward_reports_changed_instructions_and_plan_reads_new_step(monkeypatch, tmp_path):
    from careerbrew.workday import plan, self_update
    from tests.unit.test_issue_260 import git, remote_with_clone

    monkeypatch.setenv("GIT_CONFIG_GLOBAL", "/dev/null")
    monkeypatch.setenv("GIT_CONFIG_NOSYSTEM", "1")
    bare, clone = remote_with_clone(tmp_path, "source")
    (clone / "commands").mkdir()
    (clone / "commands/work.md").write_text("old work\n")
    (clone / plugins.MANIFEST).write_text("name: fresh-co\nversion: 1.0.0\nwork_steps: [/old]\n")
    git(clone, "add", ".")
    git(clone, "commit", "-m", "old plugin")
    git(clone, "push", "origin", "main")
    cli.main(["init"])
    plugins.add(str(clone))
    config.set_value("work_checkouts", "careerbrew")
    monkeypatch.setattr(paths, "REPO_ROOT", clone)
    monkeypatch.setattr(self_update, "declared_steps", lambda: [])
    other = tmp_path / "pusher"
    subprocess.run(["git", "clone", "--quiet", str(bare), str(other)], check=True)
    (other / "commands/work.md").write_text("new work\n")
    (other / "commands/new.md").write_text("new step\n")
    (other / plugins.MANIFEST).write_text("name: fresh-co\nversion: 1.0.1\nwork_steps: [/new]\nprovides: {commands: commands}\n")
    git(other, "add", ".")
    git(other, "commit", "-m", "new plugin")
    git(other, "push", "origin", "main")

    def repair(argv, cwd):
        if argv[-3:] == ["careerbrew", "plugin", "refresh"]:
            plugins.refresh()
        return True

    monkeypatch.setattr(self_update, "repair", repair)
    got = self_update.run()
    assert got["updated"] and got["ok"]
    assert str(clone / "commands/work.md") in got["changed_files"]
    assert "/new" in {s["step"] for s in plan.build()["steps"]}
    assert "/old" not in {s["step"] for s in plan.build()["steps"]}
