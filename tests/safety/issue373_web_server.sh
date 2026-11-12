#!/usr/bin/env bash
# Playwright owns this whole command group, including the real app build.
set -euo pipefail
py="${ISSUE373_PYTHON:?owned Python required}"
"$py" -I -S -B -c '
import time
started_ns = time.monotonic_ns()
import json
import os
import sys
from pathlib import Path

scenario, root, pid = sys.argv[1:]
run = Path(root)
if root != os.environ["ISSUE373_RUN"] or run.is_symlink() or (run / ".owned-issue373-ui").read_text() != "registered373\n":
    raise SystemExit("BLOCKED: startup timing requires owned UI root")
if scenario not in {"ascii", "unicode", "legacy-missing", "legacy-invalid", "legacy-naive", "legacy-unavailable", "crowded"}:
    raise SystemExit("BLOCKED: unknown startup timing scenario")
with (run / f"startup-{scenario}.json").open("x") as stream:
    json.dump({"run": root, "mode": os.environ["ISSUE373_MODE"], "scenario": scenario,
               "pid": int(pid), "started_ns": started_ns}, stream)
    stream.write("\n")
' "$1" "$2" "$$"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
scenario="$1"; run="$2"
first=ascii
case "$ISSUE373_MODE" in comfort|geometry|proof-before|proof-after) first=crowded;; esac
if [ "$scenario" = "$first" ]; then
  cd "$ROOT/web"
  "$ISSUE373_NODE" node_modules/vite/bin/vite.js build --config vite.issue373.config.ts --configLoader runner > "$run/build.stdout" 2> "$run/build.stderr"
  printf 'actual Vite exit0\n' > "$run/build.ok"
else
  # Readiness is the completed owned build, never a timeout or a sleep. The
  # maintained Playwright webServer startup bound is 180s for this whole group.
  until [ -f "$run/build.ok" ]; do sleep 0.05; done
fi
cd "$ROOT"
exec "$ROOT/.venv/bin/python" -I -B "$ROOT/tests/safety/issue373_server.py" "$scenario" "$run"
