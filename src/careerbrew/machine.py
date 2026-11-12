"""Machine role: corporate or personal, and how the two work together.

- personal: everything works. Job-hunt data lives only here.
- corporate: work mode and connectors work. Job-hunt mode (jobs, drill) is
  blocked, because a corporate machine may be monitored and job-hunt data
  must never sit on employer hardware.

Plugin `detect` signals (env vars, paths such as an MDM profile folder)
suggest the corporate role; the user confirms with `careerbrew machine set`.
State: `<data root>/machine.yaml` (one per data root).
"""

from __future__ import annotations

import fcntl
import json
import os
import stat
from pathlib import Path
from typing import Any

import yaml

from . import events, paths, plugins, store

ROLES = ("personal", "corporate")
PERSONAL_ONLY = ("jobs", "drill", "interview", "interview-patterns", "prep", "cards", "library", "import")


class RoleError(PermissionError):
    pass


def set_profile_fields(fields: dict[str, str]) -> list[str]:
    """Update only the two owner-supplied identity fields without an editor."""
    if not fields or set(fields) - {"name", "employer"}:
        raise ValueError("choose name or employer for the profile")
    if any(not isinstance(value, str) or not value.strip() or len(value) > 256 or any(ord(c) < 32 or ord(c) == 127 for c in value) for value in fields.values()):
        raise ValueError("profile fields must be nonempty single-line text")
    root = paths.data_root().resolve()
    target = root / "profile.yaml"
    nofollow = getattr(os, "O_NOFOLLOW", 0)
    if not nofollow:
        raise ValueError("safe profile updates are unavailable on this platform")
    lock_fd = os.open(root / ".profile-set.lock", os.O_RDWR | os.O_CREAT | os.O_NONBLOCK | nofollow, 0o600)
    with os.fdopen(lock_fd, "a+") as lock:
        if not stat.S_ISREG(os.fstat(lock.fileno()).st_mode):
            raise ValueError("profile lock is not a regular file")
        fcntl.flock(lock, fcntl.LOCK_EX)
        try:
            text = store.read_text_nofollow(target, create_key=False)
            if any(isinstance(token, (yaml.tokens.AnchorToken, yaml.tokens.AliasToken)) for token in yaml.scan(text)):
                raise ValueError("profile anchors need the editor")
            node = yaml.compose(text, Loader=yaml.SafeLoader)
        except (OSError, UnicodeError, yaml.YAMLError) as exc:
            raise ValueError("profile cannot be read safely; check the file and key") from exc
        if not isinstance(node, yaml.nodes.MappingNode):
            raise ValueError("profile must be a YAML mapping")
        found: dict[str, yaml.nodes.ScalarNode] = {}
        seen: set[str] = set()
        for key, value in node.value:
            if not isinstance(key, yaml.nodes.ScalarNode) or key.tag != "tag:yaml.org,2002:str" or key.value in seen:
                raise ValueError("profile must have distinct text keys")
            seen.add(key.value)
            if key.value in fields:
                if not isinstance(value, yaml.nodes.ScalarNode) or value.tag not in ("tag:yaml.org,2002:str", "tag:yaml.org,2002:null") or value.style in ("|", ">") or value.start_mark.line != value.end_mark.line:
                    raise ValueError("profile field needs the editor")
                found[key.value] = value
        for name, value in sorted(found.items(), key=lambda row: row[1].start_mark.index, reverse=True):
            separator = " " if value.start_mark.index == value.end_mark.index else ""
            text = text[: value.start_mark.index] + separator + json.dumps(fields[name], ensure_ascii=False) + text[value.end_mark.index :]
        missing = sorted(set(fields) - set(found))
        if missing:
            text = text.rstrip("\n") + "\n" + "".join(f"{name}: {json.dumps(fields[name], ensure_ascii=False)}\n" for name in missing)
        try:
            updated = yaml.safe_load(text)
        except yaml.YAMLError as exc:
            raise ValueError("profile cannot be updated safely; use the editor") from exc
        if not isinstance(updated, dict) or any(updated.get(name) != value for name, value in fields.items()):
            raise ValueError("profile cannot be updated safely; use the editor")
        store.write_text(target, text)
    return sorted(fields)


def _path() -> Path:
    return paths.data_root() / "machine.yaml"


def info() -> dict[str, Any]:
    return store.read_yaml(_path(), {}) or {}


def role() -> str:
    return info().get("role", "personal")


