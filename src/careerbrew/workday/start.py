"""The start of the work day (issue #260): refresh the checkouts, check the shell, and probe sign-in, once, before stage 1.

`careerbrew day start` runs it; `plan.build()` shows what it found (`start_of_day`) and changes nothing.

- Checkouts (`checkouts`): careerbrew's own checkout (when the `work_checkouts` setting lists
  `careerbrew`, the default), every other path in that setting, each installed plugin's source
  folder, each `checkout:<path>` a step `uses`, and the git checkout each step's file resolves to
  (a managed clone, say). Two folders with one remote are both refreshed, but only the first is
  watched (`watch`): the dev checkout before a clone a step happens to resolve to.
- Refresh (`refresh`): fast-forward only. A checkout with local changes, local commits, a
  diverged branch, no upstream, or a detached HEAD is reported and never touched. Nothing resets,
  stashes, or discards work.
- Preflight (`preflight`): the shell, the sed flavor, the bash version, and `python3`, each with
  its safe form. Every step loads one shared reference of these traps (`TRAPS`).
- Sign-in (`signin`): a step that `uses` a `browser:<name>` needs sign-in. The `work_signin_probe`
  setting is a one-line command whose exit code says whether the browser is signed in (run like a
  step's `verify`: no shell, output never read). When it fails, the day gets one question record
  (id `signin`, in the question store, `questions.py`) and those steps run after every other step
  (`stages.build(late=...)`); a passing probe closes it.

Privacy: records hold paths (shown with `~`), short commits, status words, and step names. A
remote URL may name a company host, so it is read for the duplicate check only and never stored.

Lifecycle: `run()` writes one record per day into the day's status file (`status.save_start`);
nothing stays in process memory.
"""

from __future__ import annotations

import os
import re
import shlex
import subprocess
from pathlib import Path
from typing import Any

from .. import paths
from . import mcp_check

GIT_SECONDS: float = 60
PROBE_SECONDS: float = 60
TRAPS = paths.REPO_ROOT / "skills" / "careerbrew-work-day" / "shell-traps.md"
SELF = "careerbrew"
NOT_TOUCHED = ("local changes", "local commits", "diverged", "no upstream", "detached", "fetch failed", "not a git checkout", "missing")
SIGNIN_ASK = "Sign in once in the shared browser for today's sign-in steps ({steps}), then run `careerbrew day start` again. Until then they run after every other step."
RULE = (
    "Before stage 1, run `careerbrew day start` once: it fast-forwards every checkout below (never a reset; a checkout with local work is reported, not touched), "
    "checks the shell, and probes sign-in. Give every step's subagent the shell traps reference (`traps`) to read first. "
    "Ask the sign-in question (`questions`) right away, before stage 1: it is the day's only sign-in ask, and the steps that need sign-in run last."
)


def shown(path: str | Path) -> str:
    home, s = str(Path.home()), str(path)
    return "~" + s[len(home) :] if s == home or s.startswith(home + "/") else s


def _git(where: str | Path, *args: str, seconds: float = GIT_SECONDS) -> tuple[int, str]:
    env = {**os.environ, "GIT_TERMINAL_PROMPT": "0", "GIT_OPTIONAL_LOCKS": "0"}
    for k in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"):  # a hook's git env would point every call at one repo
        env.pop(k, None)
    try:
        done = subprocess.run(["git", "-C", str(where), *args], capture_output=True, text=True, timeout=seconds, env=env, stdin=subprocess.DEVNULL, check=False)  # noqa: S603, S607
    except (OSError, subprocess.TimeoutExpired):
        return 1, ""
    return done.returncode, done.stdout.strip()


CheckoutLookup = dict[Path, str | None]


def toplevel(path: str | Path, *, lookup: CheckoutLookup | None = None) -> str | None:
    """The git checkout that holds `path` (a file or a folder), or None."""
    p = Path(path).expanduser()
    try:
        p = p.resolve()  # an agent's skill is often a symlink into its source checkout
    except OSError:
        return None
    where = p if p.is_dir() else p.parent
    if not where.is_dir():
        return None
    if lookup is not None and where in lookup:
        return lookup[where]
    rc, out = _git(where, "rev-parse", "--show-toplevel", seconds=10)
    top = out if rc == 0 and out else None
    if lookup is not None:
        lookup[where] = top
    return top


