"""Model routing with a privacy guard.

`complete()` picks the first route in config/models.yaml that may see the
data, calls it, and falls back to a deterministic answer when no model is
reachable. Rules:
- personal or business data goes only to routes with `private_data: true`;
- plugin policies (plugins.effective_policy) can narrow the allowed routes;
- text sent to a non-private route is also scanned for sensitive terms.
"""

from __future__ import annotations

import json
import os
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

from . import events, paths, privacy, store

Transport = Callable[[dict[str, Any], str], str]


@dataclass
class Route:
    name: str
    provider: str
    model: str
    private_data: bool
    base_url: str = ""
    api_key_env: str = ""


def routes() -> list[Route]:
    cfg = store.read_yaml(paths.config_dir() / "models.yaml", {}) or {}
    out = []
    for r in cfg.get("routes", []):
        out.append(
            Route(
                name=r["name"],
                provider=r.get("provider", ""),
                model=str(r.get("model", "")),
                private_data=bool(r.get("private_data", False)),
                base_url=os.path.expandvars(r.get("base_url", "")),
                api_key_env=r.get("api_key_env", ""),
            )
        )
    return out


def _on_this_machine(r: Route) -> bool:
    host = urllib.parse.urlparse(r.base_url).hostname or ""
    return host in ("localhost", "127.0.0.1", "::1")


def allowed_routes(sensitivity: str) -> list[Route]:
    if sensitivity not in privacy.SENSITIVITIES:
        raise ValueError(f"unknown sensitivity {sensitivity!r}; use one of {privacy.SENSITIVITIES}")
    from . import machine, plugins

    policy = plugins.effective_policy()
    corporate = machine.role() == "corporate"
    out = []
    for r in routes():
        if sensitivity != privacy.PUBLIC and not r.private_data:
            continue
        if policy.get("allowed_routes") is not None and r.name not in policy["allowed_routes"]:
            continue
        if corporate and sensitivity != privacy.PUBLIC and not _on_this_machine(r):
            continue  # corporate business data goes to no hosted model, whatever private_data says
        out.append(r)
    return out


def _http_transport(payload: dict[str, Any], url: str, timeout: float = 60) -> str:
    headers = {"Content-Type": "application/json"}
    key_env = payload.pop("_api_key_env", "")
    if key_env and os.environ.get(key_env):
        headers["Authorization"] = f"Bearer {os.environ[key_env]}"
    req = urllib.request.Request(url, data=json.dumps(payload).encode(), headers=headers)
    with urllib.request.urlopen(req, timeout=timeout) as resp:  # noqa: S310 (configured URL)
        body = json.loads(resp.read())
    return body["choices"][0]["message"]["content"]


def complete(
    prompt: str,
    sensitivity: str,
    fallback: Callable[[], str],
    transport: Transport | None = None,
) -> tuple[str, str]:
    """Return (text, route name). Route "fallback" means no model answered."""
    if os.environ.get("CAREERBREW_LLM", "on") == "off":
        return fallback(), "fallback"
    transport = transport or _http_transport
    candidates = allowed_routes(sensitivity)
    for r in candidates:
        if not r.private_data:
            privacy.assert_clean(prompt, f"route:{r.name}")
        payload: dict[str, Any] = {
            "model": r.model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0,
            "_api_key_env": r.api_key_env,
        }
        try:
            text = transport(payload, r.base_url.rstrip("/") + "/chat/completions")
        except (urllib.error.URLError, OSError, KeyError, ValueError, TimeoutError):
            events.emit("llm.unavailable", route=r.name)
            continue
        events.emit("llm.call", route=r.name)
        return text, r.name
    return fallback(), "fallback"
