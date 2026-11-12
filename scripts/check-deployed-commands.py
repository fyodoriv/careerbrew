#!/usr/bin/env python3
"""Doctor check: each deployed /careerbrew-* command is current (task checkup-command-drift).

agentbrew copies `commands/*.md` from the checkout it records (`commandSourceDirs` in its state file) into
each agent's commands folder on `agentbrew sync`, and records every copy in its manifest (`hashes`: path ->
sha256). Two things make an agent follow old steps, and each has the fix that clears it:
- a copy that differs from the source file: `agentbrew sync`; agentbrew skips a copy edited by hand (its hash
  differs from the manifest), so preserve and adopt those edits before source sync;
- a source checkout on main that is behind origin/main (origin has a commit to a command that the
  checkout lacks, or a new command): pull it, then sync. A source on another branch was
  synced on purpose, so it is a note.
Every worktree finds the same source through git's common dir, so each gives the same answer.
The doctor entrypoint also checks configured CLI availability and local credential
status. Content-free: command names, paths, branch names and status enums only.

Usage: check-deployed-commands.py [repo] [manifest] [state]. One doctor line per command; exit 1 on any FAIL.
"""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

import yaml  # without it the state file cannot be read: a crash, which the doctor counts as a failure

from careerbrew.agent_workflows import RepositoryWorkflow, WorkflowSourceError, agent_runtime_rows, owned_workflow_path, primary_agent_roots, read_workflow_bytes, repository_workflows

ROOT = Path(__file__).resolve().parents[1]
AGENTBREW = Path.home() / ".config" / "agentbrew"  # agentbrew src/manifest.ts and its state file
MANIFEST = AGENTBREW / "manifest.json"
STATE = AGENTBREW / "state.yaml"
SYNC = "agentbrew sync"


def _short(p: str | Path) -> str:
    p, home = str(p), str(Path.home())
    return "~" + p[len(home) :] if p.startswith(home + "/") else p


def _git(repo: Path, *args: str) -> bytes | None:
    out = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, check=False)
    return out.stdout if out.returncode == 0 else None


def _common_dir(repo: Path) -> str | None:
    found = _git(repo, "rev-parse", "--path-format=absolute", "--git-common-dir")
    return str(Path(found.decode().strip()).resolve()) if found else None


def _hashes(manifest: Path) -> dict[str, str] | None:
    try:
        hashes = json.loads(manifest.read_text(encoding="utf-8")).get("hashes")
    except (OSError, ValueError, AttributeError):
        return None
    return {str(k): str(v) for k, v in hashes.items()} if isinstance(hashes, dict) else None


def _source(repo: Path, state: Path) -> Path | None:
    """The owning checkout of a registered folder/file source, including other worktrees."""
    try:
        dirs = (yaml.safe_load(state.read_text(encoding="utf-8")) or {}).get("commandSourceDirs") or []
    except Exception:  # a missing, unreadable, or odd state file means "not deployed": a note, never a crash
        return None
    mine = _common_dir(repo)
    for d in dirs if isinstance(dirs, list) else []:
        path = Path(str(d.get("path", ""))).expanduser() if isinstance(d, dict) else None
        if not path:
            continue
        owner = Path(str(d["agentfile"])).expanduser() if isinstance(d.get("agentfile"), str) else None
        for folder in (owner, path if path.is_dir() else path.parent):
            if mine and folder and _common_dir(folder) == mine:
                top = _git(folder, "rev-parse", "--show-toplevel")
                if top:
                    return Path(top.decode().strip()).resolve()
    return None


