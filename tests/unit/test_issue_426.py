"""Measure update-only discovery on isolated synthetic checkouts."""

from careerbrew import config, paths, plugins
from careerbrew.workday import self_update, start
from tests.unit.test_issue_385 import (
    COMMIT,
    REMOTE,
    TOP,
    _seed_repo,
    own_git,  # noqa: F401
)


def test_update_discovery_reuses_only_invocation_local_lookups(tmp_path, monkeypatch, own_git):  # noqa: F811
    own = tmp_path / "self"
    other = tmp_path / "other"
    (own / "commands").mkdir(parents=True)
    (other / "commands").mkdir(parents=True)
    (own / "Agentfile.yaml").write_text("commands: [./commands]\n")
    for name in ("alpha", "beta"):
        (other / "commands" / f"{name}.md").write_text("synthetic\n")
    _seed_repo(own, own_git, "self-origin.git")
    _seed_repo(other, own_git, "other-origin.git")
    monkeypatch.setattr(paths, "REPO_ROOT", own)
    monkeypatch.setattr(config, "get", lambda name: ["careerbrew"] if name == "work_checkouts" else None)
    monkeypatch.setattr(plugins, "installed", lambda: [])
    monkeypatch.setattr(self_update, "declared_steps", lambda: [{"step": f"/{name}", "path": str(other / "commands" / f"{name}.md"), "uses": []} for name in ("alpha", "beta")])
    refreshes = []
    repairs = []
    monkeypatch.setattr(start, "refresh_one", lambda top: refreshes.append(top) or {"status": "current", "top": top})
    monkeypatch.setattr(self_update, "repair", lambda argv, cwd: repairs.append((argv, cwd)) or True)
    monkeypatch.setattr(self_update, "launcher_ready", lambda: True)
    monkeypatch.setattr(self_update, "primary_agent_roots", lambda: {"synthetic": own})
    monkeypatch.setattr(self_update.shutil, "which", lambda name: "/synthetic/agentbrew")
    calls = []
    original = start._git

    def observed(where, *argv, **kwargs):
        assert where in (own, str(own), other, str(other), other / "commands", str(other / "commands"))
        assert argv in (TOP, COMMIT, REMOTE)
        calls.append(argv)
        return original(where, *argv, **kwargs)

    monkeypatch.setattr(start, "_git", observed)
    result = self_update.run()
    assert result["ok"] and not result["updated"]
    assert refreshes == [str(own), str(other)]
    assert [argv for argv, _ in repairs] == [["uv", "sync"], ["bash", "scripts/install-launcher.sh"], ["uv", "run", "careerbrew", "plugin", "refresh"], ["/synthetic/agentbrew", "sync"]]
    assert calls.count(TOP) == 3
    assert len(calls) == 7


def test_later_invocation_rediscovers_a_new_checkout(tmp_path, monkeypatch, own_git):  # noqa: F811
    own, other = tmp_path / "self", tmp_path / "other"
    own.mkdir()
    other.mkdir()
    (own / "Agentfile.yaml").write_text("commands: []\n")
    (other / "source.md").write_text("synthetic\n")
    _seed_repo(own, own_git, "self-origin.git")
    monkeypatch.setattr(paths, "REPO_ROOT", own)
    monkeypatch.setattr(config, "get", lambda name: ["careerbrew", str(other)] if name == "work_checkouts" else None)
    monkeypatch.setattr(plugins, "installed", lambda: [])
    monkeypatch.setattr(self_update, "declared_steps", lambda: [])
    refreshes = []
    monkeypatch.setattr(start, "refresh_one", lambda top: refreshes.append(top) or {"status": "current", "top": top})
    monkeypatch.setattr(self_update, "repair", lambda argv, cwd: True)
    monkeypatch.setattr(self_update, "launcher_ready", lambda: True)
    monkeypatch.setattr(self_update, "primary_agent_roots", lambda: {})
    assert self_update.run()["ok"]
    assert refreshes == [str(own)]
    _seed_repo(other, own_git, "other-origin.git")
    refreshes.clear()
    assert self_update.run()["ok"]
    assert refreshes == [str(own), str(other)]


def test_update_preserves_duplicate_remote_discovery(tmp_path, monkeypatch, own_git):  # noqa: F811
    own, other = tmp_path / "self", tmp_path / "other"
    own.mkdir()
    other.mkdir()
    for repo in (own, other):
        (repo / "source.md").write_text("synthetic\n")
        _seed_repo(repo, own_git, "shared-origin.git")
    monkeypatch.setattr(paths, "REPO_ROOT", own)
    monkeypatch.setattr(config, "get", lambda name: ["careerbrew", str(other)] if name == "work_checkouts" else None)
    monkeypatch.setattr(plugins, "installed", lambda: [])
    monkeypatch.setattr(self_update, "declared_steps", lambda: [])
    refreshes, discovered = [], []
    original = start.checkouts

    def observe(*args, **kwargs):
        result = original(*args, **kwargs)
        discovered.extend(result)
        return result

    monkeypatch.setattr(start, "checkouts", observe)
    monkeypatch.setattr(start, "refresh_one", lambda top: refreshes.append(top) or {"status": "current", "top": top})
    monkeypatch.setattr(self_update, "repair", lambda argv, cwd: True)
    monkeypatch.setattr(self_update, "launcher_ready", lambda: True)
    monkeypatch.setattr(self_update, "primary_agent_roots", lambda: {})
    assert self_update.run()["ok"]
    assert refreshes == [str(own), str(other)]
    assert discovered[0]["duplicate_of"] == ""
    assert discovered[1]["duplicate_of"] == start.shown(str(own))
