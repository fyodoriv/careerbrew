"""rust-07c: installed manifests are re-checked on load, and MCP preflight rows come from supplied facts."""

from __future__ import annotations

import json
from typing import Any

import pytest

from careerbrew import plugins, store
from careerbrew.workday import mcp_check


def _plant(name: str, manifest: dict[str, Any]) -> None:
    root = plugins.plugins_dir() / name
    root.mkdir(parents=True, exist_ok=True)
    store.write_yaml(root / plugins.MANIFEST, manifest)
    lock = {"plugins": {name: {"name": name, "version": "0.1.0", "origin": f"plugin:{name}", "source": "local-folder", "tree_sha256": "", "exec_allowed": False}}}
    store.write_json(plugins.lock_path(), lock)


def test_a_malformed_installed_manifest_is_refused_on_load():
    _plant("synthetic-co", {"name": "synthetic-co", "version": "0.1.0", "provides": {"ladders": "missing-folder"}})
    with pytest.raises(plugins.PluginError, match=r"installed plugin synthetic-co has an invalid manifest: .*missing path"):
        plugins.installed()


def test_a_bad_installed_name_is_refused_and_remove_still_works():
    _plant("synthetic-co", {"name": "Not A Name", "version": "0.1.0"})
    with pytest.raises(plugins.PluginError, match="invalid manifest"):
        plugins.installed()
    plugins.remove("synthetic-co")
    assert plugins.installed() == []


def test_a_valid_installed_manifest_still_loads():
    _plant("synthetic-co", {"name": "synthetic-co", "version": "0.1.0"})
    assert [p["name"] for p in plugins.installed()] == ["synthetic-co"]


SERVERS: dict[str, dict[str, Any]] = {
    "a-empty": {"url": "https://mcp.example.com/x", "headers": {"X-Key": "${SYNTH_KEY}"}},
    "b-port": {"url": "http://localhost:47001/mcp"},
    "c-cmd": {"command": "synthetic-missing-tool"},
    "d-ok": {"command": "synthetic-present-tool"},
}


def _probes() -> tuple[Any, Any, Any]:
    env = {"SYNTH_KEY": "", "PATH": "/usr/bin"}
    return (env.get, lambda host, port: False, lambda cmd, path=None: "/usr/bin/x" if cmd == "synthetic-present-tool" else None)


def test_check_rows_from_recorded_facts_match_the_probing_check():
    getenv, port_open, which = _probes()
    probed = mcp_check.check(SERVERS, getenv=getenv, port_open=port_open, which=which)
    recorded = json.loads(json.dumps(mcp_check.check_facts(SERVERS, getenv, port_open, which)))
    assert mcp_check.check_rows(SERVERS, recorded) == probed
    assert [r["value"] for r in probed] == ["empty launchd var", "port 47001 closed", "command not on launchd PATH"]


def test_check_rows_reads_nothing_but_its_facts(monkeypatch: pytest.MonkeyPatch):
    def boom(*_a: Any, **_k: Any) -> Any:
        raise AssertionError("check_rows probed the machine")

    for target in ("_launchctl", "_port_open", "_github_cli_token", "_check_source_ownership"):
        monkeypatch.setattr(mcp_check, target, boom)
    monkeypatch.setattr(mcp_check.shutil, "which", boom)
    facts = {"servers": {"a-empty": {"vars": {"SYNTH_KEY": ""}}, "b-port": {"vars": {}, "port_open": True}, "c-cmd": {"vars": {}, "command_found": True}, "d-ok": {"vars": {}, "command_found": True}}}
    rows = mcp_check.check_rows(SERVERS, facts)
    assert [r["check"] for r in rows] == ["mcp a-empty"]


def test_check_rows_all_ok_gives_one_ok_row():
    facts = {"servers": {"d-ok": {"vars": {}, "command_found": True}}}
    assert mcp_check.check_rows({"d-ok": SERVERS["d-ok"]}, facts) == [{"check": "mcp", "value": "1 servers", "ok": True, "safe": "every MCP server resolves in the desktop app's launchd env"}]
