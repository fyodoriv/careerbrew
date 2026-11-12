#!/usr/bin/env bash
# US-106: the work day ends by clock: wrap-up at work_end minus a lead time, the PM draft as the
# end-of-day output, and a handoff the next morning rechecks (issue #265).
# The fake notifier (tests/fixtures/notifier) stands in for terminal-notifier, and a fake gh answers PR states: nothing shows, nothing leaves.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOTIFY=on FAKE_NOTIFIER_LOG="$SCRATCH/notifier.log"
mkdir -p "$SCRATCH/bin" "$SCRATCH/none"
ln -s "$FIX/notifier/fake-notifier.sh" "$SCRATCH/bin/terminal-notifier"
cat >"$SCRATCH/bin/gh" <<'SH'
#!/bin/sh
# A fake gh: the shop PR merged overnight, every other PR is still open.
case "$*" in *shop*) echo MERGED ;; *) echo OPEN ;; esac
SH
chmod +x "$SCRATCH/bin/gh"
export PATH="$SCRATCH/bin:$PATH"
popups() { if [ -f "$FAKE_NOTIFIER_LOG" ]; then grep -c '^--- ' "$FAKE_NOTIFIER_LOG"; else echo 0; fi; }
CANARY="zq-endofday-canary-5"

export CAREERBREW_NOW="2026-10-29T09:00+00:00"
cb init >/dev/null
for kv in "work_end 17:30" "work_wrap_minutes 30" "notify_events true" "notify_backend terminal-notifier" "notify_quiet_hours none" "work_checkouts $SCRATCH/none"; do cb config set $kv >/dev/null; done
# This clock scenario spans 9126 minutes; source expiry is not under test.
cb config set feed_tool_sources 'mcp__chat__*=chat' >/dev/null
cb config set live_source_max_age_minutes 10080 >/dev/null
printf '{"messages": []}\n' > "$SCRATCH/quiet.json"
cb work feed add "$SCRATCH/quiet.json" --source chat >/dev/null
cb work feed cycle >/dev/null
expect "$(cb config set work_wrap_minutes 999 2>&1 || true)" "from 0 to 240" "error case: a lead time out of range is refused"
expect "$(cb day run --json)" '"wrap_up": {"at": "2026-10-29T17:00+00:00", "phase": "work"' "the plan names when wrap-up starts"

# The day's work: a PR a step links, one it handled, and two open questions.
cb day update /careerbrew --status done --link https://github.com/example/shop/pull/12 --handled pr:example/tools#7 >/dev/null
touch "$SCRATCH/ask-open" "$SCRATCH/ask-answered"
cb day question add /careerbrew "Merge the $CANARY draft?" --recheck "test -e $SCRATCH/ask-open" >/dev/null
cb day question add /careerbrew "Rename the flag?" --recheck "test -e $SCRATCH/ask-answered" >/dev/null

# 1. Before the lead time: still the work phase, no reminder.
export CAREERBREW_NOW="2026-10-29T16:59+00:00"
refute "$(cb day loop pass)" "wrap-up since" "a minute before wrap-up: no switch"
expect "$(popups)" "0" "no reminder before wrap-up"

# 2. At work_end minus 30 min: wrap-up, once, with one reminder; the loop keeps running.
export CAREERBREW_NOW="2026-10-29T17:00+00:00"
out="$(cb day loop pass)"
expect "$out" "wrap-up since 17:00 (reminder: sent)" "wrap-up starts at the configured time and reminds"
expect "$out" "the loops keep running" "wrap-up keeps the cycles running"
expect "$(grep -c 'careerbrew day end' "$FAKE_NOTIFIER_LOG")" "1" "the reminder names the next step"
refute "$(cat "$FAKE_NOTIFIER_LOG")" "$CANARY" "privacy: the pop-up holds no business text"
export CAREERBREW_NOW="2026-10-29T17:15+00:00"
out="$(cb day loop pass)"
expect "$out" "wrap-up since 17:00:" "the next pass is still in wrap-up"
expect "$(popups)" "1" "one reminder a day"
expect "$(cb day handoff show --today --json)" '"section": "questions"' "wrap-up writes tomorrow's handoff"