PERSONAL_DIRS = ("jobs", "tutor", "prep", "linkedin")
JOB_HUNT_FIELDS = ("target_salary", "remote_preference", "want_to_learn")


def work_profile(text: str) -> str:
    """The example profile without job-hunt fields (data minimization on employer hardware)."""
    return "".join(line for line in text.splitlines(keepends=True) if line.split(":", 1)[0].strip() not in JOB_HUNT_FIELDS)


def has_job_hunt_fields(text: str) -> bool:
    """Whether profile text holds a job-hunt field. Parsed as YAML, so a quoted key or a `{...}` mapping counts too.

    Text that holds something other than a YAML mapping answers True: an unreadable profile is refused, never guessed.
    An empty or comments-only file has no fields and answers False."""
    try:
        data = yaml.safe_load(text)
    except yaml.YAMLError:
        return True
    if data is None:  # an empty or comments-only file has no fields; `null` or `~` is not a mapping
        return any(line.strip() and not line.strip().startswith("#") for line in text.splitlines())
    return not isinstance(data, dict) or any(str(key).strip() in JOB_HUNT_FIELDS for key in data)


def _profile_state() -> tuple[Path, str, bool]:
    prof = paths.data_root() / "profile.yaml"
    text = store.read_text(prof, "") or ""
    example = (paths.REPO_ROOT / "profile" / "examples" / "profile.yaml").read_text(encoding="utf-8")
    return prof, text, text.strip() == example.strip()


def _minimize_profile() -> None:
    prof, text, untouched = _profile_state()
    if untouched:
        store.write_text(prof, work_profile(text))


def _check_profile() -> None:
    _prof, text, untouched = _profile_state()
    if not untouched and has_job_hunt_fields(text):
        raise RoleError(f"profile.yaml has job-hunt fields or is not a YAML mapping ({', '.join(JOB_HUNT_FIELDS)}); move them to your personal machine and delete them here, then retry")


PERSONAL_OUTPUTS = ("interview-*.md", "strategy.md", "progress.html", "reports/*")  # reports/: run pages may quote job-hunt items


def _data_files(folder: Path) -> list[Path]:
    """The files in a job-hunt folder, without Finder's `.DS_Store` and `store` lock files: those hold no data.

    Other dotfiles count: a temp file a crashed write left (`.<name>.<pid>.<hex>.tmp`) holds a full copy."""
    return [f for f in folder.rglob("*") if f.is_file() and f.name != ".DS_Store" and not (f.name.startswith(".") and f.name.endswith(".lock"))]


def _clear_personal_dirs() -> None:
    """Remove untouched example files; refuse if real job-hunt data is present."""
    import shutil

    root = paths.data_root()
    examples = paths.REPO_ROOT / "profile" / "examples"
    for d in PERSONAL_DIRS:
        folder = root / d
        if not folder.exists():
            continue
        for f in _data_files(folder):
            ex = examples / f.relative_to(root)
            if not ex.exists() or ex.read_text(encoding="utf-8") != store.read_text(f):
                raise RoleError(f"job-hunt data found in {d}/; move it to your personal machine, then retry (a backup holds the whole data folder: move one only when this install holds no employer data)")
        shutil.rmtree(folder)
    for pattern in PERSONAL_OUTPUTS:  # derived job-hunt reports: rebuildable, so removed
        for f in (root / "out").glob(pattern):
            f.unlink()


def set_role(new: str, label: str = "") -> dict[str, Any]:
    if new not in ROLES:
        raise ValueError(f"role must be one of {ROLES}")
    if new == "corporate":
        _check_profile()  # refuse before deleting anything
        _check_goals()
        _clear_personal_dirs()
        _minimize_profile()
    data = {**info(), "role": new, "label": label or info().get("label") or new}
    store.write_yaml(_path(), data)
    return data


def suggest() -> str | None:
    for p in plugins.installed():
        det = p["manifest"].get("detect") or {}
        if any(os.environ.get(e) for e in det.get("env", [])) or any(Path(x).expanduser().exists() for x in det.get("paths", [])):
            return "corporate"
    return None


def guard(command_group: str) -> None:
    if command_group in PERSONAL_ONLY and role() == "corporate":
        raise RoleError(f"`{command_group}` is personal-only and this machine's role is corporate. Run it on your personal machine (job-hunt data never goes on employer hardware).")


CLOUD_DIRS = ("Dropbox", "Google Drive", "OneDrive", "Box", "pCloud Drive", "Sync")


