#!/usr/bin/env bash
# US-113: /indie and /work improve each other. A side detects its peer's new lessons and source commits,
# trials one change at a time on its own metric, and adopts or declines the lesson by the number.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-11-19T10:00+00:00"
cb init >/dev/null

PEER="$SCRATCH/checkout"
export CAREERBREW_PEER_ROOT="$PEER"
g() { git -C "$PEER" -c user.name=t -c user.email=t@example.com -c commit.gpgsign=false "$@"; }
put() { # put <file> <message>: commit stdin as the file
  mkdir -p "$PEER/$(dirname "$1")"
  cat >"$PEER/$1"
  g add -f "$1"
  g commit -q -m "$2"
}
mkdir -p "$PEER"
g init -q
printf '# Lessons\n' | put skills/careerbrew-indie/LESSONS.md "docs: indie lessons"
printf '# Lessons\n' | put skills/careerbrew-work-day/LESSONS.md "docs: work lessons"

# 1. Nothing to review yet; each side has its own voice.
out="$(cb peer status)"
expect "$out" "indie (the founder" "indie is the founder"
expect "$out" "work (the operator" "work is the operator"

# 2. /work learns something; /indie detects the lesson and the source change.
printf '# Lessons\n\n- [L-work-1] Prove each step with its verify command (metric step_fail_rate: 0.2 -> 0.1, KEEP)\n' | put skills/careerbrew-work-day/LESSONS.md "docs: a work lesson"
printf 'changed\n' | put commands/work.md "feat: a work change"
out="$(cb peer status --for indie --json)"
expect "$out" '"unreviewed_lessons": ["L-work-1"]' "indie sees the lesson it has not reviewed"
expect "$out" "--lesson L-work-1" "the next step is a trial of that lesson"
expect "$(cb peer status --for work --json)" '"unreviewed_lessons": []' "work has nothing to review yet"
event_seen peer.status

# 3. One trial at a time, on the side's own metric, and not before three days.
expect "$(cb peer experiment --side indie --start 'Prove each delivered task with its test command' --metric outcomes_per_week --lesson L-work-1)" "trial x-indie-1 started on outcomes_per_week" "indie trials the work lesson"
expect "$(cb peer experiment --side indie --start 'A second change at the same time' --metric outcomes_per_week 2>&1 || true)" "one change at a time" "error case: a second open trial"
expect "$(cb peer experiment --side indie --close 2>&1 || true)" "needs at least 3" "error case: closing a trial too early"
expect "$(cb peer experiment --side work --start 'A long enough change' --metric outcomes_per_week 2>&1 || true)" "breaks US-113" "error case: another side's metric"
expect "$(cb peer status --for indie)" "trial: x-indie-1" "the open trial shows in the status"
export CAREERBREW_NOW="2026-11-24T10:00+00:00"
expect "$(cb peer experiment --side indie --close)" "trial x-indie-1: NO-DATA" "no data is never a keep"
expect "$(cb peer experiment)" "trials: 1, 0 open, 0 kept" "the trial list"
event_seen peer.experiment

# 4. indie writes its verdict and moves its marker; then nothing is left, and work has indie's lesson to review.
printf '# Lessons\n\n- [L-work-1] declined: no number yet on the indie side\n- [L-indie-1] Try each new bet once before ranking it\nPeer source reviewed at: %s\n' "$(g rev-parse --short HEAD)" | put skills/careerbrew-indie/LESSONS.md "docs: indie reviews work"
out="$(cb peer status --for indie --json)"
expect "$out" '"unreviewed_lessons": []' "the reviewed lesson is gone from the list"
expect "$out" '"peer_commits": []' "the marker clears the reviewed commits"
expect "$out" '"declined": 1' "the verdict is counted"
expect "$(cb peer status --for work --json)" '"unreviewed_lessons": ["L-indie-1"]' "work now has the indie lesson to review"
echo "ok US-113"