def commit(top: str) -> str:
    rc, out = _git(top, "rev-parse", "--short=12", "HEAD", seconds=10)
    return out if rc == 0 else ""


def remote_key(top: str) -> str:
    """origin's URL without scheme, user, and `.git`, so an ssh and an https clone of one repo match. Never stored."""
    rc, url = _git(top, "config", "--get", "remote.origin.url", seconds=10)
    if rc or not url:
        return ""
    url = re.sub(r"^[a-z+]+://", "", url.strip())
    url = re.sub(r"^[^@/]+@", "", url)
    url = url.replace(":", "/", 1) if re.match(r"^[^/]+:[^/]", url) else url
    return url.rstrip("/").removesuffix(".git").lower()


def _plugin_sources() -> dict[str, str]:
    """{plugin name: its source folder, or "" for a plugin installed from a URL}."""
    from .. import plugins

    return {p["name"]: p.get("local_path") or "" for p in plugins.installed()}


def _copies() -> list[Path]:
    """Folders that hold careerbrew's copies of plugin files: never a step's source checkout."""
    from .. import plugins

    out = []
    for d in (plugins.plugins_dir(), plugins.commands_dir()):
        try:
            out.append(Path(d).resolve())
        except OSError:
            continue
    return out


def step_source(step: dict[str, Any], plugin_sources: dict[str, str] | None = None, *, lookup: CheckoutLookup | None = None) -> str | None:
    """The checkout a step's file comes from: a plugin step's source folder (none for a URL install), else the checkout that holds the file."""
    sources = plugin_sources if plugin_sources is not None else _plugin_sources()
    origin, path = step.get("from", ""), step.get("path")
    if origin.startswith("plugin:"):
        src = sources.get(origin.split(":", 1)[1], "")
        return toplevel(src, lookup=lookup) if src else None
    if not path:
        return None
    try:
        real = Path(path).expanduser().resolve()
    except OSError:
        return None
    if any(real.is_relative_to(c) for c in _copies()):
        return None  # a plugin's copied file, found by name: careerbrew's checkout is not its source
    return toplevel(real, lookup=lookup)


def checkouts(declared: list[dict[str, Any]], *, lookup: CheckoutLookup | None = None) -> list[dict[str, Any]]:
    """Every checkout the day reads, once each, with why (`for`), its commit, and `duplicate_of` when another folder has its remote."""
    from .. import config, plugins

    wanted = config.get("work_checkouts") or []
    self_top = toplevel(paths.REPO_ROOT, lookup=lookup)
    found: dict[str, list[str]] = {}
    order: list[str] = []
    dev: set[str] = set()

    def add(path: str | Path | None, why: str, is_dev: bool) -> None:
        top = toplevel(path, lookup=lookup) if path else None
        if top is None:
            return
        if top == self_top and SELF not in wanted:
            return  # the owner left careerbrew's own checkout out of the refresh
        if top not in found:
            found[top] = []
            order.append(top)
        if why not in found[top]:
            found[top].append(why)
        if is_dev:
            dev.add(top)

    for w in wanted:
        add(paths.REPO_ROOT if w == SELF else Path(w).expanduser(), SELF if w == SELF else "setting", True)
    for p in plugins.installed():
        if p.get("local_path"):
            add(p["local_path"], f"plugin:{p['name']}", True)
    sources = _plugin_sources()
    for s in declared:
        for u in s.get("uses", []):
            if u.startswith("checkout:"):
                add(Path(u.split(":", 1)[1]).expanduser(), f"uses:{s['step']}", True)
        add(step_source(s, sources, lookup=lookup), f"step:{s['step']}", False)
    priority = sorted(order, key=lambda t: (t not in dev, order.index(t)))  # a dev checkout is watched before a clone of its remote
    first_of: dict[str, str] = {}
    dup: dict[str, str] = {}
    for top in priority:
        key = remote_key(top)
        if key and key in first_of:
            dup[top] = first_of[key]
        elif key:
            first_of[key] = top
    out = [{"top": top, "path": shown(top), "for": found[top], "commit": commit(top), "duplicate_of": shown(dup[top]) if top in dup else ""} for top in order]
    return out