def _check_goals() -> None:
    from . import goals

    try:
        found = goals.job_hunt_goals(store.read_yaml(paths.data_root() / "goals.yaml", {}))
    except Exception:  # noqa: BLE001 (a goals file this machine cannot read is refused, never kept unchecked)
        found = 1
    if found:
        raise RoleError("job-hunt goals found in goals.yaml (interview or job-search, upcoming or past); move the file to your personal machine, then retry")


def cloud_synced(path: Path) -> str:
    """The cloud service that uploads this folder, or "" (macOS locations; nothing is uploaded on a match)."""
    home = Path.home().resolve()
    p = Path(path).expanduser().resolve()
    if not p.is_relative_to(home):
        return ""
    parts = p.relative_to(home).parts
    if parts[:2] == ("Library", "Mobile Documents"):
        return "iCloud Drive"
    if parts[:2] == ("Library", "CloudStorage") and len(parts) > 2:
        return parts[2].split("-")[0]
    if parts and (parts[0] in CLOUD_DIRS or parts[0].startswith("OneDrive")):
        return parts[0]
    icloud = home / "Library" / "Mobile Documents" / "com~apple~CloudDocs"
    if parts and parts[0] in ("Desktop", "Documents") and (icloud / parts[0]).is_dir():
        return "iCloud Desktop & Documents"
    return ""


def plugin_terms() -> list[str]:
    """The installed plugins' sensitive terms only: a personal name or a common-word employer can appear in the repo by chance."""
    lines = [ln.strip() for p in plugins.installed() if p.get("sensitive_terms_path") for ln in Path(p["sensitive_terms_path"]).read_text(encoding="utf-8").splitlines()]
    return [ln for ln in lines if ln and not ln.startswith("#")]


PER_USER_KEYS = ("email", "user", "authors")
PER_USER_CHECK = "the company plugin evidence file holds no per-person values (emails, logins, home paths)"
EVIDENCE_KEYS = ("sources", "connectors")


def _per_user(item: Any) -> bool:
    return isinstance(item, dict) and (any(k in item for k in PER_USER_KEYS) or str(item.get("path", "")).startswith(("/Users/", "/home/", "~")))


def per_user_values_in_plugins() -> list[str]:
    """Plugin evidence files that carry one person's values: a second user would inherit them (issue #48)."""
    found = []
    for origin, path in plugins.resources("evidence_sources"):
        data = store.read_yaml(path, {}) or {}
        if any(_per_user(it) for key in EVIDENCE_KEYS for it in data.get(key) or []):
            found.append(f"{origin}:{path.name}")
    return found


