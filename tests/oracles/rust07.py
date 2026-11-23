"""rust-07 Python oracle for plugin, machine, account and preflight setup.

All state lives below the invocation-owned synthetic CAREERBREW_HOME. Reports
contain typed results, fixed refusal text, modes and counts, never credential
values. The native candidate emits the same schema without starting Python.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import stat
import sys
import types
from pathlib import Path
from typing import Any

from careerbrew import cli, config, connectors, machine, msauth, plugins, store, sync
from careerbrew.workday import mcp_check, self_update, steps, workflow_check

CASES = (
    "manifest-policy",
    "plugin-lifecycle",
    "command-ownership",
    "machine-readiness",
    "oauth-refs",
    "mcp-preflight",
    "managed-repair",
    "update-workflow",
    "cli-handlers",
)


def _typed(value: Any) -> Any:
    if value is None or isinstance(value, bool | str):
        return value
    if isinstance(value, int):
        return {"int": str(value)}
    if isinstance(value, list | tuple):
        return [_typed(item) for item in value]
    if isinstance(value, dict):
        return {str(key): _typed(value[key]) for key in sorted(value, key=str)}
    return {"type": type(value).__name__}


def _call(cases: list[dict[str, Any]], name: str, fn: Any) -> None:
    try:
        value = fn()
    except Exception as exc:
        value = {"error": type(exc).__name__}
    cases.append({"id": name, "result": _typed(value)})


@contextlib.contextmanager
def _patched(module: Any, **attrs: Any):
    old = {name: getattr(module, name) for name in attrs}
    for name, value in attrs.items():
        setattr(module, name, value)
    try:
        yield
    finally:
        for name, value in old.items():
            setattr(module, name, value)


def _step_error(work_steps: Any, label: str) -> str | None:
    try:
        steps.validate(work_steps, label)
    except steps.StepError as exc:
        return str(exc)
    return None


class Workspace:
    def __init__(self, base: Path, name: str):
        self.base = base / name
        self.root = self.base / "data"
        self.home = self.base / "home"
        self.key = self.base / "keys" / "backup.key"
        self.pair = self.base / "keys" / "pair.key"
        self.root.mkdir(parents=True)
        self.home.mkdir(parents=True)
        os.environ.update(
            {
                "HOME": str(self.home),
                "CAREERBREW_HOME": str(self.root),
                "CAREERBREW_KEY_FILE": str(self.key),
                "CAREERBREW_PAIR_KEY_FILE": str(self.pair),
                "CAREERBREW_EVENTS": "off",
                "CAREERBREW_ENCRYPT": "on",
                "CAREERBREW_NOW": "2026-03-01T09:30:00+00:00",
                "CAREERBREW_COMMANDS_DIR": str(self.base / "legacy-commands"),
                "TZ": "UTC",
            }
        )


def _manifest(name: str = "acme", **changes: Any) -> dict[str, Any]:
    data: dict[str, Any] = {
        "name": name,
        "version": "1.0.0",
        "requires": ">=0.1.0",
        "provides": {"commands": "commands/"},
        "policy": {"allowed_routes": ["local"], "require_encrypted_backup": True},
        "work_steps": [{"step": "/review", "uses": ["browser:sso"], "timeout": 5}],
    }
    data.update(changes)
    return data


def _facts(*, inside: bool = True, exists: bool = True) -> dict[str, Any]:
    return {
        "symlinks": [],
        "provides": {"commands/": {"inside": inside, "exists": exists}},
        "live": {},
        "terms_inside": True,
    }


def _manifest_policy(_base: Path) -> dict[str, Any]:
    provider = Workspace(_base, "manifest-provider")
    try:
        plugins.validate_manifest(_manifest(requires=">=1.0.0"), provider.root)
    except Exception as exc:
        real_version_refusal = type(exc).__name__
    else:
        real_version_refusal = "accepted"
    valid = plugins.validate_manifest_data(_manifest(), _facts(), "1.0.0", _step_error)
    refusals = {}
    variants = {
        "mapping": [],
        "name": _manifest(name="Bad"),
        "version": _manifest(version=None),
        "provides-shape": _manifest(provides=[]),
        "provides-missing": _manifest(),
        "policy-list": _manifest(policy={"allowed_hosts": "github.com"}),
        "policy-bool": _manifest(policy={"public_metrics": "no"}),
        "export": _manifest(policy={"export_to_personal": "everything"}),
        "requires": _manifest(requires=">=１.0"),
        "requires-bool": _manifest(requires=True),
        "work-step-field": _manifest(work_steps=[{"step": "/review", "use": ["browser:sso"]}]),
        "detect": _manifest(detect={"env": "TOKEN"}),
        "connector-defaults": _manifest(connector_defaults={"jira": []}),
        "agent-reads": _manifest(agent_reads="slack"),
        "live": _manifest(live={"other": "live/x.yaml"}),
    }
    for name, data in variants.items():
        facts = _facts(exists=name != "provides-missing")
        try:
            plugins.validate_manifest_data(data, facts, "1.0.0", _step_error)
        except Exception as exc:
            refusals[name] = type(exc).__name__
        else:
            refusals[name] = "accepted"
    team = _manifest(
        "team",
        extends="acme",
        policy={
            "allowed_routes": ["local", "hosted"],
            "allowed_hosts": ["example.com"],
            "require_encrypted_backup": False,
            "agent_may_read_business": True,
            "public_metrics": False,
            "export_to_personal": "redacted",
            "backup_targets": ["~/Team"],
        },
    )
    base = _manifest(
        policy={
            "allowed_routes": ["local"],
            "allowed_hosts": ["example.com", "other.example"],
            "agent_may_read_business": True,
            "export_to_personal": "aggregates",
            "backup_targets": ["~/Base", "~/Team"],
        }
    )
    expansions = {"~/Base": str(Path("/") / "home" / "u" / "Base"), "~/Team": str(Path("/") / "home" / "u" / "Team")}
    merged = plugins.merge_policy([base, team], expansions)
    denied_team = {**team, "policy": {**team["policy"], "agent_may_read_business": False}}
    denied = plugins.merge_policy([base, denied_team], expansions)
    lock = {
        "team": {"extends": "acme"},
        "acme": {"extends": None},
        "cycle-a": {"extends": "cycle-b"},
        "cycle-b": {"extends": "cycle-a"},
        "unknown": {"extends": "absent"},
    }
    return {
        "valid": valid["name"],
        "refusals": refusals,
        "merged": merged,
        "denied_agent_read": denied["agent_may_read_business"],
        "real_version_refusal": real_version_refusal,
        "order": plugins.refresh_order(lock),
    }


def _write_plugin(root: Path, name: str = "acme", *, team: bool = False, command: bool = True) -> Path:
    root.mkdir(parents=True)
    provides: dict[str, Any] = {"evidence_sources": "evidence.yaml"}
    if command:
        (root / "commands").mkdir()
        (root / "commands" / f"{name}-check.md").write_text("---\ndescription: synthetic\n---\nRun careerbrew checkup.\n")
        provides["commands"] = "commands/"
    manifest = _manifest(
        name,
        extends="acme" if team else None,
        provides=provides,
        sensitive_terms="terms.txt",
        settings_defaults={"ladder": f"{name}-eng", "target_level": "senior"},
        policy={
            "allowed_routes": ["local"],
            "allowed_connectors": ["jira"],
            "allowed_hosts": ["example.com"],
            "secret_backends": ["keychain"],
            "require_encrypted_backup": True,
            "agent_may_read_business": False,
        },
    )
    if manifest["extends"] is None:
        manifest.pop("extends")
    import yaml

    (root / "careerbrew-plugin.yaml").write_text(yaml.safe_dump(manifest, sort_keys=False))
    (root / "evidence.yaml").write_text("retained: keep\nsources:\n  - {kind: git, path: '~/repo', authors: [owner@example.com]}\n  - {kind: git, path: '/shared/repo'}\nconnectors:\n  - {kind: jira, token: 'keychain:shared'}\n")
    (root / "terms.txt").write_text("synthetic-company\nsynthetic-codename\n")
    return root


def _plugin_lifecycle(base: Path) -> dict[str, Any]:
    w = Workspace(base, "plugin-lifecycle")
    with _patched(plugins, _run_agentbrew=lambda _folder: None):
        source = _write_plugin(w.base / "source")
        added = plugins.add(str(source), allow_exec=True)
        listed = [(p["name"], p["version"], p["exec_allowed"]) for p in plugins.installed()]
        verified = plugins.verify()
        policy = plugins.effective_policy()
        team = _write_plugin(w.base / "team", "team", team=True, command=False)
        plugins.add(str(team))
        order = plugins.refresh_order(plugins._lock()["plugins"])
        try:
            plugins.remove("acme")
        except Exception as exc:
            dependent_refusal = type(exc).__name__
        else:
            dependent_refusal = "accepted"
        installed_manifest = plugins.plugins_dir() / "acme" / "careerbrew-plugin.yaml"
        original = installed_manifest.read_bytes()
        installed_manifest.write_bytes(original + b"# tampered\n")
        tampered = plugins.verify()
        refreshed = plugins.refresh()
        verified_after = plugins.verify()
        store.write_jsonl(w.root / "work" / "events.jsonl", [{"id": "a", "origin": "plugin:acme"}, {"id": "b", "origin": "base"}])
        removed_team = plugins.remove("team")
        removed_base = plugins.remove("acme", purge_data=True)
        purged_left = [row["id"] for row in store.read_jsonl(w.root / "work" / "events.jsonl")]
        return {
            "added": {key: added[key] for key in ("name", "version", "exec_allowed", "origin")},
            "listed": listed,
            "verified": verified,
            "policy": policy,
            "order": order,
            "dependent_refusal": dependent_refusal,
            "tampered": tampered,
            "refreshed": refreshed,
            "verified_after": verified_after,
            "removed": [removed_team, removed_base],
            "purged_left": purged_left,
            "lock_empty": plugins._lock() == {"plugins": {}},
        }


def _command_ownership(base: Path) -> dict[str, Any]:
    out = {}
    w = Workspace(base, "command-lifecycle")
    with _patched(plugins, _run_agentbrew=lambda _folder: None):
        source = _write_plugin(w.base / "source")
        plugins.add(str(source))
        fragment = w.root / "agentbrew" / "Agentfile.yaml"
        import yaml

        declared = yaml.safe_load(fragment.read_text())["commands"]
        legacy = plugins.commands_dir() / "acme-check.md"
        legacy.parent.mkdir(parents=True)
        installed = plugins.plugins_dir() / "acme" / "commands" / legacy.name
        legacy.write_bytes(installed.read_bytes())
        plugins.refresh()
        archives = list((w.root / "command-archive").rglob("acme-check.md"))
        out["lifecycle"] = {
            "selected": len(declared) == 1 and declared[0].endswith("/acme/commands/acme-check.md"),
            "legacy_removed": not legacy.exists(),
            "archived": len(archives) == 1 and store.read_text(archives[0]) == installed.read_text(),
            "archive_sealed": archives[0].read_bytes().startswith(b"CBV1\n"),
            "fragment_mode": f"{stat.S_IMODE(fragment.stat().st_mode):03o}",
        }
        plugins.remove("acme")
        out["lifecycle"]["empty_after_remove"] = yaml.safe_load(fragment.read_text())["commands"] == []
    w = Workspace(base, "command-collision")
    with _patched(plugins, _run_agentbrew=lambda _folder: None):
        source = _write_plugin(w.base / "source")
        legacy = plugins.commands_dir() / "acme-check.md"
        legacy.parent.mkdir(parents=True)
        legacy.write_text("owner edit\n")
        try:
            plugins.add(str(source))
        except Exception as exc:
            out["collision"] = {"error": type(exc).__name__, "kept": legacy.read_text() == "owner edit\n", "installed": bool(plugins.installed())}
    w = Workspace(base, "command-symlink")
    with _patched(plugins, _run_agentbrew=lambda _folder: None):
        source = _write_plugin(w.base / "source")
        (source / "commands" / "alias.md").symlink_to("acme-check.md")
        try:
            plugins.add(str(source))
        except Exception as exc:
            out["symlink"] = {"error": type(exc).__name__, "staging_removed": not (plugins.plugins_dir() / ".staging").exists()}
    w = Workspace(base, "metadata-symlink")
    outside = w.base / "outside"
    outside.mkdir()
    (w.root / "agentbrew").symlink_to(outside, target_is_directory=True)
    installed = plugins.plugins_dir() / "acme"
    installed.mkdir(parents=True)
    (installed / "acme-check.md").write_text("synthetic command\n")
    (installed / "careerbrew-plugin.yaml").write_text("name: acme\nversion: 1.0.0\nprovides: {commands: acme-check.md}\n")
    lock = {"plugins": {"acme": {"commands": ["acme-check.md"]}}}
    with _patched(plugins, _run_agentbrew=lambda _folder: None):
        try:
            plugins._sync_command_sources(lock)
        except Exception as exc:
            out["metadata_symlink"] = {"error": type(exc).__name__, "outside_empty": not any(outside.iterdir())}
    return out


def _readiness_facts() -> dict[str, Any]:
    return {
        "role": "corporate",
        "profile_exists": True,
        "job_hunt_fields": False,
        "profile_name": "Synthetic Owner",
        "profile_employer": "Synthetic Company",
        "personal_data": False,
        "installed": True,
        "verified": True,
        "real_ladder": True,
        "active_origin": "plugin:acme",
        "target_level": "e5",
        "active_levels": ["e4", "e5"],
        "cloud": "",
        "synced_targets": [],
        "shared": [],
        "terms_count": 2,
        "scan_ok": True,
        "files_scanned": 42,
        "scan_files": [],
    }


def _machine_readiness(base: Path) -> dict[str, Any]:
    rows = machine.readiness_rows(_readiness_facts())
    failed = _readiness_facts()
    failed.update(role="personal", personal_data=True, cloud="Dropbox", shared=["plugin:acme:evidence.yaml"], terms_count=1, scan_ok=False, scan_files=["src/x.py"])
    failed_rows = machine.readiness_rows(failed)
    w = Workspace(base, "machine-role")
    machine.set_role("corporate", "work")
    reports = w.root / "out" / "reports"
    reports.mkdir(parents=True)
    for relative in ["out/interview-notes.md", "out/interview-notes.txt", "out/strategy.md", "out/progress.html", "out/other.md", "out/reports/run.html"]:
        (w.root / relative).write_text("synthetic")
    machine.set_role("corporate", "work")
    output_cleanup = {name: (w.root / name).exists() for name in ["out/interview-notes.md", "out/interview-notes.txt", "out/strategy.md", "out/progress.html", "out/other.md", "out/reports/run.html"]}
    tutor = w.root / "tutor"
    tutor.mkdir()
    (tutor / ".DS_Store").write_bytes(b"\0")
    (tutor / ".readiness.jsonl.lock").write_text("")
    machine.set_role("corporate", "work")
    cleared = not tutor.exists()
    tutor.mkdir()
    (tutor / ".readiness.jsonl.123.abc.tmp").write_text("synthetic partial file")
    try:
        machine.set_role("corporate")
    except Exception as exc:
        temp_refusal = type(exc).__name__
    else:
        temp_refusal = "accepted"
    repos = w.base / "repos"
    for rel in ("a/.git", "group/b/.git", "group/deep/x/c/.git", "notrepo"):
        (repos / rel).mkdir(parents=True)
    return {
        "ready_count": [sum(row["ok"] for row in rows), len(rows)],
        "failed": [row["check"] for row in failed_rows if not row["ok"]],
        "guard": machine.guard_decision("jobs", "corporate"),
        "cloud": [machine.cloud_service(parts, {"Desktop"}) for parts in [("Dropbox", "x"), ("Library", "CloudStorage", "OneDrive-Company"), ("Desktop", "x"), ("local", "x")]],
        "per_user": [machine.per_user(item) for item in [{"email": "x"}, {"path": "~/repo"}, {"path": "/shared/repo"}, []]],
        "cleared": cleared,
        "output_cleanup": output_cleanup,
        "temp_refusal": temp_refusal,
        "repos": sorted(path.name for path in machine.find_repos(repos)),
        "work_profile": machine.work_profile("name: Synthetic\ntarget_salary: 1\nemployer: Example\n"),
    }


def _oauth_refs(base: Path) -> dict[str, Any]:
    w = Workspace(base, "oauth")
    polls = [
        {"access_token": "not-reported", "refresh_token": "not-reported"},
        {"error": "authorization_pending"},
        {"error": "slow_down"},
        {"error": "authorization_declined"},
        {"error": "expired_token"},
        {},
    ]
    refreshes = [{"access_token": "not-reported"}, {"error": "invalid_grant"}, {"error": "interaction_required"}, {"error": "invalid_client"}, {}]
    with _patched(config, get=lambda key: {"outlook_client_id": "setting-client", "outlook_tenant": "setting-tenant"}.get(key)), _patched(connectors, plugin_defaults=lambda _kind: {"client_id": "plugin-client", "tenant": "plugin-tenant"}):
        settings = [msauth.settings(), msauth.settings("flag-client", "flag-tenant")]
    calls = []

    def form(_url: str, fields: dict[str, str]) -> dict[str, Any]:
        calls.append(sorted(fields))
        return {"access_token": "not-reported", "refresh_token": "rotated-not-reported", "expires_in": 3600}

    stored = []
    with _patched(msauth, http_form=form), _patched(msauth.secrets, resolve=lambda _ref: "not-reported", backend_of=lambda _ref: "keychain", store_keychain=lambda name, value: stored.append((name, bool(value))) or "keychain:" + name):
        token = msauth.bearer({"auth": "device", "token": "keychain:careerbrew.outlook.refresh", "client_id": "client", "tenant": "tenant"})
        reconnect = msauth.needs_reconnect()
    return {
        "poll": [msauth.classify_poll(answer) for answer in polls],
        "refresh": [msauth.classify_refresh(answer) for answer in refreshes],
        "settings": settings,
        "bearer_is_value": token == "not-reported",
        "rotated": stored == [("careerbrew.outlook.refresh", True)],
        "fields": calls,
        "reconnect_cleared": not reconnect,
        "state_exists": (w.root / "work" / "outlook-auth.json").exists(),
    }


def _mcp_preflight(_base: Path) -> dict[str, Any]:
    github = {"url": "https://api.githubcopilot.com/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}"}}
    local = {"url": "http://localhost:8765/mcp"}
    command = {"command": "missing-synthetic-tool"}
    servers = {"github": github, "local": local, "command": command}
    facts = {
        "servers": {
            "github": {"vars": {"GITHUB_TOKEN": "not-reported-old"}, "github_token": "not-reported-new"},
            "local": {"vars": {}, "port_open": False},
            "command": {"vars": {}, "command_found": False},
        }
    }
    rows = mcp_check.check_rows(servers, facts, repair_host="claude")
    empty_facts = {"servers": {"github": {"vars": {"GITHUB_TOKEN": ""}, "ownership_error": None}}}
    repair = mcp_check.check_rows({"github": github}, empty_facts, repair_host="claude")
    native = mcp_check.check_rows({"github": github}, empty_facts, repair_host="codex")
    strict_errors = []
    for text in ('{"mcpServers": []}', '{"mcpServers": {"x": {"enabled": "yes"}}}', '{"mcpServers": {"x": {"headers": []}}}'):
        try:
            mcp_check.servers(Path("config.json"), strict=True, contents=text)
        except ValueError:
            strict_errors.append("ValueError")
        else:
            strict_errors.append("accepted")
    return {
        "rows": rows,
        "repair": repair,
        "native": native,
        "strict": strict_errors,
        "auth": [mcp_check._github_auth_header(server) for server in [github, {**github, "url": "https://github.com.example.test/mcp"}, {**github, "headersHelper": "owner"}]],
        "vars": mcp_check._vars_in(github),
    }


def _config(base: Path, name: str) -> tuple[Workspace, Path, Path]:
    w = Workspace(base, name)
    target = w.base / "owned-config.json"
    data = {"unrelated": {"keep": True}, "mcpServers": {"fixture": {"type": "http", "url": "https://api.githubcopilot.com/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}", "X-Test": "keep"}}}}
    target.write_text(json.dumps(data))
    target.chmod(0o640)
    link = w.base / "claude.json"
    link.symlink_to(target)
    return w, link, target


def _managed_repair(base: Path) -> dict[str, Any]:
    out = {}
    w, link, target = _config(base, "repair")
    before = target.read_bytes()
    try:
        mcp_check.fix_github_headers(path=link, yes=False, getenv=lambda _key: "")
    except Exception as exc:
        out["confirmation"] = {"error": type(exc).__name__, "unchanged": target.read_bytes() == before}
    with _patched(mcp_check.shutil, which=lambda name: "/synthetic/tools/gh" if name == "gh" else None):
        result = mcp_check.fix_github_headers(path=link, yes=True, getenv=lambda _key: "")
        data = json.loads(target.read_text())
        out["success"] = {
            "result": result,
            "link": link.is_symlink() and link.readlink() == target,
            "mode": f"{stat.S_IMODE(target.stat().st_mode):03o}",
            "unrelated": data["unrelated"] == {"keep": True},
            "headers": data["mcpServers"]["fixture"]["headers"] == {"X-Test": "keep"},
            "helper": "headersHelper" in data["mcpServers"]["fixture"],
            "temporary_left": bool(list(w.base.glob(".careerbrew-mcp-*.tmp"))),
        }
        second = mcp_check.fix_github_headers(path=link, yes=True, getenv=lambda _key: "")
        out["success"]["second"] = second
        out["unknown_probe"] = mcp_check.fix_github_headers(path=link, yes=True, getenv=lambda _key: None)
    w, link, target = _config(base, "managed")
    state = w.home / ".config" / "agentbrew" / "state.yaml"
    state.parent.mkdir(parents=True)
    state.write_text("mcpServers:\n  - name: fixture\n    source: user\n")
    before = target.read_bytes()
    with _patched(mcp_check.shutil, which=lambda name: "/synthetic/tools/gh" if name == "gh" else None):
        try:
            mcp_check.fix_github_headers(path=link, yes=True, getenv=lambda _key: "")
        except Exception as exc:
            out["managed"] = {"error": type(exc).__name__, "unchanged": target.read_bytes() == before, "link": link.is_symlink()}
    w, link, target = _config(base, "foreign")
    data = json.loads(target.read_text())
    data["mcpServers"]["fixture"]["url"] = "https://github.com.example.test/mcp"
    target.write_text(json.dumps(data))
    before = target.read_bytes()
    out["foreign"] = mcp_check.fix_github_headers(path=link, yes=True, getenv=lambda _key: "")
    out["foreign"]["unchanged"] = target.read_bytes() == before
    return out


def _update_workflow(base: Path) -> dict[str, Any]:
    w = Workspace(base, "update-workflow")
    launcher = w.base / "bin" / "careerbrew"
    launcher.parent.mkdir()
    launcher.write_text("#!/bin/sh\n# careerbrew launcher, written by scripts/install-launcher.sh\n")
    launcher.chmod(0o700)
    os.environ["CAREERBREW_BIN_DIR"] = str(launcher.parent)
    ready = self_update.launcher_ready()
    launcher.chmod(0o600)
    not_executable = self_update.launcher_ready()
    repairs = [self_update.repair(["/bin/true"], w.base), self_update.repair(["/bin/false"], w.base), self_update.repair([str(w.base / "missing")], w.base)]
    repo = w.base / "repo"
    (repo / "commands").mkdir(parents=True)
    (repo / "Agentfile.yaml").write_text("commands: [./commands]\n")
    source = repo / "commands" / "work.md"
    source.write_text("# Work\n\nRun the plan.\n")
    installed = w.base / "installed.md"
    installed.write_text(source.read_text())
    report = {"version": 1, "commands": [{"sourcePath": str(source.resolve()), "targets": [{"agent": "claude-code", "ready": True, "status": "ready", "path": str(installed)}]}]}
    agentfile = w.base / "plugin-Agentfile.yaml"
    agentfile.write_text("workSteps: [/missing]\n")
    rows = workflow_check.check([{"step": "/work", "path": str(source)}], repo=repo, agentfiles=[agentfile], report=report, required_agents={"claude-code"})
    return {"launcher": [ready, not_executable], "repairs": repairs, "workflow": rows}


def _cli_handlers_with_deployer(base: Path) -> dict[str, Any]:
    w = Workspace(base, "cli-handlers")
    outputs = {}

    def invoke(name: str, fn: Any, args: types.SimpleNamespace) -> None:
        stream = io.StringIO()
        try:
            with contextlib.redirect_stdout(stream):
                code = fn(args)
        except Exception as exc:
            code, text = 1, type(exc).__name__
        else:
            text = stream.getvalue()
        text = text.replace(str(w.base), "<work>")
        outputs[name] = {"exit": code, "stdout": text}

    invoke("init", cli.cmd_init, types.SimpleNamespace())
    source = _write_plugin(w.base / "source")
    invoke("plugin-add", cli.cmd_plugin_add, types.SimpleNamespace(source=str(source), allow_exec=True))
    invoke("plugin-list", cli.cmd_plugin_list, types.SimpleNamespace())
    invoke("plugin-verify", cli.cmd_plugin_verify, types.SimpleNamespace())
    invoke("machine-set", cli.cmd_machine_set, types.SimpleNamespace(role="corporate", label="work"))
    invoke("machine-show", cli.cmd_machine_show, types.SimpleNamespace())
    store.write_yaml(w.root / "work" / "connectors.yaml", {"private_note": "keep", "connectors": [{"kind": "jira", "token": "keychain:careerbrew.jira"}]})
    store.write_yaml(w.root / "work" / "sources.yaml", {"private_note": "keep", "sources": [{"kind": "git", "path": "~/repo", "authors": ["owner@example.com"]}]})
    invoke("connectors-list", cli.cmd_connectors_list, types.SimpleNamespace())
    invoke("pair-init", cli.cmd_pair_init, types.SimpleNamespace())
    invoke("split-evidence", cli.cmd_machine_split_plugin_evidence, types.SimpleNamespace(dry_run=True))
    invoke("split-evidence-live", cli.cmd_machine_split_plugin_evidence, types.SimpleNamespace(dry_run=False))
    source_data = store.read_yaml(source / "evidence.yaml", {})
    private_sources = store.read_yaml(w.root / "work" / "sources.yaml", {})
    private_connectors = store.read_yaml(w.root / "work" / "connectors.yaml", {})
    outputs["split_fields_preserved"] = (
        source_data.get("retained") == "keep"
        and private_sources.get("private_note") == "keep"
        and private_connectors.get("private_note") == "keep"
        and len(source_data["sources"]) == 1
        and len(source_data["connectors"]) == 1
        and len(private_sources["sources"]) == 1
    )
    invoke("plugin-refresh", cli.cmd_plugin_refresh, types.SimpleNamespace())
    invoke("plugin-remove", cli.cmd_plugin_remove, types.SimpleNamespace(name="acme", purge_data=False))
    outputs["pair_key_not_printed"] = sync.key_path().is_file()
    return outputs


def _cli_handlers(base: Path) -> dict[str, Any]:
    with _patched(plugins, _run_agentbrew=lambda _folder: None):
        return _cli_handlers_with_deployer(base)


def run_all(selected: str | None = None) -> dict[str, Any]:
    base = Path(os.environ["CAREERBREW_HOME"]) / "rust07-workload"
    base.mkdir(parents=True, exist_ok=True)
    cases: list[dict[str, Any]] = []
    functions = {
        "manifest-policy": _manifest_policy,
        "plugin-lifecycle": _plugin_lifecycle,
        "command-ownership": _command_ownership,
        "machine-readiness": _machine_readiness,
        "oauth-refs": _oauth_refs,
        "mcp-preflight": _mcp_preflight,
        "managed-repair": _managed_repair,
        "update-workflow": _update_workflow,
        "cli-handlers": _cli_handlers,
    }
    for name in CASES:
        if selected is None or name == selected:
            _call(cases, name, lambda fn=functions[name]: fn(base))
    text = json.dumps({"cases": cases}, sort_keys=True, separators=(",", ":"))
    for marker in ("not-reported-old", "not-reported-new", "rotated-not-reported"):
        if marker in text:
            raise RuntimeError("synthetic credential reached the oracle report")
    return {"text": text + "\n"}


def main() -> int:
    args = sys.argv[1:]
    if args == ["--all"]:
        selected = None
    elif len(args) == 2 and args[0] == "--case" and args[1] in CASES:
        selected = args[1]
    else:
        return 2
    sys.stdout.write(run_all(selected)["text"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
