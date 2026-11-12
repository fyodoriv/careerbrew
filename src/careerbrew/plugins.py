"""Corporate plugin (overlay) system.

A plugin is a folder or git repo with `careerbrew-plugin.yaml` at its root.
It layers company content on top of the generic base: a promotion ladder,
job sources, rules, prompts, skills, evidence sources, sensitive terms, and a
data policy. The base repo never holds company content; plugins install into
`<data root>/plugins/<name>/`, which git never tracks.

Compared with agentbrew team overlays, this adds:
- several plugins at once, with conflict detection on resource ids;
- a lock file with a content hash per plugin, and `verify` to detect tampering;
- declarative only: install never runs plugin code; a `command` evidence source
  runs only after the user passes `--allow-exec` for that plugin;
- policies merge to the strictest value (allowed model routes intersect,
  encryption requirements OR together);
- clean removal: every resource and record carries `origin: plugin:<name>`,
  and `remove --purge-data` deletes the records that plugin produced.

Lifecycle: state lives in `<data root>/plugins.lock.json` (one file per data
root); nothing is cached in process memory.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import tempfile
from pathlib import Path
from typing import Any

import yaml

from . import __version__, events, paths, store
from .agent_workflows import command_report_ready, primary_agent_roots

MANIFEST = "careerbrew-plugin.yaml"
_NAME = re.compile(r"^[a-z][a-z0-9-]{1,39}$")
RESOURCE_KINDS = ("ladders", "job_sources", "rules", "prompts", "skills", "evidence_sources", "knowledge", "rubrics", "commands")


class PluginError(RuntimeError):
    pass


def lock_path() -> Path:
    return paths.sub("plugins.lock.json")


def plugins_dir() -> Path:
    d = paths.sub("plugins", ".keep").parent
    d.mkdir(parents=True, exist_ok=True)
    return d


def commands_dir() -> Path:
    """Legacy output location, inspected only for verified migration or owner collisions."""
    env = os.environ.get("CAREERBREW_COMMANDS_DIR")
    return Path(env).expanduser() if env else paths.REPO_ROOT / ".claude" / "commands"


def _command_files(manifest: dict[str, Any], root: Path) -> list[Path]:
    rel = (manifest.get("provides") or {}).get("commands")
    out: list[Path] = []
    for item in (rel if isinstance(rel, list) else [rel]) if rel else []:
        for match in sorted(root.glob(str(item))):
            out.extend(sorted(match.glob("*.md")) if match.is_dir() else [match] if match.suffix == ".md" else [])
    return out


def _check_commands(name: str, manifest: dict[str, Any], root: Path, lock: dict[str, Any]) -> None:
    """Never overwrite a command this plugin does not own."""
    owned = {f: other for other, e in lock["plugins"].items() for f in e.get("commands", [])}
    for f in _command_files(manifest, root):
        if (owned.get(f.name) not in (None, name)) or ((commands_dir() / f.name).exists() and owned.get(f.name) != name):
            raise PluginError(f"command /{f.stem} already exists and plugin {name} does not own it (fix: rename {f.name} in the plugin)")


def _install_commands(name: str, manifest: dict[str, Any], root: Path, lock: dict[str, Any]) -> list[str]:
    """Record selected canonical source names; agentbrew alone renders host outputs."""
    return [f.name for f in _command_files(manifest, root)]


def _migrate_legacy_commands(name: str, lock: dict[str, Any]) -> None:
    """Archive only old-source-identical owned leaves; archive lifecycle is data-root scoped."""
    owned = lock["plugins"].get(name, {}).get("commands", [])
    old_root = plugins_dir() / name
    old_manifest = store.read_yaml(old_root / MANIFEST, {}) if (old_root / MANIFEST).is_file() else {}
    originals = {p.name: p for p in _command_files(old_manifest or {}, old_root)}
    leaves = [(commands_dir() / filename, originals.get(filename)) for filename in owned]
    for leaf, original in leaves:
        if not leaf.exists() and not leaf.is_symlink():
            continue
        if leaf.is_symlink() or original is None or not original.is_file() or leaf.read_bytes() != original.read_bytes():
            raise PluginError("legacy command edited or prior source unavailable; preserve it and adopt changes into its source before refresh")
    for leaf, original in leaves:
        if not leaf.is_file() or original is None:
            continue
        content = leaf.read_bytes()
        digest = hashlib.sha256(content).hexdigest()
        archive = paths.data_root() / "command-archive" / name / digest / leaf.name
        if archive.exists() and (store.read_text(archive) or "").encode("utf-8") != content:
            raise PluginError("legacy command archive verification failed; existing archive preserved")
        if not archive.exists():
            store.write_text(archive, content.decode("utf-8"))
        if (store.read_text(archive) or "").encode("utf-8") != content:
            raise PluginError("legacy command archive verification failed; original preserved")
        receipt = {"sha256": digest, "archive": str(archive), "restore_target": str(leaf), "source": str(original)}
        receipt_path = archive.with_suffix(".receipt.json")
        store.write_json(receipt_path, receipt)
        if store.read_json(receipt_path, {}) != receipt:
            raise PluginError("legacy command archive receipt verification failed; original preserved")
        leaf.unlink()


def _run_agentbrew(folder: Path) -> None:
    """Run the existing command deployer in one private fragment's ownership scope."""
    command = shutil.which("agentbrew")
    if not command:
        raise PluginError("command source saved, but agentbrew is missing; install agentbrew and run careerbrew plugin refresh")
    try:
        result = subprocess.run([command, "sync", "--only", "commands"], cwd=folder, capture_output=True, text=True, timeout=120, check=False)  # noqa: S603
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise PluginError("common command sync unavailable; run careerbrew plugin refresh after repairing agentbrew") from exc
    if result.returncode:
        raise PluginError("common command sync failed; source and owner files preserved; inspect agentbrew status, then careerbrew plugin refresh")
    fragment = yaml.safe_load((folder / "Agentfile.yaml").read_text()) or {}
    requested = [str(Path(value).resolve()) for value in fragment.get("commands", [])]
    required = set(primary_agent_roots())
    if not requested or not required:
        return
    try:
        readiness = subprocess.run([command, "commands", "list", "--json"], cwd=folder, capture_output=True, text=True, timeout=120, check=False)  # noqa: S603
        report = json.loads(readiness.stdout) if readiness.returncode == 0 else None
    except (OSError, ValueError, subprocess.TimeoutExpired):
        report = None
    if not command_report_ready(report, requested, required):
        raise PluginError("requested command readiness failed or unavailable; owner files preserved; update agentbrew, inspect command readiness, then careerbrew plugin refresh")


