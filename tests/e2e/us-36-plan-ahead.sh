#!/usr/bin/env bash
# US-36: plan ahead in the web app: see the next days from calendar facts and add tasks that land in the schedule.
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
cd "$ROOT/web"
out="$(npx playwright test --reporter=line -g "Week plans ahead" 2>&1)" || { echo "$out"; fail "plan-ahead e2e"; }
expect "$out" "1 passed" "plan-ahead test passed"
echo "ok US-36 plan ahead"
