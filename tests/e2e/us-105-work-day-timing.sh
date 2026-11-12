#!/usr/bin/env bash
# US-105: each work step's row has its end and duration, a history keeps them across days, a slow step is flagged, and the day is a run page (issue #263).
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
cb config set work_steps "/standup, /deploy" >/dev/null
for day in 2026-10-23 2026-10-24 2026-10-26; do
  export CAREERBREW_NOW="${day}T09:00+00:00"
  cb day update /standup --status running >/dev/null
  export CAREERBREW_NOW="${day}T09:05+00:00"
  cb day update /standup --status done >/dev/null
done
export CAREERBREW_NOW="2026-10-28T09:00+00:00"
cb day update /standup --status running >/dev/null
cb day update /deploy --status running >/dev/null
export CAREERBREW_NOW="2026-10-28T09:18+00:00"
cb day update /standup --status done --summary "posted the synthetic-standup-note" >/dev/null
cb day update /deploy --status failed --error-class network >/dev/null
plan="$(cb day run --json)"
expect "$plan" '"seconds": 1080' "a finished row carries its duration"
expect "$plan" '"ended": "2026-10-28T09:18:00+00:00"' "a finished row carries its end stamp"
expect "$plan" '"slow": true' "a step slower than twice its median is flagged"
event_seen workday.update
out="$(cb day render)"
expect "$out" "1 slow" "the page counts the slow step"
items="$(cb report latest --run work-day)"
expect "$items" "took 18 min (slow: median 5 min)" "the page shows the duration and the median"
refute "$items" "synthetic-standup-note" "privacy: row text stays on the page only"
expect "$(cb report latest --json)" '"run": "work-day"' "the finished day is the latest run page"
hist="$(cb day history)"
expect "$hist" "fails most: /deploy (1)" "the history names the step that fails most"
expect "$hist" "4 day(s), 2 step(s)" "the history covers the week"
expect "$(cb day history --json --days 1)" '"days": [{"day": "2026-10-28"' "the history keeps each day"
expect "$(cb config set work_slow_percent 50 2>&1 || true)" "110" "error case: a slow factor under 110 percent is refused"
expect "$(cb day history --days 0 2>&1 || true)" "from 1 to 120" "error case: a zero-day history is refused"
echo "ok US-105 work day timing"
