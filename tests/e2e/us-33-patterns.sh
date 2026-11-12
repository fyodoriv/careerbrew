#!/usr/bin/env bash
# US-33: what interview loops share across companies, and a default loop built from it.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
out="$(cb interview-patterns)"
expect "$out" "shared across 5 sourced companies" "patterns over all companies"
expect "$out" "recruiter-screen 100%" "step frequency"
expect "$out" "typical order: recruiter-screen" "typical order"
expect "$out" "AI tools allowed:" "AI policy counts"
expect "$out" "AI pattern:" "shared AI patterns"
expect "$(cb interview "Unknown Startup Inc" --show)" "built from the steps most sourced companies use" "default loop is derived from patterns"
echo "ok US-33 patterns"
