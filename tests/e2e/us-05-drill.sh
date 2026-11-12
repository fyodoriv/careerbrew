#!/usr/bin/env bash
# US-05: drill with spaced repetition; graded by a binary rubric with feedback.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
out="$(cb drill next)"
expect "$out" "[learn]" "new item is taught first"
expect "$out" "Lesson:" "lesson shown before the first attempt"
expect "$out" "Rubric (binary):" "rubric shown"
item="$(cb drill next --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')"
crit="$(cb drill next --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["rubric"])')"
case "$crit" in coding) met="clarify,approach";; system-design) met="requirements";; *) met="star";; esac
out="$(cb drill answer "$item" --met "$met")"
expect "$out" "-> again" "low score maps to FSRS Again"
expect "$out" "Practice:" "missed criteria are named"
out="$(cb drill answer "$item" --met bogus 2>&1 || true)"
expect "$out" "unknown criteria" "error case: bad criterion"
event_seen tutor.attempt
# the interactive drill loop: question, Enter, a y/N per criterion
out="$(printf 'y\n%.0s' $(seq 40) | cb drill session --count 1)"  # not `yes | head`: SIGPIPE fails the pipe under pipefail
expect "$out" "Question:" "the session asks a question"
expect "$out" "session done: 1 item(s)" "one item is graded from the keyboard"
out="$(printf '\n' | cb drill session --count 1 2>&1)"
expect "$out" "session done: 0 item(s)" "error case: input that ends mid-question stops cleanly"
refute "$out" "Traceback" "no traceback"
echo "ok US-05 drill"
