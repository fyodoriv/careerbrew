#!/usr/bin/env bash
# US-31: careerbrew pivots when the promotion ladder, a plugin, or a company's process changes.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
export CAREERBREW_NOTIFY=off
cp -R "$FIX/plugin-acme" "$SCRATCH/acme"
cb plugin add "$SCRATCH/acme" >/dev/null
expect "$(cb changes)" "first run only records a baseline" "baseline"
expect "$(cb changes)" "no changes" "nothing changed"
python3 - "$SCRATCH/acme/ladders/acme-eng.yaml" <<'PY'
import sys; p=sys.argv[1]; s=open(p).read(); open(p,"w").write(s.replace("expect: {e4: 1, e5: 3}", "expect: {e4: 2, e5: 5}"))
PY
cb plugin add "$SCRATCH/acme" >/dev/null   # company updated its promotion criteria
out="$(cb changes)"
expect "$out" "changed: ladder acme-eng" "ladder change detected"
expect "$out" "promotion case re-assessed" "pivot: promo re-assessed and plan rebuilt"
[ -s "$CAREERBREW_HOME/changes/events.jsonl" ] || fail "pivot event recorded"
echo "ok US-31 changes"
