"""Read-only work-command and plan completeness checks (#305).

Lifecycle: each preflight reads current files. It never deletes edited commands
or caches their contents. Agentbrew remains the owner of installed artifacts.
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
from pathlib import Path
from typing import Any

import yaml

from .. import paths, plugins
from ..agent_workflows import WorkflowSourceError, command_report_ready, primary_agent_roots, read_workflow_bytes, repository_workflows


def command_report(repo: Path) -> Any:
    binary = shutil.which("agentbrew")
    if not binary:
        return None

    try:
        done = subprocess.run([binary, "commands", "list", "--json"], cwd=repo, capture_output=True, text=True, stdin=subprocess.DEVNULL, timeout=15, check=False)  # noqa: S603
        return json.loads(done.stdout) if done.returncode == 0 else None
    except (OSError, ValueError, subprocess.TimeoutExpired):
        return None


def reported_bodies(report: Any, source: Path, required_agents: set[str]) -> list[str]:
    """Read only the reported artifacts for this selected source, including stale copies.

    Native targets can be directories. Inspect their owned Markdown resources
    without guessing adapter filenames or following an outside symlink.
    """
    if not isinstance(report, dict) or report.get("version") != 1 or not isinstance(report.get("commands"), list):
        return []
    texts = []
    for row in report["commands"]:
        if not isinstance(row, dict) or row.get("sourcePath") != str(source.resolve()) or not isinstance(row.get("targets"), list):
            continue
        for target in row["targets"]:
            if not isinstance(target, dict) or target.get("agent") not in required_agents or not isinstance(target.get("path"), str):
                continue
            selected = Path(target["path"])
            if not selected.is_absolute() or selected.is_symlink():
                continue
            candidates = selected.rglob("*.md") if selected.is_dir() else [selected]
            for candidate in candidates:
                try:
                    if candidate.is_symlink() or (selected.is_dir() and not candidate.resolve().is_relative_to(selected.resolve())):
                        continue
                    texts.append(candidate.read_text(encoding="utf-8"))
                except (OSError, UnicodeError):
                    continue
    return texts


def check(declared: list[dict[str, Any]], *, repo: Path | None = None, agentfiles: list[Path] | None = None, report: Any = None, required_agents: set[str] | None = None) -> list[dict[str, Any]]:
    repo = repo or paths.REPO_ROOT
    required_agents = set(primary_agent_roots()) if required_agents is None else required_agents
    if agentfiles is None:
        agentfiles = [repo / "Agentfile.yaml", Path.home() / ".config/agentbrew/Agentfile.yaml", *(Path(p["local_path"]) / "Agentfile.yaml" for p in plugins.installed() if p.get("local_path"))]
    inventory = repository_workflows(repo)
    selected = [entry.path for entry in inventory.entries if entry.kind == "command" and entry.path.name == "work.md"]
    source = selected[0] if len(selected) == 1 else repo / "commands/work.md"
    unreadable = bool(inventory.problems) or len(selected) != 1
    try:
        want = read_workflow_bytes(source, repo) if not unreadable else b""
    except (OSError, WorkflowSourceError):
        want, unreadable = b"", True
    if required_agents and report is None:
        report = command_report(repo)
    ready = not unreadable and (not required_agents or command_report_ready(report, [str(source.resolve())], required_agents))
    installed = reported_bodies(report, source, required_agents) if not unreadable else []
    texts = [want.decode("utf-8", errors="replace"), *installed]
    missing_paths = []
    for text in texts:
        plain = re.sub(r"(?ms)^\s*(```|~~~).*?^\s*\1[^\n]*$", "", text)
        missing_paths.extend(p for p in re.findall(r"~/[^\s`\"'<>),;]+", plain) if not Path(p.rstrip(".:")).expanduser().exists())
    planned = {s["step"] for s in declared}
    lost = []
    for agentfile in agentfiles:
        try:
            data = yaml.safe_load(agentfile.read_text()) or {}
            legacy = data.get("workSteps", []) if isinstance(data, dict) else []
            if not isinstance(legacy, list):
                lost.append("invalid workSteps")
                continue
            for entry in legacy:
                name = entry.get("step") if isinstance(entry, dict) else entry
                if not isinstance(name, str) or "/" + name.lstrip("/") not in planned:
                    lost.append("legacy step")
        except FileNotFoundError:
            continue
        except (OSError, ValueError, yaml.YAMLError):
            lost.append("unreadable Agentfile")
    missing_steps = [s for s in declared if not s.get("path") or not Path(s["path"]).expanduser().is_file()]
    return [
        {
            "check": "installed-command",
            "ok": ready,
            "value": "ready" if ready else "missing, stale or readiness unavailable",
            "safe": "preserve and adopt edited /work copies into the source, then run agentbrew sync; never remove an unknown owner edit",
        },
        {"check": "command-paths", "ok": not missing_paths, "value": f"{len(set(missing_paths))} missing path(s)", "safe": "repair the missing ~/ paths in the installed /work source, then agentbrew sync"},
        {"check": "plan-steps", "ok": not lost, "value": f"{len(lost)} missing legacy step(s)", "safe": "move Agentfile workSteps into the plugin manifest work_steps, then careerbrew plugin refresh"},
        {"check": "step-files", "ok": not missing_steps, "value": f"{len(missing_steps)} unresolved step(s)", "safe": "repair each source_error in careerbrew day run --json; restore its declared file in its owning source"},
    ]
