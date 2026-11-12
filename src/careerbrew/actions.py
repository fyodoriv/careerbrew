"""Every careerbrew action, three ways: the CLI, agents, and the web app, from one source.

The registry is generated from the CLI parser, so a new command is an action the moment it
exists; nothing has to be listed twice. Agents discover actions with `careerbrew actions --json`
and run them as CLI commands. The web app lists them (`GET /api/actions`) and runs one with
`POST /api/actions/<name>` through `run()`, which calls the same CLI handler in-process.

A few actions need a terminal (a hidden secret prompt, an interactive session, an editor); they
are listed with that reason and a hint instead of a form. Actions that change or send something
hard to undo need `confirm`. `tests/unit/test_action_parity.py` fails when any CLI command misses
one of the three paths.

Lifecycle: `_LOCAL` is thread-local scope: while a thread runs an action, its stdout, stderr, and
stdin go to that run's own buffers through `_Router` (installed once, process-wide, on the first
run); other threads keep the real streams, so concurrent requests never wait on or mix output.
`OWNER_UI` is a context variable set only during a run that the owner started from the web app
(token-guarded, local only), reset in `finally`.
"""

from __future__ import annotations

import argparse
import contextvars
import io
import sys
import threading
from typing import Any

TERMINAL_ONLY = {
    "start": "it starts the web app you are already using; run `careerbrew` in a terminal",
    "serve": "it is the web app itself",
    "open": "it opens the web app you are already using; run `careerbrew open` in a terminal",
    "secret.set": "the secret goes to a hidden prompt in your own terminal, never through a browser form",
    "pair.init": "the pairing key prints once, to your terminal only",
    "pair.join": "the pairing key is typed into a hidden prompt",
    "drill.session": "an interactive question-and-answer loop (use Practice in the web app)",
    "cards.review": "an interactive card session (use Practice in the web app)",
    "data.edit": "it opens your $EDITOR",
    "notify.open": "it opens the web app you are already using; use Open in the Notifications view",
    "day.pm-sent": "it reads a text file on your machine; a plugin skill or agent that sent the update runs it",
    "draft.fit": "it reads a thread file on your machine; the agent that drafts the reply runs it",
    "draft.replace": "it reads a draft file on your machine; the agent that drafts the reply runs it",
    "draft.check": "it reads two text files on your machine; the agent that saved the draft runs it",
    "draft.route": "it reads an ask file and a channel directory on your machine; the agent that handles the access ask runs it",
    "draft.judge": "it reads a draft file and a channel directory on your machine; the judge step runs it before the draft is saved",
    "day.loop": "`run` keeps going until end of day and a pass can run for many minutes; the /work agent runs it in its own session",
}
CONFIRM = {
    "work.project.rules-set": "replaces or removes the project's standing rules",
    "data.decrypt": "writes every data file as plaintext",
    "backup.restore": "replaces data with a backup",
    "plugin.remove": "uninstalls a plugin (and with --purge-data deletes its records)",
    "sync.import": "imports a bundle from another machine",
    "machine.set": "changes this machine's role",
    "machine.new-user": "sets this machine to the corporate role and writes your own connectors",
    "machine.split-plugin-evidence": "rewrites the company plugin's evidence file and reinstalls the plugin (try --dry-run first)",
    "day.end": "stops careerbrew's loops for the day and writes tomorrow's handoff",
    "day.send": "sends the PM update (Slack, or an email draft)",
    "day.start": "fast-forwards clean checkouts; with --fix, changes Claude GitHub connector authentication to a connection-time helper (no GitHub token is saved)",
    "plugin.add": "installs a plugin (with --allow-exec its commands can run on this machine)",
    "data.encrypt": "re-encrypts every data file",
    "sync.export": "writes an encrypted bundle of your data",
    "approvals.decide": "records your decision on an agent request",
    "exercises.submit": "runs the solution code on this machine",
    "research.page": "fetches a web page",
    "work.project.import": "adds or updates many project records at once (try --dry-run first)",
    "work.promise.accept": "records your decision on one current PM promise outcome",
    "work.project.remove": "deletes a project's record (its memory file and tasks stay; archive keeps it instead)",
    "telemetry.send": "sends the waiting anonymous usage counts to PostHog EU (try --dry-run first)",
    "web.rotate-token": "signs out every open web app tab and bookmark, this one too (`careerbrew open` signs you in again)",
    "day.accept-all": "accepts every suggestion of the group at once (tasks planned, sources added)",
    "day.dismiss-all": "dismisses every suggestion of the group at once; they do not come back",
    "work.session.end": "archives each project session that wrote its handoff and releases its claims (an ended session is not reopened)",
    "day.question.recheck": "runs each open question's recheck command on this machine (a plugin's only with --allow-exec) and closes the ones it finds closed",
}
# Actions whose CLI asks for --yes: the confirm above is the owner's yes, so a web run passes it on.
YES_FROM_CONFIRM = {"day.send", "web.rotate-token", "day.accept-all", "day.dismiss-all", "work.project.remove", "day.start", "work.promise.accept", "work.project.rules-set", "data.decrypt"}
# What an agent must do differently, shown in `careerbrew actions` (issue #46).
AGENT_NOTES = {
    "work.task": "agents: pass --why, and one --link (a full https:// URL, or LABEL=URL) per file, ticket, or channel the task needs; the task waits in the owner's inbox until accepted",
    "day.send": "agents: show the draft first; add --yes only after the user says send",
    "day.update": "agents (/work): mark a step running when it starts and done, blocked, or failed when it ends, with a one-line --summary; blocked needs --needs-you (the ask), failed needs --error-class; done runs the step's verify and exits 1 when it fails",
    "day.question.add": "agents (/work): a step never asks the user mid-run; save each ask here with --option, --pick, and --recheck (a command that exits 0 while it is still open, or a link you check), then go on",
    "day.question.recheck": "agents (/work): run it after the last step, then ask every item it lists in one batched ask with its age; check each `agent` item's link and close a done one",
    "day.start": "agents (/work): run once before stage 1; ask its sign-in question right away (the one ask before the batched ask after the last step); give every step the shell traps file it names. --fix changes Claude connector settings: add --yes only after explicit owner approval, then reconnect using an exposed host tool and recheck; never infer approval from a failed preflight",
    "day.check": "agents (/work): run before each stage and on each watch pass; run a step it set back for a retry again",
    "day.suggest": "plugins: pass a stable --ref from the feed; use this instead of adding a task the owner did not ask for",
    "work.feed.add": "plugins: one JSON object per line (id, at, title; source, kind, actor me|other|bot, to_me, text, personal) and --plugin <name>; keep your own store out",
    "work.feed.mark": "agents: mark an event personal or work only when the user says so",
    "work.standup": "plugins and agents: pass the rows with --rows <file|-> (title, status, owner, keys, project, repos, aliases; optional groups and ticket_projects); relay the counts and the page path, never titles",
    "notify.act": "agents: accept or dismiss a suggestion only when the user says so; it decides it in the inbox",
    "day.accept-all": "agents: show the group's label, its count, and what Accept all does first; add --yes --count <the count you showed> only after the user says yes",
    "day.dismiss-all": "agents: show the group's label and its count first; add --yes --count <the count you showed> only after the user says yes (dismissed suggestions never come back)",
    "work.project.remove": "agents: show the project's name first; add --yes only after the user says remove, and prefer archive (it keeps the record)",
    "work.feed.research": "agents: open every link `work feed links --pending` lists for the event first; one {url, summary} per link, or {url, opened: false, reason} when it needs a sign-in; never skip a link",
    "approvals.request": "agents: read the owner's notes it prints (standing guidance from past decisions) and follow them; act only on approved",
    "work.project.quiz": "agents: ask the quiz in chat (format, rubric, questions, logistics), then store only the counts; never write a question or an answer; after misses, re-quiz each miss and store that run",
    "work.project.gate": "agents: mark tool, questions, or a link read only after the owner says it is done; reading a link yourself does not count",
    "work.project.check": "agents: run --since first to see what changed; then stamp only the sources you read, one ref at a time, never a reference",
    "draft.judge": "agents: run it on every draft before `draft replace`; on exit 1 fix the draft (link the open thread, drop a non-member mention, move it to the owning channel) and judge it again; never send",
    "draft.fit": "agents: read the thread to its newest message first, pass --seen <its ts>, and follow the kind: none (no reply), emoji (suggest the reaction), ack or stop (use its text), reply (write or keep yours)",
    "draft.replace": "agents: every draft goes through this (or `draft check` after a chat tool save); a step that writes drafts declares `uses: slack-draft:<target>`; on a mismatch, tell the owner before anyone sends",
    "data.show": "agents: add --keys to check a file's structure (key names and types, never a value); only files at the top of the data root and in jobs/, tutor/, prep/, and linkedin/ print their text to agents; any other file's text (work/, day/, ladders/, cache/, plugins/, out/) prints only to the owner's terminal or the web app, unless the plugin policy sets agent_may_read_business",
    "data.set": "agents: set profile name or employer when the owner supplies them; command arguments may appear in process listings and shell history, so never use it for secrets",
}
OWNER_UI: contextvars.ContextVar[bool] = contextvars.ContextVar("owner_ui", default=False)
_LOCAL = threading.local()
_INSTALL_LOCK = threading.Lock()


