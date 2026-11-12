#!/usr/bin/env python3
"""Time the web API on the synthetic e2e seed: two requests in a row per endpoint (issues #117, #142).

Usage: uv run python scripts/api-timing.py [--json] [--check] [--port N]

It seeds a fresh data root in a temp folder with tests/e2e/web-seed-and-serve.sh (synthetic data
only, never private/), starts `careerbrew serve` there, waits for its start warm (issue #142: the
server builds the work views at start, `/api/ping` says `warm: done`), and prints, per endpoint:
the first and the second request in milliseconds, the response size, and the `X-Careerbrew-Cache`
header (`hit` when the server answered from its in-memory cache). It also checks that the
`/api/changes` version did not move: a read must not look like new data to the live views.
Then new data arrives (a note from the CLI, then `careerbrew day watch once`); once the server
warmed again, it opens every warm view once: each must answer from memory. Then a second watcher
cycle has nothing new (task watch-idle-no-change): it must not move the version, and every cached
view must still answer from memory after it.
`--check` exits 1 unless every cached view (the four that issue #117 timed among them) answered
its second request from memory, every warm view answered its first request from memory (at start
and after new data), no read moved the version, and the idle cycle moved nothing.
Times are printed, never asserted: they depend on the machine's load.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))
from careerbrew.server import FOCUS_BLOCKS, WARM_VIEWS  # noqa: E402  (the server's own lists)

TIMED_IN_ISSUE = {"/api/daylog", "/api/week?days=7", "/api/today", "/api/work"}
ENDPOINTS = (
    "/api/daylog",
    "/api/week?days=7",
    "/api/today",
    "/api/work",
    "/api/focus",
    *(f"/api/focus/{b}" for b in FOCUS_BLOCKS),
    "/api/inbox",
    "/api/feed/asks",
    "/api/notifications",
    "/api/jobs",
    "/api/interviews",
    "/api/cards",
    "/api/library",
    "/api/goals",
    "/api/mistakes",
    "/api/exercises",
    "/api/review",
    "/api/overview",
    "/api/config",
    "/api/actions",
)


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def get(port: int, token: str, path: str) -> tuple[float, int, str, bytes]:
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", headers={"X-Careerbrew-Token": token, "Host": f"127.0.0.1:{port}"})
    t0 = time.perf_counter()
    with urllib.request.urlopen(req, timeout=120) as resp:  # noqa: S310 (local seed server)
        body = resp.read()
        cache = resp.headers.get("X-Careerbrew-Cache", "-")
        status = resp.status
    return (time.perf_counter() - t0) * 1000, status, cache, body


def watch_once(env: dict[str, str]) -> None:
    """One watcher cycle on the seed's data root, the way `careerbrew day watch once` runs it."""
    subprocess.run([sys.executable, "-m", "careerbrew", "day", "watch", "once"], env=env, check=True, capture_output=True, timeout=600)  # noqa: S603


