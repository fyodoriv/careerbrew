#!/usr/bin/env bash
# US-17: know every step of a company's interview loop, with what is evaluated, tips, traps, and sources.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
for c in Google Amazon Stripe GitLab Datadog; do
  expect "$(cb interview "$c")" "sourced company loop" "$c has a sourced loop"
done
out="$(cb interview Stripe --show)"
expect "$out" "## 1." "steps listed"
expect "$out" "- Tip:" "tips listed"
expect "$out" "- Avoid:" "traps listed"
expect "$out" "They evaluate:" "evaluation criteria listed"
expect "$out" "## Sources" "sources listed"
expect "$out" "https://" "source URLs"
out="$(cb interview "Tiny Unknown Startup")"
expect "$out" "shared default loop" "error case: unknown company falls back to the default loop, marked as such"
expect "$(cb interview "Tiny Unknown Startup" --level 1 --show)" "Recruiter screen" "default loop has the shared steps"
seed_jobs
cb jobs scan >/dev/null
expect "$(cb jobs interview-gaps)" "research next:" "pipeline companies without a loop are listed"
echo "ok US-17 interview"
