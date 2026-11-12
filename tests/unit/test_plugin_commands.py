"""Plugins ship slash commands (issue #8)."""

import hashlib
from pathlib import Path

import pytest

from careerbrew import plugins

_RUN_AGENTBREW = plugins._run_agentbrew


def _plugin(root, name="acme", cmd="acme-checkup"):
    (root / "commands").mkdir(parents=True)
    (root / "commands" / f"{cmd}.md").write_text("---\ndescription: synthetic\n---\nRun careerbrew checkup.\n")
    (root / "careerbrew-plugin.yaml").write_text(f"name: {name}\nversion: 0.1.0\nprovides: {{commands: commands/}}\n")
    return root


def test_plugin_commands_deploy_on_add_and_refresh_and_go_on_remove(home, tmp_path):
    src = _plugin(tmp_path / "p")
    plugins.add(str(src))
    target = plugins.plugins_dir() / "acme" / "commands"
    assert (target / "acme-checkup.md").exists()
    (src / "commands" / "acme-new.md").write_text("new\n")
    plugins.refresh()
    assert (target / "acme-new.md").exists()
    (src / "commands" / "acme-new.md").unlink()
    plugins.refresh()
    assert not (target / "acme-new.md").exists(), "a command dropped from the plugin is removed"
    plugins.remove("acme")
    assert not (target / "acme-checkup.md").exists()


def test_plugin_command_never_overwrites_a_command_it_does_not_own(home, tmp_path):
    target = plugins.commands_dir()
    target.mkdir(parents=True)
    (target / "acme-checkup.md").write_text("mine\n")
    with pytest.raises(plugins.PluginError, match="does not own"):
        plugins.add(str(_plugin(tmp_path / "p")))
    assert (target / "acme-checkup.md").read_text() == "mine\n"
    assert plugins.installed() == []


@pytest.mark.parametrize("pattern", ["../*.md", "/etc/*", "commands/../../x"])
def test_commands_path_outside_the_plugin_is_refused_and_staging_cleaned(home, tmp_path, pattern):
    (tmp_path / "outside.md").write_text("not the plugin's\n")
    root = tmp_path / "p" / "inner"
    root.mkdir(parents=True)
    (root / "careerbrew-plugin.yaml").write_text(f"name: acme\nversion: 0.1.0\nprovides: {{commands: '{pattern}'}}\n")
    with pytest.raises(plugins.PluginError):
        plugins.add(str(root))
    assert not plugins.commands_dir().exists() or not any(plugins.commands_dir().iterdir())
    assert not (plugins.plugins_dir() / ".staging").exists()


def test_plugin_commands_are_private_sources_for_common_cross_host_sync(home, tmp_path, monkeypatch):
    calls = []
    monkeypatch.setattr(plugins, "_run_agentbrew", lambda folder: calls.append(folder), raising=False)
    source = _plugin(tmp_path / "source")
    plugins.add(str(source))
    fragment = home / "agentbrew" / "Agentfile.yaml"
    assert fragment.is_file()
    import yaml

    declared = yaml.safe_load(fragment.read_text())["commands"]
    assert declared == [str(plugins.plugins_dir() / "acme" / "commands" / "acme-checkup.md")]
    assert calls == [fragment.parent]
    assert not plugins.commands_dir().exists(), "never write an agent output or a second command copy"
    # Removal must empty the same owner's declaration, even if a source directory survives.
    original = plugins.plugins_dir() / "acme" / "commands"
    plugins.remove("acme")
    original.mkdir(parents=True)
    assert yaml.safe_load(fragment.read_text())["commands"] == []
    assert calls == [fragment.parent, fragment.parent]


def test_common_sync_errors_are_not_reported_as_plugin_success(home, tmp_path, monkeypatch):
    def fail(folder):
        raise plugins.PluginError("common command sync failed")

    monkeypatch.setattr(plugins, "_run_agentbrew", fail, raising=False)
    with pytest.raises(plugins.PluginError, match="common command sync failed"):
        plugins.add(str(_plugin(tmp_path / "source")))


