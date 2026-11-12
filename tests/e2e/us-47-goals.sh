#!/usr/bin/env bash
# US-47: only the owner sets goals, and changing the upcoming goal re-plans the schedule at once.
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
"$ROOT/.venv/bin/careerbrew" init >/dev/null
expect "$(CAREERBREW_TEST_OWNER= cb goals set "Learn hashing" --kind learn </dev/null || true)" "only you set goals" "an agent (no terminal) cannot set goals"
expect "$(cb goals show)" "no upcoming goal" "nothing was set"
out="$(CAREERBREW_TEST_OWNER=1 cb goals set "Learn hashing" --kind learn --topics "arrays and hashing" --minutes 60)"
expect "$out" "Today re-planned" "setting a goal re-plans today"
cb day plan >/dev/null
expect "$(cb report latest)" "goal:exercise" "the day plan starts with the goal's study block"
CAREERBREW_TEST_OWNER=1 cb goals set "Promotion case" --kind promotion >/dev/null
cb day plan >/dev/null
out="$(cb report latest)"; expect "$out" "goal:promotion" "a new goal changes the plan"; refute "$out" "goal:exercise" "the old goal's blocks are gone"
expect "$(CAREERBREW_TEST_OWNER=1 cb goals set "x" --kind learn --date tomorrow || true)" "date must be a date" "bad input is refused"
event_seen goals.update
cd "$ROOT/web"
# a fresh seeded data root (this script's goals must not leak into the web server), on a later Friday than the
# seed's default Monday: the web tests derive every date from the pinned clock, so they pass on any day
out="$(env -u CAREERBREW_HOME CAREERBREW_NOW=2026-12-04T09:00 npx playwright test --reporter=line -g "Goals: only the owner sets them" 2>&1)" || { echo "$out"; fail "web goals form on a later date"; }
expect "$out" "1 passed" "the web goals form passes on a later date"
# a long-term goal, set by the owner
cd "$ROOT"
expect "$(CAREERBREW_TEST_OWNER=1 cb goals add-long-term "Staff engineer" --by 2027-12-31)" "long-term goals: 1" "a long-term goal is added"
expect "$(cb goals show)" "long-term: Staff engineer (by 2027-12-31)" "it shows with the goals"
echo "ok US-47 goals"