def refresh_one(top: str) -> dict[str, Any]:
    """Fast-forward one checkout to its upstream, or say why it was left alone. Never resets, stashes, or discards."""
    where = Path(top).expanduser()
    out: dict[str, Any] = {"path": shown(where), "status": "", "before": "", "after": "", "behind": 0}
    if not where.is_dir():
        return {**out, "status": "missing"}
    if toplevel(where) is None:
        return {**out, "status": "not a git checkout"}
    out["before"] = out["after"] = commit(str(where))
    rc, branch = _git(where, "symbolic-ref", "--quiet", "--short", "HEAD", seconds=10)
    if rc or not branch:
        return {**out, "status": "detached"}
    rc, _up = _git(where, "rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}", seconds=10)
    if rc:
        return {**out, "status": "no upstream"}
    rc, dirty = _git(where, "status", "--porcelain", "--untracked-files=no", seconds=30)
    if rc or dirty:
        return {**out, "status": "local changes"}
    rc, _ = _git(where, "fetch", "--quiet", "--no-tags")
    if rc:
        return {**out, "status": "fetch failed"}
    rc, counts = _git(where, "rev-list", "--left-right", "--count", "HEAD...@{u}", seconds=10)
    try:
        ahead, behind = (int(x) for x in counts.split())
    except ValueError:
        return {**out, "status": "fetch failed"}
    out["behind"] = behind
    if ahead and behind:
        return {**out, "status": "diverged"}
    if ahead:
        return {**out, "status": "local commits"}
    if not behind:
        return {**out, "status": "current"}
    rc, _ = _git(where, "merge", "--ff-only", "--quiet", "@{u}")
    if rc:
        return {**out, "status": "local changes"}  # git refused: an untracked file the update would overwrite
    return {**out, "status": "fast-forwarded", "after": commit(str(where))}


def _run(argv: list[str], seconds: float = 10) -> tuple[int, str]:
    try:
        done = subprocess.run(argv, capture_output=True, text=True, timeout=seconds, stdin=subprocess.DEVNULL, check=False)  # noqa: S603
    except (OSError, subprocess.TimeoutExpired):
        return 127, ""
    return done.returncode, (done.stdout or done.stderr).strip()


def preflight(env: dict[str, str] | None = None, runner=_run) -> list[dict[str, Any]]:
    """The shell traps on this machine, with warnings and blocking failures kept distinct."""
    env = dict(os.environ if env is None else env)
    shell = Path(env.get("SHELL", "")).name or "unknown"
    out = [
        {
            "check": "shell",
            "value": shell,
            "ok": True,
            "status": "warn" if shell == "zsh" else "ok",
            "safe": "zsh does not split $F into words: loop over an array, or `${=F}`, or run the line with bash -c" if shell == "zsh" else 'quote every "$var"; loop over arrays, not split strings',
        }
    ]
    rc, text = runner(["sed", "--version"])
    gnu = rc == 0 and "GNU" in text
    sed_rc, sed_path = runner(["sh", "-c", "command -v sed"]) if not gnu else (0, "sed")
    bsd = bool(sed_rc == 0 and sed_path and rc != 0 and "illegal option" in text.lower())
    sed_known = gnu or bsd
    out.append(
        {
            "check": "sed",
            "value": "GNU" if gnu else "BSD" if bsd else "unknown",
            "ok": sed_known,
            "status": "warn" if sed_known else "fail",
            "safe": "GNU sed is first on PATH, so `sed -i ''` fails: edit files with the Edit tool or Python, or `perl -pi -e`"
            if gnu
            else "BSD sed needs `sed -i ''`, which GNU sed refuses: edit files with the Edit tool or Python, or `perl -pi -e`"
            if bsd
            else "check that sed is installed and its version probe works before using it",
        }
    )
    rc, text = runner(["bash", "-c", "echo ${BASH_VERSINFO[0]}"])
    major = int(text) if rc == 0 and text.isdigit() else 0
    out.append(
        {
            "check": "bash",
            "value": str(major) if major else "missing",
            "ok": major >= 4,
            "safe": "bash 4 or later: `declare -A` and `mapfile` work" if major >= 4 else "bash 3 has no `declare -A` or `mapfile`: use a plain list, or Python",
        }
    )
    rc, text = runner(["sh", "-c", "command -v python3"])
    out.append(
        {
            "check": "python3",
            "value": shown(text) if rc == 0 and text else "missing",
            "ok": bool(rc == 0 and text),
            "status": "warn" if rc == 0 and text else "fail",
            "safe": "bare python3 lacks a project's packages: run tests with `uv run python -m unittest` (or pytest), or the project's `.venv/bin/python`"
            if rc == 0 and text
            else "python3 is missing: restore it on PATH or use the project's verified interpreter",
        }
    )
    out.append(
        {
            "check": "push guard",
            "value": "on",
            "ok": True,
            "status": "warn",
            "safe": "a `$VAR` in a gh or git write is blocked: write the literal value in the command",
        }
    )
    out.extend(mcp_check.check())  # issue #312: MCP servers as the desktop app's launchd env sees them
    from . import cron_check, self_update, workflow_check

    out.extend(workflow_check.check(self_update.declared_steps()))
    out.extend(cron_check.check(env))
    return out


