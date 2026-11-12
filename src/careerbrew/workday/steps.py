"""Where the work day's steps come from: careerbrew's built-in steps, the `work_steps` setting, then plugins.

A step names a slash command or skill (`/careerbrew`, `/standup`, `/acme:review`). The first
declaration of a name wins, so a plugin cannot move or replace a step careerbrew or the owner set.
Each step records where it came from (`careerbrew`, `settings`, or `plugin:<name>`) and the file
it runs (`path`): careerbrew's own `commands/` first, then the agent's skills and commands folders.
"""

from __future__ import annotations

import re
import shlex
from pathlib import Path
from typing import Any

import yaml

from .. import paths
from ..agent_workflows import WorkflowSourceError, read_workflow_bytes, repository_workflows
from . import watch

# careerbrew's own first step: start the watcher and the web app, refresh, and show what matters now.
BUILTIN_STEPS = ("/careerbrew",)
NAME = re.compile(r"^/?[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$")
MAX_STEPS = 30
# The fields a step may declare: order (#258; `longRunning` is read as `long_running`), then its proof and time limit (#259),
# then its watch-loop entry and the inputs that re-run it (#264, `watch.check_fields`).
FIELDS = ("scope", "after", "long_running", "uses", "verify", "timeout", "cycle", "watch", "about", "operator_turn")
MAX_ABOUT_WORDS = 15  # issue #311: one plain line that says what the step does
# Named exclusive resources: two steps that list the same entry never run in the same stage.
USE_KINDS = ("browser", "slack-draft", "file", "checkout")
USE = re.compile(r"^(?:" + "|".join(USE_KINDS) + r"):\S+$")
MAX_LIST = 10
MAX_VALUE = 200
MAX_VERIFY = 500
MAX_TIMEOUT = 1440  # minutes: one day
BARE_PYTHON = r"python(?:3(?:\.\d+)?)?"  # #260: a bare interpreter lacks the venv's packages, so a step names its own


class StepError(ValueError):
    pass


def normalize(name: str) -> str:
    """`standup` and `/standup` are one step; anything else that is not a short command name is refused."""
    name = str(name).strip()
    if not NAME.match(name) or ".." in name:  # `:` maps to a folder in `resolve`, so `..` could leave the skills folder
        raise StepError(f"step {name[:40]!r}: use a command or skill name such as /standup (letters, digits, . _ : -)")
    return "/" + name.lstrip("/")


def _strings(value: Any, field: str, where: str) -> list[str]:
    """A field that holds one string or a list of them: short, no spaces, at most MAX_LIST."""
    items = [value] if isinstance(value, str) else value
    if not isinstance(items, list) or not all(isinstance(i, str) for i in items):
        raise StepError(f"{where}: `{field}` must be a string or a list of strings")
    if len(items) > MAX_LIST:
        raise StepError(f"{where}: `{field}` holds at most {MAX_LIST} entries")
    for i in items:
        if not 0 < len(i) <= MAX_VALUE or any(c.isspace() for c in i):
            raise StepError(f"{where}: `{field}` entry {i[:40]!r} must be 1 to {MAX_VALUE} characters with no spaces")
    return list(dict.fromkeys(items))