def _sync_command_sources(lock: dict[str, Any]) -> None:
    """Private source-path config is plaintext metadata; Careerbrew records stay encrypted."""
    files = []
    for name, entry in sorted(lock["plugins"].items()):
        root = plugins_dir() / name
        manifest = store.read_yaml(root / MANIFEST, {}) or {}
        files.extend(str(p.resolve()) for p in _command_files(manifest, root))
        if entry.get("commands") and not any(p.is_file() for p in _command_files(manifest, root)):
            raise PluginError("plugin command source missing; run careerbrew plugin refresh")
    folder = paths.data_root() / "agentbrew"
    fragment = folder / "Agentfile.yaml"
    if not files and not fragment.exists():
        return
    folder.mkdir(mode=0o700, parents=True, exist_ok=True)
    if folder.is_symlink() or not folder.is_dir():
        raise PluginError("unsafe agent metadata directory; inspect the plugin data folder")
    folder.chmod(0o700)
    fd, name = tempfile.mkstemp(prefix=".Agentfile.", suffix=".tmp", dir=folder)
    temp = Path(name)
    owner = None
    try:
        owner = os.fstat(fd)
        with os.fdopen(fd, "w", encoding="utf-8", closefd=False) as stream:
            yaml.safe_dump({"commands": sorted(set(files))}, stream)
        current = temp.lstat()
        if not stat.S_ISREG(current.st_mode) or (current.st_dev, current.st_ino) != (owner.st_dev, owner.st_ino):
            raise PluginError("agent metadata temporary ownership changed; inspect the plugin data folder")
        temp.replace(fragment)
    finally:
        try:
            if owner is not None:
                current = temp.lstat()
                if stat.S_ISREG(current.st_mode) and (current.st_dev, current.st_ino) == (owner.st_dev, owner.st_ino):
                    temp.unlink()
        except FileNotFoundError:
            pass
        finally:
            os.close(fd)
    _run_agentbrew(folder)


