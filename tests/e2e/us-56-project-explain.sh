#!/usr/bin/env bash
# US-56: explain a tracked project in layers (base, short, deep), draft a PRD, and ship only when the checker passes.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
export CAREERBREW_NOW="2026-10-21T09:00+00:00"
# Error case first: an unknown project names its fix.
expect "$(cb work project explain billing 2>&1 || true)" "careerbrew work project add billing" "error case: unknown project names the fix"
cb work project add billing >/dev/null
cb work project source billing tracker PROJ-1 >/dev/null
out="$(cb work project explain billing)"
expect "$out" "explain-billing.md" "outline written"
expect "$(data_cat out/explain-billing.md)" "| S1 | primary | PROJ-1 | TODO |" "reading list comes from the project record"
# An unfilled outline fails the checker with exit 1.
if cb work project explain billing --check "$CAREERBREW_HOME/out/explain-billing.md" >"$SCRATCH/o" 2>&1; then fail "unfilled outline passed"; fi
expect "$(cat "$SCRATCH/o")" "problem(s)" "error case: checker names problems"
# A filled doc on a real project (careerbrew itself) passes.
expect "$(cb work project explain billing --check "$ROOT/skills/careerbrew-project-explainer/references/example-careerbrew.md")" "explain check: pass" "real example passes"
event_seen work.explain
# Deep mode (issue #223): the outline adds How it runs, and the plain example lacks it.
expect "$(cb work project explain billing --deep)" "--deep" "deep outline written"
expect "$(data_cat out/explain-billing.md)" "## How it runs" "deep outline has How it runs"
if cb work project explain billing --deep --check "$ROOT/skills/careerbrew-project-explainer/references/example-careerbrew.md" >"$SCRATCH/o" 2>&1; then fail "deep check passed without How it runs"; fi
expect "$(cat "$SCRATCH/o")" "How it runs" "error case: deep check names the missing section"
# PRD rules (issue #223): a slot in dev weeks, then the checker.
expect "$(cb work project prd billing --slot 2)" "prd-billing.md" "PRD outline written"
expect "$(data_cat out/prd-billing.md)" "Plan slot: 2 dev weeks" "PRD outline has the plan slot"
if cb work project prd billing --check "$CAREERBREW_HOME/out/prd-billing.md" >"$SCRATCH/o" 2>&1; then fail "unfilled PRD passed"; fi
expect "$(cat "$SCRATCH/o")" "estimate in dev weeks" "error case: PRD checker names the estimate rule"
expect "$(cb work project prd billing --check "$ROOT/skills/careerbrew-project-explainer/references/example-prd.md")" "prd check: pass" "example PRD passes"
event_seen work.prd
echo "ok US-56 project explain"
