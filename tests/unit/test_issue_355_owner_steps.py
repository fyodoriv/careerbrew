"""Owner work-step mapping and plugin ownership controls (#355)."""

import json
from pathlib import Path

import pytest

from careerbrew import cli, config, plugins
from careerbrew.workday import stages, steps


@pytest.fixture(autouse=True)
def setup(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T09:00:00+00:00")
    assert cli.main(["init"]) == 0


def test_owner_mapping_resolves_and_orders_external_step(tmp_path, monkeypatch):
    prompt = Path.home() / ".codex/prompts/external-checkup.md"
    prompt.parent.mkdir(parents=True)
    prompt.write_text("---\ndescription: Inspect the synthetic checkout.\n---\n")
    plugin = tmp_path / "plugin"
    commands = plugin / "commands"
    commands.mkdir(parents=True)
    (commands / "refresh.md").write_text("---\ndescription: Refresh synthetic sources.\n---\n")
    monkeypatch.setattr(
        plugins,
        "installed",
        lambda: [
            {"origin": "plugin:fixture", "root": str(plugin), "manifest": {"provides": {"commands": "commands"}, "work_steps": [{"step": "/refresh", "scope": ["refresh-inputs"]}, {"step": "/external-checkup", "scope": ["plugin-shadow"]}]}}
        ],
    )
    raw = [{"step": "/external-checkup", "scope": ["external-work"], "uses": ["file:external-result"], "after": ["/refresh"]}]
    assert cli.main(["config", "set", "work_steps", json.dumps(raw)]) == 0
    found = steps.declared()
    external = next(row for row in found if row["step"] == "/external-checkup")
    assert external["from"] == "settings"
    assert Path(external["path"]).expanduser() == prompt
    assert external["scope"] == ["external-work"] and external["uses"] == ["file:external-result"]
    assert external["after"] == ["/refresh"] and not external.get("source_error")
    declared, _ = stages.build(found)
    rows = [{"step": "/careerbrew", "status": "done"}, {"step": "/refresh", "status": "waiting"}, {"step": "/external-checkup", "status": "waiting"}]
    assert stages.ready(declared, rows) == ["/refresh"]
    rows[1]["status"] = "done"
    assert stages.ready(declared, rows) == ["/external-checkup"]


def test_comma_names_remain_normalized():
    assert config.set_value("work_steps", "standup, /review-prs") == ["/standup", "/review-prs"]
    assert config.get("work_steps") == ["/standup", "/review-prs"]


def test_invalid_settings_do_not_write(home):
    config.set_value("work_steps", "/standup")
    path = home / "settings.yaml"
    before = path.read_bytes()
    for value in ('[{"step":"/x"}', '[{"step":"/x","use":["file:x"]}]', '[{"scope":["x"]}]', "[" * 1200 + "]" * 1200):
        with pytest.raises(ValueError):
            config.set_value("work_steps", value)
        assert path.read_bytes() == before


def test_plugin_step_does_not_fall_back_to_host(tmp_path, monkeypatch):
    host = Path.home() / ".codex/prompts/external-checkup.md"
    host.parent.mkdir(parents=True)
    host.write_text("---\ndescription: Synthetic host command.\n---\n")
    plugin = tmp_path / "plugin"
    plugin.mkdir()
    monkeypatch.setattr(plugins, "installed", lambda: [{"origin": "plugin:fixture", "root": str(plugin), "manifest": {"provides": {}, "work_steps": [{"step": "/external-checkup", "scope": ["plugin"]}]}}])
    row = next(row for row in steps.declared() if row["step"] == "/external-checkup")
    assert row["path"] is None and "plugin source unavailable" in row["source_error"]
