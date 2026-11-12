#!/usr/bin/env bash
# US-18: an interactive preparation check for a company's loop, with a score, a trend, and gaps that name their fix.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
out="$(cb prep check Amazon)"
expect "$out" "prep for Amazon:" "check runs"
expect "$out" "→ " "gaps name their action"
ids="$(cb prep check Amazon --json | python3 -c 'import json,sys; g=json.load(sys.stdin)["gaps"]; print(",".join(x["id"] for x in g if not x.get("auto")))')"
[ -n "$ids" ] || fail "ask items listed in --json"
out="$(cb prep check Amazon --yes "$ids")"
expect "$out" "trend" "trend shown"
for i in 1 2 3 4 5 6; do cb prep story "Story $i" --competencies "leadership,conflict,impact,ownership" --result "result $i" >/dev/null; done
for item in beh-conflict-1 beh-impact-1; do cb drill answer "$item" --met star,ownership,result,tradeoff,learning >/dev/null; done
json="$(cb prep check Amazon --json)"
python3 - "$json" <<'PY' || fail "story bank and behavioral readiness are measured automatically"
import json, sys
d = json.loads(sys.argv[1])
ids = {g["id"] for g in d["gaps"]}
assert "behavioral:stories" not in ids, ids
assert "behavioral:readiness" not in ids, ids
assert d["score"] > 0.5, d["score"]
PY
echo "ok US-18 prep"
