#!/usr/bin/env bash
# US-16: a public-safe LinkedIn Experience block from my work results, ready to paste.
source "$(dirname "$0")/lib.sh"
seed_jobs
seed_repo "$SCRATCH/svc"
echo "sources: [{kind: git, path: '$SCRATCH/svc', authors: [me@example.com]}]" > "$CAREERBREW_HOME/work/sources.yaml"
cb work collect >/dev/null
cb work note "Led the $CANARY_EMPLOYER on-call rotation redesign" --tags leadership --impact "pages per week down 60%" >/dev/null
out="$(cb work linkedin --role "Senior Software Engineer" --show)"
expect "$out" "chars" "reports size against the limit"
expect "$out" "never posts" "says careerbrew does not post"
expect "$out" "• " "has bullets"
expect "$out" "60%" "keeps numbers by default"
refute "$out" "$CANARY_EMPLOYER" "no sensitive term in public text"
refute "$out" "svc" "no repo name in public text"
refute "$out" "feat:" "no commit prefixes"
cb work note "Ran the zebra-oncall rotation and rotated bot token test-token-0-not-real on k8s-prod-useast1" --tags ownership --impact "pages down 30%" >/dev/null
out="$(cb work linkedin --show)"
refute "$out" "test-token" "token-shaped words never published"
refute "$out" "useast1" "resource names never published"
expect "$out" "may be internal names: zebra-oncall" "suspect words listed for review"
block="$(cat "$CAREERBREW_HOME/out/linkedin.md")"
[ "${#block}" -le 2001 ] || fail "block exceeds LinkedIn's 2000-char limit"
# error case: policy forbids public metrics
cp -R "$FIX/plugin-acme" "$SCRATCH/plugin-acme"
printf '  public_metrics: false\n' >> "$SCRATCH/plugin-acme/careerbrew-plugin.yaml"
cb plugin add "$SCRATCH/plugin-acme" >/dev/null
out="$(cb work linkedin --show)"
expect "$out" "public metrics removed by policy" "policy removes numbers"
refute "$out" "60%" "number removed"
echo "ok US-16 linkedin"
