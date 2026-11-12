"""`careerbrew checkup`: one pass/fail line per step for this machine.

Steps: stale project sources first, then pull-only clone and no cloud-synced data or backups (corporate role),
venv architecture, web app build, plugin verify,
corporate readiness, privacy scan, evidence collect (network; `--offline`
skips it), and the promo coverage
change since the last checkup. A plugin adds steps with a manifest
`checkup: [{name, run: [argv...]}]`; they run only for a plugin installed
with `--allow-exec`.

Lifecycle: the last coverage lives in `<data root>/checkup.json` (one file per
data root); nothing is kept in process memory. Output is content-free.
"""

from __future__ import annotations

import os
import platform
import re
import subprocess
import sys
from collections.abc import Callable
from typing import Any

from . import events, machine, paths, plugins, privacy, store

Step = dict[str, Any]


def hardware_arch() -> str:
    """The CPU's native architecture; on macOS this sees through Rosetta."""
    if sys.platform == "darwin":
        res = subprocess.run(["sysctl", "-n", "hw.optional.arm64"], capture_output=True, text=True, check=False)
        if res.stdout.strip() == "1":
            return "arm64"
    return platform.machine()


def arch_step(hw: str | None = None, py: str | None = None) -> Step:
    hw, py = hw or hardware_arch(), py or platform.machine()
    ok = hw == py or {hw, py} == {"arm64", "aarch64"}
    fix = "" if ok else "rm -rf .venv && uv venv --python cpython-3.12-macos-aarch64-none && uv sync"
    return {"step": f"venv architecture {py} on {hw} hardware", "ok": ok, "fix": fix}


def pull_only_step() -> Step:
    if machine.role() != "corporate":
        return {"step": "pull-only clone (personal role: not required)", "ok": True, "fix": ""}
    res = subprocess.run(["git", "-C", os.environ.get("CAREERBREW_REPO_DIR") or str(paths.REPO_ROOT), "remote", "get-url", "--push", "origin"], capture_output=True, text=True, check=False)
    url = res.stdout.strip()
    ok = res.returncode != 0 or not url or not _pushable(url)
    return {"step": "pull-only clone (push to origin disabled)", "ok": ok, "fix": "" if ok else "git remote set-url --push origin DISABLED"}


def _pushable(url: str) -> bool:
    """A real remote: a URL, a `user@host:path` form, or a folder that exists. Placeholders like
    `no-push-from-corporate` (docs/corporate-machine.md) or `DISABLED` are not."""
    from pathlib import Path

    return "://" in url or re.match(r"^[^/\s]+@[^/\s]+:", url) is not None or Path(url).expanduser().exists()


def cloud_step() -> Step:
    if machine.role() != "corporate":
        return {"step": "no cloud-synced data or backups (personal role: not required)", "ok": True, "fix": ""}
    from . import backup

    found = [x for x in [machine.cloud_synced(paths.data_root()), *(machine.cloud_synced(t) for t in backup.configured_targets())] if x]
    return {"step": f"no cloud-synced data or backups ({', '.join(sorted(set(found))) or 'none found'})", "ok": not found, "fix": "careerbrew machine ready" if found else ""}


def web_step() -> Step:
    from . import server

    ok = server.dist_exists()
    return {"step": "web app built (web/dist)", "ok": ok, "fix": "" if ok else "cd web && npm ci && npm run build"}


def projects_step(refresh: bool = True) -> Step:
    """Refresh stale project sources first (research.refresh), then report what is still stale."""
    from .work import projects, research

    found = ""
    if refresh and projects.stale_sources():
        read_allowed = sys.stdout.isatty() or bool(plugins.effective_policy().get("agent_may_read_business"))
        r = research.refresh(read_allowed)
        found = f"; refreshed {r['refreshed']}, proposed {len(r['proposed'])} new source(s)"
    every = projects.stale_sources()
    agent = [x for x in every if x["reader"] == "agent"]
    stale = [x for x in every if x["reader"] != "agent"]
    if agent:
        found += f"; {len(agent)} more the agent reads itself (agent_reads)"
    bad_memory = [n for n, p in projects._load().items() if projects.is_memory_index(p.get("memory", ""))]
    if bad_memory:
        return {"step": f"project notes paths ({', '.join(bad_memory)} point at a MEMORY.md index)", "ok": False, "fix": f"careerbrew work project memory {bad_memory[0]} <folder or file>"}
    first = f"; stalest: {stale[0]['project']} {stale[0]['kind']} {stale[0]['ref']}" if stale else ""
    return {"step": f"project sources fresh ({len(stale)} stale{first}{found})", "ok": not stale, "fix": f"read it, then careerbrew work project check {stale[0]['project']} {stale[0]['ref']}" if stale else ""}


