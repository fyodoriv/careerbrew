"""MCP connector preflight (#312), plus an explicitly approved GitHub repair.

Preflight is read-only. Only fix_github_headers writes agent settings after --yes.

On macOS the Claude desktop app reads env vars and PATH from launchd, not from the shell, so a shell-side probe can
call a server healthy while the app cannot reach it. Per server in the agent config (`~/.claude.json`, or
`CAREERBREW_CLAUDE_CONFIG`), plus native TOML (`CAREERBREW_CODEX_CONFIG`):

- an http header or url `${VAR}` whose launchd value is empty fails (the app sends `Bearer `);
- a localhost url whose port does not answer fails (an IDE server that is not running);
- a stdio command that does not resolve on the launchd PATH fails.

Values are tested for emptiness. A supported GitHub connector also compares its
launchd credential with the GitHub CLI credential; neither value is returned or printed.
"""

from __future__ import annotations

import contextlib
import fcntl
import hmac
import json
import os
import re
import selectors
import shlex
import shutil
import socket
import stat
import subprocess
import sys
import tempfile
import time
import tomllib
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

import yaml

VAR = re.compile(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(?::-[^}]*)?\}")
LOCAL_HOSTS = {"localhost", "127.0.0.1", "::1"}
DEFAULT_LAUNCHD_PATH = "/usr/bin:/bin:/usr/sbin:/sbin"
GITHUB_TOKEN_MAX_BYTES = 4096
GITHUB_TOKEN_TIMEOUT = 8.0


def config_path() -> Path:
    return Path(os.environ.get("CAREERBREW_CLAUDE_CONFIG") or Path.home() / ".claude.json").expanduser()


def servers(path: Path | None = None, *, strict: bool = False, contents: str | None = None) -> dict[str, dict[str, Any]]:
    """Read one host's config; strict preflight callers distinguish invalid from absent."""
    try:
        selected = path or config_path()
        text = contents if contents is not None else selected.read_text()
        data = tomllib.loads(text) if selected.suffix == ".toml" else json.loads(text)
        if not isinstance(data, dict):
            raise ValueError("MCP config is not an object")
        found = data.get("mcp_servers" if selected.suffix == ".toml" else "mcpServers")
        if found is not None and not isinstance(found, dict):
            raise ValueError("MCP server map is not an object")
        if strict:
            for entry in (found or {}).values():
                if not isinstance(entry, dict):
                    raise ValueError("MCP server entry is not an object")
                if entry.get("enabled") is False:
                    continue
                if "enabled" in entry and not isinstance(entry["enabled"], bool):
                    raise ValueError("MCP enabled flag is not a boolean")
                for field in ("headers", "http_headers", "env_http_headers"):
                    value = entry.get(field)
                    if value is not None and (not isinstance(value, dict) or not all(isinstance(k, str) and isinstance(v, str) for k, v in value.items())):
                        raise ValueError("MCP header map must contain string keys and values")
        return {str(k): v for k, v in (found or {}).items() if isinstance(v, dict) and v.get("enabled") is not False}
    except (OSError, ValueError):
        if strict:
            raise
        return {}


def _launchctl(var: str) -> str | None:
    """The launchd value, or None when its probe fails. A successful unset probe reads as ''."""
    try:
        done = subprocess.run(["launchctl", "getenv", var], capture_output=True, text=True, timeout=5, stdin=subprocess.DEVNULL, check=False)  # noqa: S603, S607
    except (OSError, subprocess.TimeoutExpired):
        return None
    return done.stdout.strip() if done.returncode == 0 else None


def _port_open(host: str, port: int) -> bool:
    try:
        with socket.create_connection((host, port), timeout=1):
            return True
    except OSError:
        return False


def _github_cli_token() -> str | None:
    gh = shutil.which("gh")
    if not gh:
        return None
    env = {key: value for key, value in os.environ.items() if key not in {"GH_TOKEN", "GITHUB_TOKEN"}}
    try:
        proc = subprocess.Popen([gh, "auth", "token", "--hostname", "github.com"], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL, env=env)  # noqa: S603
    except OSError:
        return None
    try:
        assert proc.stdout is not None
        deadline = time.monotonic() + GITHUB_TOKEN_TIMEOUT
        data = bytearray()
        with selectors.DefaultSelector() as selector:
            selector.register(proc.stdout, selectors.EVENT_READ)
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not selector.select(remaining):
                    return None
                chunk = os.read(proc.stdout.fileno(), GITHUB_TOKEN_MAX_BYTES + 1 - len(data))
                if not chunk:
                    break
                data.extend(chunk)
                if len(data) > GITHUB_TOKEN_MAX_BYTES:
                    return None
        if proc.wait(timeout=max(0, deadline - time.monotonic())) != 0:
            return None
        token = data.decode("utf-8").strip()
        return token if token and not any(c.isspace() or ord(c) < 32 for c in token) else None
    except (OSError, UnicodeError, subprocess.TimeoutExpired):
        return None
    finally:
        if proc.poll() is None:
            with contextlib.suppress(ProcessLookupError):
                proc.kill()
        proc.wait()
        if proc.stdout is not None:
            proc.stdout.close()


