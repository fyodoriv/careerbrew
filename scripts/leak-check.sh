#!/usr/bin/env bash
# Block commits and pushes that carry private data: any path under private/, or any
# sensitive term (profile, private/sensitive-terms.txt, installed plugins). Prints
# term ids, never the terms. Usage: scripts/leak-check.sh [--staged]
set -euo pipefail
cd "$(dirname "$0")/.."
if [ -x .venv/bin/careerbrew ]; then exec .venv/bin/careerbrew privacy scan "$@"; fi
exec uv run --quiet careerbrew privacy scan "$@"