def test_legacy_owned_copy_is_archived_only_when_old_source_matches(home, tmp_path):
    source = _plugin(tmp_path / "p")
    plugins.add(str(source))
    old = plugins.plugins_dir() / "acme" / "commands" / "acme-checkup.md"
    legacy = plugins.commands_dir() / old.name
    legacy.parent.mkdir(parents=True, exist_ok=True)
    legacy.write_bytes(old.read_bytes())
    original = legacy.read_bytes()
    plugins.refresh()
    assert not legacy.exists()
    from careerbrew import store

    archived = home / "command-archive" / "acme" / hashlib.sha256(original).hexdigest() / old.name
    assert store.read_text(archived).encode() == original
    assert original not in archived.read_bytes(), "archive encrypted at rest"
    plugins.refresh()
    assert store.read_text(archived).encode() == original


def test_legacy_edited_copy_survives_and_fails_with_attention(home, tmp_path):
    source = _plugin(tmp_path / "p")
    plugins.add(str(source))
    legacy = plugins.commands_dir() / "acme-checkup.md"
    legacy.parent.mkdir(parents=True, exist_ok=True)
    legacy.write_text("owner addition")
    with pytest.raises(plugins.PluginError, match="legacy.*edited"):
        plugins.refresh()
    assert legacy.read_text() == "owner addition"


def test_archive_verification_failure_preserves_legacy_leaf(home, tmp_path, monkeypatch):
    plugins.add(str(_plugin(tmp_path / "p")))
    legacy = plugins.commands_dir() / "acme-checkup.md"
    legacy.parent.mkdir(parents=True, exist_ok=True)
    legacy.write_bytes((plugins.plugins_dir() / "acme" / "commands" / legacy.name).read_bytes())
    from careerbrew import store

    original = legacy.read_bytes()
    real_read = store.read_text
    monkeypatch.setattr(store, "read_text", lambda path, *args, **kwargs: "wrong" if "command-archive" in Path(path).parts else real_read(path, *args, **kwargs))
    with pytest.raises(plugins.PluginError, match="archive verification"):
        plugins.remove("acme")
    assert legacy.read_bytes() == original


def test_empty_refresh_retries_a_failed_removal_sync(home, tmp_path, monkeypatch):
    plugins.add(str(_plugin(tmp_path / "p")))

    def fail(folder):
        raise plugins.PluginError("common command sync failed")

    monkeypatch.setattr(plugins, "_run_agentbrew", fail)
    with pytest.raises(plugins.PluginError):
        plugins.remove("acme")
    calls = []
    monkeypatch.setattr(plugins, "_run_agentbrew", lambda folder: calls.append(folder))
    assert plugins.refresh() == []
    assert calls == [home / "agentbrew"]


def test_exact_file_selection_does_not_export_unselected_siblings(home, tmp_path):
    source = _plugin(tmp_path / "p")
    (source / "commands" / "not-selected.md").write_text("private unselected workflow")
    (source / "careerbrew-plugin.yaml").write_text("name: acme\nversion: 0.1.0\nprovides: {commands: commands/acme-checkup.md}\n")
    plugins.add(str(source))
    import yaml

    selected = yaml.safe_load((home / "agentbrew" / "Agentfile.yaml").read_text())["commands"]
    assert selected == [str(plugins.plugins_dir() / "acme" / "commands" / "acme-checkup.md")]


def test_missing_prior_canonical_source_preserves_legacy_copy(home, tmp_path):
    plugins.add(str(_plugin(tmp_path / "p")))
    old = plugins.plugins_dir() / "acme" / "commands" / "acme-checkup.md"
    original = old.read_bytes()
    legacy = plugins.commands_dir() / old.name
    legacy.parent.mkdir(parents=True, exist_ok=True)
    legacy.write_bytes(original)
    old.unlink()
    with pytest.raises(plugins.PluginError, match="prior source unavailable"):
        plugins.refresh()
    assert legacy.read_bytes() == original