def _vars_in(server: dict[str, Any]) -> list[str]:
    texts = [str(server.get("url") or "")] + [str(v) for key in ("headers", "http_headers") for v in (server.get(key) or {}).values()]
    names = {m for t in texts for m in VAR.findall(t)}
    references = [*(server.get("env_http_headers") or {}).values(), server.get("bearer_token_env_var")]
    names.update(value for value in references if isinstance(value, str) and value)
    return sorted(names)


def check(server_map: dict[str, dict[str, Any]] | None = None, getenv=_launchctl, port_open=_port_open, which=shutil.which, *, repair_host: str = "") -> list[dict[str, Any]]:
    """One preflight row per failing server (`ok` false, `safe` names the fix); one ok row when all pass."""
    if server_map is None:
        if sys.platform != "darwin":
            return []
        native = Path(os.environ.get("CAREERBREW_CODEX_CONFIG") or Path.home() / ".codex" / "config.toml").expanduser()
        scoped = []
        for host, cfg in [("claude", config_path()), ("codex", native)]:
            try:
                if not cfg.exists() and not cfg.is_symlink():
                    continue
                entries = {f"{host}:{name}": entry for name, entry in servers(cfg, strict=True).items()}
                checked = check(entries, getenv=getenv, port_open=port_open, which=which, repair_host=host)
            except (OSError, ValueError):
                scoped.append({"check": f"mcp {host} config", "value": "config unreadable or invalid", "ok": False, "safe": f"repair the {host} MCP config syntax/readability, then retry; config values are not shown"})
                continue
            for row in checked:
                if row["check"] == "mcp":
                    row["check"] = f"mcp {host}"
                scoped.append(row)
        return scoped
    if not server_map:
        return []
    rows: list[dict[str, Any]] = []
    launchd_path = getenv("PATH") or DEFAULT_LAUNCHD_PATH
    for name, server in sorted(server_map.items()):
        empty = []
        values = {}
        for var in _vars_in(server):
            value = getenv(var)
            values[var] = value
            if value is not None and value == "":
                empty.append(var)
        if empty:
            names = ", ".join(empty)
            safe = f"the desktop app sees {names} empty: `launchctl setenv {empty[0]} <value>`, then restart the app"
            if repair_host == "claude" and empty == ["GITHUB_TOKEN"] and _github_auth_header(server):
                try:
                    _check_source_ownership(name.removeprefix("claude:"))
                except ValueError as exc:
                    safe = str(exc)
                else:
                    safe = "after the owner approves changing Claude connector settings, run careerbrew day start --fix --yes; reconnect with the host's supported tool and recheck; no token belongs in chat"
            rows.append({"check": f"mcp {name}", "value": "empty launchd var", "ok": False, "safe": safe})
            continue
        launchd_token = values.get("GITHUB_TOKEN")
        if repair_host == "claude" and launchd_token and _github_auth_header(server):
            github_token = _github_cli_token()
            if github_token and not hmac.compare_digest(launchd_token.encode(), github_token.encode()):
                rows.append({"check": f"mcp {name}", "value": "GitHub credential mismatch", "ok": False, "safe": "the desktop app and GitHub CLI use different credentials; reconnect the GitHub connector and recheck"})
                continue
        url = str(server.get("url") or "")
        if url:
            parsed = urlparse(url)
            if parsed.hostname in LOCAL_HOSTS:
                port = parsed.port or (443 if parsed.scheme == "https" else 80)
                if not port_open(parsed.hostname or "127.0.0.1", port):
                    rows.append({"check": f"mcp {name}", "value": f"port {port} closed", "ok": False, "safe": f"nothing answers on localhost:{port}: start the app that serves it (an IDE), or remove the server"})
            continue
        command = str(server.get("command") or "")
        if command and not (os.path.isabs(command) and os.access(command, os.X_OK)) and not which(command, path=launchd_path):
            rows.append(
                {"check": f"mcp {name}", "value": "command not on launchd PATH", "ok": False, "safe": f"the desktop app cannot find `{command}`: use its full path in the config, or `launchctl setenv PATH ...`, then restart the app"}
            )
    if not rows:
        rows.append({"check": "mcp", "value": f"{len(server_map)} servers", "ok": True, "safe": "every MCP server resolves in the desktop app's launchd env"})
    return rows


def _github_auth_header(server: dict[str, Any]) -> str | None:
    try:
        url = urlparse(str(server.get("url") or ""))
        port = url.port
    except ValueError:
        return None
    if url.scheme != "https" or url.hostname not in {"github.com", "api.github.com", "api.githubcopilot.com"} or url.username or url.password or port not in (None, 443):
        return None
    if server.get("type", "http") not in ("http", "sse") or "headersHelper" in server:
        return None
    headers = server.get("headers") or {}
    if not isinstance(headers, dict):
        return None
    names = [k for k in headers if isinstance(k, str) and k.casefold() == "authorization"]
    if len(names) != 1:
        return None
    key = names[0]
    value = headers[key]
    return key if isinstance(value, str) and re.fullmatch(r"(?i:Bearer)\s+\$\{GITHUB_TOKEN\}", value) else None