def verify_step() -> Step:
    bad = [n for n, ok in plugins.verify() if not ok]
    return {"step": f"plugin files unchanged ({len(bad)} changed)", "ok": not bad, "fix": "careerbrew plugin refresh" if bad else ""}


def ready_step() -> Step:
    if machine.role() != "corporate":
        return {"step": "corporate readiness (personal role: not required)", "ok": True, "fix": ""}
    rows = machine.readiness()
    bad = sum(not r["ok"] for r in rows)
    return {"step": f"corporate readiness {len(rows) - bad}/{len(rows)}", "ok": not bad, "fix": "careerbrew machine ready" if bad else ""}


def privacy_step() -> Step:
    rep = privacy.scan_repo(None, terms=machine.plugin_terms())
    return {"step": f"privacy scan {len(rep.findings)} finding(s)", "ok": rep.ok, "fix": "" if rep.ok else "careerbrew privacy scan"}


def encryption_step() -> Step:
    from . import vault

    state = vault.status()
    plaintext = int(state["plaintext"])
    bad = bool(state["enabled"]) and plaintext > 0
    return {"step": f"data encryption ({plaintext} file(s) not sealed by data encryption)", "ok": not bad, "fix": "careerbrew data encrypt" if bad else ""}


def collect_step() -> Step:
    from .work import evidence

    res = evidence.collect()
    return {"step": f"evidence collect {res['new']} new, {res['errors']} source error(s)", "ok": not res["errors"], "fix": "careerbrew work collect" if res["errors"] else ""}


def coverage_step() -> Step:
    from .work import promo

    settings = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
    try:
        asm = promo.assess(settings.get("ladder", "generic-swe"), settings.get("target_level", "senior"))
    except promo.LadderError as e:
        return {"step": "promo coverage", "ok": False, "fix": str(e)}
    state_path = paths.sub("checkup.json")
    last = (store.read_json(state_path, {}) or {}).get("coverage")
    store.write_json(state_path, {"coverage": asm["overall"], "at": asm["at"]})
    delta = "first run" if last is None else f"{(asm['overall'] - last) * 100:+.0f} points since last checkup"
    return {"step": f"promo coverage {asm['overall']:.0%} ({delta})", "ok": True, "fix": ""}


def plugin_steps() -> list[Step]:
    out = []
    for p in plugins.installed():
        for s in p["manifest"].get("checkup") or []:
            name = f"{p['name']}: {s.get('name', 'step')}"
            if not p.get("exec_allowed"):
                out.append({"step": f"{name} (skipped: plugin installed without --allow-exec)", "ok": True, "fix": ""})
                continue
            res = subprocess.run([str(x) for x in s.get("run", [])], cwd=p["root"], capture_output=True, text=True, check=False)
            out.append({"step": name, "ok": res.returncode == 0, "fix": "" if res.returncode == 0 else f"exit {res.returncode}"})
    return out


STEPS: list[Callable[[], Step]] = [projects_step, pull_only_step, cloud_step, arch_step, web_step, verify_step, ready_step, privacy_step, encryption_step, collect_step, coverage_step]


def offline_collect_step() -> Step:
    return {"step": "evidence collect (skipped: --offline)", "ok": True, "fix": ""}


def run(offline: bool = False) -> list[Step]:
    rows = []
    for fn in STEPS:
        if offline and fn is collect_step:
            fn = offline_collect_step
        if offline and fn is projects_step:
            rows.append(projects_step(refresh=False))
            continue
        try:
            rows.append(fn())
        except Exception as e:  # one broken step must not hide the others
            rows.append({"step": fn.__name__.removesuffix("_step"), "ok": False, "fix": type(e).__name__})
    rows.extend(plugin_steps())
    events.emit("checkup", status="ok" if all(r["ok"] for r in rows) else "fail", failed=sum(not r["ok"] for r in rows))
    return rows
