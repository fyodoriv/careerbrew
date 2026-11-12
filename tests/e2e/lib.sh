#!/usr/bin/env bash
# Shared e2e helpers (not a story file). Each story runs the real CLI against a
# fresh synthetic data root in a temp dir, so the user's private/ is never touched.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIX="$ROOT/tests/fixtures"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/careerbrew-e2e.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT
# Never the Keychain key, never the real endpoint: no test sends telemetry (US-89).
export CAREERBREW_POSTHOG_KEY="" CAREERBREW_TELEMETRY_URL="http://127.0.0.1:9"
export CAREERBREW_HOME="$SCRATCH/data" CAREERBREW_LLM=off CAREERBREW_EVENTS=on
export CAREERBREW_PUBLIC_CARDS="${CAREERBREW_PUBLIC_CARDS:-$SCRATCH/public-cards}"
export CAREERBREW_KEY_FILE="$SCRATCH/key/backup.key" CAREERBREW_BACKUP_DIRS="$SCRATCH/backups" CAREERBREW_PAIR_KEY_FILE="$SCRATCH/key/pair.key"
export CAREERBREW_COMMANDS_DIR="$SCRATCH/commands"
# Plugin command deployment is an external integration: never touch owner config in a story.
mkdir -p "$SCRATCH/bin" "$SCRATCH/client-home"
export CAREERBREW_CLIENT_HOME="$SCRATCH/client-home"
cat > "$SCRATCH/bin/agentbrew" <<'AGENTBREW'
#!/bin/sh
[ "$*" = "sync --only commands" ] || exit 2
[ -f Agentfile.yaml ] || exit 3
AGENTBREW
chmod +x "$SCRATCH/bin/agentbrew"
export PATH="$SCRATCH/bin:$PATH"
export CAREERBREW_CODEX_CONFIG="$CAREERBREW_CLIENT_HOME/.codex/config.toml"
export CAREERBREW_CLAUDE_CONFIG="$CAREERBREW_CLIENT_HOME/.claude.json"

export CAREERBREW_EXTRA_KNOWLEDGE=0  # only the base knowledge set (5 sample companies): the same result with or without the optional batch files
export TZ="Etc/GMT+4"  # the zone an empty `timezone` setting falls back to: fixed UTC-4, no place name
export XDG_STATE_HOME="$SCRATCH/state"  # checkup runs (issue #232): never the owner's ~/.local/state
export CAREERBREW_TEST_OWNER=1  # e2e scripts act as the owner (adds tasks, sets goals); agent paths unset it
CANARY_PERSON="CANARY-PERSON-7Q"
CANARY_EMPLOYER="CANARY-EMPLOYER-3X"

cb() {
  if [ -x "$ROOT/.venv/bin/careerbrew" ]; then HOME="$CAREERBREW_CLIENT_HOME" "$ROOT/.venv/bin/careerbrew" "$@"; else HOME="$CAREERBREW_CLIENT_HOME" uv run --quiet --project "$ROOT" careerbrew "$@"; fi
}
fail() { echo "FAIL: $*" >&2; exit 1; }
# A here-string, not `printf | grep -q`: with pipefail, grep -q exiting on an early
# match sent SIGPIPE to printf and failed the pipe (test: test_e2e_lib.py).
expect() { # expect <haystack> <needle> <message>
  grep -qF -- "$2" <<<"$1" || { printf '%s\n' "$1" >&2; fail "$3 (missing: $2)"; }
}
refute() {
  if grep -qF -- "$2" <<<"$1"; then printf '%s\n' "$1" >&2; fail "$3 (found: $2)"; fi
}
# Data files are encrypted at rest: read them decrypted, the way careerbrew does.
data_cat() { "$ROOT/.venv/bin/python" -c 'import sys; from pathlib import Path; from careerbrew import store; sys.stdout.write(store.read_text(Path(sys.argv[1])) or "")' "$CAREERBREW_HOME/$1"; }
data_write() { "$ROOT/.venv/bin/python" -c 'import sys; from pathlib import Path; from careerbrew import store; store.write_text(Path(sys.argv[1]), sys.stdin.read())' "$CAREERBREW_HOME/$1"; }
data_has() { # data_has <term>: true when any data file holds the term once decrypted (a leak check must not pass because of ciphertext)
  "$ROOT/.venv/bin/python" -c 'import sys
from careerbrew import store, vault
sys.exit(0 if any(sys.argv[1] in (store.read_text(p) or "") for p in vault.files()) else 1)' "$1"
}
event_seen() { grep -q "\"event\": \"$1\"" "$CAREERBREW_HOME/logs/events.jsonl" || fail "event $1 not logged"; }
# A story that runs Playwright calls this first: a free port and an output folder of its own
# (web/test-results/<story>/, screenshots included), so browser stories run in parallel (scripts/e2e.sh).
# It also gets a Playwright compile cache of its own, removed with SCRATCH. Playwright 1.59 writes a cache entry in
# place, so a run beside another on a cold cache could read it empty and list that spec with 0 tests and no error
# (task us75-list-count-cold-start). Fixed upstream in 1.62 (microsoft/playwright#41739); drop this line after an upgrade.
browser_story() {
  export CAREERBREW_E2E_OUT="$ROOT/web/test-results/$(basename "$0" .sh)"
  CAREERBREW_E2E_PORT="$("$ROOT/.venv/bin/python" -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')"
  export CAREERBREW_E2E_PORT PWTEST_CACHE_DIR="$SCRATCH/playwright-cache"
}
# pw_total <test dir> [playwright args...]: print how many tests `npx playwright test [args] --list` finds, run in web/.
# It fails, and shows the list with its stderr, when the list fails or has no test from a spec file in <test dir>.
pw_total() {
  local dir="$1" list spec missing=""; shift
  list="$(npx playwright test "$@" --list 2>&1)" || { printf '%s\n' "$list" >&2; fail "Playwright lists the tests in $dir/"; }
  for spec in "$dir"/*.spec.ts; do
    grep -qF " $(basename "$spec"):" <<<"$list" || missing="$missing $(basename "$spec")"
  done
  [ -z "$missing" ] || { printf '%s\n' "$list" >&2; fail "Playwright lists a test from every spec file in $dir/ (none from:$missing)"; }
  sed -n 's/^Total: \([0-9]*\) tests.*/\1/p' <<<"$list"
}

