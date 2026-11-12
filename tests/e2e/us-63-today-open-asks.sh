#!/usr/bin/env bash
# US-63: Today lists the work feed's open asks (newest first; past 50, Show all loads the rest (issue #275); with the source link and how long each waited),
# and Mark personal runs the same `work feed mark` action as the CLI.
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
export UV_OFFLINE=1 UV_NO_SYNC=1 UV_PYTHON_DOWNLOADS=never npm_config_offline=true
export CAREERBREW_NOW="2026-10-19T09:00" CAREERBREW_NOTIFY=off
cb init >/dev/null
# 1. The CLI counts the same asks, with the overdue ones (the story's metric: work hours only, so the weekend does not count).
cb work feed add "$FIX/feed/web-asks.jsonl" >/dev/null
expect "$(cb work feed show)" "open asks=2 (1 waiting over 8 work hours)" "open asks and the overdue count"
cb config set feed_ask_overdue_hours 24 >/dev/null
expect "$(cb work feed show)" "open asks=2 (0 waiting over 24 work hours)" "the overdue hours are a setting"
expect "$(cb config set feed_ask_overdue_hours 0 2>&1 || true)" "from 1 to 168" "error case: a bad setting names the range"
expect "$(cb work feed mark slack:nope personal 2>&1 || true)" "no feed event slack:nope" "error case: an unknown event is named with its fix"
cb work feed mark slack:D0DANA:1792410900.000400 personal >/dev/null
expect "$(cb work feed show)" "open asks=1 (0 waiting over 24 work hours)" "a personal ask is no longer open"
# 2. The web app: the list, the source link, Mark personal and Undo, and the error state of the button.
cd "$ROOT/web"
# The installed line reporter emits cursor escapes even for captured output.
# Keep its plain progress and summary, then require one complete pass line.
export PW_TEST_DEBUG_REPORTERS=1 PLAYWRIGHT_FORCE_TTY=0 FORCE_COLOR=0
expect_five_passes() {
  local summaries
  local duration='([[:blank:]]+\([0-9]+(\.[0-9]+)?(ms|s|m|h|d)\))?'
  summaries="$(grep -Ex "[[:blank:]]*[0-9]+ passed${duration}[[:blank:]]*" <<<"$1")" || true
  if ! [[ "$summaries" =~ ^[[:blank:]]*5\ passed${duration}[[:blank:]]*$ ]]; then
    printf '%s\n' "$1" >&2
    fail "$2 (expected one exact 5-passed summary)"
  fi
}
out="$(env -u CAREERBREW_HOME -u CAREERBREW_NOW npx --no-install --offline playwright test --reporter=line e2e/feed.spec.ts 2>&1)" || { echo "$out"; fail "open asks e2e"; }
expect_five_passes "$out" "Today lists open asks newest first; Show all loads past 50; Mark personal runs work feed mark; its error shows the fix; Next step links its selected ask"
[ -s "$CAREERBREW_E2E_OUT/today-open-asks.png" ] || fail "screenshot of the open asks"
[ -s "$CAREERBREW_E2E_OUT/today-open-asks-show-all.png" ] || fail "screenshot of Show all past 50 open asks"
# The read-only expansion remains usable during a live refresh, regardless of response order.
out="$(env -u CAREERBREW_HOME -u CAREERBREW_NOW npx --no-install --offline playwright test --reporter=line e2e/open-asks-refresh-click.spec.ts 2>&1)" || { echo "$out"; fail "open asks refresh click"; }
expect "$out" "3 passed" "Show all works during a refresh in both response orders"
# 3. The same test on a winter Saturday: the seed moves the asks to the pinned day, so the waits and the order hold.
out="$(env -u CAREERBREW_HOME CAREERBREW_NOW=2026-12-05T09:00 npx --no-install --offline playwright test --reporter=line e2e/feed.spec.ts 2>&1)" || { echo "$out"; fail "open asks e2e on a later day"; }
expect_five_passes "$out" "open asks on any pinned day: a winter Saturday shows the same waits, order and Next step link"
event_seen work.feed.ingest
event_seen work.feed.mark
echo "ok US-63 open asks on Today"
