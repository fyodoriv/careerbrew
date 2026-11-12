"""Refresh work-day inputs before reading a plan (#330).

Lifecycle: results belong to this invocation. Child output is discarded because
plugin installs can name private paths. No update receipt or credential is saved.
"""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path
from typing import Any

from .. import paths
from ..agent_workflows import primary_agent_roots
from . import start


def declared_steps() -> list[dict[str, Any]]:
    from . import steps

    return [{**s, **steps.resolve(s["step"], origin=s["from"])} for s in steps.declared()]


def repair(argv: list[str], cwd: Path) -> bool:
    try:
        done = subprocess.run(argv, cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL, timeout=300, check=False)  # noqa: S603
    except (OSError, subprocess.TimeoutExpired):
        return False
    return done.returncode == 0


def launcher_ready() -> bool:
    launcher = Path(os.environ.get("CAREERBREW_BIN_DIR", str(Path.home() / ".local/bin"))) / "careerbrew"
    try:
        with launcher.open("rb") as stream:
            return os.access(launcher, os.X_OK) and b"# careerbrew launcher, written by scripts/install-launcher.sh" in stream.read(4096)
    except OSError:
        return False


def run() -> dict[str, Any]:
    from .steps import StepError

    root = paths.REPO_ROOT
    try:
        declared = declared_steps()
    except StepError:
        declared = []  # stale declarations must not prevent refreshing their owning sources
    lookup: start.CheckoutLookup = {}
    found = start.checkouts(declared, lookup=lookup)
    own = start.toplevel(root, lookup=lookup) or str(root)
    del lookup  # Discovery only; refresh must observe current Git state.
    if not any(c["top"] == own for c in found):
        found.insert(0, {"top": own, "for": ["careerbrew"], "duplicate_of": ""})
    refresh, changed = [], []
    for checkout in found:
        result = start.refresh_one(checkout["top"])
        refresh.append({**result, "for": checkout["for"]})
        if result["status"] == "fast-forwarded":
            rc, names = start._git(checkout["top"], "diff", "--name-only", result["before"], result["after"])
            if rc == 0:
                changed.extend(start.shown(Path(checkout["top"]) / name) for name in names.splitlines() if name.endswith(".md"))
    refused = any(r["status"] in start.NOT_TOUCHED for r in refresh)
    out: dict[str, Any] = {"ok": not refused, "updated": any(r["status"] == "fast-forwarded" for r in refresh), "changed_files": changed, "refresh": refresh, "repairs": []}
    if refused:
        return out
    commands = [
        ("dependencies", ["uv", "sync"]),
        ("launcher", ["bash", "scripts/install-launcher.sh"]),
        ("plugins", ["uv", "run", "careerbrew", "plugin", "refresh"]),
    ]
    agentbrew = shutil.which("agentbrew")
    if primary_agent_roots() and (root / "Agentfile.yaml").is_file():
        if not agentbrew:
            out["ok"] = False
            out["repairs"].append({"check": "agent commands: install agentbrew", "ok": False})
            return out
        commands.append(("agent commands", [agentbrew, "sync"]))
    for name, argv in commands:
        ok = repair(argv, root)
        if ok and name == "launcher":
            ok = launcher_ready()  # the installer preserves foreign commands and skips linked worktrees
        out["repairs"].append({"check": name, "ok": ok})
        if not ok:
            out["ok"] = False
            break
    return out