def _lock() -> dict[str, Any]:
    return store.read_json(lock_path(), {"plugins": {}})


def _save_lock(lock: dict[str, Any]) -> None:
    store.write_json(lock_path(), lock)


def tree_hash(root: Path) -> str:
    h = hashlib.sha256()
    for path in sorted(p for p in root.rglob("*") if p.is_file() and ".git" not in p.parts):
        h.update(path.relative_to(root).as_posix().encode())
        h.update(b"\0")
        h.update(path.read_bytes())
    return h.hexdigest()


def _version_tuple(v: str) -> tuple[int, ...]:
    return tuple(int(x) for x in re.findall(r"\d+", v)[:3])


def validate_manifest(data: Any, root: Path) -> dict[str, Any]:
    if not isinstance(data, dict):
        raise PluginError(f"{MANIFEST} must be a mapping")
    name = data.get("name")
    if not isinstance(name, str) or not _NAME.match(name):
        raise PluginError("plugin name must match ^[a-z][a-z0-9-]{1,39}$ (fix: set `name:` in the manifest)")
    if "version" not in data:
        raise PluginError("plugin manifest needs `version:`")
    req = str(data.get("requires", "")).lstrip(">= ")
    if req and _version_tuple(__version__) < _version_tuple(req):
        raise PluginError(f"plugin needs careerbrew >= {req}; this is {__version__} (fix: update careerbrew)")
    provides = data.get("provides", {}) or {}
    unknown = set(provides) - set(RESOURCE_KINDS)
    if unknown:
        raise PluginError(f"unknown provides keys: {sorted(unknown)}; allowed: {list(RESOURCE_KINDS)}")
    links = [p for p in root.rglob("*") if p.is_symlink()]
    if links:
        raise PluginError(f"plugin contains a symlink ({links[0].relative_to(root)}); plugins must be plain files")
    for kind, rel in provides.items():
        for item in rel if isinstance(rel, list) else [rel]:
            target = (root / str(item)).resolve()
            if Path(str(item)).is_absolute() or ".." in Path(str(item)).parts or not target.is_relative_to(root.resolve()):
                raise PluginError(f"provides.{kind} escapes the plugin folder: {item}")
            if not any(root.glob(str(item))):
                raise PluginError(f"provides.{kind} points to a missing path: {item}")
    policy = data.get("policy", {}) or {}
    if not isinstance(policy, dict):
        raise PluginError("policy must be a mapping")
    defaults = data.get("connector_defaults") or {}
    if not isinstance(defaults, dict) or any(not isinstance(v, dict) for v in defaults.values()):
        raise PluginError("connector_defaults maps a connector kind to its fields, e.g. {jira: {base_url: ...}}")
    agent_reads = data.get("agent_reads") or []
    if not isinstance(agent_reads, list) or any(not isinstance(k, str) for k in agent_reads):
        raise PluginError("agent_reads is a list of connector kinds the agent reads itself, e.g. [slack, gdocs]")
    live = data.get("live") or {}
    if not isinstance(live, dict) or set(live) - {"sources", "team"}:
        raise PluginError("live maps sources and team to files in the plugin, e.g. {sources: live/sources.yaml, team: live/team.yaml}")
    for key, rel in live.items():
        target = (root / str(rel)).resolve()
        if Path(str(rel)).is_absolute() or not target.is_relative_to(root.resolve()) or not target.is_file():
            raise PluginError(f"live.{key} must be a file inside the plugin folder: {rel}")
    from .workday import steps  # local: steps reads the installed plugins

    try:
        steps.validate(data.get("work_steps"), f"plugin:{name}")
    except steps.StepError as e:
        raise PluginError(f"{e} (fix: list names like /standup under `work_steps:`)") from None
    return data


