#!/usr/bin/env bash
# Helper (not a story file): seed a synthetic data root and run `careerbrew serve` for the Playwright suite.
# Usage: web-seed-and-serve.sh <port>   (env CAREERBREW_SERVE_TOKEN fixes the token)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
FIX="$ROOT/tests/fixtures"
# Never the Keychain key, never the real endpoint: no test sends telemetry (US-89).
export CAREERBREW_POSTHOG_KEY="" CAREERBREW_TELEMETRY_URL="http://127.0.0.1:9"
export CAREERBREW_HOME="${CAREERBREW_HOME:-$(mktemp -d "${TMPDIR:-/tmp}/careerbrew-web.XXXXXX")/data}"
# Seeder/server process scope only: the parent browser keeps its HOME and cached browser binaries.
export HOME="${CAREERBREW_CLIENT_HOME:-$(dirname "$CAREERBREW_HOME")/client-home}"
mkdir -p "$HOME"
export CAREERBREW_CLAUDE_CONFIG="$HOME/.claude.json" CAREERBREW_CODEX_CONFIG="$HOME/.codex/config.toml"
export XDG_STATE_HOME="$HOME/.local/state"
# Only the base knowledge set (5 sample companies): pictures do not depend on optional batch files.
export CAREERBREW_EXTRA_KNOWLEDGE=0
export CAREERBREW_LLM=off CAREERBREW_NOTIFY=off CAREERBREW_NOW="${CAREERBREW_NOW:-2026-10-19T09:00}"
export CAREERBREW_KEY_FILE="${CAREERBREW_KEY_FILE:-$CAREERBREW_HOME/../key}" CAREERBREW_PAIR_KEY_FILE="${CAREERBREW_PAIR_KEY_FILE:-$CAREERBREW_HOME/../pair}" CAREERBREW_BACKUP_DIRS="${CAREERBREW_BACKUP_DIRS:-$CAREERBREW_HOME/../backups}"
# Every seed step runs in one Python process (tests/e2e/web_seed.py): ~30 CLI processes took 14 s at load 5 and
# about 50 s at load ~100, and every browser story pays it.
"$ROOT/.venv/bin/python" -I "$ROOT/tests/e2e/web_seed.py" "$FIX"
# The seed connected Slack with this synthetic token, and the server reads recorded answers instead of the network.
export CB_SLACK="synthetic-not-a-token" CAREERBREW_HTTP_FIXTURES="${CAREERBREW_HTTP_FIXTURES:-$FIX/http}"
exec "$ROOT/.venv/bin/careerbrew" serve --port "${1:-8799}"