class _Router(io.TextIOBase):
    """sys.stdout/stderr/stdin stand-in: a thread running an action uses its own buffer, every other thread the real stream."""

    def __init__(self, real: Any, attr: str):
        self._real, self._attr = real, attr

    def _target(self) -> Any:
        return getattr(_LOCAL, self._attr, None) or self._real

    def write(self, s: str) -> int:
        return self._target().write(s)

    def flush(self) -> None:
        self._target().flush()

    def readline(self, size: int = -1) -> str:
        return self._target().readline(size)

    def read(self, size: int = -1) -> str:
        return self._target().read(size)

    def isatty(self) -> bool:
        return getattr(_LOCAL, self._attr, None) is None and self._real.isatty()

    def fileno(self) -> int:
        return self._real.fileno()


def _install_routers() -> None:
    with _INSTALL_LOCK:
        if not isinstance(sys.stdout, _Router):
            sys.stdout = _Router(sys.stdout, "out")
            sys.stderr = _Router(sys.stderr, "out")
            sys.stdin = _Router(sys.stdin, "inp")


class ActionError(ValueError):
    pass


def _leaves(p: argparse.ArgumentParser, path: tuple[str, ...] = ()) -> list[tuple[tuple[str, ...], argparse.ArgumentParser]]:
    out = []
    for a in p._actions:
        if isinstance(a, argparse._SubParsersAction):
            for name, sp in a.choices.items():
                out += _leaves(sp, (*path, name))
    return out or [(path, p)]


