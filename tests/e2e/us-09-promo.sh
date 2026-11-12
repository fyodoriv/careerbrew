#!/usr/bin/env bash
# US-09: the best promotion doc my evidence supports, plus what is missing by priority.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
seed_repo "$SCRATCH/svc"
echo "sources: [{kind: git, path: '$SCRATCH/svc', authors: [me@example.com]}]" > "$CAREERBREW_HOME/work/sources.yaml"
cb work collect >/dev/null
out="$(cb work promo --level senior)"
expect "$out" "overall coverage" "coverage reported"
doc="$CAREERBREW_HOME/out/promo-generic-swe-senior.md"
[ -f "$doc" ] || fail "promo doc written"
expect "$(cat "$doc")" "## Evidence by competency" "doc has evidence"
expect "$(cat "$doc")" "launch export" "doc cites real evidence"
expect "$(cat "$doc")" "## What is missing (highest priority first)" "doc has gap list"
[ -f "$CAREERBREW_HOME/out/promo-generic-swe-senior.html" ] || fail "coverage chart written"
gaps="$(cb work gaps --level senior)"
python3 - "$gaps" <<'PY' || fail "gaps sorted by priority"
import re, sys
p = [float(x) for x in re.findall(r"priority ([0-9.]+)", sys.argv[1])]
assert p and p == sorted(p, reverse=True), p
PY
out="$(cb work promo --level wizard || true)"
expect "$out" "error: " "error case: unknown level"
echo "ok US-09 promo"
