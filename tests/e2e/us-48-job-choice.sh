#!/usr/bin/env bash
# US-48: careerbrew suggests jobs; only the owner chooses them and their tier.
source "$(dirname "$0")/lib.sh"
seed_jobs
cb jobs scan >/dev/null
out="$(cb jobs strategy --json)"
python3 -c 'import json,sys; d=json.loads(sys.argv[1]); assert not any(d["tiers"].values()) and any(d["suggested"].values())' "$out" || fail "nothing is chosen until the owner chooses"
expect "$(cb jobs choose gh:Northwind:101 target)" "chosen: gh:Northwind:101 as target" "the owner chooses"
expect "$(cat "$CAREERBREW_HOME/out/strategy.md" 2>/dev/null; cb jobs strategy >/dev/null; cat "$CAREERBREW_HOME/out/strategy.md")" "| Northwind |" "the chosen job is in the plan"
expect "$(cb jobs unchoose gh:Northwind:101)" "0 job(s) chosen" "the owner can take it back"
event_seen jobs.choose
echo "ok US-48 job choice"
