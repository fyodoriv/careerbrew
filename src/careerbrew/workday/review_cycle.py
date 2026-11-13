"""Read one approved plugin's PR inventory and offer private review questions."""

import json
import os
import selectors
import shlex
import subprocess
import time
from datetime import date
from typing import Any

from ..work import review_ask
from . import questions, watch

MAX_INVENTORY_BYTES = 256 * 1024
READY = {
    "owned": True,
    "open": True,
    "draft": False,
    "checks_green": True,
    "mergeable": True,
    "threads_resolved": True,
    "reviewer_requested": False,
    "review_submitted": False,
}


def _identity(url: str) -> str:
    ref = review_ask.recheck_pr_link(url)
    return f"https://{ref['host']}/{ref['owner']}/{ref['repo']}/pull/{ref['number']}".lower()


def _inventory(command: str, seconds: float) -> list[Any]:
    argv = shlex.split(command)
    deadline = time.monotonic() + seconds
    data = bytearray()
    with (
        subprocess.Popen(  # noqa: S603 (approved work step)
            argv,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            env={**os.environ, watch.SKIP_ENV: "skip"},
        ) as process,
        selectors.DefaultSelector() as selector,
    ):
        assert process.stdout is not None
        selector.register(process.stdout, selectors.EVENT_READ)
        try:
            while len(data) <= MAX_INVENTORY_BYTES:
                if not selector.select(max(0, deadline - time.monotonic())):
                    raise subprocess.TimeoutExpired(argv, seconds)
                chunk = os.read(process.stdout.fileno(), 8192)
                if not chunk:
                    break
                data.extend(chunk)
            if len(data) > MAX_INVENTORY_BYTES or process.wait(timeout=max(0, deadline - time.monotonic())):
                raise ValueError("review inventory failed")
        finally:
            if process.poll() is None:
                process.kill()
    body = json.loads(data)
    if not isinstance(body, dict) or body.get("complete") is not True or not isinstance(body.get("pull_requests"), list):
        raise ValueError("review inventory is incomplete")
    return body["pull_requests"]


def run(step: dict[str, Any], seconds: float) -> bool:
    """Close verified review questions and offer at most one question per eligible PR."""
    existing = set()
    for question in questions.open_records():
        recheck = question["recheck"]
        if questions.kind(recheck) != "review-pr":
            continue
        try:
            existing.add(_identity(recheck.partition(" ")[2]))
        except ValueError:
            continue
        if question["step"] == step["step"] and question["source"] == step["from"]:
            state, why = questions.check_one(question)
            if state == "closed":
                questions.close(question["id"], why, date.fromisoformat(question["day"]), expected=question)
    try:
        rows = _inventory(step["review_ask_cycle"], seconds)
    except (OSError, ValueError, subprocess.TimeoutExpired, RecursionError):
        return False

    candidates: dict[str, bool] = {}
    for row in rows:
        if not isinstance(row, dict) or not isinstance(row.get("url"), str):
            return False
        try:
            url = _identity(row["url"])
        except ValueError:
            return False
        ready = all(row.get(key) is expected for key, expected in READY.items())
        candidates[url] = candidates.get(url, True) and ready
    for url, ready in candidates.items():
        text = f"Draft a review ask for {url}?"
        if ready and url not in existing and len(text) <= questions.MAX_TEXT:
            questions.add(step["step"], text, recheck=f"careerbrew:review-pr {url}", source=step["from"])
    return True
