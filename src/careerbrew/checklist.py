"""Getting started: the first steps a new user takes, each marked done from data careerbrew already has (issue #147).

Dashboard shows the checklist (`/api/checklist`) and `careerbrew checklist` prints it, until every step is
done or the owner hides it (`first_run_checklist false`; Hide checklist in the web app sets the same
setting). Each step says why it matters, where to do it in the web app, and the command for it. A step is
done when its data exists: a step cannot be ticked by hand, so the list never says done when it is not.
"""

from __future__ import annotations

from collections.abc import Callable
from typing import Any

from . import config, goals


def _connected() -> bool:
    from .work import evidence

    return bool(evidence.sources_list())


def _watching() -> bool:
    from .day import watch

    return watch.read_state().get("state") in ("on", "paused")


def _has_task() -> bool:
    from .work import priorities

    return bool(priorities.load())


def _has_goal() -> bool:
    g = goals.load()
    return bool(g["queue"] or g["long_term"])


def _decided() -> bool:
    from .day import inbox

    return any(i.get("status") in ("accepted", "dismissed", "answered") for i in inbox.load())


def _pm_set() -> bool:
    return bool(config.get("pm_slack") or config.get("pm_email"))


# id, title, why it matters, where in the web app, the command, and the test that marks it done.
STEPS: list[tuple[str, str, str, str, str, Callable[[], bool]]] = [
    ("connect", "Connect a work tool", "careerbrew reads your commits, PRs, tickets, and messages from it, so the log and the suggestions fill in", "#/setup", "careerbrew connect", _connected),
    ("watch", "Turn on the day watcher", "it keeps Today, the day log, and the suggestions fresh while you work", "#/setup", "careerbrew day watch on", _watching),
    ("task", "Add your first task", "the planner fits it into your day and Next step names it", "#/week", 'careerbrew work task "<title>" --hours 1', _has_task),
    ("goal", "Set a goal", "Teach me and the plan work toward it", "#/goals", 'careerbrew goals set "<title>" --date <YYYY-MM-DD>', _has_goal),
    ("decide", "Accept or dismiss one suggestion", "a suggestion changes nothing until you decide; Today explains each one", "#/today", "careerbrew day inbox", _decided),
    ("pm", "Say where your PM update goes", "Copy update and Send use it", "#/setup", "careerbrew config set pm_slack <#channel>", _pm_set),
]


def build() -> dict[str, Any]:
    """Each step with its done mark, the count, and whether Dashboard shows the list (not hidden, not all done)."""
    steps = [{"id": sid, "title": title, "why": why, "view": view, "command": command, "done": bool(test())} for sid, title, why, view, command, test in STEPS]
    done = sum(s["done"] for s in steps)
    hidden = not config.get("first_run_checklist")
    return {"steps": steps, "done": done, "total": len(steps), "hidden": hidden, "show": not hidden and done < len(steps)}
