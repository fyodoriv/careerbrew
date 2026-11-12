#!/usr/bin/env bash
# US-25: plan my day from facts: meetings, free time, deadlines, promotion gaps, and how long tasks really take.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
echo "calendars: [{kind: ics, path: '$FIX/calendar/work.ics'}]" > "$CAREERBREW_HOME/day/calendars.yaml"
cb work task "write design doc" --impact 3 --hours 2 --competencies design >/dev/null
cb work task "fix flaky test" --impact 3 --hours 1 >/dev/null
cb work task "huge migration" --impact 5 --hours 9 >/dev/null
cb work task "old one" --impact 1 --hours 1 >/dev/null
cb work done t004 2 >/dev/null   # took 2x: calibration 2.0
out="$(CAREERBREW_NOW=2026-10-19T08:00 cb day plan)"
expect "$out" "2 meeting(s)" "meetings from the calendar"
expect "$out" "estimates x2.0" "calibration from real task history"
out="$(cb report latest)"  # the run page as text
expect "$out" "No room today for t003" "too-big task left out"
expect "$out" "closes promotion gap design" "reasons shown"
echo "calendars: [{kind: ics, url: 'https://calendar.example.com/private.ics'}]" > "$CAREERBREW_HOME/day/calendars.yaml"
CAREERBREW_NOW=2026-10-19T08:00 cb day plan >/dev/null
expect "$(cb report latest)" "ics: SecretError" "error case: a literal private ICS link is refused"
echo "ok US-25 day plan"
