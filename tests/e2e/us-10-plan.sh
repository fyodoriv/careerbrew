#!/usr/bin/env bash
# US-10: today's plan puts deadline and promotion-gap work first, within my hours.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
cb work task "routine cleanup" --impact 2 --urgency 2 --hours 3 >/dev/null
cb work task "write design doc" --impact 3 --urgency 3 --hours 2 --competencies design >/dev/null
cb work task "huge rewrite" --impact 5 --urgency 1 --hours 20 >/dev/null
out="$(cb work plan --hours 5)"
expect "$out" "plan: 5h of 5h" "fills capacity"
first="$(printf '%s\n' "$out" | sed -n 2p)"
expect "$first" "t002" "promo-gap task ranks first"
refute "$out" "t003" "20h task does not fit a 5h day"
expect "$(cb work done t002 3)" "3.0h vs 2.0h planned" "done records actuals"
echo "ok US-10 plan"