def entry(raw: Any, origin: str) -> dict[str, Any]:
    """One declared step: a name, or a mapping with `step:` and the order fields (#258): scope, after, long_running, uses,
    plus `verify` (a one-line command whose exit code proves the step's result) and `timeout` (whole minutes) (#259).

    A field left out reads as empty; `timeout` then reads as None (the `work_step_timeout` setting applies). Unknown keys are refused, so a typo (`use:`) never silently drops a lock."""
    fields: dict[str, Any] = {}
    if isinstance(raw, dict):
        if "step" not in raw:
            raise StepError(f"a work step from {origin} is a mapping without `step:`")
        fields = {("long_running" if k == "longRunning" else "operator_turn" if k == "operatorTurn" else k): v for k, v in raw.items() if k != "step"}
        raw = raw["step"]
    if not isinstance(raw, str):
        raise StepError(f"a work step from {origin} must be a name or a mapping with `step:`")
    step = normalize(raw)
    where = f"work step {step} from {origin}"
    unknown = sorted(set(fields) - set(FIELDS))
    if unknown:
        raise StepError(f"{where}: unknown field(s) {', '.join(unknown)}; a step declares {', '.join(FIELDS)} (docs/plugins.md)")
    long_running = fields.get("long_running", False)
    if not isinstance(long_running, bool):
        raise StepError(f"{where}: `longRunning` is true or false")
    operator_turn = fields.get("operator_turn", False)
    if not isinstance(operator_turn, bool) or (operator_turn and long_running):
        raise StepError(f"{where}: operatorTurn is true or false and cannot be longRunning; it only dispatches quick work")
    uses = _strings(fields.get("uses", []), "uses", where)
    for u in uses:
        if not USE.match(u):
            raise StepError(f"{where}: `uses` entry {u[:40]!r} must be <kind>:<name> with kind {', '.join(USE_KINDS)} (for example browser:sso)")
    verify = fields.get("verify", None)
    if verify is not None and (not isinstance(verify, str) or not verify.strip()):
        raise StepError(f"{where}: `verify` must be a command, such as `careerbrew feed check`")
    if verify is not None and (len(verify) > MAX_VERIFY or any(c in verify for c in "\n\r\0")):
        raise StepError(f"{where}: `verify` must be one line of at most {MAX_VERIFY} characters")
    if verify is not None:
        try:
            argv = shlex.split(verify)
        except ValueError:
            raise StepError(f"{where}: `verify` has an unclosed quote") from None
        head = argv
        if head and Path(head[0]).name == "env":  # `env [-S] [VAR=1] python3 ...` runs the bare interpreter too
            head = [a for a in head[1:] if not a.startswith("-") and "=" not in a]
        if head and Path(head[0]).name in ("sh", "bash", "zsh") and "-c" in head[1:]:
            head = (head[head.index("-c") + 1 :] or [""])[0].split()  # the first word of `sh -c '...'`
        if head and re.fullmatch(BARE_PYTHON, head[0]):
            raise StepError(f"{where}: `verify` names its interpreter: `uv run python -m unittest` or `.venv/bin/python`, never bare {head[0]} (it lacks the project's packages; issue #260)")
    about = fields.get("about", "")
    if not isinstance(about, str) or ("about" in fields and not about.strip()) or any(c in about for c in "\n\r\0") or len(about.split()) > MAX_ABOUT_WORDS:
        raise StepError(f"{where}: `about` is one plain line of at most {MAX_ABOUT_WORDS} words that says what the step does")
    timeout = fields.get("timeout", 3 if operator_turn else None)
    if timeout is not None and long_running:
        raise StepError(f"{where}: a long-running step never ends, so it takes no `timeout`")
    if timeout is not None and (isinstance(timeout, bool) or not isinstance(timeout, int) or not 1 <= timeout <= MAX_TIMEOUT):
        raise StepError(f"{where}: `timeout` is whole minutes from 1 to {MAX_TIMEOUT}, for example `timeout: 30`")
    return {
        "step": step,
        "from": origin,
        "scope": _strings(fields.get("scope", []), "scope", where),
        "after": [normalize(a) for a in _strings(fields.get("after", []), "after", where)],
        "long_running": long_running,
        "operator_turn": operator_turn,
        "uses": uses,
        "verify": (verify or "").strip(),
        "timeout": timeout,
        "about": about.strip(),
        **watch.check_fields(fields, where),
    }


def validate(raw: Any, origin: str) -> list[dict[str, Any]]:
    """A `work_steps` list as declared (manifest or settings), checked; refuses anything else."""
    if raw in (None, ""):
        return []
    if not isinstance(raw, list):
        raise StepError(f"work_steps from {origin} must be a list of step names")
    if len(raw) > MAX_STEPS:
        raise StepError(f"work_steps from {origin}: at most {MAX_STEPS} steps")
    return [entry(r, origin) for r in raw]


def _roots() -> list[Path]:
    """Where a step's file is looked up: this repo first (it ships /careerbrew and /work), then the agent's folders."""
    return [paths.REPO_ROOT, Path.home() / ".agents", Path.home() / ".codex", Path.home() / ".claude"]


def _plugin_dirs(origin: str = "") -> list[tuple[Path | None, Path | None]]:
    """Canonical resource folders for installed plugins, optionally limited to one declared owner."""
    from .. import plugins

    out: list[tuple[Path | None, Path | None]] = []
    for p in plugins.installed():
        if origin and p["origin"] != origin:
            continue
        root, provides = Path(p["root"]), (p["manifest"].get("provides") or {})
        for kind in ("skills", "commands"):
            rel = provides.get(kind)
            for item in (rel if isinstance(rel, list) else [rel]) if rel else []:
                for match in sorted(root.glob(str(item))):
                    if match.is_dir() or (kind == "commands" and match.is_file() and match.suffix == ".md"):
                        out.append((match, None) if kind == "skills" else (None, match))
    return out


