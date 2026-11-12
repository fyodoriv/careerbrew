#!/usr/bin/env bash
# US-02: scan public ATS boards and filter by my criteria.
source "$(dirname "$0")/lib.sh"
seed_jobs
out="$(cb jobs scan)"
expect "$out" "scanned 8 posting(s) from 3 source(s); 5 pass your criteria" "scan + filter counts"
expect "$out" "title-excluded=2" "rejection reasons are shown"
event_seen jobs.source
# error case: an unknown board kind fails with a fix, not a traceback
echo "sources: [{kind: workday, board: x}]" > "$CAREERBREW_HOME/jobs/sources.yaml"
out="$(cb jobs scan 2>&1 || true)"
expect "$out" "error: " "bad source kind gives a clean error"
refute "$out" "Traceback" "no traceback"
# log an application stage; an unknown tier is refused with the choices
seed_jobs
cb jobs scan >/dev/null
expect "$(cb jobs log Northwind dream applied)" "logged dream applied" "an application stage is logged"
expect "$(data_cat jobs/applications.jsonl)" "Northwind" "the stage is stored for the season strategy"
expect "$(cb jobs log Northwind someday applied 2>&1 || true)" "invalid choice: 'someday' (choose from practice, target, dream)" "error case: an unknown tier names the choices"
echo "ok US-02 jobs scan + filter"
