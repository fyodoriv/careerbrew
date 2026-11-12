"""Issue #312: the start of the work day checks MCP connectors against the desktop app's launchd env."""

import json
from pathlib import Path

import pytest

from careerbrew.workday import mcp_check, start

SECRET = "canary-token-value-312"


def _env(values):
    return lambda var: values.get(var, "")


def test_an_empty_launchd_var_in_a_header_fails_with_the_fix():
    rows = mcp_check.check(
        {"github": {"type": "http", "url": "https://api.example.com/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}"}}}, getenv=_env({"PATH": "/usr/bin"}), port_open=lambda h, p: True, which=lambda c, path=None: c
    )
    assert rows == [{"check": "mcp github", "value": "empty launchd var", "ok": False, "safe": "the desktop app sees GITHUB_TOKEN empty: `launchctl setenv GITHUB_TOKEN <value>`, then restart the app"}]


def test_a_set_var_passes_and_its_value_is_never_shown():
    rows = mcp_check.check({"github": {"url": "https://api.example.com/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}"}}}, getenv=_env({"GITHUB_TOKEN": SECRET}), port_open=lambda h, p: True)
    assert [r["ok"] for r in rows] == [True]
    assert SECRET not in json.dumps(rows)


def test_a_dead_localhost_port_fails():
    rows = mcp_check.check({"ide": {"type": "http", "url": "http://127.0.0.1:61234/mcp"}}, getenv=_env({}), port_open=lambda h, p: False)
    assert rows[0]["check"] == "mcp ide" and rows[0]["value"] == "port 61234 closed" and not rows[0]["ok"]


def test_a_stdio_command_missing_on_the_launchd_path_fails():
    seen = {}

    def which(cmd, path=None):
        seen["path"] = path
        return None

    rows = mcp_check.check({"tool": {"command": "uvx", "args": ["x"]}}, getenv=_env({"PATH": "/usr/bin:/bin"}), which=which)
    assert seen["path"] == "/usr/bin:/bin"
    assert rows[0]["value"] == "command not on launchd PATH" and "`uvx`" in rows[0]["safe"]


def test_no_launchctl_means_no_false_alarm():
    rows = mcp_check.check({"github": {"url": "https://api.example.com/mcp", "headers": {"A": "${GITHUB_TOKEN}"}}}, getenv=lambda v: None, which=lambda c, path=None: c)
    assert [r["ok"] for r in rows] == [True]


def test_servers_reads_the_agent_config(tmp_path, monkeypatch):
    cfg = tmp_path / "claude.json"
    cfg.write_text(json.dumps({"mcpServers": {"a": {"command": "x"}, "bad": 3}}))
    monkeypatch.setenv("CAREERBREW_CLAUDE_CONFIG", str(cfg))
    assert mcp_check.servers() == {"a": {"command": "x"}}
    cfg.write_text("{not json")
    assert mcp_check.servers() == {}


def test_preflight_includes_the_mcp_rows(monkeypatch):
    monkeypatch.setattr(mcp_check, "check", lambda: [{"check": "mcp x", "value": "port 1 closed", "ok": False, "safe": "fix"}])
    rows = start.preflight(env={"SHELL": "/bin/bash"}, runner=lambda argv: (1, ""))
    assert {"check": "mcp x", "value": "port 1 closed", "ok": False, "safe": "fix"} in rows


def test_native_toml_uses_enabled_servers_and_header_names(tmp_path):
    path = tmp_path / "config.toml"
    path.write_text('[mcp_servers.on]\ncommand="uvx"\n[mcp_servers.on.http_headers]\nX-Test="${SAFE_TEST_VAR}"\n[mcp_servers.off]\nenabled=false\ncommand="missing"\n')
    assert mcp_check.servers(path) == {"on": {"command": "uvx", "http_headers": {"X-Test": "${SAFE_TEST_VAR}"}}}
    rows = mcp_check.check(mcp_check.servers(path), getenv=_env({"PATH": "/usr/bin"}), which=lambda c, path=None: c)
    assert rows[0]["value"] == "empty launchd var"


def test_default_preflight_labels_both_host_configs(tmp_path, monkeypatch):
    claude, native = tmp_path / "claude.json", tmp_path / "config.toml"
    claude.write_text('{"mcpServers":{"fixture":{"command":"safe"}}}')
    native.write_text('[mcp_servers.fixture]\ncommand="safe"\n')
    monkeypatch.setenv("CAREERBREW_CLAUDE_CONFIG", str(claude))
    monkeypatch.setenv("CAREERBREW_CODEX_CONFIG", str(native))
    monkeypatch.setattr(mcp_check.sys, "platform", "darwin")
    rows = mcp_check.check(getenv=_env({}), which=lambda c, path=None: None)
    assert {r["check"] for r in rows} == {"mcp claude:fixture", "mcp codex:fixture"}


def test_native_header_environment_references_are_checked_without_values():
    rows = mcp_check.check(
        {"native": {"url": "https://api.example.com/mcp", "bearer_token_env_var": "FIXTURE_BEARER", "env_http_headers": {"X-Token": "FIXTURE_HEADER"}}},
        getenv=_env({}),
    )
    assert rows[0]["value"] == "empty launchd var"
    assert "FIXTURE_BEARER" in rows[0]["safe"] and "FIXTURE_HEADER" in rows[0]["safe"]