def _arg(a: argparse.Action) -> dict[str, Any] | None:
    if isinstance(a, argparse._HelpAction | argparse._VersionAction | argparse._SubParsersAction):
        return None
    flag = next((o for o in a.option_strings if o.startswith("--")), None)
    if isinstance(a, argparse._StoreTrueAction):
        kind = "flag"
    elif a.choices:
        kind = "choice"
    elif a.type is int:
        kind = "int"
    elif a.type is float:
        kind = "number"
    else:
        kind = "text"
    return {
        "name": a.dest,
        "flag": flag,
        "kind": kind,
        "choices": [str(c) for c in a.choices] if a.choices else None,
        "required": (not a.option_strings and a.nargs not in ("?", "*")) or bool(a.required),
        "many": a.nargs in ("*", "+") or isinstance(a, argparse._AppendAction),
        "help": a.help or "",
        "default": None if a.default in (None, argparse.SUPPRESS, False) else a.default,
    }


def catalog() -> list[dict[str, Any]]:
    from .cli import build_parser

    out = []
    root = build_parser()  # once: a parser per action cost about 1 s of CPU per call
    for path, sp in _leaves(root):
        if not path:
            continue
        name = ".".join(path)
        args = [x for a in sp._actions if (x := _arg(a))]
        out.append(
            {
                "name": name,
                "command": "careerbrew " + " ".join(path),
                "help": (sp.description or "").strip() or _help_of(root, path),
                "args": args,
                "ui": name not in TERMINAL_ONLY,
                "terminal_only": TERMINAL_ONLY.get(name),
                "confirm": CONFIRM.get(name),
                "agent_note": AGENT_NOTES.get(name),
            }
        )
    return out


