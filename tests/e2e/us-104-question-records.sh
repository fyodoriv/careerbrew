#!/usr/bin/env bash
# US-104: work-day questions are records with a recheck; the batched ask drops closed items and shows ages (issue #261).
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-28T06:00+00:00"
cb init >/dev/null
mkdir -p "$SCRATCH/plugin-ask"
cat >"$SCRATCH/plugin-ask/careerbrew-plugin.yaml" <<'YAML'
name: ask-co
version: 1.0.0
work_steps:
  - {step: /synthetic-review, scope: review}
YAML
cb plugin add "$SCRATCH/plugin-ask" >/dev/null  # no --allow-exec: its rechecks never run
touch "$SCRATCH/draft-open"
expect "$(cb day question add /careerbrew 'Send the synthetic-canary-ask draft?' --option send --option hold --pick hold --recheck "test -f $SCRATCH/draft-open")" "saved for /careerbrew (command recheck)" "a step saves its question as a record"
cb day question add /careerbrew 'Merge the synthetic PR?' --recheck "test -f $SCRATCH/pr-gone-already" >/dev/null
export CAREERBREW_NOW="2026-10-28T08:30+00:00"
cb day question add /careerbrew 'Review the synthetic link?' --recheck https://example.com/synthetic/1 >/dev/null
cb day question add /synthetic-review 'Approve the synthetic step?' --recheck "touch $SCRATCH/plugin-ran" >/dev/null
expect "$(cb day question add /careerbrew 'x?' --option a --pick b 2>&1 || true)" "--pick must be one of the --option values" "error case: a pick that is not an option"
expect "$(cb day question add /not-a-step 'x?' 2>&1 || true)" "not in today's plan" "error case: a step that is not in the plan"
expect "$(cb day question add /careerbrew 'x?' --recheck "sh -c 'open" 2>&1 || true)" "unclosed quote" "error case: a recheck a shell could not split"

export CAREERBREW_NOW="2026-10-28T09:00+00:00"
list="$(cb day question list)"
expect "$list" "questions: 4 open" "list shows every open record"
expect "$list" "3 h old" "each record shows its age"
refute "$list" "synthetic-canary-ask" "privacy: question text is hidden from an agent without the policy"
expect "$(cb day run --json)" '"question_records": [' "the plan lists the open question records"
refute "$(cb day run --json)" "synthetic-canary-ask" "privacy: the plan JSON hides question text"

out="$(cb day question recheck)"
expect "$out" "questions: 3 to ask, 1 dropped as closed, 1 for an agent to check" "the batched ask drops the closed item"
expect "$out" "3 h old  dropped: recheck: closed (exit 1)" "the dropped item shows its age"
expect "$out" "[agent]" "a link is the agent's to check, never fetched"
expect "$out" "--allow-exec" "error case: a plugin's recheck does not run without --allow-exec"
[ ! -e "$SCRATCH/plugin-ran" ] || fail "a plugin recheck ran without --allow-exec"
event_seen workday.recheck

agent_id="$(cb day question recheck --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["agent"][0])')"
expect "$(cb day question close "$agent_id")" "question $agent_id closed" "an agent closes the link item it checked"
expect "$(cb day question close "$agent_id" 2>&1 || true)" "no open question" "error case: closing it twice"
rm "$SCRATCH/draft-open"
expect "$(cb day question recheck)" "questions: 1 to ask, 1 dropped as closed" "a draft that is gone drops on the next recheck"

expect "$(cb day update /careerbrew --status blocked --needs-you 'Pick a synthetic reviewer')" "is blocked" "a blocked row"
expect "$(cb day question list)" "questions: 2 open" "a blocked row's ask is a question record too"
cb day update /careerbrew --status done --needs-you '' >/dev/null
expect "$(cb day question list)" "questions: 1 open" "clearing the row's ask closes its record"

cb day render >/dev/null
expect "$(cb report latest --run work-day)" "Question from /synthetic-review" "the page lists each saved question"
export CAREERBREW_NOW="2026-10-29T09:00+00:00"
expect "$(cb day question list)" "24 h old" "an open question carries to the next morning"
expect "$(cb day handoff show)" "questions: 1 open from the work day" "the handoff points at the open questions"
echo "ok US-104 question records"