# `careerbrew open` and `notify open` start the web app in the background and hand its link to the
# browser: a fake browser ($BROWSER, read by Python's webbrowser) logs the link, a free port keeps the
# real 8765 alone, and web_stop stops the server the story started (call it from the story's trap).
fake_browser() {
  export FAKE_BROWSER_LOG="$SCRATCH/browser.log"
  # exit 0 always: a failing browser makes Python's webbrowser try the next one, the real browser
  printf '#!/bin/sh\nprintf "%%s\\n" "$1" >> "%s"\nexit 0\n' "$FAKE_BROWSER_LOG" > "$SCRATCH/fake-browser"
  chmod +x "$SCRATCH/fake-browser"
  export BROWSER="$SCRATCH/fake-browser"
  cb config set serve_port "$("$ROOT/.venv/bin/python" -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')" >/dev/null
}
web_stop() {
  local pid
  pid="$(data_cat day/web.json 2>/dev/null | "$ROOT/.venv/bin/python" -c 'import json, sys; print(json.load(sys.stdin).get("pid", ""))' 2>/dev/null || true)"
  [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
  [ -n "$pid" ] && printf '{}' | data_write day/web.json  # a second call (the trap) must not kill a reused pid
  return 0
}

seed_jobs() {
  cb init >/dev/null
  cat > "$CAREERBREW_HOME/jobs/sources.yaml" <<YAML
sources:
  - {kind: greenhouse, board: northwind, company: Northwind, url: 'file://$FIX/ats/greenhouse-northwind.json'}
  - {kind: lever, board: contoso, company: Contoso, url: 'file://$FIX/ats/lever-contoso.json'}
  - {kind: ashby, board: fabrikam, company: Fabrikam, url: 'file://$FIX/ats/ashby-fabrikam.json'}
YAML
  cat > "$CAREERBREW_HOME/profile.yaml" <<YAML
name: $CANARY_PERSON
employer: $CANARY_EMPLOYER
current_level: 3
target_salary: 180000
remote_preference: remote
skills: {python: 4, typescript: 3, sql: 3}
want_to_learn: [llm, rust]
YAML
}

seed_repo() { # seed_repo <dir>: a git repo with commits by me@example.com
  local repo="$1"; mkdir -p "$repo"; git init -q -b main "$repo"
  local i=0
  for msg in "feat: launch export" "feat: ship search" "fix: flaky checkout test" "docs: design doc for queue" "perf: cut p99 latency 40%"; do
    i=$((i + 1)); echo "$i" > "$repo/f$i.txt"; git -C "$repo" add .
    git -C "$repo" -c user.email=me@example.com -c user.name=Me -c commit.gpgsign=false commit -q --no-verify -m "$msg"
  done
}