def needs_attention(check: dict[str, Any]) -> bool:
    """Keep warnings and legacy failed rows visible without weakening failures."""
    return check.get("status") == "warn" or not check["ok"]


def signin_steps(declared: list[dict[str, Any]]) -> list[str]:
    """Steps that need a signed-in browser: any `uses` entry of kind `browser`."""
    return [s["step"] for s in declared if any(u.startswith("browser:") for u in s.get("uses", []))]


def probe(command: str) -> tuple[bool, str]:
    """(signed in, why) for the `work_signin_probe` command: run without a shell, output never read."""
    from .gate import run_verify

    return run_verify(command)


def signin(declared: list[dict[str, Any]], command: str) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    """(the sign-in record, the day's question records): one question for every step that needs sign-in, or none."""
    need = signin_steps(declared)
    if not need:
        return {"status": "none needed", "steps": [], "why": ""}, []
    if not command:
        return {"status": "no probe", "steps": need, "why": "set work_signin_probe to check sign-in before stage 1"}, []
    ok, why = probe(command)
    if ok:
        return {"status": "ok", "steps": need, "why": ""}, []
    from . import questions

    ask = questions.record("start", SIGNIN_ASK.format(steps=", ".join(need)), recheck=questions.SIGNIN, source="careerbrew", ident="signin", steps=need)
    return {"status": "failed", "steps": need, "why": why}, [ask]


def run(declared: list[dict[str, Any]]) -> dict[str, Any]:
    """`careerbrew day start`: refresh every checkout, check the shell, probe sign-in, and save the day's record."""
    from .. import config, events
    from ..calendar import now
    from ..day import pm_promises
    from . import status

    pm_promises.reconcile(now().date())
    found = checkouts(declared)
    results = []
    for c in found:
        r = refresh_one(c["top"])
        results.append({**r, "for": c["for"], "duplicate_of": c["duplicate_of"]})
    checks = preflight()
    sign, asks = signin(declared, config.get("work_signin_probe") or "")
    from . import end  # issue #265: last handoff's items, each rechecked; closed ones leave it

    record = {"at": now().isoformat(timespec="seconds"), "refresh": results, "preflight": checks, "signin": sign, "handoff": end.morning()}
    status.save_start(record)
    from . import questions  # the one question store (#261): the sign-in ask is a record like every other

    asks = [questions.save(q, reopen=True) for q in asks]  # a probe that fails again asks again
    if sign["status"] in ("ok", "none needed"):
        questions.close("signin", "signed in" if sign["status"] == "ok" else "no step needs sign-in")
    record = {**record, "questions": asks}
    events.emit(
        "workday.start",
        checkouts=len(results),
        fast_forwarded=sum(1 for r in results if r["status"] == "fast-forwarded"),
        not_touched=sum(1 for r in results if r["status"] in NOT_TOUCHED),
        traps=sum(1 for c in checks if needs_attention(c)),
        signin=sign["status"],
    )
    return record


