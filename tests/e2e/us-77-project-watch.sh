#!/usr/bin/env bash
# US-77: one list of the active projects and their sources, for an agent's routine and the Work view (issue #172).
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
export CAREERBREW_NOW="2026-10-01T09:00+00:00"
# Error cases first: an unknown project, a bad kind, and an empty update name their fix.
expect "$(cb work project archive billing 2>&1 || true)" "careerbrew work project add billing" "error case: unknown project names the fix"
cb work project add billing >/dev/null
cb work project add ledger >/dev/null
cb work project source billing tracker PROJ-1 >/dev/null
cb work project source billing chat '#billing-team' >/dev/null
cb work project check billing PROJ-1 >/dev/null
expect "$(cb work project update billing gossip 'x' 2>&1 || true)" "use one of company, project, team, other" "error case: a bad update kind"
expect "$(cb work project update billing team '  ' 2>&1 || true)" "needs one line" "error case: an empty update"
# The JSON list an agent's routine loops over.
out="$(cb work project list --active --json)"
expect "$out" '"name": "billing"' "billing is watched"
expect "$out" '"name": "ledger"' "ledger is watched"
expect "$out" '"last_checked_at": "2026-10-01T09:00:00+00:00"' "the last check is listed"
expect "$out" '"ref": "#billing-team"' "each source to watch is listed"
python3 -c 'import json,sys; d=json.load(sys.stdin); assert [p["name"] for p in d]==["billing","ledger"], d' <<<"$out" || fail "the list is valid JSON"
# An update is the latest change; without a terminal its text stays out of the list.
expect "$(cb work project update billing team 'a new tech lead joined')" "team update noted" "note an update"
out="$(cb work project list --active --json)"
expect "$out" '"kind": "team"' "the latest change's kind is listed"
refute "$out" "a new tech lead joined" "update text is business text: an agent without a terminal does not see it"
# Archiving takes a project off the list and out of the stale sources.
expect "$(cb work project archive ledger)" "ledger archived" "archive"
refute "$(cb work project list --active --json)" '"name": "ledger"' "an archived project leaves the active list"
refute "$(cb work project list --json)" '"name": "ledger"' "active is the default"
expect "$(cb work project list --all --json)" '"archived": true' "--all still has it, marked archived"
expect "$(cb work project list --all)" "archived" "the text list marks it archived"
refute "$(cb work project list)" "ledger" "the text list leaves it out by default"
cb work project source ledger doc doc-9 >/dev/null
refute "$(cb day next)" "ledger" "an archived project's sources are never stale"
expect "$(cb work project unarchive ledger)" "watched again" "watch it again"
expect "$(cb work project list --active --json)" '"name": "ledger"' "it is back on the list"
# Remove deletes the record only after --yes; the memory file stays (issue #173).
cb work project add done --memory notes/done.md >/dev/null
expect "$(cb work project remove done 2>&1; true)" "rerun with --yes" "error case: remove asks for --yes"
expect "$(cb work project list)" "done" "nothing removed without --yes"
expect "$(cb work project remove done --yes)" "done removed; its memory file stays" "remove with --yes"
refute "$(cb work project list --all)" "done" "the record is gone"
event_seen work.project
echo "ok US-77 project watch"
