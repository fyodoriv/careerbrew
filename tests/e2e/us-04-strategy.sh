#!/usr/bin/env bash
# US-04: a season strategy: learn -> practice -> target -> dream, one loop per company.
source "$(dirname "$0")/lib.sh"
seed_jobs
cb jobs scan >/dev/null
cb jobs rate gh:Northwind:101 5 >/dev/null
out="$(cb jobs strategy)"
expect "$out" "choose the jobs you want" "careerbrew asks the owner to choose; it never chooses"
expect "$(cb jobs choose gh:Northwind:101 dream)" "chosen: gh:Northwind:101 as dream" "the owner chooses a job"
expect "$(cb jobs choose gh:Nope:1 dream || true)" "unknown posting" "an unknown posting is refused"
out="$(cb jobs strategy)"
expect "$out" "phase: learn" "starts in learn phase with no drills"
expect "$out" "NOW → Learn" "current step is marked"
expect "$out" "gaps: " "skill gaps listed"
plan="$CAREERBREW_HOME/out/strategy.md"
[ -f "$plan" ] || fail "strategy.md written"
expect "$(cat "$plan")" "## Dream companies" "plan has tiers"
expect "$(cat "$plan")" "| Northwind |" "rated-5 company is in the plan"
dups="$(cb jobs strategy --json | python3 -c 'import json,sys; t=json.load(sys.stdin)["tiers"]; c=[r["company"] for v in t.values() for r in v]; print(len(c)-len(set(c)))')"
[ "$dups" = 0 ] || fail "a company appears in two tiers"
expect "$(cb drill list)" "gap-" "gaps became drill items"
echo "ok US-04 strategy"