def _help_of(p: argparse.ArgumentParser, path: tuple[str, ...]) -> str:
    help_text = ""
    for part in path:
        sub = next((a for a in p._actions if isinstance(a, argparse._SubParsersAction)), None)
        if sub is None:
            break
        help_text = next((c.help for c in sub._choices_actions if c.dest == part), "") or help_text
        p = sub.choices[part]
    return help_text or ""


def argv_for(action: dict[str, Any], values: dict[str, Any]) -> list[str]:
    argv = action["name"].split(".")
    positional: list[str] = []
    known = {a["name"] for a in action["args"]}
    unknown = set(values) - known
    if unknown:
        raise ActionError(f"unknown field(s) {sorted(unknown)} for {action['name']}")
    for a in action["args"]:
        v = values.get(a["name"])
        if v in (None, "", [], False):
            if a["required"]:
                raise ActionError(f"{a['name']} is required for {action['name']}")
            continue
        if isinstance(v, list) and not a["many"]:
            raise ActionError(f"{a['name']} takes one value")
        items = v if isinstance(v, list) else [v]
        if a["choices"] and any(str(x) not in a["choices"] for x in items):
            raise ActionError(f"{a['name']} must be one of {a['choices']}")
        if a["kind"] in ("int", "number"):
            for x in items:
                try:
                    float(x) if a["kind"] == "number" else int(str(x))
                except ValueError:
                    raise ActionError(f"{a['name']} must be a {'number' if a['kind'] == 'number' else 'whole number'}") from None
        if a["kind"] == "flag":
            if v is not True:
                raise ActionError(f"{a['name']} is on or off")
            argv.append(a["flag"])
        elif a["flag"]:
            argv += [f"{a['flag']}={x}" for x in items]  # `--flag=value`: a value starting with "-" stays a value
        else:
            if any(str(x).startswith("-") for x in items):
                raise ActionError(f"{a['name']} cannot start with '-'")
            positional += [str(x) for x in items]
    return argv + (["--", *positional] if positional else [])


def run(name: str, values: dict[str, Any] | None = None, confirm: bool = False) -> dict[str, Any]:
    """Run one action as the owner from the web app: the same CLI handler, its printed output, its exit code."""
    from . import cli

    action = next((a for a in catalog() if a["name"] == name), None)
    if action is None:
        raise ActionError(f"unknown action {name} (see `careerbrew actions`)")
    if action["terminal_only"]:
        raise ActionError(f"{name} runs in a terminal only: {action['terminal_only']}")
    if action["confirm"] and not confirm:
        raise ActionError(f"{name} {action['confirm']}: confirm to run it")
    argv = argv_for(action, values or {})
    if name in YES_FROM_CONFIRM:
        cut = argv.index("--") if "--" in argv else len(argv)
        argv.insert(cut, "--yes")  # the confirm above is the owner's yes; before any "--" so it stays a flag
    out = io.StringIO()
    _install_routers()
    _LOCAL.out, _LOCAL.inp = out, io.StringIO("")  # this thread only: output captured, nothing to type
    token = OWNER_UI.set(True)
    try:
        code = cli.main(argv)
    except SystemExit as e:  # argparse errors
        code = int(e.code or 0) if isinstance(e.code, int) else 2
    finally:
        _LOCAL.out = _LOCAL.inp = None
        OWNER_UI.reset(token)
    shown = ["careerbrew", "data", "set", "[arguments redacted]"] if name == "data.set" else ["careerbrew", *argv]
    return {"action": name, "argv": shown, "exit": code, "output": out.getvalue()[-20000:]}