@pytest.mark.parametrize("host", ["claude", "codex"])
@pytest.mark.parametrize("failure", ["malformed", "unreadable"])
def test_present_malformed_native_config_is_a_scoped_failure(tmp_path, monkeypatch, host, failure):
    native = tmp_path / ("config.toml" if host == "codex" else "claude.json")
    native.write_text('[mcp_servers.fixture\nsecret="CANARY-CONFIG-CONTENT"' if host == "codex" else '{"secret":"CANARY-CONFIG-CONTENT"')
    monkeypatch.setenv("CAREERBREW_CODEX_CONFIG" if host == "codex" else "CAREERBREW_CLAUDE_CONFIG", str(native))
    if failure == "unreadable":
        real_read = Path.read_text

        def denied(path, *args, **kwargs):
            if path == native:
                raise PermissionError("CANARY-CONFIG-CONTENT")
            return real_read(path, *args, **kwargs)

        monkeypatch.setattr(Path, "read_text", denied)
    monkeypatch.setattr(mcp_check.sys, "platform", "darwin")
    rows = mcp_check.check(getenv=_env({}), which=lambda c, path=None: c)
    assert any(r["check"] == f"mcp {host} config" and not r["ok"] for r in rows)
    assert "CANARY-CONFIG-CONTENT" not in json.dumps(rows)


SHAPE_CANARY = "CANARY_CONFIG_SHAPE_VALUE"
BAD_ENABLED_CONFIGS = [
    pytest.param(SHAPE_CANARY, id="scalar-entry"),
    *[pytest.param({"command": "safe", field: [SHAPE_CANARY]}, id=f"{field}-container") for field in ("headers", "http_headers", "env_http_headers")],
    *[pytest.param({"command": "safe", field: {"X-Test": [SHAPE_CANARY]}}, id=f"{field}-value") for field in ("headers", "http_headers", "env_http_headers")],
    pytest.param({"url": f"http://[{SHAPE_CANARY}"}, id="url-bracket"),
    pytest.param({"url": f"http://localhost:{SHAPE_CANARY}/mcp"}, id="localhost-port"),
    pytest.param({"command": "safe", "enabled": "false"}, id="enabled-string"),
    pytest.param({"command": "safe", "enabled": 0}, id="enabled-integer"),
]


def _inline_toml(value):
    if isinstance(value, dict):
        return "{" + ", ".join(f"{json.dumps(k)} = {_inline_toml(v)}" for k, v in value.items()) + "}"
    if isinstance(value, list):
        return "[" + ", ".join(_inline_toml(v) for v in value) + "]"
    return json.dumps(value)


def _host_shape_config(tmp_path, monkeypatch, host, entries):
    claude, native = tmp_path / "claude.json", tmp_path / "config.toml"
    monkeypatch.setenv("CAREERBREW_CLAUDE_CONFIG", str(claude))
    monkeypatch.setenv("CAREERBREW_CODEX_CONFIG", str(native))
    monkeypatch.setattr(mcp_check.sys, "platform", "darwin")
    cfg = native if host == "codex" else claude
    cfg.write_text("mcp_servers = " + _inline_toml(entries) + "\n" if host == "codex" else json.dumps({"mcpServers": entries}))
    return cfg


@pytest.mark.parametrize("host", ["claude", "codex"])
@pytest.mark.parametrize("entry", BAD_ENABLED_CONFIGS)
def test_enabled_config_shape_fails_safely(tmp_path, monkeypatch, host, entry):
    _host_shape_config(tmp_path, monkeypatch, host, {"fixture": entry})
    rows = mcp_check.check(getenv=_env({"PATH": "/usr/bin"}), port_open=lambda h, p: True, which=lambda c, path=None: c)
    assert len(rows) == 1 and rows[0]["check"] == f"mcp {host} config" and rows[0]["ok"] is False
    assert rows[0]["value"] == "config unreadable or invalid"
    assert SHAPE_CANARY not in json.dumps(rows)


@pytest.mark.parametrize("host", ["claude", "codex"])
@pytest.mark.parametrize("control", ["valid", "disabled", "permissive"])
def test_host_config_validation_controls(tmp_path, monkeypatch, host, control):
    valid = {"url": "https://api.example.com/mcp", "enabled": True, "headers": {"X-A": "literal"}, "http_headers": {"X-B": "${FIXTURE_TOKEN}"}, "env_http_headers": {"X-C": "FIXTURE_HEADER"}}
    entries = {"fixture": valid}
    if control == "disabled":
        entries = {"fixture": {"enabled": False, "headers": [SHAPE_CANARY], "url": f"http://[{SHAPE_CANARY}"}}
    elif control == "permissive":
        entries["bad"] = SHAPE_CANARY
    cfg = _host_shape_config(tmp_path, monkeypatch, host, entries)
    if control == "permissive":
        assert mcp_check.servers(cfg) == {"fixture": valid}
        return
    rows = mcp_check.check(getenv=_env({"PATH": "/usr/bin", "FIXTURE_TOKEN": SECRET, "FIXTURE_HEADER": SECRET}), port_open=lambda h, p: True, which=lambda c, path=None: c)
    if control == "disabled":
        assert rows == []
    else:
        assert len(rows) == 1 and rows[0]["check"] == f"mcp {host}" and rows[0]["ok"] is True
    assert SHAPE_CANARY not in json.dumps(rows) and SECRET not in json.dumps(rows)
