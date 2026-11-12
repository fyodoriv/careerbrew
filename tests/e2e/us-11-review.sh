#!/usr/bin/env bash
# US-11: careerbrew measures outcomes and tells me when to pivot.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
cb work task "a" --hours 1 >/dev/null
cb work done t001 4 >/dev/null
expect "$(cb review --html)" "review: blocked: " "the run ends with its page and one summary line"
out="$(cb report latest)"  # the run page as text
expect "$out" "blocker  estimate_ratio is PIVOT" "missed threshold triggers PIVOT"
expect "$out" "Split tasks" "pivot action shown"
expect "$out" "done     want_agreement: no data yet" "missing data is no-data, not zero"
[ -f "$CAREERBREW_HOME/out/dashboard.html" ] || fail "dashboard written"
[ -s "$CAREERBREW_HOME/review.jsonl" ] || fail "review snapshot stored"
echo "ok US-11 review"
