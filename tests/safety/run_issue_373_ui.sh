#!/usr/bin/env bash
# Root authorizes each exact373 mode separately. Never install or touch owner data.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
mode="${1:-}"
case "$mode" in primary|dates|comfort|geometry|proof-before|proof-after) ;; *) echo 'BLOCKED: fixed373 mode required' >&2; exit 2;; esac
[ "$#" = 1 ] || exit 2
umask 077
for folder in "$ROOT/.metrics" "$ROOT/.metrics/issue373" "$ROOT/.metrics/issue373/ui-runs"; do
  [ ! -L "$folder" ] || { echo 'BLOCKED: proof parent is a symlink' >&2; exit 2; }
done
mkdir -p "$ROOT/.metrics/issue373/ui-runs"
run="$(mktemp -d "$ROOT/.metrics/issue373/ui-runs/$mode.XXXXXX")"
printf 'registered373\n' > "$run/.owned-issue373-ui"
for name in home data tmp config cache state runtime keys clients commands backups public-cards cargo rustup target; do mkdir "$run/$name"; done
printf '' > "$run/git-global"
printf 'issue373 retained root: %s\n' "$run"
node="$(command -v node || true)"
python="$ROOT/.venv/bin/python"
[ -n "$node" ] && [ -x "$node" ] && [ -x "$python" ] || { echo 'BLOCKED: installed runtime missing' >&2; exit 2; }
case "$node" in /*) ;; *) echo 'BLOCKED: Node path must be absolute' >&2; exit 2;; esac
# Resolve the pinned Playwright headless-shell through its registry. Never install a browser here.
discovery_env=("HOME=${HOME:?HOME required}" "PATH=/usr/bin:/bin")
[ -z "${XDG_CACHE_HOME:-}" ] || discovery_env+=("XDG_CACHE_HOME=$XDG_CACHE_HOME")
[ -z "${PLAYWRIGHT_BROWSERS_PATH:-}" ] || discovery_env+=("PLAYWRIGHT_BROWSERS_PATH=$PLAYWRIGHT_BROWSERS_PATH")
browser="$(cd "$ROOT/web" && /usr/bin/env -i "${discovery_env[@]}" "$node" -e 'const p=require("node:path"); const r=require(p.join(p.dirname(require.resolve("playwright-core/package.json")),"lib/server/registry/index.js")).registry; process.stdout.write(r.findExecutable("chromium-headless-shell").executablePathOrDie("javascript"));')" || { echo 'BLOCKED: installed Playwright browser missing' >&2; exit 2; }
case "$browser" in /*) [ -x "$browser" ] || { echo 'BLOCKED: installed Playwright browser missing' >&2; exit 2; };; *) echo 'BLOCKED: browser path must be absolute' >&2; exit 2;; esac
# env -i precedes every JS/Python/app import. Build only the actual app, with
# envDir=false and owned output/cache. Existing dependencies are read-only.
exec /usr/bin/env -i HOME="$run/home" PATH=/usr/bin:/bin:/opt/homebrew/bin TMPDIR="$run/tmp" LANG=C LC_ALL=C TZ=America/New_York \
  XDG_CONFIG_HOME="$run/config" XDG_CACHE_HOME="$run/cache" XDG_STATE_HOME="$run/state" XDG_RUNTIME_DIR="$run/runtime" \
  CAREERBREW_HOME="$run/data" CAREERBREW_KEY_FILE="$run/keys/backup.key" CAREERBREW_PAIR_KEY_FILE="$run/keys/pair.key" \
  CAREERBREW_CLAUDE_CONFIG="$run/clients/claude.json" CAREERBREW_CODEX_CONFIG="$run/clients/codex.toml" OP_CONFIG_DIR="$run/clients/op" \
  CAREERBREW_COMMANDS_DIR="$run/commands" CAREERBREW_BACKUP_DIRS="$run/backups" CAREERBREW_PUBLIC_CARDS="$run/public-cards" \
  CAREERBREW_NOW=2026-11-02T15:00:00-05:00 CAREERBREW_EVENTS=off CAREERBREW_LLM=off CAREERBREW_NOTIFY=off CAREERBREW_POSTHOG_KEY= \
  CAREERBREW_TELEMETRY_URL=http://127.0.0.1:9 DO_NOT_TRACK=1 PYTHONDONTWRITEBYTECODE=1 PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 \
  UV_OFFLINE=1 UV_NO_SYNC=1 UV_PYTHON_DOWNLOADS=never CARGO_NET_OFFLINE=true CARGO_HOME="$run/cargo" RUSTUP_HOME="$run/rustup" \
  GIT_CONFIG_GLOBAL="$run/git-global" GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_COUNT=0 GIT_TERMINAL_PROMPT=0 \
  ISSUE373_RUN="$run" ISSUE373_MODE="$mode" ISSUE373_NODE="$node" ISSUE373_PYTHON="$python" ISSUE373_BROWSER="$browser" PWTEST_CACHE_DIR="$run/cache/playwright" DEBUG=pw:browser,pw:webserver \
  /bin/bash --noprofile --norc "$ROOT/tests/safety/issue373_ui_child.sh"
