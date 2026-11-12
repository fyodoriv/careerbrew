#!/usr/bin/env bash
# US-13: my private and business data never leaks: not to logs, outputs, git, or agents.
source "$(dirname "$0")/lib.sh"
seed_jobs
cp -R "$FIX/plugin-acme" "$SCRATCH/plugin-acme"
cb plugin add "$SCRATCH/plugin-acme" --allow-exec >/dev/null
cb work note "Worked with $CANARY_EMPLOYER on PROJECTZEBRA" --tags delivery >/dev/null
out="$( { cb jobs scan; cb work collect; cb work promo; cb work gaps; cb review --html; cb plugin list; } 2>&1 || true)"
for t in "$CANARY_PERSON" "$CANARY_EMPLOYER" PROJECTZEBRA AcmeSecretCustomer; do
  refute "$out" "$t" "sensitive term in CLI output"
  if grep -rqF "$t" "$CAREERBREW_HOME/logs/"; then fail "sensitive term in event log"; fi
  if grep -qaF "$t" "$CAREERBREW_HOME/../backups/"*.cbk 2>/dev/null; then fail "term in backup"; fi
done
out="$(cb work promo --show || true)"
expect "$out" "refusing --show" "piped --show refuses business content"
# git: a repo that stages private data or a term is blocked by the scan
repo="$SCRATCH/repo"; mkdir -p "$repo/private"; git init -q "$repo"
echo "contact $CANARY_EMPLOYER" > "$repo/notes.md"; echo "x: 1" > "$repo/private/p.yaml"
git -C "$repo" add -f .
out="$(cb privacy scan --repo "$repo" --staged || true)"
expect "$out" "2 finding(s)" "staged leaks found"
refute "$out" "$CANARY_EMPLOYER" "scan never prints the term"
# a draft issue (checkup last step) is scanned before it is posted
printf '## Problem\nevidence collect failed for %s, see private/work/evidence.jsonl\n' "$CANARY_EMPLOYER" > "$SCRATCH/draft.md"
out="$(cb privacy scan --file "$SCRATCH/draft.md" || true)"
expect "$out" "2 finding(s)" "a draft with a term and a private/ path is blocked"
refute "$out" "$CANARY_EMPLOYER" "the draft scan never prints the term"
printf '## Problem\n`scripts/doctor.sh` names a fix script that does not exist\n' > "$SCRATCH/clean.md"
expect "$(cb privacy scan --file "$SCRATCH/clean.md")" "0 finding(s)" "a clean draft passes"
# the real repo is clean
expect "$(CAREERBREW_HOME="$SCRATCH/empty" cb privacy scan)" "0 finding(s)" "this repo has no leaks (scanned with an empty data root; fixtures hold the canaries on purpose)"
echo "ok US-13 privacy"
