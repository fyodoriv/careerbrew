#!/usr/bin/env bash
# US-41: the most efficient work action for every horizon, with why.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
# Error case first: no tasks yet, so the plan horizons say how to create data.
out="$(cb work focus)"
expect "$out" "Today: nothing yet. Add a task" "error case: an empty horizon names the action that creates its data"
cb work task "Write the queue design doc" --impact 3 --hours 1.5 --competencies design >/dev/null
cb work task "Plan the platform roadmap" --impact 4 --hours 12 --competencies impact >/dev/null
cb work note "feat: shipped order export" --tags delivery >/dev/null
out="$(cb work focus)"
for h in "Right now" "Today" "This week" "This month" "This quarter" "This year" "This company"; do
  expect "$out" "$h:" "horizon $h is listed"
done
expect "$out" "why:" "each top action has a why"
event_seen work.focus
echo "ok US-41 focus dashboard"
