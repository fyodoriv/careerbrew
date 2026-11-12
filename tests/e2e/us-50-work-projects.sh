#!/usr/bin/env bash
# US-50: track a work project's sources with checked_at, and see stale ones first.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
export CAREERBREW_NOW="2026-10-01T09:00+00:00"
# Error case first: an unknown project names its fix.
expect "$(cb work project show billing 2>&1 || true)" "careerbrew work project add billing" "error case: unknown project names the fix"
expect "$(cb work project add billing --memory notes/billing.md --stale-after 7d)" "stale after 7d" "add"
cb work project source billing tracker PROJ-1 >/dev/null
cb work project source billing doc doc-42 >/dev/null
expect "$(cb work project source billing doc doc-42 2>&1 || true)" "already a source" "error case: duplicate source refused"
# Check one source, then all of them.
expect "$(cb work project check billing PROJ-1 --status 'epic 3 of 5')" "checked 1 source(s)" "check one"
out="$(cb work project show billing)"
expect "$out" "STALE  memory" "never-checked memory is stale"
expect "$out" "ok     tracker  PROJ-1" "checked source is fresh"
expect "$(cb work project check billing)" "checked 3 source(s)" "check all"
# Stale flag after N days (frozen clock), shown first in checkup, day, and focus.
export CAREERBREW_NOW="2026-10-09T09:00+00:00"
expect "$(cb work project show billing)" "STALE  tracker  PROJ-1  8d ago" "stale after 7 days"
expect "$(cb work focus | head -1)" "stale: billing" "focus lists stale sources first"
expect "$(cb day next | head -1)" "stale: billing" "day lists stale sources first"
# List order: the project with stale sources comes first.
cb work project add fresh >/dev/null
cb work project source fresh repo svc-x >/dev/null
cb work project check fresh >/dev/null
expect "$(cb work project list | head -1)" "billing" "stale project listed first"
# A link that opens a sign-in page is a reference: never stale; any source can be marked one (issue #70).
cb work project source billing url https://outlook.office.com/mail/inbox/id/AAQkSYNTH >/dev/null
expect "$(cb work project show billing)" "ref    url      https://outlook.office.com" "sign-in link is a reference"
refute "$(cb day next)" "outlook.office.com" "a reference is never a stale source"
expect "$(cb work project reference billing nope 2>&1 || true)" "has no source nope" "error case: unknown ref names the fix"
expect "$(cb work project reference billing PROJ-1)" "never goes stale" "mark a source reference-only"
expect "$(cb work project show billing)" "ref    tracker  PROJ-1" "marked source is a reference"
expect "$(cb work project reference billing PROJ-1 --off)" "tracked again" "track it again"
expect "$(cb work project show billing)" "STALE  tracker  PROJ-1" "tracked source is stale again"
# A url on a host outside the plugin allowed_hosts is a reference you read, not a stale source (issue #101).
mkdir -p "$SCRATCH/p"
printf 'name: acme\nversion: 0.1.0\nagent_reads: [slack, gdocs]\npolicy: {allowed_hosts: [jira.example.com]}\n' > "$SCRATCH/p/careerbrew-plugin.yaml"
cb plugin add "$SCRATCH/p" >/dev/null
cb work project source billing url https://portal.sso.example.com/app/ >/dev/null
out="$(cb work project show billing)"
expect "$out" "ref    url      https://portal.sso.example.com/app/" "url outside allowed_hosts is a reference"
expect "$out" "not in the plugin policy allowed_hosts" "show names the host policy"
refute "$(cb day next)" "portal.sso.example.com" "a url careerbrew may not read is never a stale source"
# With agent_reads: [url], the agent reads it: tracked and stale, labeled for the agent.
printf 'name: acme\nversion: 0.1.0\nagent_reads: [slack, gdocs, url]\npolicy: {allowed_hosts: [jira.example.com]}\n' > "$SCRATCH/p/careerbrew-plugin.yaml"
cb plugin refresh >/dev/null
expect "$(cb work project show billing)" "STALE  url      https://portal.sso.example.com/app/  never checked  read by agent" "agent_reads url hands it to the agent"
event_seen work.project
# link a task as the project's next step
tid="$(cb work task "Close the billing epic" --hours 1 | sed -nE 's/^added (t[0-9]+).*/\1/p' | head -1)"
[ -n "$tid" ] || fail "a task id to link"
expect "$(cb work project next billing "$tid")" "next step $tid linked to billing" "a task becomes the next step"
echo "ok US-50 work projects"
