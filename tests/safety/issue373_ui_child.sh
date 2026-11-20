#!/usr/bin/env bash
# Runs only after the parent installed its exact owned environment.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT/web"
printf 'issue373 UI invocation: mode=%s pid=%s output=%s\n' "$ISSUE373_MODE" "$$" "$ISSUE373_RUN"
status=2
finish() { printf '%s\n' "$status" > "$ISSUE373_RUN/runner.exit"; }
trap finish EXIT
py="${ISSUE373_PYTHON:?owned Python required}"
"$py" -I -S -B "$ROOT/tests/safety/issue373_metadata.py" entry
set +e
"$ISSUE373_NODE" node_modules/playwright/cli.js test --config playwright.issue373.config.ts > "$ISSUE373_RUN/playwright.stdout" 2> "$ISSUE373_RUN/playwright.stderr"
status=$?
set -e
printf '%s\n' "$status" > "$ISSUE373_RUN/playwright.exit"
set +e
"$py" -I -S -B "$ROOT/tests/safety/issue373_metadata.py" terminal
status=$?
set -e
exit "$status"
