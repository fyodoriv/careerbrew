"""Read one approved plugin's PR inventory and offer private review questions."""

import json
import os
import selectors
import shlex
import signal
import subprocess
import threading
import time
from datetime import date
from typing import Any

from ..work import review_ask
from . import questions, watch

MAX_INVENTORY_BYTES = 256 * 1024
GROUP_OWNER_ENV = "CAREERBREW_REVIEW_GROUP_OWNER"
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


def _question_source(step: dict[str, Any]) -> str:
    return "owner" if step["from"] == "settings" else step["from"]


def _stop_owned_group(process: subprocess.Popen[Any]) -> None:
    """Stop the invocation-scoped process group and reap its direct child."""
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    except PermissionError:
        if process.poll() is None:
            process.kill()
        # macOS can refuse a group with a live or unreaped leader.
        process.wait()
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        return
    process.wait()


def _inventory(command: str, seconds: float) -> list[Any]:
    argv = shlex.split(command)
    deadline = time.monotonic() + seconds
    data = bytearray()
    if seconds <= 0:
        raise subprocess.TimeoutExpired(argv, seconds)
    process = subprocess.Popen(  # noqa: S603 (approved work step)
        argv,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        env={**os.environ, watch.SKIP_ENV: "skip", GROUP_OWNER_ENV: "1"},
        start_new_session=True,
    )
    with selectors.DefaultSelector() as selector:
        assert process.stdout is not None
        selector.register(process.stdout, selectors.EVENT_READ)
        try:
            while len(data) <= MAX_INVENTORY_BYTES:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not selector.select(remaining):
                    raise subprocess.TimeoutExpired(argv, seconds)
                chunk = os.read(process.stdout.fileno(), 8192)
                if not chunk:
                    break
                data.extend(chunk)
            if len(data) > MAX_INVENTORY_BYTES or process.wait(timeout=max(0, deadline - time.monotonic())):
                raise ValueError("review inventory failed")
        finally:
            process.stdout.close()
            # Popen.__exit__ also waits; never enter it after a cleanup error.
            _stop_owned_group(process)
    body = json.loads(data)
    if not isinstance(body, dict) or body.get("complete") is not True or not isinstance(body.get("pull_requests"), list):
        raise ValueError("review inventory is incomplete")
    return body["pull_requests"]


def _check_one(question: dict[str, Any], deadline: float) -> tuple[str, str]:
    """Bound a read-only recheck; a late answer is discarded and never closes a record."""
    done = threading.Event()
    result: list[tuple[str, str]] = []

    def check() -> None:
        try:
            result.append(questions.check_one(question, deadline=deadline))
        except Exception:
            result.append(("unknown", "review recheck: could not verify PR state"))
        finally:
            done.set()

    threading.Thread(target=check, daemon=True).start()
    if not done.wait(max(0, deadline - time.monotonic())):
        return "unknown", "review recheck: deadline exceeded"
    return result[0]


def run(step: dict[str, Any], seconds: float) -> bool:
    """Close verified review questions and offer at most one question per eligible PR."""
    deadline = time.monotonic() + seconds
    source = _question_source(step)
    existing = set()
    for question in questions.open_records():
        recheck = question["recheck"]
        if questions.kind(recheck) != "review-pr":
            continue
        try:
            existing.add(_identity(recheck.partition(" ")[2]))
        except ValueError:
            continue
        if question["step"] == step["step"] and question["source"] == source:
            state, why = _check_one(question, deadline)
            if why == "review recheck: deadline exceeded":
                return False
            if state == "closed":
                questions.close(question["id"], why, date.fromisoformat(question["day"]), expected=question)
        if time.monotonic() >= deadline:
            return False
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        return False
    try:
        rows = _inventory(step["review_ask_cycle"], remaining)
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
        if time.monotonic() >= deadline:
            return False
        text = f"Draft a review ask for {url}?"
        if ready and url not in existing and len(text) <= questions.MAX_TEXT:
            questions.add(step["step"], text, recheck=f"careerbrew:review-pr {url}", source=source)
    return True