def _fetch(source: str, dest: Path) -> None:
    src = Path(source).expanduser()
    if src.is_dir():
        shutil.copytree(src, dest, symlinks=True, ignore=shutil.ignore_patterns(".git"))
        return
    res = subprocess.run(["git", "clone", "--depth", "1", source, str(dest)], capture_output=True, text=True, check=False)
    if res.returncode != 0:
        raise PluginError(f"could not clone plugin source (fix: check the URL and your git access): {res.stderr.strip()[:200]}")


def add(source: str, allow_exec: bool = False) -> dict[str, Any]:
    staging = plugins_dir() / ".staging"
    shutil.rmtree(staging, ignore_errors=True)
    _fetch(source, staging)
    manifest_path = staging / MANIFEST
    if not manifest_path.exists():
        shutil.rmtree(staging, ignore_errors=True)
        raise PluginError(f"no {MANIFEST} at the plugin root")
    try:
        manifest = validate_manifest(store.read_yaml(manifest_path), staging)
    except PluginError:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    name = manifest["name"]
    lock = _lock()
    base = manifest.get("extends")
    if base and base not in lock["plugins"]:
        shutil.rmtree(staging, ignore_errors=True)
        raise PluginError(f"plugin {name} extends {base}, which is not installed (fix: `careerbrew plugin add <{base} repo>` first)")
    _check_conflicts(name, manifest, staging, lock)
    try:
        _check_commands(name, manifest, staging, lock)
    except PluginError:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    _migrate_legacy_commands(name, lock)
    dest = plugins_dir() / name
    shutil.rmtree(dest, ignore_errors=True)
    staging.rename(dest)
    dest.chmod(0o700)
    commands = _install_commands(name, manifest, dest, lock)
    defaults = manifest.get("settings_defaults") or {}
    if defaults:
        settings_path = paths.data_root() / "settings.yaml"
        settings = store.read_yaml(settings_path, {}) or {}
        # Values the user has not chosen themselves: the generic ones, or the base plugin's own defaults.
        replaceable = {"ladder": {None, "generic-swe"}, "target_level": {None, "senior"}}
        if base:
            base_defaults = (store.read_yaml(plugins_dir() / base / MANIFEST, {}) or {}).get("settings_defaults") or {}
            for k in replaceable:
                replaceable[k].add(base_defaults.get(k))
        for k, v in defaults.items():
            if k in replaceable and isinstance(v, str) and settings.get(k) in replaceable[k]:
                settings[k] = v
        store.write_yaml(settings_path, settings)
    prior = lock["plugins"].get(name) or {}
    from .calendar import now_utc  # local: calendar reaches this module through connectors

    now_iso = now_utc().isoformat(timespec="seconds")  # the pinned clock, like the evidence that setup time counts to
    lock["plugins"][name] = {
        "name": name,
        "version": str(manifest["version"]),
        "source": source if not Path(source).expanduser().is_dir() else "local-folder",
        # The folder stays in the private lock only, so `plugin refresh` can reinstall without printing it.
        "local_path": str(Path(source).expanduser().resolve()) if Path(source).expanduser().is_dir() else None,
        "tree_sha256": tree_hash(dest),
        "installed_at": now_iso,
        # kept across `plugin refresh`, so setup time (US-22) is measured from the first install
        "first_installed_at": prior.get("first_installed_at") or prior.get("installed_at") or now_iso,
        "exec_allowed": bool(allow_exec),
        "extends": base,
        "commands": commands,
        "origin": f"plugin:{name}",
    }
    _save_lock(lock)
    _sync_command_sources(lock)
    events.emit("plugin.add", status="ok")
    return lock["plugins"][name]