def resolve(step: str, roots: list[Path] | None = None, *, origin: str = "") -> dict[str, Any]:
    """The file a step runs: a skill's SKILL.md, then a command file; `user_only` when the skill hides itself from the model."""
    name = step.lstrip("/").replace(":", "/")
    if not name or Path(name).is_absolute() or any(part in {".", ".."} for part in name.split("/")):
        return {"path": None, "user_only": False, "description": "", "source_error": "workflow resource name is empty or unsafe; use a relative step name"}
    candidates: list[tuple[Path, bool, Path | None]] = []
    source_roots = roots if roots is not None else _roots()
    if origin.startswith("plugin:"):
        dirs = _plugin_dirs(origin)
    else:
        if roots is None:
            inventory = repository_workflows(paths.REPO_ROOT)
            if inventory.problems:
                return {"path": None, "user_only": False, "description": "", "source_error": "own declared workflow sources are unavailable; repair the owning Agentfile before this step"}
            names = [name] if "/" in name or name.startswith("careerbrew-") else ["careerbrew-" + name, name]
            for kind, lookup in [("skill", value) for value in names] + [("command", name)]:
                for entry in inventory.entries:
                    matches = (entry.path.parent.name if kind == "skill" else entry.path.stem) == lookup
                    matches = matches or (kind == "command" and "/" in name and entry.path == paths.REPO_ROOT / "commands" / f"{name}.md")
                    if entry.kind == kind and matches:
                        candidates.append((entry.path, True, None))
            if "/" in name:
                # Requested legacy resources do not become recursively published commands or skills.
                legacy = [(folder / name / "SKILL.md", folder) for folder in inventory.skill_sources]
                legacy += [(folder / f"{name}.md", folder) for folder in inventory.command_sources if folder.is_dir()]
                candidates.extend((path, True, folder) for path, folder in legacy if path.is_file())
            source_roots = [r for r in source_roots if r != paths.REPO_ROOT]
        dirs = [pair for r in source_roots for pair in ((r / "skills", r / "commands"), (None, r / "prompts"))]
        if roots is None:
            dirs += _plugin_dirs()
    for skills, commands in dirs:
        skill_names = [name]
        if skills == paths.REPO_ROOT / "skills" and "/" not in name and not name.startswith("careerbrew-"):
            skill_names.insert(0, "careerbrew-" + name)
        host_candidates = [skills / skill_name / "SKILL.md" for skill_name in skill_names] if skills else []
        if commands:
            if commands.is_file():
                if commands.name == f"{name}.md":
                    host_candidates.append(commands)
            else:
                host_candidates.append(commands / f"{name}.md")
        candidates.extend((path, False, None) for path in host_candidates)
    for path, owned, source_directory in candidates:
        if owned or path.is_file():
            if owned:
                try:
                    head = read_workflow_bytes(path, paths.REPO_ROOT, source_directory=source_directory).decode("utf-8")[:2000]
                except (OSError, UnicodeError, WorkflowSourceError):
                    return {"path": None, "user_only": False, "description": "", "source_error": "own declared workflow source changed or is unreadable; repair it before this step"}
            else:
                try:
                    head = path.read_text(encoding="utf-8")[:2000]
                except OSError:
                    head = ""
            front = re.match(r"^---\n(.*?)\n---", head.replace("\r\n", "\n"), re.S)
            user_only = bool(front and re.search(r"(?m)^disable-model-invocation:\s*[\"']?true[\"']?\s*(#.*)?$", front.group(1)))
            desc = re.search(r"(?m)^description:\s*(.+)$", front.group(1)) if front else None
            description = desc.group(1) if desc else ""
            if front and re.fullmatch(r">(?:[+-]?[1-9]?|[1-9][+-]?)(?:\s+#.*)?\s*", description):
                try:
                    metadata = yaml.safe_load(front.group(1))
                    if isinstance(metadata, dict) and isinstance(metadata.get("description"), str):
                        description = metadata["description"]
                except (yaml.YAMLError, RecursionError):
                    pass  # Preserve the prior description for malformed front matter.
            home = str(Path.home())
            shown = "~" + str(path)[len(home) :] if str(path).startswith(home + "/") else str(path)
            return {"path": shown, "user_only": user_only, "description": first_sentence(description) if desc else ""}
    missing = {"source_error": f"plugin source unavailable for {step}; repair its declared plugin resource before this step"} if origin.startswith("plugin:") else {}
    return {"path": None, "user_only": False, "description": "", **missing}


def first_sentence(text: str) -> str:
    """The first sentence of a frontmatter `description:` value, quotes removed (issue #311)."""
    text = text.strip().strip("\"'").strip()
    m = re.match(r"(.+?[.!?])(\s|$)", text)
    return (m.group(1) if m else text)[:160]


def about(step: dict[str, Any], resolved: dict[str, Any]) -> str:
    """What the step does in one line: its own `about`, else its file's first description sentence, else empty."""
    return step.get("about") or resolved.get("description") or ""


def missing_about(found: list[dict[str, Any]]) -> list[str]:
    """Planned steps with neither an `about` nor a file description; `day run` warns about each."""
    return sorted(s["step"] for s in found if not s.get("about"))


def declared() -> list[dict[str, Any]]:
    """Every declared step in run order, first declaration wins, each with its origin, file, and timeout in minutes."""
    from .. import config, plugins

    found = [entry(s, "careerbrew") for s in BUILTIN_STEPS]
    found += validate(config.get("work_steps"), "settings")
    for p in plugins.installed():
        found += validate(p["manifest"].get("work_steps"), p["origin"])
    seen: set[str] = set()
    out: list[dict[str, Any]] = []
    for s in found:
        if s["step"] in seen:
            continue
        seen.add(s["step"])
        resolved = resolve(s["step"], origin=s["from"])
        out.append({**s, "timeout": None if s["long_running"] else s["timeout"] or config.get("work_step_timeout"), **{k: v for k, v in resolved.items() if k != "description"}, "about": about(s, resolved)})
    return out
