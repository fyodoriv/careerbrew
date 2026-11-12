"""Worked-example code tours that fade as you learn, checked by a maintainer.

A tour is a list of steps, each a file and line in a staged repo with an explanation
and, optionally, the concept it teaches. `export` writes a CodeTour file
(https://github.com/microsoft/codetour) to `out/tours/`: a step whose concept is
mastered loses its explanation and asks you to predict instead (fading worked
examples, Renkl & Atkinson 2003). A maintainer approves a tour at a commit; when a
file it points at changes after that commit, the export says the tour needs review.
The tour itself is stored encrypted under `learn/<project>/tours/`.
"""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path
from typing import Any

from .. import calendar, events, paths, store
from . import project as lp

FADED = "Predict before you read: what does this code do, and why is it here? ({title})"


def _repos(project: str) -> dict[str, Path]:
    return {s["label"]: Path(s["path"]) for s in lp._manifest(project).get("sources", []) if s["kind"] == "repo"}


def _path(project: str, name: str) -> Path:
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]*", name):
        raise ValueError("a tour name is lowercase letters, digits, and hyphens")
    return lp.folder(project) / "tours" / f"{name}.json"


def check(project: str, tour: dict[str, Any]) -> list[str]:
    repos, known = _repos(project), {c["id"] for c in lp.concepts(project)}
    problems = []
    repo = repos.get(str(tour.get("repo", "")))
    if repo is None:
        return [f"repo must be a staged repo: {', '.join(sorted(repos)) or 'none staged'}"]
    if not str(tour.get("title", "")).strip():
        problems.append("a tour needs a title")
    steps = tour.get("steps", [])
    if not steps:
        problems.append("a tour needs at least one step")
    for i, s in enumerate(steps, 1):
        f = repo / str(s.get("file", ""))
        if not s.get("file") or not f.is_file():
            problems.append(f"step {i}: no file {s.get('file')} in {repo.name}")
            continue
        lines = len(f.read_text(encoding="utf-8", errors="replace").splitlines())
        if not isinstance(s.get("line"), int) or not 1 <= s["line"] <= lines:
            problems.append(f"step {i}: line must be 1 to {lines}")
        if not str(s.get("description", "")).strip():
            problems.append(f"step {i}: needs a description")
        if s.get("concept") and s["concept"] not in known:
            problems.append(f"step {i}: concept {s['concept']} is not imported")
    return problems


def add(project: str, name: str, tour: dict[str, Any]) -> dict[str, Any]:
    problems = check(project, tour)
    if problems:
        raise ValueError("tour not added:\n  " + "\n  ".join(problems))
    record = {**tour, "approved": None}
    store.write_json(_path(project, name), record)
    events.emit("learn.tour", action="add", steps=len(tour["steps"]))
    return record


def _load(project: str, name: str) -> dict[str, Any]:
    tour = store.read_json(_path(project, name), None)
    if not tour:
        raise KeyError(f"no tour {name} in {project}; add one with `careerbrew learn tour add {project} {name} FILE`")
    return tour


def approve(project: str, name: str, by: str) -> dict[str, Any]:
    tour = _load(project, name)
    repo = _repos(project)[tour["repo"]]
    sha = subprocess.run(["git", "-C", str(repo), "rev-parse", "HEAD"], capture_output=True, text=True, check=False).stdout.strip()
    tour["approved"] = {"by": by, "sha": sha, "at": calendar.now_utc().isoformat(timespec="seconds")}
    store.write_json(_path(project, name), tour)
    events.emit("learn.tour", action="approve")
    return tour


def review_state(project: str, tour: dict[str, Any]) -> tuple[str, list[str]]:
    """approved, unreviewed, or stale (with the toured files that changed since approval)."""
    if not tour.get("approved"):
        return "unreviewed", []
    repo = _repos(project)[tour["repo"]]
    files = sorted({s["file"] for s in tour["steps"]})
    out = subprocess.run(["git", "-C", str(repo), "diff", "--name-only", tour["approved"]["sha"], "--", *files], capture_output=True, text=True, check=False)
    changed = out.stdout.split() if out.returncode == 0 else files
    return ("stale" if changed else "approved"), changed


def export(project: str, name: str) -> tuple[Path, dict[str, Any]]:
    tour = _load(project, name)
    log = lp._by_concept(lp._log(project))
    titles = {c["id"]: c["title"] for c in lp.concepts(project)}
    state, changed = review_state(project, tour)
    steps, faded = [], 0
    for s in tour["steps"]:
        mastered = bool(s.get("concept")) and lp._mastered(log.get(s["concept"], []))
        faded += mastered
        steps.append({"file": s["file"], "line": s["line"], "description": FADED.format(title=titles[s["concept"]]) if mastered else s["description"]})
    if state != "approved":
        note = "No maintainer has approved this tour yet." if state == "unreviewed" else f"Changed since a maintainer approved it: {', '.join(changed)}. Read with care."
        steps[0] = {**steps[0], "description": f"**{note}**\n\n{steps[0]['description']}"}
    out = paths.data_root() / "out" / "tours" / f"{lp.slug(project)}-{name}.tour"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({"$schema": "https://aka.ms/codetour-schema", "title": tour["title"], "steps": steps}, indent=2), encoding="utf-8")
    events.emit("learn.tour", action="export", status=state, steps=len(steps), faded=faded)
    return out, {"state": state, "faded": faded, "steps": len(steps), "changed": changed}
