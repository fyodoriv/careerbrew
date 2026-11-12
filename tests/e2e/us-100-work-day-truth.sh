#!/usr/bin/env bash
# US-100: a work step is done only when its verify passes; a hung step is retried once, then failed; blocked differs from failed (issue #259).
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-28T09:00+00:00"
cb init >/dev/null
mkdir -p "$SCRATCH/plugin-truth"
cat >"$SCRATCH/plugin-truth/careerbrew-plugin.yaml" <<'YAML'
name: truth-co
version: 1.0.0
work_steps:
  - {step: /feed-proof, scope: feed, verify: "sh -c 'echo synthetic-verify-output; exit 3'"}
  - {step: /checkup-proof, scope: checkup, verify: "true"}
  - {step: /slow, scope: slow, timeout: 5}
  - {step: /ask, scope: ask}
YAML
cb plugin add "$SCRATCH/plugin-truth" --allow-exec >/dev/null  # a plugin verify runs only after --allow-exec
out="$(cb day update /checkup-proof --status done)"
expect "$out" "/checkup-proof is done" "a step whose verify passes is done"
set +e
out="$(cb day update /feed-proof --status done 2>&1)"
rc=$?
set -e
expect "$rc" "1" "a failed verify exits 1"
expect "$out" "/feed-proof is failed (tool)" "error case: a step whose verify exits non-zero is failed"
expect "$out" "verify exit 3" "the verify result names the exit code"
refute "$out" "synthetic-verify-output" "privacy: the verify output is never printed"
event_seen workday.verify
expect "$(cb day update /slow --status running)" "/slow is running" "the row is written when the step starts"
export CAREERBREW_NOW="2026-10-28T09:06+00:00"
expect "$(cb day run --json)" '"overdue": true' "a running row past its timeout is flagged"
expect "$(cb day check)" "/slow timed out after 5 min: retry 2 of 2" "a step past its timeout is retried once"
cb day update /slow --status running >/dev/null
export CAREERBREW_NOW="2026-10-28T09:12+00:00"
expect "$(cb day check --json)" '"failed": ["/slow"]' "after the retry times out too, the step is failed"
event_seen workday.timeout
expect "$(cb day run --json)" '"error_class": "tool"' "the failed step carries its class"
expect "$(cb day update /ask --status blocked 2>&1 || true)" "blocked needs a question record" "error case: a blocked row without a question is refused"
expect "$(cb day update /ask --status blocked 2>&1 || true)" "breaks US-100" "error case: the refusal names the story"
expect "$(cb day update /ask --status failed 2>&1 || true)" "failed needs an error class" "error case: a failed row without a class is refused"
expect "$(cb day update /ask --status blocked --needs-you 'Pick a synthetic slot')" "next: answer the ask" "a blocked row with its question carries a next action"
out="$(cb day render)"
expect "$out" "1 blocked, 2 failed" "the page counts blocked and failed apart"
items="$(cb report latest --run work-day)"
expect "$items" "blocker  /feed-proof failed: failed (tool)" "the page names the failed row's class"
expect "$items" "blocker  /ask blocked: blocked: waits on you" "the page shows the blocked row waits on the owner"
refute "$items" "Pick a synthetic slot" "privacy: the ask stays on the page only"
echo "ok US-100 work day truth"
