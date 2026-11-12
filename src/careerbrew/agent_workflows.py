"""Read-only agent readiness. No process cache; config ownership stays with agentbrew."""

from __future__ import annotations

import json
import shutil
import subprocess
from collections import deque
from dataclasses import dataclass
from os.path import abspath
from pathlib import Path
from typing import Any, Literal

import yaml

PRIMARY_AGENTS = {"claude-code": ".claude", "codex": ".codex"}


class WorkflowSourceError(ValueError):
    pass


@dataclass(frozen=True)
class RepositoryWorkflow:
    kind: Literal["command", "skill"]
    path: Path


@dataclass(frozen=True)
class RepositoryInventory:
    entries: tuple[RepositoryWorkflow, ...]
    command_sources: tuple[Path, ...]
    problems: tuple[str, ...]
    skill_sources: tuple[Path, ...] = ()


def owned_workflow_path(path: Path, checkout: Path) -> Path:
    """Resolve the current selection before a body read; never use a prior pointer target."""
    try:
        selected, owner = Path(abspath(path)), checkout.resolve()
        if not selected.is_relative_to(owner):
            raise WorkflowSourceError("declared workflow source is outside its owning checkout")
        real = selected.resolve()
        if not real.is_relative_to(owner):
            raise WorkflowSourceError("declared workflow source is outside its owning checkout")
        return real
    except (OSError, RuntimeError) as exc:
        raise WorkflowSourceError("declared workflow source is unreadable") from exc


def read_workflow_bytes(path: Path, checkout: Path, *, source_directory: Path | None = None) -> bytes:
    selected = owned_workflow_path(path, checkout)
    if source_directory is not None and not selected.is_relative_to(owned_workflow_path(source_directory, checkout)):
        raise WorkflowSourceError("requested resource is outside its selected source directory")
    return selected.read_bytes()


def repository_workflows(checkout: Path) -> RepositoryInventory:
    """One check invocation owns this inventory. Agentbrew owns deployment and state.

    Read only this checkout's Agentfile sources. Keep file selection and explicit
    command/skill roles; the same file can have both roles. Missing legacy fields
    retain the old folders. No inventory survives the caller's invocation.
    """
    checkout = checkout.resolve()
    config = checkout / "Agentfile.yaml"
    try:
        data = yaml.safe_load(config.read_text()) if config.is_file() else {}
        if data is None:
            data = {}
        if not isinstance(data, dict):
            raise ValueError
    except (OSError, ValueError, yaml.YAMLError):
        return RepositoryInventory((), (), ("owning Agentfile workflow declarations are unreadable",))
    entries: dict[tuple[str, Path], RepositoryWorkflow] = {}
    commands: list[Path] = []
    skill_sources: list[Path] = []
    problems: list[str] = []

    def add(kind: Literal["command", "skill"], path: Path) -> None:
        owned_workflow_path(path, checkout)
        entries[kind, path] = RepositoryWorkflow(kind, path)

    def children(folder: Path) -> list[Path]:
        real = owned_workflow_path(folder, checkout)
        return [folder / p.name for p in sorted(real.iterdir())]

    def skill_files(folder: Path) -> list[Path]:
        found: dict[str, Path] = {}
        # Match agentbrew's standard collection roots, then its bounded deep fallback.
        for base in (folder, folder / "skills"):
            if owned_workflow_path(base, checkout).is_dir():
                for child in children(base):
                    real = owned_workflow_path(child, checkout)
                    if real.is_dir() and (real / "SKILL.md").is_file():
                        found.setdefault(child.name, child / "SKILL.md")
        if not found:
            queue = deque([(folder, 0)])
            while queue:
                base, depth = queue.popleft()
                if depth > 6:
                    continue
                for child in children(base):
                    if child.name.startswith(".") or child.name in {"node_modules", "__pycache__"} or child.is_symlink():
                        continue
                    if not owned_workflow_path(child, checkout).is_dir():
                        continue
                    if (child / "SKILL.md").is_file():
                        found.setdefault(child.name, child / "SKILL.md")
                    queue.append((child, depth + 1))
        return list(found.values())

    declarations: tuple[tuple[Literal["command", "skill"], str, str], ...] = (("command", "commands", "commands"), ("skill", "skillDirectories", "skills"))
    for kind, field, default in declarations:
        selected = data.get(field, ["./" + default])
        if not isinstance(selected, list) or not all(isinstance(p, str) for p in selected):
            problems.append(f"Agentfile {field} must be a list of source paths")
            continue
        for value in selected:
            path = Path(value).expanduser()
            path = path if path.is_absolute() else checkout / path
            path = Path(abspath(path))
            try:
                real = owned_workflow_path(path, checkout)
                if not real.exists() and field not in data:
                    continue
                if kind == "command":
                    commands.append(path)
                    files = [path] if real.is_file() and path.suffix == ".md" else [p for p in children(path) if p.suffix == ".md" and p.is_file()] if real.is_dir() else None
                else:
                    if real.is_dir():
                        skill_sources.append(path)
                    files = skill_files(path) if real.is_dir() else None
                if files is None:
                    raise WorkflowSourceError("declared workflow source is missing or has the wrong kind")
                for source in files:
                    add(kind, source)
            except (OSError, WorkflowSourceError) as exc:
                problems.append(str(exc) if isinstance(exc, WorkflowSourceError) else "declared workflow source is missing or unreadable")
    return RepositoryInventory(tuple(entries.values()), tuple(commands), tuple(problems), tuple(skill_sources))


