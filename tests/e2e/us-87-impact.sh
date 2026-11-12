#!/usr/bin/env bash
# US-87: a task carries its expected and measured impact; the daily update and promo evidence read it (issue #214).
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-21T10:00+00:00" CAREERBREW_TEST_OWNER=1
cb init >/dev/null
tid="$(cb work task "Fix the synthetic retry" | sed -nE 's/^added (t[0-9]+).*/\1/p' | head -1)"
[ -n "$tid" ] || fail "a task to measure"
expect "$(cb work impact "$tid" --expect "fewer failed exports" --who support --metric "failed exports per day")" "expected impact set (hypothesis, metric, who); measured not yet" "expected impact at the start"
expect "$(cb work impact "$tid" --result "failed exports 40 to 3 per day" --evidence https://example.com/pr/1)" "measured yes, 1 evidence link(s)" "measured result at delivery"
# The PM update's line gets an owner-only "why it matters" note (task pm-why-it-matters); it is never in the sent text.
expect "$(cb day send 2>&1 || true)" "why it matters: 1 note(s) for the owner" "an agent sees that a note exists, not its text"
sent="$(cb day send --via email)"
expect "$sent" "impact%3A%20failed%20exports" "the sent text names the result"
refute "$sent" "support" "the note (who benefits) is not in the sent text"
expect "$(cb work impact "$tid" 2>&1 || true)" "breaks US-87" "error case: nothing to record"
expect "$(cb work impact t999 --result x 2>&1 || true)" "unknown task t999" "error case: an unknown task"
echo "ok US-87 impact"
