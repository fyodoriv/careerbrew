#!/usr/bin/env bash
# US-45: teacher mode gives one step at a time, approvals wait for the owner, and feedback becomes proposals.
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
"$ROOT/.venv/bin/careerbrew" init >/dev/null
expect "$(cb teach next)" "next: Set a goal first" "with no goal, the teacher asks for one instead of a lesson (#54)"
cb goals set "Practice" >/dev/null
expect "$(cb teach next)" "next: Lesson: arrays and hashing" "first step teaches the first pattern"
out="$(cb approvals request "Rebase the feature branch" --detail "three commits behind main" --by claude)"
expect "$out" "waiting for the owner" "an agent can only request"
id="$(cb approvals list | awk '/Rebase/{print $1}')"
expect "$(cb teach next)" "Approve or reject: Rebase the feature branch" "a waiting approval comes first"
expect "$(CAREERBREW_TEST_OWNER= cb approvals decide "$id" approved </dev/null || true)" "only you set goals" "an agent (no terminal) cannot decide"
if cb approvals show "$id" >/dev/null; then fail "an undecided request reads as approved"; fi
CAREERBREW_TEST_OWNER=1 cb approvals decide "$id" approved --note ok >/dev/null
cb approvals show "$id" >/dev/null || fail "the owner's approval is readable by the agent"
cb teach feedback "hints should come sooner" --step exercise >/dev/null
expect "$(cb teach improvements)" "[written] You wrote: hints should come sooner" "written feedback becomes a proposal"
event_seen teach.feedback
cd "$ROOT/web"
# a fresh seeded data root: this script's goal must not leak into the web server (it starts with no goal)
out="$(env -u CAREERBREW_HOME npx playwright test --reporter=line -g "Teach me: confirm a lesson|Teach me: runs a whole flashcard session" 2>&1)" || { echo "$out"; fail "web teacher flow"; }
expect "$out" "2 passed" "web teacher flow and a whole flashcard session pass"
echo "ok US-45 teacher"