def primary_agent_roots(state: dict[str, Any] | None = None) -> dict[str, Path]:
    """Discover configured primary clients without changing opt-outs or auth settings."""
    if state is None:
        try:
            state = yaml.safe_load((Path.home() / ".config/agentbrew/state.yaml").read_text()) or {}
        except (OSError, ValueError, yaml.YAMLError):
            state = {}
    records = state.get("agents", []) if isinstance(state, dict) else []
    records = records if isinstance(records, list) else []
    by_name = {r.get("name"): r for r in records if isinstance(r, dict)}
    roots = {}
    for name, folder in PRIMARY_AGENTS.items():
        record = by_name.get(name, {})
        if record.get("detected") is False:
            continue
        configured = record.get("skillsDir") or record.get("commandsDir")
        root = Path(str(configured)).expanduser().parent if configured else Path.home() / folder
        if root.is_dir() or record.get("detected") is True:
            roots[name] = root
    return roots


def _local_auth_status(binary: str, executable: str) -> Literal["signed in", "needs login", "unknown"]:
    """Reduce one local status response to an enum; never retain account output."""
    args = ["auth", "status", "--json"] if binary == "claude" else ["login", "status"]
    try:
        done = subprocess.run([executable, *args], capture_output=True, text=True, timeout=2, stdin=subprocess.DEVNULL, check=False)  # noqa: S603
    except (OSError, subprocess.TimeoutExpired):
        return "unknown"
    if binary == "claude":
        try:
            data = json.loads(done.stdout)
        except (ValueError, TypeError):
            return "unknown"
        logged_in = data.get("loggedIn") if isinstance(data, dict) else None
        if logged_in is True and done.returncode == 0:
            return "signed in"
        if logged_in is False and done.returncode == 1:
            return "needs login"
        return "unknown"
    if done.returncode == 0:
        return "signed in"
    if done.returncode == 1 and any(line.strip() == "Not logged in" for line in (done.stdout + "\n" + done.stderr).splitlines()):
        return "needs login"
    return "unknown"


def agent_runtime_rows(state: dict[str, Any] | None = None) -> list[tuple[bool, str, str]]:
    """Per-doctor-invocation probes only; optional clients never trigger auth checks."""
    required = primary_agent_roots(state)
    rows = []
    for agent in PRIMARY_AGENTS:
        binary = "claude" if agent == "claude-code" else agent
        executable = shutil.which(binary)
        if agent not in required:
            text = f"{binary} CLI installed; auth not checked (agent not configured)" if executable else f"{binary} CLI not installed (optional; agent not configured)"
            rows.append((True, text, ""))
        elif not executable:
            rows.append((False, f"{binary} CLI missing for configured agent", f"install {binary} and add it to PATH"))
        else:
            status = _local_auth_status(binary, executable)
            login = "claude auth login" if binary == "claude" else "codex login"
            retry = "claude auth status --json" if binary == "claude" else "codex login status"
            fix = "" if status == "signed in" else login if status == "needs login" else f"retry `{retry}` locally and repair the CLI before claiming agent readiness"
            rows.append((status == "signed in", f"{binary} CLI installed; local auth: {status}", fix))
    return rows


def command_report_ready(report: Any, sources: list[str], required_agents: set[str]) -> bool:
    """Validate the upstream provenance result for requested files and primary hosts only."""
    if not isinstance(report, dict) or type(report.get("version")) is not int or report.get("version") != 1:
        return False
    commands = report.get("commands")
    if not isinstance(commands, list):
        return False
    for source in sources:
        rows = [row for row in commands if isinstance(row, dict) and row.get("sourcePath") == source]
        if len(rows) != 1 or not isinstance(rows[0].get("targets"), list):
            return False
        for agent in required_agents:
            targets = [item for item in rows[0]["targets"] if isinstance(item, dict) and item.get("agent") == agent]
            if len(targets) != 1:
                return False
            target = targets[0]
            if target.get("ready") is not True or target.get("status") != "ready" or not isinstance(target.get("path"), str) or not Path(target["path"]).is_absolute():
                return False
    return True