def _check_source_ownership(name: str) -> None:
    """Respect the agentbrew registry; it owns generated connector settings."""
    path = Path.home() / ".config/agentbrew/state.yaml"
    try:
        if not path.exists() and not path.is_symlink():
            return
        state = yaml.safe_load(path.read_text())
        if not isinstance(state, dict):
            raise ValueError
        registered = state.get("mcpServers", [])
        if not isinstance(registered, list) or any(not isinstance(item, dict) or not isinstance(item.get("name"), str) for item in registered):
            raise ValueError
    except (OSError, UnicodeError, ValueError, yaml.YAMLError):
        raise ValueError("connector source ownership is unreadable; repair the agentbrew source registry and retry") from None
    if any(item["name"] == name for item in registered):
        raise ValueError("agentbrew owns this connector: change its registered source, then run agentbrew sync; do not edit the generated Claude settings")


def _github_header_helper(gh: str) -> str:
    """Client-owned connection-time command; never executed by the repair itself."""
    code = (
        "import json, subprocess, sys\n"
        "try:\n"
        f" r = subprocess.run({[gh, 'auth', 'token', '--hostname', 'github.com']!r}, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL, text=True, timeout=8)\n"
        "except (OSError, subprocess.TimeoutExpired, UnicodeError):\n sys.exit(1)\n"
        "token = r.stdout.strip()\n"
        "if r.returncode or not token or any(c.isspace() or ord(c) < 32 for c in token):\n sys.exit(1)\n"
        "print(json.dumps({'Authorization': 'Bearer ' + token}))\n"
    )
    return f"{shlex.quote(str(Path(sys.executable).resolve()))} -c {shlex.quote(code)}"


def fix_github_headers(*, yes: bool = False, path: Path | None = None, getenv: Any = None) -> dict[str, Any]:
    """Repair only empty launchd GitHub auth after owner approval.

    Lifecycle: the configured Claude JSON file owns the helper. Its resolved target
    has a repair-file lock, released by the OS on exit; keep the empty lock file
    for future writers. Temporary files stay next to that target and are removed
    on failure. The repair never invokes credential lookup or returns setting values; it preserves existing settings and stores only a command for future GitHub credentials. Other
    clients, endpoints, existing helpers and unset/unknown launchd probes stay as-is.
    """
    if not yes:
        raise ValueError("--fix changes Claude connector settings; use --yes only after the owner approves this repair")
    requested = path or config_path()
    getenv = getenv or _launchctl
    temporary = None
    try:
        target = requested.resolve(strict=True)
        with target.with_name(f".{target.name}.careerbrew-mcp.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            if requested.resolve(strict=True) != target:
                raise ValueError("connector settings changed during repair; retry after the other writer finishes")
            original = target.read_bytes()
            before = target.stat()
            data = json.loads(original)
            configured = servers(requested, strict=True, contents=original.decode("utf-8"))
            changed = 0
            repaired: list[str] = []
            for name, server in configured.items():
                auth = _github_auth_header(server)
                if not auth or getenv("GITHUB_TOKEN") != "":
                    continue
                _check_source_ownership(name)
                headers = server.get("headers") or {}
                gh = shutil.which("gh")
                if not gh:
                    raise ValueError("install the GitHub CLI and sign in before approving a connector repair")
                entry = data["mcpServers"][name]
                entry["headers"] = {k: v for k, v in headers.items() if k != auth}
                entry["headersHelper"] = _github_header_helper(str(Path(gh).absolute()))
                changed += 1
                repaired.append(name)
            if changed:
                with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=target.parent, prefix=".careerbrew-mcp-", suffix=".tmp", delete=False) as stream:
                    temporary = Path(stream.name)
                    stream.write(json.dumps(data, indent=2) + "\n")
                    stream.flush()
                    os.fsync(stream.fileno())
                os.chmod(temporary, stat.S_IMODE(before.st_mode))
                latest = target.stat()
                if requested.resolve(strict=True) != target or target.read_bytes() != original or (latest.st_ino, latest.st_mode, latest.st_mtime_ns) != (before.st_ino, before.st_mode, before.st_mtime_ns):
                    raise ValueError("connector settings changed during repair; retry after the other writer finishes")
                for name in repaired:
                    _check_source_ownership(name)
                os.replace(temporary, target)
            return {
                "changed": changed,
                "status": "configured" if changed else "unchanged",
                "verification": "configuration only; live authentication is not checked",
                "next": "reconnect the GitHub connector with the host's supported tool, then run careerbrew day start" if changed else "no automatic change; rerun careerbrew day start and keep any remaining recovery question",
            }
    except (OSError, UnicodeError, json.JSONDecodeError):
        raise ValueError("Claude connector settings are unavailable or malformed; no repair was completed") from None
    finally:
        if temporary is not None:
            with contextlib.suppress(OSError):
                temporary.unlink(missing_ok=True)
