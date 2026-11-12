"""`careerbrew plugin new <company>`: a corporate plugin for a new job in one command.

It writes a ready plugin folder (outside this repo, default
`~/careerbrew-plugins/careerbrew-<company>/`), initializes git there, installs
it, and then walks through connecting each chosen work tool:

  careerbrew plugin new acme --connect jira,github,slack --jira-url https://jira.acme.example.com

What it writes:
- careerbrew-plugin.yaml with safe policy defaults (local model only, redacted
  export to personal machine, agents may not read business data, public
  metrics off on the corporate role and on elsewhere);
- ladders/<company>-eng.yaml copied from the generic ladder, to edit with the
  company's real criteria (the promo-research skill helps);
- sensitive-terms.txt seeded with the company name;
- evidence.yaml with one connector per tool, each token a Keychain reference
  `keychain:careerbrew.p-<hash>.<tool>` (no token in any file, no company
  name in the id, so printed setup commands survive redaction);
- README.md with next steps.
Tokens are asked with a hidden prompt only on a real terminal; otherwise the
exact `careerbrew connect` commands are printed for the user to run.
"""

from __future__ import annotations

import getpass
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

import yaml

from . import __version__, connectors, paths, plugins, secrets, store, vault

# Hosts the built-in connectors, mail, and calendars need; a company plugin can narrow them later.
WORK_HOSTS = {
    "api.github.com",
    "slack.com",
    "www.googleapis.com",
    "api.zoom.us",
    "graph.microsoft.com",
    "login.microsoftonline.com",  # Outlook sign-in and token refresh (issue #199)
    "outlook.office365.com",
    "api.fastmail.com",
    "fastmail.com",
    "calendar.google.com",
    "icloud.com",
    "proton.me",
}

# Personal mail and public code hosts: a plugin made on a corporate machine never allows them (issue #10).
PERSONAL_HOSTS = {"api.github.com", "api.fastmail.com", "fastmail.com", "icloud.com", "proton.me"}

NEEDS = {
    "jira": ("base_url", "email"),
    "github": ("user",),
    "slack": (),
    "gdocs": (),
    "zoom": (),
    "outlook": (),
}


def slug(company: str) -> str:
    s = re.sub(r"[^a-z0-9]+", "-", company.lower()).strip("-")
    if not re.match(r"^[a-z][a-z0-9-]{1,39}$", s):
        raise plugins.PluginError(f"cannot make a plugin name from {company!r}; use letters, digits, and dashes")
    return s


def service_prefix(name: str) -> str:
    """Keychain ids avoid the company name (a sensitive term) so the printed setup commands stay copyable."""
    return f"careerbrew.p-{store.digest(name)[:6]}"


def default_dir(name: str) -> Path:
    return Path.home() / "careerbrew-plugins" / f"careerbrew-{name}"


# On the corporate role, a plugin allows only the hosts its chosen tools need (issue #10).
TOOL_HOSTS = {"slack": {"slack.com"}, "gdocs": {"www.googleapis.com"}, "zoom": {"api.zoom.us"}, "outlook": {"graph.microsoft.com", "login.microsoftonline.com"}}


def work_hosts(role: str, tools: list[str]) -> set[str]:
    if role == "corporate":
        return set().union(*(TOOL_HOSTS.get(t, set()) for t in tools))
    return set(WORK_HOSTS)


