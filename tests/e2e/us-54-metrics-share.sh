#!/usr/bin/env bash
# US-54: opt-in weekly aggregate metrics, numbers only.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_METRICS_DIR="$SCRATCH/metrics"
cb init >/dev/null
cb work note "Shipped with $CANARY_PERSON at $CANARY_EMPLOYER" --tags delivery >/dev/null
# Error case first: off by default, nothing written.
expect "$(cb metrics weekly)" "nothing written" "error case: off by default"
[ ! -d "$CAREERBREW_METRICS_DIR" ] || fail "a file was written while sharing is off"
cb config set metrics_share true >/dev/null
expect "$(cb metrics weekly)" "wrote" "opted in: file written"
f="$(ls "$CAREERBREW_METRICS_DIR"/weekly-*.json)"
expect "$(cat "$f")" '"tool": "careerbrew"' "shared provider format"
if grep -qF "$CANARY_PERSON" "$f" || grep -qF "$CANARY_EMPLOYER" "$f"; then fail "private text in the metrics file"; fi
event_seen metrics.weekly
echo "ok US-54 metrics share"
