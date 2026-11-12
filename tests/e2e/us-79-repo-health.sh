#!/usr/bin/env bash
# US-79: the repo health export is allowlisted and content-free, so an agent may publish it as the shared Dashboard.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-25T12:00"
cb init >/dev/null

# A synthetic checkout: the gate defines three steps; its events also name a step it does not define.
repo="$SCRATCH/repo"; mkdir -p "$repo/scripts" "$repo/.metrics"
printf 'step docs scripts/check-docs.sh\nstep unit uv run pytest\nstep e2e scripts/e2e.sh\n' > "$repo/scripts/check.sh"
today="2026-10-25"
{
  for n in docs unit e2e; do printf '{"ts":"%sT09:00:00Z","event":"check.step","name":"%s","seconds":2,"ok":true}\n' "$today" "$n"; done
  printf '{"ts":"%sT10:00:00Z","event":"check.step","name":"e2e","seconds":90,"ok":false}\n' "$today"
  printf '{"ts":"%sT10:00:00Z","event":"check.step","name":"%s","seconds":1,"ok":false}\n' "$today" "$(printf '%s' "$CANARY_EMPLOYER" | tr 'A-Z' 'a-z')"
} > "$repo/.metrics/events.jsonl"
printf '{"results": {"tests/e2e/%s.sh": "ok"}}\n' "$CANARY_PERSON" > "$repo/.metrics/e2e-results.json"
export CAREERBREW_REPO_DIR="$repo"

# 1. The text summary and the agents' JSON: counts and gate step names only.
out="$(cb report public)"
expect "$out" "1 gate run(s), 1/4 step run(s) failed, e2e 1/1, checkup none, 0 open PR(s)" "the summary line counts the gate"
expect "$out" "e2e            1/2 failed" "the worst step comes first"
json="$(cb report public --json)"
expect "$json" '"gate_runs": 1' "agents get the counts"
refute "$json" "$(printf '%s' "$CANARY_EMPLOYER" | tr 'A-Z' 'a-z')" "a step the gate does not define never leaves"
refute "$json" "$CANARY_PERSON" "story names never leave"

# 2. The Dashboard's row files.
cb report public --out "$SCRATCH/rows" >/dev/null
for f in summary steps daily; do expect "$(cat "$SCRATCH/rows/$f.json")" '"rows"' "$f.json holds rows"; done
event_seen report.public

# 3. A sensitive term refuses the whole export, and nothing is written.
echo "unit" > "$CAREERBREW_HOME/sensitive-terms.txt"
out="$(cb report public --out "$SCRATCH/rows2" 2>&1)" && fail "a sensitive term must refuse the export"
expect "$out" "public export refused" "the refusal says so"
[ ! -e "$SCRATCH/rows2" ] || fail "nothing is written after a refusal"

# 4. Each machine keeps its own Dashboard link, validated like any setting.
cb config set checkup_artifact_url https://claude.ai/artifact/AbC123xyz9 >/dev/null
expect "$(cb config)" "checkup_artifact_url" "the link is a setting"
cb config set checkup_artifact_url https://example.com/x >/dev/null 2>&1 && fail "only a claude.ai artifact link is accepted"
echo "ok US-79 repo health"