def test_external_deployer_failure_does_not_print_its_payload(tmp_path, capsys):
    import shutil

    folder = tmp_path / "fragment"
    folder.mkdir()
    (folder / "Agentfile.yaml").write_text("commands: []\n")
    # The home fixture puts a scratch-only deployer on PATH, including for child CLIs.
    helper = Path(shutil.which("agentbrew") or "")
    assert helper.is_relative_to(tmp_path)
    _RUN_AGENTBREW(folder)
    helper.write_text("#!/bin/sh\necho CANARY-DEPLOYER-PAYLOAD\nexit 1\n")
    helper.chmod(0o755)
    with pytest.raises(plugins.PluginError, match="common command sync failed"):
        _RUN_AGENTBREW(folder)
    assert "CANARY-DEPLOYER-PAYLOAD" not in capsys.readouterr().out


@pytest.mark.parametrize("failure", ["kept", "missing-target", "missing-source", "invalid-report", "boolean-version", "numeric-path", "relative-path"])
def test_zero_exit_sync_cannot_hide_requested_command_readiness(tmp_path, monkeypatch, failure):
    import json
    import shutil
    import sys

    folder = tmp_path / "fragment"
    folder.mkdir()
    source = tmp_path / "selected.md"
    source.write_text("---\ndescription: Fixture\n---\n")
    import yaml

    (folder / "Agentfile.yaml").write_text(yaml.safe_dump({"commands": [str(source)]}))
    agent_home = tmp_path / "agent-home"
    for host in (".claude", ".codex"):
        (agent_home / host).mkdir(parents=True)
    monkeypatch.setattr(Path, "home", lambda: agent_home)
    targets = [{"agent": "claude-code", "ready": True, "status": "ready", "path": "/synthetic/claude"}, {"agent": "codex", "ready": True, "status": "ready", "path": "/synthetic/native"}]
    if failure == "kept":
        targets[1].update(ready=False, status="kept")
    if failure == "missing-target":
        targets.pop()
    report = {"version": 1, "commands": [{"filename": source.name, "sourcePath": str(source), "targets": targets}]}
    if failure == "missing-source":
        report["commands"] = []
    if failure == "boolean-version":
        report["version"] = True
    if failure == "numeric-path":
        targets[1]["path"] = 123
    if failure == "relative-path":
        targets[1]["path"] = "relative/target"

    encoded = "CANARY-INVALID-REPORT" if failure == "invalid-report" else json.dumps(report)
    helper = Path(shutil.which("agentbrew") or "")
    assert helper.is_relative_to(tmp_path)
    helper.write_text(f"#!{sys.executable}\nimport sys\nif sys.argv[1:] == ['commands', 'list', '--json']: print({encoded!r})\n")
    helper.chmod(0o755)
    with pytest.raises(plugins.PluginError, match="readiness"):
        _RUN_AGENTBREW(folder)


def test_internal_command_symlink_alias_is_explicitly_refused(home, tmp_path):
    source = _plugin(tmp_path / "alias")
    shared = source / "shared"
    shared.mkdir()
    (shared / "check.md").write_text("Synthetic alias resource\n")
    (source / "commands" / "team-check.md").symlink_to("../shared/check.md")
    with pytest.raises(plugins.PluginError, match="symlink.*plain files"):
        plugins.add(str(source))
    assert not (home / "agentbrew" / "Agentfile.yaml").exists()
    assert plugins.installed() == []


def test_cli_test_processes_use_scratch_client_home(tmp_path):
    import subprocess
    import sys

    output = subprocess.check_output([sys.executable, "-c", "from pathlib import Path; print(Path.home())"], text=True)
    assert Path(output.strip()) == tmp_path / ".test-client-home"