def view(declared: list[dict[str, Any]], record: dict[str, Any] | None, open_questions: list[dict[str, Any]] | None = None, *, lookup: CheckoutLookup | None = None) -> dict[str, Any]:
    """The plan's `start_of_day`: what `careerbrew day start` will refresh, and what it found today (empty before it ran).

    `questions` is the open sign-in record from the question store (`questions.py`), when there is one."""
    record = record or {}
    found = checkouts(declared, lookup=lookup)
    sign = record.get("signin") or {}
    return {
        "command": "careerbrew day start",
        "rule": RULE,
        "ran_at": record.get("at", ""),
        "checkouts": [{k: v for k, v in c.items() if k != "top"} for c in found],
        "watch": [c["path"] for c in found if not c["duplicate_of"]],
        "refresh": record.get("refresh", []),
        "preflight": record.get("preflight") or preflight(),  # read-only, so the plan shows the traps before `day start` too
        "traps": shown(TRAPS),
        "signin": {"status": sign.get("status", "not run"), "steps": sign.get("steps", signin_steps(declared)), "probe": bool(_probe_setting())},
        "questions": [q for q in open_questions or [] if q["id"] == "signin"],
        "handoff": record.get("handoff") or {"from": "", "items": []},  # issue #265: each item of the last handoff, rechecked
    }


def late(declared: list[dict[str, Any]], record: dict[str, Any] | None) -> set[str]:
    """Steps that wait until every other step ran: today's sign-in probe failed, and they need sign-in."""
    sign = (record or {}).get("signin") or {}
    return set(signin_steps(declared)) if sign.get("status") == "failed" else set()


def _probe_setting() -> str:
    from .. import config

    return config.get("work_signin_probe") or ""


def labels(declared: list[dict[str, Any]], *, lookup: CheckoutLookup | None = None) -> dict[str, str]:
    """{a path an agent must not see: its label}. Only careerbrew's own checkout shows as a path; every other checkout
    shows as its first reason (`plugin:<name>`, `uses:<step>`, `step:<step>`, `setting`), and so does each `checkout:`
    path a step declares, since a folder name may name the company."""
    out: dict[str, str] = {}
    self_shown = shown(toplevel(paths.REPO_ROOT, lookup=lookup) or paths.REPO_ROOT)
    for c in checkouts(declared, lookup=lookup):
        if c["path"] != self_shown:
            out[c["path"]] = c["for"][0] if c["for"] else "checkout"
    for name, src in _plugin_sources().items():
        if src and (top := toplevel(src, lookup=lookup)):
            out.setdefault(shown(top), f"plugin:{name}")
    for s in declared:
        for u in s.get("uses", []):
            if u.startswith("checkout:"):
                raw = u.split(":", 1)[1]
                out.setdefault(raw, f"uses:{s['step']}")
                out.setdefault(shown(Path(raw).expanduser()), f"uses:{s['step']}")
    return out


def redact(obj: Any, names: dict[str, str]) -> Any:
    """`obj` with every path in `names` replaced by its label, wherever it shows in a string (a `checkout:` entry, a `why`)."""
    if isinstance(obj, dict):
        return {k: redact(v, names) for k, v in obj.items()}
    if isinstance(obj, list):
        return [redact(v, names) for v in obj]
    if isinstance(obj, str):
        for path in sorted(names, key=len, reverse=True):  # the longest path first, so a folder inside another keeps its own label
            if path and path in obj:
                obj = re.sub(re.escape(path) + r"(?=$|[/\s,;:)\]'\"])(?:/[^\s,;)\]'\"]*)?", names[path], obj)
    return obj


def check_probe(v: str) -> str:
    """The `work_signin_probe` setting: empty, or one line a shell could split, run without a shell."""
    v = v.strip()
    if not v:
        return ""
    if len(v) > 500 or any(c in v for c in "\n\r\0"):
        raise ValueError("use one line of at most 500 characters")
    try:
        shlex.split(v)
    except ValueError:
        raise ValueError("the command has an unclosed quote") from None
    return v


def check_checkouts(items: list[str]) -> list[str]:
    """The `work_checkouts` setting: `careerbrew` (its own checkout) or absolute / `~` paths."""
    for i in items:
        if i != SELF and not (i.startswith("/") or i.startswith("~/")):
            raise ValueError(f"{i[:40]!r}: use careerbrew (its own checkout) or a full path such as ~/apps/notes")
    return list(dict.fromkeys(items))
