"""Real synthetic373 server; Playwright webServer owns this process.

No external process, transport, secret resolution or notification is permitted.
The test uses the normal Handler and app build. Only its data/output binding
and deny-only boundary differ. Mutable records live for this one server.
"""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import time
import webbrowser
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCENARIOS = ("ascii", "unicode", "legacy-missing", "legacy-invalid", "legacy-naive", "legacy-unavailable", "crowded")
TOKEN = "issue373-synthetic-not-a-secret"


def write(path: Path, data: object) -> None:
    raw = json.dumps(data, indent=2, sort_keys=True) + "\n"
    with path.open("x") as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())
    assert path.read_text() == raw


class Environment:
    def setenv(self, key: str, value: str) -> None:
        os.environ[key] = value

    def delenv(self, key: str, raising: bool = False) -> None:
        os.environ.pop(key, None)


def main() -> int:
    if len(sys.argv) != 3 or sys.argv[1] not in SCENARIOS:
        raise SystemExit("BLOCKED: fixed373 scenario and owned run root required")
    scenario, run = sys.argv[1], Path(sys.argv[2])
    if run.is_symlink() or not run.is_dir() or not (run / ".owned-issue373-ui").is_file():
        raise SystemExit("BLOCKED: missing owned UI root")
    if not run.resolve().is_relative_to(ROOT / ".metrics/issue373/ui-runs"):
        raise SystemExit("BLOCKED: outside declared output root")
    own = run / scenario
    own.mkdir(mode=0o700)
    sys.path.insert(0, str(ROOT))
    from tests.unit.test_issue_373 import DAY, RAW, CutWorld, snapshot

    env = Environment()
    world = CutWorld(own / "world", env, own / "evidence")
    from careerbrew import connectors, secrets, server, store
    from careerbrew.day import log, pm_learn, results
    from careerbrew.work import priorities, projects

    denied = []

    def deny(*_args, **_kwargs):
        denied.append("forbidden process/transport/secret/browser operation")
        raise AssertionError("BLOCKED: server attempted a forbidden host operation")

    # Reuse the existing data-show fixture's deny-only process/transport boundary.
    subprocess.run = subprocess.Popen = deny
    socket.create_connection = socket.socket.connect = socket.socket.connect_ex = socket.socket.sendto = deny
    connectors.http_json = connectors.http_get = secrets.resolve = webbrowser.open = deny
    store.write_yaml(world.home / "settings.yaml", {"timezone": "America/New_York", "first_run_checklist": False, "notify_events": False})
    results.append([{"id": "synthetic-373-review", "at": "2026-11-02T14:00:00-05:00", "kind": "github", "title": "Reviewed 4 pull requests", "source": "synthetic", "repo": "", "origin": "user"}], date.fromisoformat(DAY))
    task = priorities.add("reconcile amber invoices", 3, 3, 0.05, [], source="user")
    projects.add("billing")
    projects.link_task("billing", task["id"])
    labels = ["Other"]
    legacy_records = {}
    if scenario == "unicode":
        labels = ["Équipe"]
    elif scenario == "crowded":
        labels = [*(f"cut-{i:02d}" for i in range(1, 15)), "Équipe réconciliation des factures et des relevés"]
    for n, label in enumerate(labels):
        if label == "Other":
            continue
        t = priorities.add(f"check synthetic receipt {n + 1}", 3, 3, 0.05, [], source="user")
        # Fixed synthetic legacy shape, accepted by _load/task_projects. This
        # does not assert that the current CLI creates Unicode project names.
        legacy_records[label] = {"created_at": "2026-10-26T19:00:00+00:00", "memory": "", "stale_after_days": 7, "sources": [], "tasks": [t["id"]]}
    if legacy_records:
        store.write_yaml(projects.path(), {"projects": {**projects.records(), **legacy_records}})
        assert all(k in projects._load() for k in legacy_records)
        assert all(projects.task_projects()[r["tasks"][0]] == k for k, r in legacy_records.items())
    built = log.build(date.fromisoformat(DAY))
    raw = "\n".join(log.pm_lines(built, learn=False))
    if scenario == "ascii" or scenario.startswith("legacy-"):
        assert raw == RAW, "BLOCKED: real UI builder differs from registered primary input"
    for label in labels:
        assert any(line.startswith(label + ":") for line in raw.splitlines()), "BLOCKED: real raw label not supplied"
    kept = "\n".join(line for line in raw.splitlines() if line.partition(":")[0] not in labels)
    assert kept
    world.cuts(raw=raw, body=kept)
    assert set(labels) <= set(pm_learn.rules()["cut"])
    if scenario.startswith("legacy-"):
        rows = store.read_jsonl(pm_learn.path())[:2]
        for n, row in enumerate(rows):
            row.pop("saved", None)
            if scenario == "legacy-invalid":
                row["saved"] = "not-a-time"
            elif scenario == "legacy-naive":
                row["saved"] = "2026-10-27T15:00:00"
            elif scenario == "legacy-unavailable":
                if n == 0:
                    row["day"] = "invalid-day"
                else:
                    row.pop("day", None)
        store.write_jsonl(pm_learn.path(), rows)
        assert set(labels) <= set(pm_learn.rules()["cut"])
    store.write_text(world.home / "work/owner-notes.md", "synthetic373 notes stay separate")
    pm = log.pm_report(built)
    assert pm["body"] == kept and set(labels) <= set(pm["left_out"])
    server.DIST = run / "build"
    if not (server.DIST / "index.html").is_file():
        raise RuntimeError("BLOCKED: missing real Vite build")
    # Real views settle their existing write-on-change state before measurement.
    server.daylog_view()
    server.today()
    server.daylog_view()
    requests = []
    original = server.Handler.do_POST

    def guarded_post(handler):
        path = handler.path.split("?", 1)[0]
        requests.append({"method": "POST", "path": path})
        if path not in ("/api/session", "/api/timing"):
            denied.append("forbidden business POST")
            handler.send_error(403)
            return
        return original(handler)

    server.Handler.do_POST = guarded_post
    srv = server.make_server(0, TOKEN)
    identity = {"pid": os.getpid(), "ppid": os.getppid(), "pgid": os.getpgid(0), "sid": os.getsid(0)}
    write(
        own / "ready.json",
        {
            "schema": 1,
            "scenario": scenario,
            "identity": identity,
            "port": srv.server_address[1],
            "raw": raw,
            "kept": kept,
            "labels": labels,
            "legacy_records": legacy_records,
            "initial_pm": pm,
            "seed_snapshot": snapshot(world.root),
            "source": "real encrypted records, builder, rules and Handler; no response injection",
        },
    )
    outcome = "unexpected-return"
    try:
        address = f"http://127.0.0.1:{srv.server_address[1]}/"
        print(f"issue373-ready {scenario} {address}", flush=True)
        ready_ns = time.monotonic_ns()
        write(
            own / "startup-stop.json",
            {
                "run": str(run),
                "mode": os.environ["ISSUE373_MODE"],
                "scenario": scenario,
                "identity": identity,
                "address": address,
                "ready_ns": ready_ns,
            },
        )
        srv.serve_forever()
    except KeyboardInterrupt:
        outcome = "owned-Playwright-SIGINT"
    finally:
        srv.server_close()
        write(
            own / "terminal.json",
            {
                "identity": identity,
                "outcome": outcome,
                "server_closed": True,
                "forbidden_attempts": denied,
                "requests": requests,
                "final_snapshot": snapshot(world.root),
                "logical": world.logical(),
                "subprocess_attempts": 0 if not denied else "see-forbidden-attempts",
            },
        )
    return 0 if not denied else 2


if __name__ == "__main__":
    raise SystemExit(main())