def split_plugin_evidence(dry_run: bool = False) -> dict[str, Any]:
    """Move per-person sources and connectors out of each plugin's evidence file into this person's private
    `work/sources.yaml` and `work/connectors.yaml`, as they are; shared entries stay (issue #102).

    It edits the plugin's own folder (named in the private lock) and reinstalls the plugin from it, so
    `plugin verify` and `plugin refresh` keep working. A plugin with no folder here (installed from a git
    URL, or the folder moved) is skipped with the fix. `command` sources stay: they run inside the plugin.
    Exact duplicates of entries you already have are not added twice.
    """
    from .work import evidence

    files = {"sources": paths.sub("work", "sources.yaml"), "connectors": paths.sub("work", "connectors.yaml")}
    data = {k: store.read_yaml(p, {}) or {} for k, p in files.items()}
    mine = {k: list(data[k].get(k) or []) for k in EVIDENCE_KEYS}
    had = {k: len(mine[k]) for k in EVIDENCE_KEYS}
    total = len(evidence.sources_list())
    by_origin = {p["origin"]: p for p in plugins.installed()}
    flagged = set(per_user_values_in_plugins())
    rows: list[dict[str, Any]] = []
    edits: list[tuple[Path, dict[str, Any]]] = []
    reinstall: dict[str, dict[str, Any]] = {}
    moved_total = 0
    for origin, path in plugins.resources("evidence_sources"):
        p = by_origin.get(origin)
        if p is None:
            continue
        rel = path.relative_to(p["root"])
        folder = Path(p["local_path"]) if p.get("local_path") else None
        if folder is None or not folder.is_dir():
            if f"{origin}:{path.name}" in flagged:
                rows.append({"plugin": p["name"], "file": rel.as_posix(), "skipped": "no plugin folder on this machine to edit (installed from a git URL, or the folder moved): run `careerbrew plugin add <its folder>` once, then rerun"})
            continue
        target = folder / rel
        doc = store.read_yaml(target, {}) or {}
        moved = {k: [it for it in doc.get(k) or [] if _per_user(it) and it.get("kind") != "command"] for k in EVIDENCE_KEYS}
        if not any(moved.values()):
            continue
        before = sum(len(doc.get(k) or []) for k in EVIDENCE_KEYS)
        count = sum(len(v) for v in moved.values())
        moved_total += count
        rows.append({"plugin": p["name"], "file": rel.as_posix(), "before": before, "after": before - count, "sources": len(moved["sources"]), "connectors": len(moved["connectors"])})
        for k in EVIDENCE_KEYS:
            mine[k] += [it for it in moved[k] if it not in mine[k]]
        edits.append((target, {**doc, **{k: [it for it in doc.get(k) or [] if it not in moved[k]] for k in EVIDENCE_KEYS if k in doc}}))
        reinstall[p["name"]] = p
    if edits and not dry_run:
        # Private files first: if a later step fails, nothing is lost (an entry in both places is read once by id).
        for k in EVIDENCE_KEYS:
            if len(mine[k]) != had[k]:
                store.write_yaml(files[k], {**data[k], k: mine[k]})
        header = "# Shared evidence sources only. Per-person sources and connectors live in each person's private\n# work/sources.yaml and work/connectors.yaml (`careerbrew machine split-plugin-evidence`).\n"
        for target, doc in edits:
            target.write_text(header + yaml.safe_dump(doc, sort_keys=False, allow_unicode=True), encoding="utf-8")  # plugin files are plain by design
        for p in reinstall.values():
            plugins.add(p["local_path"], allow_exec=bool(p.get("exec_allowed")))
    added = sum(len(mine[k]) - had[k] for k in EVIDENCE_KEYS)
    after = total - moved_total + added if dry_run or not edits else len(evidence.sources_list())
    left = per_user_values_in_plugins()
    skipped = sum(1 for r in rows if r.get("skipped"))
    events.emit("machine.split_evidence", status="dry_run" if dry_run else "ok" if edits else "none", moved=moved_total, skipped=skipped, left=len(left))
    return {"plugins": rows, "mine": {k: (had[k], len(mine[k])) for k in EVIDENCE_KEYS}, "total": (total, after), "left": left, "dry_run": dry_run}


def find_repos(root: Path, depth: int = 2) -> list[Path]:
    """Git repositories under root (the folder itself and up to `depth` levels below)."""
    root = root.expanduser()
    out: list[Path] = []
    frontier = [root]
    for _ in range(depth + 1):
        nxt = []
        for d in frontier:
            if (d / ".git").exists():
                out.append(d)
                continue
            nxt += sorted(p for p in d.iterdir() if p.is_dir() and not p.name.startswith(".")) if d.is_dir() else []
        frontier = nxt
    return out


def new_user(repos_root: Path, name: str, employer: str, email: str, github_user: str = "") -> dict[str, Any]:
    """Set up this person on a corporate machine that already has the company plugin: role, profile, their
    git sources and connectors (from the plugin's connector_defaults), then readiness (issue #48)."""
    from . import wizard

    company = next((p for p in plugins.installed()), None)
    if company is None:
        raise ValueError("install the company plugin first: careerbrew plugin add <its git URL or folder>")
    if not email:
        raise ValueError("no email: set it with `git config --global user.email you@company.example` or pass --email")
    if role() != "corporate":
        set_role("corporate")
    prof_path = paths.data_root() / "profile.yaml"
    prof = store.read_yaml(prof_path, {}) or {}
    if prof.get("name") in (None, "", "Your Name"):
        prof["name"] = name
    if prof.get("employer") in (None, "", "Your Employer"):
        prof["employer"] = employer
    store.write_yaml(prof_path, prof)
    repos = find_repos(repos_root)
    store.write_yaml(paths.sub("work", "sources.yaml"), {"sources": [{"kind": "git", "path": str(r), "authors": [email]} for r in repos]})
    pol = company["manifest"].get("policy") or {}
    tools = list(pol.get("allowed_connectors") or (company["manifest"].get("connector_defaults") or {}).keys())
    wizard.write_user_connectors(wizard.user_connectors(company["name"], tools, {"email": email, "user": github_user}))
    todo = [f"careerbrew secret set {wizard.service_prefix(company['name'])}.{t}" for t in tools]
    return {"plugin": company["name"], "repos": len(repos), "tools": tools, "todo": todo, "readiness": readiness()}