def _native_rows(src: Path, hashes: dict[str, str], source_bytes: bytes) -> list[tuple[bool, str, str]]:
    """Check tracked native artifacts against source bytes and adapter safeguards."""
    out = []
    body = source_bytes.decode("utf-8").replace("\r\n", "\n")
    front = re.match(r"^---\n(.*?)\n---\n?", body, re.S)
    metadata = yaml.safe_load(front.group(1)) if front else {}
    metadata = metadata if isinstance(metadata, dict) else {}
    raw_body = body[front.end() :] if front else body
    refs = [Path(p) for p in hashes if p.endswith("/references/command.md") and Path(p).parent.parent.name == "agentbrew-command-" + src.stem]
    prompts = [Path(p) for p in hashes if p.endswith(f"/prompts/{src.name}")]
    for prompt in prompts:
        ok = prompt.is_file() and not prompt.is_symlink()
        if ok:
            text = prompt.read_text(encoding="utf-8")
            head = re.match(r"^---\n(.*?)\n---\n?", text, re.S)
            fields = yaml.safe_load(head.group(1)) if head else {}
            expected = {k: metadata[k] for k in ("description", "argument-hint") if k in metadata}
            ok = (fields or {}) == expected and (text[head.end() :] if head else text).strip() == raw_body.strip()
        if not ok:
            out.append((False, f"/{src.stem} native prompt missing, edited or stale: {_short(prompt)} (preserve owner edits before source sync)", SYNC))
    for ref in refs:
        root = ref.parent.parent
        wrapper, policy = root / "SKILL.md", root / "agents" / "openai.yaml"
        leaves = [ref, wrapper, policy]
        safe = all(p.is_file() and not p.is_symlink() for p in leaves) and not root.is_symlink() and not ref.parent.is_symlink() and not policy.parent.is_symlink()
        if safe:
            text = wrapper.read_text(encoding="utf-8")
            try:
                settings = yaml.safe_load(policy.read_text(encoding="utf-8")) or {}
                explicit = settings.get("policy", {}).get("allow_implicit_invocation") is False
            except (ValueError, AttributeError, yaml.YAMLError):
                explicit = False
            safe = ref.read_bytes() == source_bytes and explicit and all(t in text for t in ["references/command.md", "Keep the current approval rules", "Do not use shell expansion"])
            safe = safe and all(hashes.get(str(p)) == hashlib.sha256(p.read_bytes()).hexdigest() for p in leaves)
        if not safe:
            out.append((False, f"/{src.stem} native command bridge missing, edited or unsafe: {_short(root)} (preserve owner edits before source sync)", SYNC))
    return out


def _same_skill(source: Path, target: Path, checkout: Path) -> bool:
    """Verify required source resources; extra owner files are preserved."""
    try:
        owned_workflow_path(source, checkout)
        if not target.is_dir():
            return False
        if target.resolve() == source.resolve():
            return True
        files = [p for p in source.rglob("*") if p.is_file() and "__pycache__" not in p.parts and p.name != ".DS_Store"]
        return all((target / p.relative_to(source)).is_file() and (target / p.relative_to(source)).read_bytes() == read_workflow_bytes(p, checkout) for p in files)
    except (OSError, WorkflowSourceError):
        return False


def _required_skill_rows(checkout: Path, clients: dict[str, Path], entries: tuple[RepositoryWorkflow, ...]) -> list[tuple[bool, str, str]]:
    out = []
    for skill in (entry.path for entry in entries if entry.kind == "skill"):
        for agent, client in clients.items():
            candidates = [client / "skills" / skill.parent.name]
            if agent == "codex":
                candidates.append(Path.home() / ".agents" / "skills" / skill.parent.name)
            if not any(_same_skill(skill.parent, target, checkout) for target in candidates):
                out.append((False, f"required {agent} skill {skill.parent.name} missing or stale; preserve owner edits and sync its source", SYNC))
    return out


