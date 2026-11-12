#!/usr/bin/env bash
# US-07: collect evidence of my work from git, deduplicated and cached.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
seed_repo "$SCRATCH/svc"
echo "sources: [{kind: git, path: '$SCRATCH/svc', authors: [me@example.com]}]" > "$CAREERBREW_HOME/work/sources.yaml"
expect "$(cb work collect)" "evidence: 5 new, 5 total, 0 source error(s)" "first collect"
expect "$(cb work collect)" "evidence: 0 new, 5 total" "second collect dedups"
ls "$CAREERBREW_HOME/cache/"*.json >/dev/null || fail "git read is cached by HEAD"
expect "$(cb work note 'Mentored a new hire on the payments service' --tags leadership)" "tags=leadership" "manual note"
echo "sources: [{kind: git, path: '$SCRATCH/missing'}]" > "$CAREERBREW_HOME/work/sources.yaml"
out="$(cb work collect || true)"
expect "$out" "1 source error(s)" "error case: broken source is counted"
echo "ok US-07 evidence"