# 3. End of day: the PM draft is the output, no second report; the loops stop.
export CAREERBREW_NOW="2026-10-29T17:40+00:00"
before="$(ls "$CAREERBREW_HOME/day")"
out="$(cb day end)"
expect "$out" "PM draft (generated" "end of day reads careerbrew's PM draft"
expect "$out" "careerbrew open --view dashboard" "end of day links the dashboard"
expect "$out" "handoff for tomorrow: 2 open PR(s), 2 open question(s)" "open PRs and questions go into the handoff"
expect "$out" "stopped watch day-loop" "end of day stops careerbrew's loop"
expect "$out" "end of day: PM draft ready (not sent)" "one summary line"
refute "$out" "$CANARY" "privacy: no question text for an agent"
added="$(comm -13 <(printf '%s\n' "$before") <(ls "$CAREERBREW_HOME/day") | tr '\n' ' ')"
refute "$added" "report" "end of day writes no report file of its own: $added"
expect "$(cb day loop status)" "0 loop(s) running" "no loop outlives the day"
event_seen workday.end

# 4. The next morning: each handoff item with its recheck result; the closed ones leave.
rm "$SCRATCH/ask-answered"
export CAREERBREW_NOW="2026-10-30T09:00+00:00"
out="$(cb day start)"
expect "$out" "handoff from 2026-10-29: 4 item(s) rechecked: 2 still open, 2 closed (dropped)" "the morning shows each item's recheck result"
refute "$out" "$CANARY" "privacy: the morning lines hold ids and states"
left="$(cb day handoff show --json)"
refute "$left" "example/shop" "the merged PR left the handoff"
expect "$left" '"section": "prs"' "the open PR stays"
expect "$(cb day question list)" "questions: 1 open" "the answered ask is closed in the question store"
event_seen workday.morning

# 5. By clock: a pass at work_end ends a day nobody ended.
export CAREERBREW_NOW="2026-10-30T17:30+00:00"
expect "$(cb day loop pass)" "end of day by clock (17:30)" "the first pass at work_end ends the day"
expect "$(cb day loop status)" "0 loop(s) running" "the loop ended with the day"
# Moving and holding apply to this day only; a late turn catches up once.
export CAREERBREW_NOW="2026-11-03T09:00+00:00"
expect "$(cb day wrap --at 18:15)" "wrap-up: 18:15; end 18:45" "move the whole wrap-up window"
expect "$(cb day wrap --hold)" "wrap-up: held" "hold automatic wrap-up and clock ending"
export CAREERBREW_NOW="2026-11-03T17:40+00:00"
cb day loop pass >/dev/null
expect "$(cb day run --json)" '"phase": "held"' "a hold survives the original end time"
expect "$(cb day check)" "automatic day end is paused" "day check names the active hold"
export CAREERBREW_NOW="2026-11-03T17:50+00:00"
expect "$(cb day loop catch-up --json)" '"status": "ran"' "the next turn runs a recorded pass more than five minutes late"
expect "$(cb day loop catch-up --json)" '"status": "current"' "the same due event is not repeated"
expect "$(cb day wrap --at 18:15)" "wrap-up: 18:15; end 18:45" "moving resumes a held wrap-up"
cb day wrap --hold >/dev/null
expect "$(cb day end)" "end of day: PM draft ready" "an explicit owner end overrides the hold"

# A project session finishes its current step and writes a fresh wrap report.
export CAREERBREW_NOW="2026-11-04T09:00+00:00"
cb work project add wrap-demo >/dev/null
cb work session open wrap-demo --id project-agent >/dev/null
cb work session handoff project-agent "Earlier checkpoint" >/dev/null
cb day loop pass >/dev/null
export CAREERBREW_NOW="2026-11-04T17:06+00:00"
expect "$(cb day loop catch-up --json)" project-agent "late wrap-up records a graceful request for the project"
set +e
out="$(cb work session end project-agent --json)"
session_rc=$?
set -e
[ "$session_rc" = 1 ] || fail "an earlier checkpoint must not archive a wrapping session"
expect "$out" '"missing": ["project-agent"]' "the session stays open until its new report"
cb work session handoff project-agent "Current step saved; drafts updated; next task waits for tomorrow" >/dev/null
expect "$(cb work session end project-agent --json)" '"ended": ["project-agent"]' "a fresh report permits session archival"
echo "ok US-106 end of day by clock"
