"""Replay a real merged PR as a hands-on task, graded by the PR's own tests.

The learner gets the PR's title and its tests on the code from just before the PR,
and makes those tests pass. Doing the real change beats answering about it
(docs/learn.md, "Replay a PR"). A task lives in a shared clone under
`learn/<project>/replay/<pr>/` in the data root, so nothing is written into the
learned repo, its `.git` included.

Only squash-merged PRs on the first-parent line count (a subject that ends in `(#N)`),
and only small ones: at most SMALL_FILES files and SMALL_LINES changed lines.
A task is runnable when the PR's tests fail on the parent and pass on the PR.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

from .. import calendar, events, store
from . import project as lp

SMALL_FILES = 12
SMALL_LINES = 500
TEST_TIMEOUT = 300  # seconds for one test run: a hang guard, not a speed limit
PR = re.compile(r"\(#(\d+)\)$")
PY_TEST = re.compile(r"(^|/)(tests?/.+\.py|test_[^/]+\.py|[^/]+_test\.py)$")


def _git(repo: Path, *args: str, check: bool = True) -> str:
    out = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True, check=False)
    if check and out.returncode != 0:
        raise RuntimeError(f"git {' '.join(args[:2])} failed in {repo.name}: {out.stderr.strip()[:200]}")
    return out.stdout


def _changes(repo: Path, sha: str) -> list[tuple[str, int, str]]:
    """(path, changed lines, status) for each file the commit changed."""
    stats = {}
    for line in _git(repo, "show", "--numstat", "--format=", sha).splitlines():
        add, delete, path = line.split("\t", 2)
        stats[path] = (int(add) if add.isdigit() else 0) + (int(delete) if delete.isdigit() else 0)
    status: dict[str, str] = {}
    for line in _git(repo, "show", "--name-status", "--format=", "--no-renames", sha).splitlines():
        if "\t" in line:
            code, path = line.split("\t", 1)
            status[path] = code
    return [(p, n, status.get(p, "M")) for p, n in stats.items()]


def candidates(repo: Path, limit: int = 50) -> list[dict[str, Any]]:
    """The newest `limit` small squash-merged PRs, each with its test and source files."""
    out: list[dict[str, Any]] = []
    for line in _git(repo, "log", "--first-parent", "--format=%H%x09%s", "HEAD").splitlines():
        sha, _, subject = line.partition("\t")
        m = PR.search(subject)
        if not m:
            continue
        files = _changes(repo, sha)
        if len(files) > SMALL_FILES or sum(n for _, n, _ in files) > SMALL_LINES:
            continue
        tests = sorted(p for p, _, s in files if PY_TEST.search(p) and s != "D" and not p.endswith("conftest.py"))
        out.append({"pr": int(m.group(1)), "sha": sha, "title": PR.sub("", subject).strip(), "tests": tests, "sources": sorted(p for p, _, _ in files if p not in tests), "files": len(files)})
        if len(out) >= limit:
            break
    return out


def _repo(project: str, repo: str | None) -> Path:
    if repo:
        return Path(repo).expanduser().resolve()
    staged = [s for s in lp._manifest(project).get("sources", []) if s["kind"] == "repo"]
    if not staged:
        raise ValueError(f"{project} has no staged repo: pass --repo PATH or run `careerbrew learn start {project} --repo PATH`")
    return Path(staged[0]["path"])


def _command(tests: list[str], python: str | None) -> list[str]:
    return [python or sys.executable, "-m", "pytest", "-q", "-x", "-p", "no:cacheprovider", "-p", "no:xdist", *tests]


def _run(folder: Path, cmd: list[str]) -> tuple[bool, str]:
    env = {**os.environ, "PYTHONPATH": str(folder / "src") + os.pathsep + str(folder)}
    for key in [k for k in env if k.startswith("CAREERBREW_") and k not in ("CAREERBREW_NOW",)]:
        env.pop(key)  # the replayed tests set up their own data folder; never hand them the real one
    try:
        out = subprocess.run(cmd, cwd=folder, env=env, capture_output=True, text=True, timeout=TEST_TIMEOUT, check=False)
    except subprocess.TimeoutExpired:
        return False, f"timed out after {TEST_TIMEOUT} s"
    tail = (out.stdout + out.stderr).strip().splitlines()
    return out.returncode == 0, tail[-1] if tail else ""


def _clone(repo: Path, dest: Path, parent: str, sha: str, tests: list[str]) -> None:
    """A shared clone at the PR's parent, with only the PR's tests brought in."""
    if dest.exists():
        shutil.rmtree(dest)
    dest.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["git", "clone", "-q", "--shared", "--no-checkout", str(repo), str(dest)], check=True, capture_output=True)
    _git(dest, "checkout", "-q", "--detach", parent)
    _git(dest, "checkout", sha, "--", *tests)


def _pick(repo: Path, pr: int) -> dict[str, Any]:
    for c in candidates(repo, limit=10_000):
        if c["pr"] == pr:
            return c
    raise KeyError(f"no small squash-merged PR #{pr} in {repo.name}; see `careerbrew learn replay list`")


def start(project: str, pr: int, repo: str | None = None, python: str | None = None) -> dict[str, Any]:
    root = _repo(project, repo)
    c = _pick(root, pr)
    if not c["tests"]:
        raise ValueError(f"PR #{pr} changed no Python test file, so nothing can grade it; pick another (`careerbrew learn replay list`)")
    parent = _git(root, "rev-parse", f"{c['sha']}^").strip()
    dest = lp.folder(project) / "replay" / str(pr)
    _clone(root, dest, parent, c["sha"], c["tests"])
    task = {"pr": pr, "title": c["title"], "sha": c["sha"], "parent": parent, "tests": c["tests"], "dir": str(dest), "cmd": _command(c["tests"], python), "started": calendar.now_utc().isoformat(timespec="seconds")}
    store.write_json(lp.folder(project) / "replay" / f"{pr}.json", task)
    events.emit("learn.replay", action="start", files=c["files"])
    return task


def check(project: str, pr: int) -> dict[str, Any]:
    task = store.read_json(lp.folder(project) / "replay" / f"{pr}.json", None)
    if not task:
        raise KeyError(f"no replay task #{pr} in {project}; start it with `careerbrew learn replay start {project} {pr}`")
    passed, last = _run(Path(task["dir"]), task["cmd"])
    row = {"pr": pr, "passed": passed, "at": calendar.now_utc().isoformat(timespec="seconds")}
    store.append_jsonl(lp.folder(project) / "replay.jsonl", [row])
    events.emit("learn.replay", action="check", status="pass" if passed else "fail")
    return {**row, "last": last, "tests": task["tests"]}


def runnable(repo: Path, c: dict[str, Any], python: str | None = None) -> tuple[bool, str]:
    """Do the PR's tests fail on the parent and pass on the PR? (the replay coverage metric)."""
    if not c["tests"]:
        return False, "no Python test file"
    if not [s for s in c["sources"] if not s.endswith(".md")]:
        return False, "only docs changed besides the tests"
    parent = _git(repo, "rev-parse", f"{c['sha']}^").strip()
    with tempfile.TemporaryDirectory(prefix="careerbrew-replay-") as tmp:
        dest = Path(tmp) / "clone"
        _clone(repo, dest, parent, c["sha"], c["tests"])
        cmd = _command(c["tests"], python)
        before, _ = _run(dest, cmd)
        if before:
            return False, "the tests already pass before the PR"
        _git(dest, "checkout", "-q", "--detach", c["sha"])
        after, last = _run(dest, cmd)
        return (True, "fails before, passes after") if after else (False, f"the tests fail on the PR itself: {last[:80]}")


def measure(repo: Path, limit: int = 50, python: str | None = None) -> dict[str, Any]:
    rows = []
    for c in candidates(repo, limit):
        ok, why = runnable(repo, c, python)
        rows.append({"pr": c["pr"], "runnable": ok, "why": why})
    share = round(sum(r["runnable"] for r in rows) / len(rows), 3) if rows else None
    events.emit("learn.replay", action="measure", prs=len(rows), share=share or 0)
    return {"prs": len(rows), "runnable": sum(r["runnable"] for r in rows), "share": share, "rows": rows}