def _resource_ids(manifest: dict[str, Any], root: Path) -> set[str]:
    ids = set()
    for kind, rel in (manifest.get("provides") or {}).items():
        for item in rel if isinstance(rel, list) else [rel]:
            for p in root.glob(str(item)):
                if kind == "ladders" and p.is_file():
                    data = store.read_yaml(p, {}) or {}
                    ids.add(f"ladder:{data.get('id', p.stem)}")
    return ids


def _check_conflicts(name: str, manifest: dict[str, Any], root: Path, lock: dict[str, Any]) -> None:
    mine = _resource_ids(manifest, root)
    for other in lock["plugins"]:
        if other == name:
            continue
        other_root = plugins_dir() / other
        other_manifest = store.read_yaml(other_root / MANIFEST, {}) or {}
        clash = mine & _resource_ids(other_manifest, other_root)
        if clash:
            raise PluginError(f"plugin {name} conflicts with {other} on {sorted(clash)} (fix: rename the ids or remove {other})")


def remove(name: str, purge_data: bool = False) -> dict[str, int]:
    lock = _lock()
    if name not in lock["plugins"]:
        raise PluginError(f"plugin {name} is not installed (see `careerbrew plugin list`)")
    dependents = [p["name"] for p in installed() if (p["manifest"].get("extends") == name)]
    if dependents:
        raise PluginError(f"plugin {name} is extended by {dependents}; remove those first")
    _migrate_legacy_commands(name, lock)
    shutil.rmtree(plugins_dir() / name, ignore_errors=True)
    del lock["plugins"][name]
    _save_lock(lock)
    _sync_command_sources(lock)
    purged = 0
    if purge_data:
        purged = purge_origin(f"plugin:{name}")
    # Derived outputs (docs, charts, strategy, cache) may quote this plugin's data, and its
    # sensitive terms stop protecting them after removal. They are rebuildable: delete them.
    clear_derived()
    events.emit("plugin.remove", status="ok", purged=purged)
    return {"removed": 1, "purged_records": purged}


def clear_derived() -> None:
    root = paths.data_root()
    for d in ("out", "cache"):
        shutil.rmtree(root / d, ignore_errors=True)
    (root / "jobs" / "strategy.json").unlink(missing_ok=True)


def purge_origin(origin: str) -> int:
    """Delete every JSONL record under the data root that carries this origin, and the inbox items it made."""
    from .day import inbox  # local: the inbox reads the installed plugins

    n = inbox.purge_origin(origin)
    for path in paths.data_root().rglob("*.jsonl"):
        if "logs" in path.parts:
            continue
        rows = store.read_jsonl(path)
        keep = [r for r in rows if r.get("origin") != origin]
        if len(keep) != len(rows):
            n += len(rows) - len(keep)
            store.write_jsonl(path, keep)
    return n


def refresh() -> list[str]:
    """Reinstall every plugin from where it came from (after editing it). Base plugins go first."""
    lock = _lock()["plugins"]
    order: list[str] = []

    def visit(name: str, seen: tuple[str, ...] = ()) -> None:
        if name in order or name not in lock or name in seen:
            return
        visit(lock[name].get("extends") or "", (*seen, name))
        order.append(name)

    for name in sorted(lock):
        visit(name)
    for name in order:
        e = lock[name]
        src = e.get("local_path") or (e["source"] if e["source"] != "local-folder" else None)
        if not src:
            raise PluginError(f"plugin {name} was installed before refresh existed (fix: `careerbrew plugin add <its folder>` once)")
        if e.get("local_path") and not Path(src).is_dir():
            raise PluginError(f"plugin {name}: its source folder was moved or deleted (fix: `careerbrew plugin add <its new folder>`); the installed copy is unchanged")
        add(src, allow_exec=bool(e.get("exec_allowed")))
    if not order:
        _sync_command_sources(_lock())
    return order


