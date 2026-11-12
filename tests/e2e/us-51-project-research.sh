#!/usr/bin/env bash
# US-51: a project absorbs links, files, tasks, and channels, and checkup researches its stale sources.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_HTTP_FIXTURES="$FIX/http" CB_T="synthetic" CAREERBREW_NOW="2026-10-07T09:00+00:00"
cb init >/dev/null
for k in github gdocs slack; do cb connect "$k" --ref env:CB_T --user me --no-verify >/dev/null; done
cb connect jira --ref env:CB_T --base-url https://jira.example.com --email me@example.com --no-verify >/dev/null
cb work project add billing --memory work/billing-notes.md >/dev/null
cb work project source billing tracker PROJ-1 >/dev/null
# Error case: with business reading off (no terminal, no policy), absorb keeps the ref and the date only.
out="$(cb work project absorb billing https://github.com/acme/billing/pull/41)"
expect "$out" "ref and date only" "error case: disallowed read stores the ref only"
# The owner allows agents to read business data through a plugin policy.
mkdir -p "$SCRATCH/p" && printf 'name: allow\nversion: 0.1.0\npolicy: {agent_may_read_business: true}\n' > "$SCRATCH/p/careerbrew-plugin.yaml"
cb plugin add "$SCRATCH/p" >/dev/null
# A PR stays one URL and writes only its own title/state, never repository-wide notes.
expect "$(cb work project absorb billing https://github.com/acme/billing/pull/41)" "absorbed url" "PR is an item source"
# Explicit raw source links normalize before the duplicate comparison and connector read.
cb work project source billing tracker https://jira.example.com/browse/PROJ-7 >/dev/null
# Absorb a link with the project matched automatically (same tracker key prefix).
expect "$(cb work project absorb https://tracker.example.com/browse/PROJ-7)" "into billing" "auto-match by tracker key"
expect "$(cb work project absorb billing https://docs.google.com/document/d/doc-prd-1/edit)" "note(s) written to the project memory" "absorb a doc link"
printf '# Kickoff\nDecision: freeze the schema on 2026-10-20\nNothing else\n' > "$SCRATCH/kickoff-billing.md"
expect "$(cb work project absorb billing "$SCRATCH/kickoff-billing.md")" "absorbed doc" "absorb a file"
expect "$(cb work project absorb billing '#billing-eng')" "absorbed chat #billing-eng" "absorb a channel"
cb work task "Backfill old invoices" --hours 2 >/dev/null
expect "$(cb work project absorb billing t001)" "absorbed task t001" "absorb a task id"
notes="$("$ROOT/.venv/bin/python" -c 'from careerbrew import paths, store; print(store.read_text(paths.data_root() / "work" / "billing-notes.md"))')"
expect "$notes" "PR #41 merged: Ship billing ledger" "one PR's title and state"
expect "$notes" "freeze the schema" "file key lines in memory"
expect "$notes" "cut over on 2026-11-15" "doc comments in memory"
# Checkup researches stale sources; an unreadable one stays stale with its reason.
cb work project source billing url https://unknown-host.example.com/x >/dev/null
export CAREERBREW_NOW="2026-10-20T09:00+00:00"
cb checkup >/dev/null 2>&1 || true
expect "$(cb report latest)" "refreshed" "checkup refreshes stale sources"
out="$(cb work project show billing)"
expect "$out" "why stale:" "unreadable source stays stale with its reason"
expect "$out" "proposed: tracker PROJ-101" "child issue proposed"
event_seen work.project
echo "ok US-51 project research"
