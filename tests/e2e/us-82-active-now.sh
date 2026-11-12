#!/usr/bin/env bash
# US-82: Active now shows the ranked projects on Today and Work, and Up, Down, Remove, and Put back run
# the same `work priority set` and `remove` actions as the CLI (issue #198).
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
cb init >/dev/null
# 1. The CLI ranking the web block shows: its numbers are the ones `work priority show` prints.
expect "$(cb work priority show)" "no project priority yet" "no ranking yet"
expect "$(cb work priority remove nope 2>&1 || true)" "unknown project nope" "error case: an unknown project is named"
expect "$(cb work priority set billing --rank 0 2>&1 || true)" "rank starts at 1" "error case: a bad rank names the fix"
cb work task "Migrate billing jobs" --impact 5 --hours 6 >/dev/null
cb work priority set billing --match '*billing*' >/dev/null
cb work priority set ledger >/dev/null
expect "$(cb work priority show)" "1. billing" "billing is rank 1"
expect "$(cb work priority show)" "1 open task, 0 items today" "the counts Active now shows"
expect "$(cb work priority set ledger --rank 1 && cb work priority show)" "1. ledger" "move a project up"
# 2. The web app: the block on Today and Work, each control's real effect, and the Projects order.
cd "$ROOT/web"
out="$(env -u CAREERBREW_HOME -u CAREERBREW_NOW npx playwright test --reporter=line e2e/active-now.spec.ts 2>&1)" || { echo "$out"; fail "active now e2e"; }
expect "$out" "2 passed" "Active now on Today and Work; Up, Down, Remove, Put back change the ranking"
[ -s "$CAREERBREW_E2E_OUT/today-active-now.png" ] || fail "screenshot of Active now"
event_seen work.priority
echo "ok US-82 active now"
