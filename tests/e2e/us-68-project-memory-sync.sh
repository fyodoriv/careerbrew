#!/usr/bin/env bash
# US-68: a project and its memory file stay in sync both ways. An edit to the memory file updates the
# project; Slack, Jira, and PagerDuty items that match the project become dated notes there, once.
# Personal events never go there, and careerbrew never reads its own writes back.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-21T16:00" CAREERBREW_NOTIFY=off CAREERBREW_HTTP_FIXTURES="$FIX/http/feed" CB_E2E_TOKEN=test-token-not-real
cb init >/dev/null
mem="$SCRATCH/agent-memory/billing-research-log.md" # an agent's notes file, outside the data root
mkdir -p "$(dirname "$mem")" && printf '# billing research log\n\nNotes from the agent.\n' > "$mem"
cb work project add search >/dev/null
cb work project add billing --memory "$mem" >/dev/null
cb work project source billing tracker PROJ-1 >/dev/null
cb work priority set billing --match '*billing*' >/dev/null
for k in slack pagerduty outlook; do cb connect "$k" --ref env:CB_E2E_TOKEN --no-verify >/dev/null; done
cb connect jira --ref env:CB_E2E_TOKEN --base-url https://jira.example.com --email me@example.com --no-verify >/dev/null
data_write work/feed-rules.yaml <<< 'rules: [{name: family, when: {text: [dentist]}, personal: true}]'
printf '{"id": "standup-1", "at": "2026-10-21T13:30:00Z", "title": "PROJ-7 standup moved", "text": "after the dentist"}\n' | cb work feed add - >/dev/null

# 1. The agent writes what it learned into the memory file; the watcher's next cycle updates the project.
cat >> "$mem" <<'MD'

status: schema frozen, migration next week
trackers: PROJ-7
channels: #eng-billing
links: search
- design doc: https://docs.google.com/document/d/doc-billing-9/edit
- repo: https://github.com/acme/billing
MD
cb day watch once >/dev/null
show="$(cb work project show billing)"
expect "$show" "tracker  PROJ-7" "a trackers line adds a tracker source"
expect "$show" "chat     #eng-billing" "a channels line adds a chat source"
expect "$show" "doc      doc-billing-9" "a Google Docs link adds a doc source"
expect "$show" "repo     acme/billing" "a GitHub link adds a repo source"
expect "$show" "links: search" "a links line links an existing project"

# 2. Error case: with business reading off (no terminal, no plugin policy), no work content goes into the agent's file.
refute "$(cat "$mem")" "work tools:" "error case: no notes while business reading is off"
expect "$(cb work project sync)" "business reading is off" "error case: the sync says why it wrote no notes"

# 3. The owner allows agents to read business data; the next cycle writes the matching items as dated notes.
mkdir -p "$SCRATCH/p" && printf 'name: allow\nversion: 0.1.0\npolicy: {agent_may_read_business: true}\n' > "$SCRATCH/p/careerbrew-plugin.yaml"
cb plugin add "$SCRATCH/p" >/dev/null
cb day watch once >/dev/null
notes="$(cat "$mem")"
expect "$notes" "work tools: 3 new item(s)" "one dated section per write"
expect "$notes" "- 2026-10-21 slack: Mention from sam in #eng-billing <https://example.slack.com/archives/C0BILL/p1792594800000300>" "a Slack mention in the project's channel, with its link"
expect "$notes" "- 2026-10-20 jira: PROJ-7: Freeze the billing schema <https://jira.example.com/browse/PROJ-7>" "a Jira issue by its tracker key"
expect "$notes" "- 2026-10-21 pagerduty: Page: Billing queue backlog <https://example.pagerduty.com/incidents/Q2INC>" "a page by the project's priority pattern"
refute "$notes" "standup moved" "a personal event never goes into memory"
refute "$notes" "Checkout latency" "an item of no project stays out"
refute "$notes" "OPS-9" "another project's issue stays out"
refute "$notes" "Old billing cleanup" "an item older than a week stays out"

# 4. Idempotent, and no loop: collecting again adds nothing, and careerbrew's own write is not read back.
cb day watch once >/dev/null
cb work collect >/dev/null
out="$(cb work project sync)"
expect "$out" "memory in: 0 changed file(s)" "careerbrew's own writes are not read back"
expect "$out" "memory out: 0 new note(s)" "re-collecting adds nothing"
[ "$(grep -c 'Mention from sam' "$mem")" = 1 ] || fail "a note was written twice"

# 5. A later edit updates the status; marking an event personal removes its note.
printf 'status: blocked on the infra review\n' >> "$mem"
cb day watch once >/dev/null
expect "$(cb work project show billing)" "blocked on the infra review" "a new status line updates the project"
expect "$(cb work feed mark slack:C0BILL:1792594800.000300 personal)" "1 project memory note(s) removed" "marking an event personal removes its note"
refute "$(cat "$mem")" "Mention from sam" "the personal event's note is gone"
expect "$(cat "$mem")" "Freeze the billing schema" "other notes stay"

# 6. A child project with no memory file of its own: its notes go into its parent's file, headed with its name, once.
cb work project add checkout --parent billing >/dev/null
cb work priority set checkout --match '*checkout*' >/dev/null
cb day watch once >/dev/null
notes="$(cat "$mem")"
expect "$notes" "work tools: 2 new item(s) (project checkout)" "a child's notes are headed with its name in the parent's file"
expect "$notes" "- 2026-10-21 pagerduty: Page: Checkout latency over SLO <https://example.pagerduty.com/incidents/Q1INC>" "a page that matches the child's priority pattern"
expect "$notes" "- 2026-10-21 pagerduty: Acknowledged page: Checkout latency over SLO <https://example.pagerduty.com/incidents/Q1INC>" "and my acknowledgement of it"
cb day watch once >/dev/null
cb work collect >/dev/null
[ "$(grep -c 'Checkout latency over SLO' "$mem")" = 2 ] || fail "a child's note was written twice"
expect "$(cb work project sync)" "memory in: 0 changed file(s)" "careerbrew's write for the child is not read back"

event_seen work.memory.in
event_seen work.memory.out
event_seen work.memory.forget
grep -qE "eng-billing|PROJ-7|schema" "$CAREERBREW_HOME/logs/events.jsonl" && fail "event log holds content"
echo "ok US-68 project memory sync"
