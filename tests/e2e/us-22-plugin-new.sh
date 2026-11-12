#!/usr/bin/env bash
# US-22: start a new job and set up its plugin and connections with one command.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
out="$(cb plugin new "Globex Corp" --dir "$SCRATCH/globex" --connect jira,github --jira-url https://jira.globex.example.com --github-user me --no-prompt)"
expect "$out" "plugin-new: blocked: plugin created and installed: 2 connector(s), 2 token(s) to type" "the run ends with its page and one summary line"
refute "$out" "Globex" "the company name stays out of the output"
out="$(cb report latest)"  # the run page as text
expect "$out" "done     Created plugin" "plugin created"
expect "$out" "-> careerbrew secret set careerbrew.p-" "token steps listed without the company name"
expect "$out" "create it at https://" "each token step says where to create it"
[ -f "$SCRATCH/globex/careerbrew-plugin.yaml" ] && [ -d "$SCRATCH/globex/.git" ] || fail "plugin repo scaffolded"
expect "$(cb plugin list)" "provides=evidence_sources,ladders" "plugin installed"
expect "$(cb plugin new "Globex Corp" --dir "$SCRATCH/globex" --no-prompt 2>&1 || true)" "not empty" "error case: never overwrites"
echo "ok US-22 plugin new"
