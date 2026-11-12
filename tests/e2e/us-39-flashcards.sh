#!/usr/bin/env bash
# US-39: flashcards: queue (due first, interleaved, daily cap), grade, leech suspension, stats, and the web Practice view.
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
cb init >/dev/null
cb cards add "What does a load balancer do?" "Spreads requests across servers." --tags "System design" >/dev/null
cb cards add "What is a heap?" "A tree where each parent beats its children." --tags "Computer Science" >/dev/null
out="$(cb cards next --json)"
expect "$out" '"mode": "new"' "new cards queued"
id="$(echo "$out" | "$ROOT/.venv/bin/python" -c 'import json,sys;print(json.load(sys.stdin)[0]["id"])')"
expect "$(cb cards grade "$id" good)" "next review" "grade schedules the next review"
expect "$(cb cards stats)" "reviewed today 1" "stats count the review"
expect "$(cb cards grade "$id" meh 2>&1 || true)" "invalid choice" "error case: unknown grade refused"
grep -q '"cards.review"' "$CAREERBREW_HOME/logs/"*.jsonl || fail "log event cards.review"
cd "$ROOT/web"
# its own web seed: the story's CLI review above would make "Reviewed today" 3, not 2
out="$(env -u CAREERBREW_HOME npx playwright test --reporter=line -g "Practice: recall" 2>&1)" || { echo "$out"; fail "web Practice view"; }
expect "$out" "1 passed" "web Practice view: recall, reveal, grade"
# the interactive review: recall, reveal, grade from the keyboard; a suspended leech goes back in the queue
cd "$ROOT"
out="$(printf '\n3\n' | cb cards review --count 1)"
expect "$out" "A: " "the answer shows after recall"
expect "$out" "next review" "the keyboard grade schedules the card"
printf '{"suspended": ["%s"]}' "$id" | data_write tutor/card-meta.json  # as a leech: suspended after 8 lapses
expect "$(cb cards stats)" "leeches 1" "the card counts as a suspended leech"
expect "$(cb cards unsuspend "$id")" "is back in the queue" "a leech is put back"
expect "$(cb cards stats)" "leeches 0" "it is no longer suspended"
expect "$(cb cards unsuspend no-such-card 2>&1 || true)" "unknown card" "error case: an unknown card is named"
echo "ok US-39 flashcards"