def wait_warm(port: int, token: str, timeout: float = 600) -> float:
    """Wait until the server warmed every warm view for the current data (`/api/ping`); return the ms waited."""
    t0 = time.perf_counter()
    while time.perf_counter() - t0 < timeout:
        if json.loads(get(port, token, "/api/ping")[3]).get("warm") in ("done", "off"):
            return (time.perf_counter() - t0) * 1000
        time.sleep(0.05)
    raise TimeoutError("the server did not finish warming")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--json", action="store_true", help="print JSON rows instead of a table")
    ap.add_argument("--port", type=int, default=0, help="port for the seed server (default: a free one)")
    ap.add_argument("--check", action="store_true", help="exit 1 unless every cached view answered its second request from memory, and neither a read nor an idle watcher cycle moved /api/changes")
    args = ap.parse_args()
    port = args.port or free_port()
    token = "timing-" + os.urandom(8).hex()
    scratch = Path(tempfile.mkdtemp(prefix="careerbrew-timing."))
    env = {**os.environ, "CAREERBREW_HOME": str(scratch / "data"), "CAREERBREW_SERVE_TOKEN": token}
    # The seed's own defaults, set here so the watcher cycles below read the same encrypted data root and clock.
    env |= {"CAREERBREW_LLM": "off", "CAREERBREW_NOTIFY": "off", "CAREERBREW_NOW": os.environ.get("CAREERBREW_NOW") or "2026-10-19T09:00"}
    env |= {"CAREERBREW_KEY_FILE": str(scratch / "key"), "CAREERBREW_PAIR_KEY_FILE": str(scratch / "pair"), "CAREERBREW_BACKUP_DIRS": str(scratch / "backups")}
    log = (scratch / "serve.log").open("w")
    proc = subprocess.Popen([str(ROOT / "tests" / "e2e" / "web-seed-and-serve.sh"), str(port)], env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)  # noqa: S603
    try:
        deadline = time.time() + 600  # seeding runs the real CLI; a busy machine is slow
        while True:
            if proc.poll() is not None:
                print(f"api-timing: the seed server exited; see {scratch / 'serve.log'}", file=sys.stderr)
                return 1
            try:
                get(port, token, "/api/config")
                break
            except (urllib.error.URLError, ConnectionError, TimeoutError):
                if time.time() > deadline:
                    print("api-timing: the seed server did not start in 600 s", file=sys.stderr)
                    return 1
                time.sleep(0.5)
        start_warm_ms = wait_warm(port, token)
        version = json.loads(get(port, token, "/api/changes")[3])["version"]
        rows = []
        for path in ENDPOINTS:
            first_ms, status, first_cache, body = get(port, token, path)
            second_ms, _, second_cache, _ = get(port, token, path)
            rows.append({"endpoint": path, "status": status, "first_ms": round(first_ms, 1), "second_ms": round(second_ms, 1), "bytes": len(body), "cache": f"{first_cache}/{second_cache}"})
        moved = json.loads(get(port, token, "/api/changes")[3])["version"] != version
        # New data: a note from the CLI, then a watcher cycle, which also takes in what the seed added after its
        # own cycle. The server warms again; then the first open of each warm view must answer from memory.
        cached_paths = [r["endpoint"] for r in rows if r["cache"] != "-/-"]
        subprocess.run([sys.executable, "-m", "careerbrew", "work", "note", "api-timing: a synthetic note", "--tags", "delivery"], env={**env, "CAREERBREW_TEST_OWNER": "1"}, check=True, capture_output=True, timeout=600)  # noqa: S603
        watch_once(env)
        update_warm_ms = wait_warm(port, token)
        after_update = {path: get(port, token, path)[2] for path in WARM_VIEWS}
        for _ in range(2):  # a rebuild may keep its new output once (the day log): read until nothing is left to write
            for path in cached_paths:
                get(port, token, path)
        idle_version = json.loads(get(port, token, "/api/changes")[3])["version"]
        watch_once(env)
        idle_moved = json.loads(get(port, token, "/api/changes")[3])["version"] != idle_version
        idle_hits = {path: get(port, token, path)[2] for path in cached_paths}
    finally:
        try:
            os.killpg(proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass  # it already exited
        proc.wait(timeout=30)
        log.close()
    shutil.rmtree(scratch, ignore_errors=True)
    first_hits = {r["endpoint"]: r["cache"].split("/")[0] for r in rows if r["endpoint"] in WARM_VIEWS}
    if args.json:
        print(
            json.dumps(
                {
                    "rows": rows,
                    "start_warm_ms": round(start_warm_ms),
                    "update_warm_ms": round(update_warm_ms),
                    "first_open_cache": first_hits,
                    "after_update_cache": after_update,
                    "changes_version_moved": moved,
                    "idle_cycle_moved_version": idle_moved,
                    "idle_cycle_cache": idle_hits,
                },
                indent=2,
            )
        )
    else:
        print(f"{'endpoint':<22} {'1st ms':>9} {'2nd ms':>9} {'bytes':>8}  cache")
        for r in rows:
            print(f"{r['endpoint']:<22} {r['first_ms']:>9.1f} {r['second_ms']:>9.1f} {r['bytes']:>8}  {r['cache']}")
        print(f"start warm: {start_warm_ms:.0f} ms; warm after new data: {update_warm_ms:.0f} ms")
        print(f"warm views answered their first open from memory: {sum(c == 'hit' for c in first_hits.values())}/{len(first_hits)}")
        print(f"warm views answered the first open after new data from memory: {sum(c == 'hit' for c in after_update.values())}/{len(after_update)}")
        print(f"/api/changes version moved during the reads: {'yes' if moved else 'no'}")
        print(f"/api/changes version moved by an idle watcher cycle: {'yes' if idle_moved else 'no'}")
        print(f"cached views answered from memory after the idle cycle: {sum(c == 'hit' for c in idle_hits.values())}/{len(idle_hits)}")
    if not args.check:
        return 0
    cached = {r["endpoint"]: r["cache"] for r in rows if r["cache"] != "-/-"}
    problems = [f"{e}: the second request was not answered from memory ({c})" for e, c in cached.items() if not c.endswith("/hit")]
    problems += [f"{e}: not cached" for e in sorted(TIMED_IN_ISSUE - set(cached))]
    problems += [f"{e}: the first open after the start warm was not answered from memory ({c})" for e, c in first_hits.items() if c != "hit"]
    problems += [f"{e}: the first open after new data was not answered from memory ({c})" for e, c in after_update.items() if c != "hit"]
    problems += ["reading views moved the /api/changes version"] if moved else []
    problems += ["an idle watcher cycle moved the /api/changes version"] if idle_moved else []
    problems += [f"{e}: not answered from memory after an idle watcher cycle ({c})" for e, c in idle_hits.items() if c != "hit"]
    for p in problems:
        print(f"api-timing: FAIL {p}", file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