def rows(repo: Path = ROOT, manifest: Path = MANIFEST, state: Path = STATE) -> list[tuple[bool, str, str]]:
    """(ok, what, fix) per command."""
    try:
        saved = yaml.safe_load(state.read_text(encoding="utf-8")) or {}
    except (OSError, ValueError, yaml.YAMLError):
        saved = {}
    clients = primary_agent_roots(saved)
    out: list[tuple[bool, str, str]] = []
    hashes = _hashes(manifest)
    if hashes is None:
        if not clients:
            return [(True, "deployed commands not checked: no agentbrew manifest (agentbrew sync deploys them)", "")]
        hashes = {}
        out.append((False, "required agent command manifest missing or unreadable; agent workflows are not ready", SYNC))
    checkout = _source(repo, state)
    if checkout is None:
        if not clients:
            return [(True, "careerbrew commands not deployed by agentbrew (optional: agentbrew sync in this repo)", "")]
        checkout = repo.resolve()
        out.append((False, "required Careerbrew command source is not registered; sync from the Careerbrew checkout", SYNC))
    inventory = repository_workflows(checkout)
    out.extend((False, problem, SYNC) for problem in inventory.problems)
    commands = [entry.path for entry in inventory.entries if entry.kind == "command"]
    branch = (_git(checkout, "branch", "--show-current") or b"").decode().strip()
    out.extend(_required_skill_rows(checkout, clients, inventory.entries))
    if branch and branch != "main":
        out.append((True, f"commands deploy from branch {branch} in {_short(checkout)}", ""))
    pull = f"git -C {_short(checkout)} pull --ff-only && {SYNC}"
    if branch == "main":
        added = b"\n".join(_git(checkout, "diff", "--name-only", "--diff-filter=A", "HEAD...origin/main", "--", str(path.relative_to(checkout))) or b"" for path in inventory.command_sources)
        for name in sorted({Path(p).name for p in added.decode().split() if p.endswith(".md")} - {p.name for p in commands}):
            out.append((False, f"/{name.removesuffix('.md')} is on origin/main but not in the source checkout {_short(checkout)}", pull))
    for src in sorted(commands):
        try:
            want = read_workflow_bytes(src, checkout)
        except (OSError, WorkflowSourceError):
            out.append((False, "declared command body is outside its checkout or unreadable", SYNC))
            continue
        cmd = "/" + src.stem
        native_root = clients.get("codex")
        native_hashes = {p: h for p, h in hashes.items() if native_root and Path(p).is_relative_to(native_root)}
        if "codex" in clients:
            native_hashes.setdefault(str(clients["codex"] / "skills" / ("agentbrew-command-" + src.stem) / "references" / "command.md"), "")
        if "claude-code" in clients:
            required = clients["claude-code"] / "commands" / src.name
            if str(required) not in hashes and (not required.is_file() or required.read_bytes() != want):
                out.append((False, f"required Claude command {cmd} missing or stale; inspect agentbrew prerequisites and preserve owner edits", SYNC))
        out.extend(_native_rows(src, native_hashes, want))
        # agentbrew writes a copy only when the agent's own folder exists, so a removed agent's entry is not a copy to check
        copies = [p for p in hashes if p.endswith(f"/commands/{src.name}") and Path(p).parent.parent.is_dir()]
        missing = [p for p in copies if not Path(p).is_file()]
        stale = [p for p in copies if p not in missing and Path(p).read_bytes() != want]
        edited = [p for p in stale if hashlib.sha256(Path(p).read_bytes()).hexdigest() != hashes[p]]
        behind = branch == "main" and (_git(checkout, "rev-list", "--count", "HEAD..origin/main", "--", str(src.relative_to(checkout))) or b"0").strip() != b"0"
        if not copies:
            if not clients:
                out.append((True, f"{cmd} not deployed yet (optional: {SYNC})", ""))
        elif edited:
            out.append((False, f"{cmd} deployed copy was edited by hand, so agentbrew sync skips it: {', '.join(map(_short, edited))}", "agentbrew status --verbose"))
        elif stale or missing:
            what = ", ".join([*(f"{_short(p)} (older than its source)" for p in stale), *(f"{_short(p)} (missing)" for p in missing)])
            out.append((False, f"{cmd} deployed copy is out of date: {what}", SYNC))
        elif behind:
            out.append((False, f"{cmd} source checkout {_short(checkout)} is behind origin/main", pull))
        else:
            out.append((True, f"{cmd} deployed and current ({len(copies)} copies)", ""))
    return out


def main(argv: list[str]) -> int:
    repo = Path(argv[1]) if len(argv) > 1 else ROOT
    manifest = Path(argv[2]) if len(argv) > 2 else MANIFEST
    state = Path(argv[3]) if len(argv) > 3 else STATE
    found = rows(repo, manifest, state)
    try:
        saved = yaml.safe_load(state.read_text(encoding="utf-8")) or {}
    except (OSError, ValueError, yaml.YAMLError):
        saved = {}
    found.extend(agent_runtime_rows(saved))
    for ok, what, fix in found:
        print(f"ok    {what}" if ok else f"FAIL  {what} -> fix: {fix}")
    return 0 if all(ok for ok, _, _ in found) else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
