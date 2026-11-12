#!/usr/bin/env bash
# US-86: see how much time careerbrew saves me, per week, from my own runs.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_EVENTS=on
cb init >/dev/null
cb work note "Shipped with $CANARY_PERSON" --tags delivery >/dev/null
# Error case first: an unknown action is refused, and no run yet means 0 minutes.
if cb time baseline lunch 5 >/dev/null 2>&1; then fail "an unknown action was accepted"; fi
expect "$(cb metrics time-saved)" "0 run(s)" "error case: nothing measured yet"
# Happy case: a real action run, then a measured baseline.
cb day plan >/dev/null
expect "$(cb time baseline day_plan 15)" "15 min (measured)" "baseline recorded"
out="$(cb metrics time-saved)"
expect "$out" "1 run(s)" "the day plan run counts"
expect "$out" "1/8 baselines measured" "measured and estimate baselines are told apart"
expect "$out" "(estimate, " "unset baselines are labeled estimates"
json="$(cb metrics time-saved --json)"
expect "$json" '"minutes_saved_total"' "json for agents"
if grep -qF "$CANARY_PERSON" "$CAREERBREW_HOME/out/time-saved.json"; then fail "text in the time-saved file"; fi
# Web app runs: the owner's own seconds from the open to Copy or Send (synthetic web.timing events) replace CLI time.
printf '%s\n' \
  '{"ts": "2026-10-20T14:00:00Z", "event": "web.timing", "saves": "pm_update", "step": "open", "seconds": 0.4}' \
  '{"ts": "2026-10-20T14:03:00Z", "event": "web.timing", "saves": "pm_update", "step": "copy", "seconds": 180}' \
  '{"ts": "2026-10-20T15:02:00Z", "event": "web.timing", "saves": "standup", "step": "copy", "seconds": 120}' >> "$CAREERBREW_HOME/logs/events.jsonl"
json="$(cb metrics time-saved --json)"
expect "$json" '"minutes_spent": 3.0' "a PM update run has the owner's time up to copy"
expect "$json" '"time": "web"' "a web run is labeled web"
expect "$(cb metrics time-saved)" "your time to copy, send, or accept in the web app" "the text says which time a run used"
f="$(cb metrics field)"
expect "$f" "field m-" "field totals under a salted-hash label"
expect "$f" "% ok" "field success rate from real runs"
if grep -qF "$(hostname)" "$CAREERBREW_HOME/out/field.json"; then fail "the host name in the field file"; fi
event_seen cli.run
echo "ok US-86 time saved"
