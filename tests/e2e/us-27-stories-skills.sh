#!/usr/bin/env bash
# US-27: every user story has a mode, acceptance criteria, a metric, a skill, and an e2e test; every skill has evals.
source "$(dirname "$0")/lib.sh"
out="$(cd "$ROOT" && .venv/bin/python scripts/check-stories.py 2>&1)" || { code=$?; printf '%s\n' "$out" >&2; exit "$code"; }
expect "$out" "0 problem(s)" "stories complete"
expect "$out" "job helper=" "job helper stories"
expect "$out" "job search=" "job search stories"
out="$(cd "$ROOT" && evals/run.sh 2>&1)" || { code=$?; printf '%s\n' "$out" >&2; exit "$code"; }
expect "$out" "0 failure(s)" "skills have act and refrain evals; CLI cases pass"
echo "ok US-27 stories and skills"
