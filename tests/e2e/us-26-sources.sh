#!/usr/bin/env bash
# US-26: every strategy rule, metric, interview tip, and company fact cites a public source.
source "$(dirname "$0")/lib.sh"
out="$(cd "$ROOT" && .venv/bin/python scripts/check-sources.py 2>&1 || uv run --quiet python scripts/check-sources.py)"
expect "$out" "0 problem(s)" "the knowledge base is fully sourced"
expect "$out" "5 companies" "the base set: five sample companies"
# error case: an unsourced company fact fails the gate (run on a scratch copy)
cp -R "$ROOT/knowledge" "$ROOT/config" "$ROOT/scripts" "$SCRATCH/"
printf 'id: bad\nname: Bad Co\nloop:\n  - {step: behavioral, notes: unsourced claim}\nsources: []\n' > "$SCRATCH/knowledge/companies/bad.yaml"
out="$(cd "$SCRATCH" && "$ROOT/.venv/bin/python" scripts/check-sources.py 2>&1 || true)"
expect "$out" "company bad loop 1: no sources" "unsourced loop step fails"
cb init >/dev/null
expect "$(cb interview Google --show)" "Sources" "guides print their sources"
echo "ok US-26 sources"
