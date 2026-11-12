#!/usr/bin/env bash
# US-49: every mistake is named, becomes a prevention card, and a repeat is called out.
source "$(dirname "$0")/lib.sh"
"$ROOT/.venv/bin/careerbrew" init >/dev/null
cb goals set "Practice" >/dev/null
printf 'def contains_duplicate(nums):\n    return nums[0] in nums[1:]\n' > "$SCRATCH/crash.py"
out="$(cb exercises submit contains-duplicate "$SCRATCH/crash.py" || true)"
expect "$out" "mistake (" "the mistake is named"
refute "$out" "repeat mistake" "the first time is not a repeat"
expect "$(cb cards stats)" "mistakes" "a prevention card joins the mistakes deck"
expect "$(cb exercises submit contains-duplicate "$SCRATCH/crash.py" || true)" "repeat mistake" "a repeat is called out"
expect "$(cb teach next)" "This mistake repeated" "the teacher makes the repeat the next step"
event_seen exercise.submit
echo "ok US-49 mistakes"
