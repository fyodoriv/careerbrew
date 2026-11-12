#!/usr/bin/env bash
# US-01 (happy path): a new contributor runs the one-line setup and gets a
# healthy machine. Copies the repo to a scratch dir so the real checkout is
# never touched. Replace or extend with the first real user story.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/e2e-setup.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT

# Other stories run beside this one (scripts/e2e.sh) and write test-results and
# __pycache__ files; a file removed mid-copy fails tar, and a fresh clone has neither.
tar -C "$root" --exclude=.git --exclude=node_modules --exclude=.venv --exclude=target \
  --exclude=private --exclude=.env --exclude=.metrics \
  --exclude=test-results --exclude=playwright-report --exclude=__pycache__ -cf - . | tar -C "$scratch" -xf -
# Dummy values for required env keys, so the doctor can pass in the copy.
[ -f "$scratch/.env.example" ] && sed -E 's/^([A-Z][A-Z0-9_]*)=[^#]*# required/\1=e2e-dummy # required/' \
  "$scratch/.env.example" > "$scratch/.env"
# Fast mode reuses nothing and installs nothing; E2E_COLD=1 runs the full stranger install.
skip=1; [ "${E2E_COLD:-0}" = 1 ] && skip=0
client_home="$scratch/client-home"
bin_dir="$client_home/.local/bin"
mkdir -p "$bin_dir"
out="$(cd "$scratch" && HOME="$client_home" CAREERBREW_BIN_DIR="$bin_dir" PATH="$bin_dir:$PATH" \
  RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" \
  CAREERBREW_HOME="$scratch/data" CAREERBREW_KEY_FILE="$scratch/key" CAREERBREW_PAIR_KEY_FILE="$scratch/pair" \
  CAREERBREW_BACKUP_DIRS="$scratch/backups" XDG_STATE_HOME="$scratch/state" \
  CAREERBREW_CLAUDE_CONFIG="$client_home/.claude.json" CAREERBREW_CODEX_CONFIG="$client_home/.codex/config.toml" \
  CAREERBREW_POSTHOG_KEY="" CAREERBREW_TELEMETRY_URL="http://127.0.0.1:9" CAREERBREW_LLM=off CAREERBREW_NOTIFY=off \
  SETUP_SKIP_DEPS=$skip scripts/setup.sh 2>&1)" || { code=$?; printf '%s\n' "$out" >&2; exit "$code"; }
grep -q "doctor: all checks passed" <<<"$out" || { echo "$out"; echo "FAIL: setup did not end healthy"; exit 1; }
[ -d "$scratch/.metrics" ] || { echo "FAIL: setup did not create .metrics/"; exit 1; }
echo "ok US-01 setup"
