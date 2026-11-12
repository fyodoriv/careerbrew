#!/usr/bin/env bash
# US-88: grade a PM update against the next ladder level; tagged lines count as promo evidence (issue #216).
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-21T10:00+00:00" CAREERBREW_TEST_OWNER=1
cb init >/dev/null
expect "$(cb ladder levels --level senior --json)" '"looks_like"' "the next level's dimensions as JSON"
expect "$(cb ladder levels --ladder nope 2>&1 || true)" "breaks US-88" "error case: an unknown ladder"
cb work note "Shipped the synthetic retry" >/dev/null
out="$(cb day pm-grade --record)"
expect "$out" "never part of the sent update" "coverage is for the owner only"
expect "$out" "recorded" "tagged lines kept as evidence"
expect "$(cb work promo)" "overall coverage" "promo still reads the evidence"
echo "ok US-88 ladder grade"
