#!/usr/bin/env bash
# US-06: see my readiness as charts built from my real attempts.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
cb drill answer beh-conflict-1 --met star,ownership,result,tradeoff,learning >/dev/null
cb drill answer arrays-two-pointers-1 --met correct >/dev/null
out="$(cb drill progress)"
expect "$out" "over 2 attempt(s)" "progress counts attempts"
html="$CAREERBREW_HOME/out/progress.html"
[ -f "$html" ] || fail "progress.html written"
c="$(cat "$html")"
expect "$c" "<svg" "has charts"
expect "$c" "Mastery by topic" "mastery chart"
expect "$c" "Readiness over time" "trend chart"
expect "$c" "Table view" "table view for accessibility"
refute "$c" "<script" "no external scripts"
echo "ok US-06 progress chart"