def installed(*, read_root: Path | None = None) -> list[dict[str, Any]]:
    out = []
    lock = _lock() if read_root is None else store.read_json(read_root / "plugins.lock.json", {"plugins": {}}, create_key=False)
    for name, entry in sorted(lock["plugins"].items()):
        root = (plugins_dir() if read_root is None else read_root / "plugins") / name
        manifest = store.read_yaml(root / MANIFEST, {}, create_key=read_root is None) or {}
        terms = manifest.get("sensitive_terms")
        out.append(
            {
                **entry,
                "root": str(root),
                "manifest": manifest,
                "sensitive_terms_path": str(root / terms) if terms else None,
            }
        )
    return out


def verify() -> list[tuple[str, bool]]:
    out = []
    for p in installed():
        out.append((p["name"], tree_hash(Path(p["root"])) == p["tree_sha256"]))
    return out


def resources(kind: str) -> list[tuple[str, Path]]:
    """(origin, path) for every resource of a kind: base first, then plugins."""
    if kind not in RESOURCE_KINDS:
        raise PluginError(f"unknown resource kind {kind}")
    out: list[tuple[str, Path]] = []
    base = paths.REPO_ROOT / "knowledge" / kind
    if base.is_dir():
        out.extend(("base", p) for p in sorted(base.glob("*")) if p.is_file())
    for p in installed():
        rel = (p["manifest"].get("provides") or {}).get(kind)
        if not rel:
            continue
        root = Path(p["root"])
        for item in rel if isinstance(rel, list) else [rel]:
            for match in sorted(root.glob(str(item))):
                files = sorted(match.rglob("*")) if match.is_dir() else [match]
                out.extend((p["origin"], f) for f in files if f.is_file())
    return out


def effective_policy(*, read_root: Path | None = None) -> dict[str, Any]:
    """Strictest merge of every plugin policy."""
    policy: dict[str, Any] = {
        "allowed_routes": None,
        "allowed_connectors": None,
        "allowed_hosts": None,
        "secret_backends": None,
        "require_encrypted_backup": False,
        "backup_targets": None,
        "export_to_personal": None,
        "agent_may_read_business": False,
        "public_metrics": True,
    }
    export_order = ["none", "aggregates", "redacted", "full"]
    agent_votes: list[bool] = []
    for p in installed(read_root=read_root) if read_root is not None else installed():
        pol = p["manifest"].get("policy") or {}
        for key in ("allowed_routes", "allowed_connectors", "allowed_hosts", "secret_backends"):
            if key in pol:
                mine = set(pol[key])
                cur = policy[key]
                policy[key] = sorted(mine if cur is None else set(cur) & mine)
        if "export_to_personal" in pol:
            level = pol["export_to_personal"] if pol["export_to_personal"] in export_order else "none"
            cur = policy["export_to_personal"]
            policy["export_to_personal"] = level if cur is None else export_order[min(export_order.index(cur), export_order.index(level))]
        if pol.get("public_metrics") is False:
            policy["public_metrics"] = False
        if "agent_may_read_business" in pol:
            agent_votes.append(bool(pol["agent_may_read_business"]))
        if pol.get("require_encrypted_backup"):
            policy["require_encrypted_backup"] = True
        if "backup_targets" in pol:
            mine = {os.path.expanduser(t) for t in pol["backup_targets"]}
            cur = policy["backup_targets"]
            policy["backup_targets"] = sorted(mine if cur is None else set(cur) & mine)
    # Agents may read business data only if some plugin allows it and none forbids it.
    policy["agent_may_read_business"] = bool(agent_votes) and all(agent_votes)
    return policy


def detect() -> list[tuple[str, list[str]]]:
    """Which installed plugins' detect signals fire on this machine (env vars only)."""
    out = []
    for p in installed():
        det = p["manifest"].get("detect") or {}
        fired = [f"env:{e}" for e in det.get("env", []) if os.environ.get(e)]
        out.append((p["name"], fired))
    return out
