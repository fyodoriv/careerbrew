#!/usr/bin/env bash
# US-59 isolated seed. Playwright owns the normal careerbrew server.
set -euo pipefail
umask 077
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
world="${CAREERBREW_PM_CUT_WORLD:?owned world required}"
[[ "$world" == /*/pm-cut-browser && -d "$world" && ! -L "$world" ]]
[[ "$(cd "$world" && pwd -P)" == "$world" ]]
[[ -f "$world/.owner" && ! -L "$world/.owner" ]]
[[ "$(cat "$world/.owner")" == "US-59 isolated PM cut fixture" ]]
[[ ! -e "$world/data" && ! -e "$world/seed.json" ]]
mkdir "$world/data" "$world/userhome" "$world/tmp"
exec /usr/bin/env -i HOME="$world/userhome" PATH=/usr/bin:/bin TMPDIR="$world/tmp" LANG=C LC_ALL=C TZ=America/New_York \
  CAREERBREW_PM_CUT_WORLD="$world" CAREERBREW_HOME="$world/data" CAREERBREW_KEY_FILE="$world/key" \
  CAREERBREW_PAIR_KEY_FILE="$world/pair" CAREERBREW_BACKUP_DIRS="$world/backups" CAREERBREW_PUBLIC_CARDS="$world/cards" \
  CAREERBREW_CODEX_CONFIG="$world/userhome/no-codex.toml" CAREERBREW_CLAUDE_CONFIG="$world/userhome/no-claude.json" \
  CAREERBREW_COMMANDS_DIR="$world/commands" XDG_STATE_HOME="$world/state" XDG_CONFIG_HOME="$world/config" \
  XDG_CACHE_HOME="$world/cache" CAREERBREW_NOW=2026-11-02T15:00:00-05:00 CAREERBREW_TEST_OWNER=1 \
  CAREERBREW_ENCRYPT=on CAREERBREW_LLM=off CAREERBREW_NOTIFY=off CAREERBREW_EVENTS=off CAREERBREW_POSTHOG_KEY= \
  CAREERBREW_TELEMETRY_URL=http://127.0.0.1:9 CAREERBREW_SERVE_TOKEN="${CAREERBREW_SERVE_TOKEN:?token required}" \
  /bin/bash --noprofile --norc -c '
    set -e
    "$1/.venv/bin/python" -I -B "$1/tests/e2e/seed_pm_cut.py"
    exec "$1/.venv/bin/python" -I -B -m careerbrew serve --port "$2"
  ' seed-and-serve "$ROOT" "${1:?port required}"