def scaffold(company: str, dest: Path | None, tools: list[str], options: dict[str, str], role: str = "personal") -> Path:
    name = slug(company)
    dest = (dest or default_dir(name)).expanduser()
    if vault.sealable(dest / "careerbrew-plugin.yaml"):
        raise plugins.PluginError("plugin source cannot be in a protected data root folder (fix: choose a folder outside the data root)")
    if dest.exists() and any(dest.iterdir()):
        raise plugins.PluginError(f"{dest} is not empty (fix: choose --dir or remove it)")
    bad = [t for t in tools if t not in connectors.KINDS]
    if bad:
        raise plugins.PluginError(f"unknown tools {bad}; choose from {list(connectors.KINDS)}")
    (dest / "ladders").mkdir(parents=True)
    generic = store.read_yaml(paths.REPO_ROOT / "knowledge" / "ladders" / "generic-swe.yaml", {})
    ladder = {**generic, "id": f"{name}-eng", "name": f"{company} engineering ladder (edit me with the real criteria)"}
    (dest / "ladders" / f"{name}-eng.yaml").write_text(yaml.safe_dump(ladder, sort_keys=False), encoding="utf-8")
    (dest / "sensitive-terms.txt").write_text(f"# Names that must never leave careerbrew: codenames, customers, people.\n{company}\n", encoding="utf-8")
    # Shared: where the company's tools live (connector_defaults). Per person: who you are on them (email,
    # login, token), which goes to each user's private connectors, never into the plugin (issue #48).
    defaults: dict[str, dict[str, str]] = {}
    for t in tools:
        if t == "jira":
            defaults["jira"] = {"base_url": options.get("jira_url", f"https://jira.{name}.example.com")}
        if t == "github" and options.get("github_api"):
            defaults["github"] = {"api": options["github_api"]}
    (dest / "evidence.yaml").write_text("# Shared evidence sources only. Per-person connectors are generated by `careerbrew machine new-user`.\nsources: []\n", encoding="utf-8")
    hosts = sorted({re.sub(r"^https?://([^/]+).*$", r"\1", v.get("base_url") or v.get("api") or "") for v in defaults.values()} - {""})
    if any(h.endswith(".atlassian.net") for h in hosts):
        hosts.append("api.atlassian.com")  # scoped Atlassian tokens work only through this gateway
    manifest = {
        "name": name,
        "version": "0.1.0",
        "requires": f">={__version__}",
        "description": f"{company}: ladder, connectors, sensitive terms, and data policy.",
        "provides": {"ladders": "ladders/*.yaml", "evidence_sources": "evidence.yaml"},
        "sensitive_terms": "sensitive-terms.txt",
        "settings_defaults": {"ladder": f"{name}-eng"},
        "connector_defaults": defaults,
        "policy": {
            "allowed_routes": ["local"],
            "allowed_connectors": tools or [],
            **({"allowed_hosts": sorted(set(hosts) | work_hosts(role, tools))} if hosts or role == "corporate" else {}),
            "export_to_personal": "none",
            "agent_may_read_business": False,
            "public_metrics": role != "corporate",  # an employer's numbers go public only when its policy allows
            "require_encrypted_backup": True,
        },
    }
    (dest / "careerbrew-plugin.yaml").write_text(yaml.safe_dump(manifest, sort_keys=False), encoding="utf-8")
    (dest / "README.md").write_text(
        f"# careerbrew-{name}\n\nPrivate careerbrew plugin for {company}. Keep this repo private.\n\n"
        f"1. Edit `ladders/{name}-eng.yaml` with the real promotion criteria (skill: promo-research).\n"
        "2. Add codenames, customers, and people to `sensitive-terms.txt`.\n"
        "3. Reinstall after edits: `careerbrew plugin add <this folder>`.\n",
        encoding="utf-8",
    )
    subprocess.run(["git", "init", "-q", "-b", "main", str(dest)], check=False, capture_output=True)
    return dest


def user_connectors(plugin: str, tools: list[str], me: dict[str, str]) -> list[dict[str, Any]]:
    """One person's connectors: the plugin's shared defaults plus who they are (email, login) and their token ref."""
    out = []
    for t in tools:
        c: dict[str, Any] = {"kind": t, "token": f"keychain:{service_prefix(plugin)}.{t}", **{k: v for k, v in connectors.plugin_defaults(t).items() if k != "token_page"}}
        if t == "jira" and me.get("email"):
            c["email"] = me["email"]
        if t == "github" and me.get("user"):
            c["user"] = me["user"]
        out.append(c)
    return out


def write_user_connectors(conns: list[dict[str, Any]]) -> None:
    """Merge into this person's `work/connectors.yaml` (private, encrypted); an existing kind is replaced."""
    path = paths.sub("work", "connectors.yaml")
    data = store.read_yaml(path, {"connectors": []}) or {"connectors": []}
    kinds = {c["kind"] for c in conns}
    data["connectors"] = [c for c in data.get("connectors", []) if c.get("kind") not in kinds] + conns
    store.write_yaml(path, data)


def connect_prompts(name: str, tools: list[str], interactive: bool) -> list[str]:
    """Store each tool's token in the Keychain (hidden prompt) or return the commands to run."""
    todo = []
    for t in tools:
        service = f"{service_prefix(name)}.{t}"
        if interactive:
            token = getpass.getpass(f"{t} token for {name} (input hidden, Enter to skip): ")
            if token:
                secrets.store_keychain(service, token)
                continue
        todo.append(f"careerbrew secret set {service}")
    return todo


def new(company: str, dest: Path | None = None, tools: list[str] | None = None, options: dict[str, str] | None = None, prompt: bool = True) -> dict[str, Any]:
    tools = tools or []
    from . import machine

    folder = scaffold(company, dest, tools, options or {}, role=machine.role())
    entry = plugins.add(str(folder))
    write_user_connectors(user_connectors(entry["name"], tools, {"email": (options or {}).get("email", ""), "user": (options or {}).get("github_user", "")}))
    todo = connect_prompts(entry["name"], tools, interactive=prompt and sys.stdin.isatty())
    return {"folder": folder, "plugin": entry, "todo": todo}
