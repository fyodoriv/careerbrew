#!/usr/bin/env bash
# US-24: careerbrew tells me what to work on next, given my calendar and priorities.
source "$(dirname "$0")/lib.sh"
export UV_OFFLINE=1 UV_NO_SYNC=1 UV_PYTHON_DOWNLOADS=never
cb init >/dev/null
echo "calendars: [{kind: ics, path: '$FIX/calendar/work.ics'}]" > "$CAREERBREW_HOME/day/calendars.yaml"
cb work task "write design doc" --impact 3 --hours 0.5 --competencies design >/dev/null  # WSJF: (3+3+3 gap)/sqrt(0.5) beats (2+3)/sqrt(0.25)
cb work task "quick tweak" --impact 2 --hours 0.25 >/dev/null
expect "$(CAREERBREW_NOW=2026-10-19T09:23 cb day next)" "Meeting in 7 min" "prepare before a meeting"
expect "$(CAREERBREW_NOW=2026-10-19T09:35 cb day next)" "in a meeting" "quiet during meetings"
out="$(CAREERBREW_NOW=2026-10-19T10:00 cb day next)"
expect "$out" "Work on t001" "best task that fits the free slot"
expect "$out" "why: closes promotion gap design" "reason given"
cb work done t001 0.5 >/dev/null
expect "$(CAREERBREW_NOW=2026-10-19T12:40 cb day next)" "t002" "a 20-minute slot gets the short task"
expect "$(CAREERBREW_NOW=2026-10-19T19:00 cb day next)" "Work hours are over" "after hours"
# A fresh data root keeps the preceding tasks and calendar out of this fallback.
export CAREERBREW_HOME="$SCRATCH/next-step"
export CAREERBREW_NOW="2026-10-19T17:16:00"
cb config set work_end 17:30 >/dev/null
cb work feed add "$FIX/feed/next-step-ask.jsonl" >/dev/null
out="$(cb day next)"
expect "$out" "Read and respond to this ask (5 min)." "an open ask fits the final 14 minutes"
refute "$out" "https://example.com/next-step/rollout" "day next does not print the source URL"
echo "ok US-24 day next"