def readiness() -> list[dict[str, Any]]:
    """Is this corporate machine ready for work mode with a real company plugin?

    Each row: {ok, check, fix}. `careerbrew machine ready` prints them; exit 1 if any fail.
    """
    from . import privacy
    from .work import promo

    root = paths.data_root()
    rows: list[dict[str, Any]] = []

    def row(ok: bool, check: str, fix: str) -> None:
        rows.append({"ok": bool(ok), "check": check, "fix": "" if ok else fix})

    row(role() == "corporate", "machine role is corporate", "careerbrew machine set corporate --label <name>")
    prof = root / "profile.yaml"
    text = store.read_text(prof, "") or ""
    row(prof.exists() and not has_job_hunt_fields(text), "profile.yaml exists and has no job-hunt fields", "careerbrew init, then delete job-hunt fields from profile.yaml")
    prof_data = store.read_yaml(prof, {}) or {} if prof.exists() else {}
    row(
        prof_data.get("name") not in (None, "", "Your Name") and prof_data.get("employer") not in (None, "", "Your Employer"),
        "profile.yaml has your real name and company (both become leak terms)",
        "set name and employer with `careerbrew data set profile.yaml --name '<name>' --employer '<employer>'`",
    )
    # a folder with no data file holds no job-hunt data: `paths.sub` makes parents on a read too (issue #183)
    row(not any(_data_files(root / d) for d in PERSONAL_DIRS), "no job-hunt folders on this machine", "careerbrew machine set corporate (it removes untouched examples and refuses real job-hunt data)")
    installed = plugins.installed()
    row(bool(installed), "a company plugin is installed", "careerbrew plugin new <company> --connect jira,github,slack (see docs/corporate-machine.md)")
    row(bool(installed) and all(ok for _, ok in plugins.verify()), "installed plugins match their lock hashes", "careerbrew plugin refresh")
    generic = {k: v for k, v in (store.read_yaml(paths.REPO_ROOT / "knowledge" / "ladders" / "generic-swe.yaml", {}) or {}).items() if k not in ("id", "name")}
    plugin_ladders = [v for v in promo.ladders().values() if str(v.get("origin", "")).startswith("plugin:")]
    row(
        any({k: x for k, x in v.items() if k not in ("id", "name", "origin")} != generic for v in plugin_ladders),
        "a plugin ladder holds real criteria (not the generic copy)",
        "edit ladders/*.yaml in the plugin with the company's promotion criteria (promo-research skill), then careerbrew plugin refresh",
    )
    settings = store.read_yaml(root / "settings.yaml", {}) or {}
    active = promo.ladders().get(str(settings.get("ladder", "generic-swe"))) or {}
    row(str(active.get("origin", "")).startswith("plugin:"), "the active ladder comes from a plugin", "set `ladder: <org>-eng` with `careerbrew data edit settings.yaml` (plugin new does this)")
    level = str(settings.get("target_level", "senior"))
    row(level in [str(x) for x in active.get("levels", [])], f"target_level {level!r} is a level of the active ladder", f"set target_level with `careerbrew data edit settings.yaml` to one of {active.get('levels', [])}")
    cloud = cloud_synced(root)
    row(not cloud, f"the data folder is not uploaded by a cloud service{f' (found: {cloud})' if cloud else ''}", "move it out of synced folders: set CAREERBREW_HOME to ~/careerbrew-data and move the folder there")
    from . import backup

    synced = [str(t) for t in backup.configured_targets() if cloud_synced(t)]
    row(not synced, "no backup target is in a cloud-synced folder", "set backup_targets with `careerbrew data edit settings.yaml` to a local or company-managed folder")
    shared = per_user_values_in_plugins()
    row(
        not shared,
        PER_USER_CHECK,
        f"move them out of {', '.join(shared[:3])} into your private files, as they are, with `careerbrew machine split-plugin-evidence`; a coworker gets their own with `careerbrew machine new-user`",
    )
    terms = plugin_terms()
    row(len(terms) >= 2, "sensitive terms list the company plus its codenames", "add project codenames, customers, and internal hosts to the plugin's sensitive-terms.txt, then careerbrew plugin refresh")
    # Only the plugins' terms: a personal name or a common-word employer can appear in the repo by chance.
    rep = privacy.scan_repo(terms=terms)
    files = sorted({path for path, _ in rep.findings})
    row(rep.ok, f"the careerbrew repo has no company terms ({rep.files_scanned} files scanned)", f"move company content out of: {', '.join(files[:5])}; it belongs in the plugin")
    return rows
