#!/usr/bin/env bash
# US-96: work-day steps declare order, scope, and shared resources; the plan runs safe steps together (issue #258).
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-28T09:00+00:00"
cb init >/dev/null
mkdir -p "$SCRATCH/plugin-stages"
cat >"$SCRATCH/plugin-stages/careerbrew-plugin.yaml" <<'YAML'
name: stages-co
version: 1.0.0
work_steps:
  - {step: /feed, longRunning: true}
  - {step: /example-prep, scope: repo, uses: ["checkout:~/code/repo"]}
  - {step: /status-report, scope: report, after: /example-prep}
  - {step: /review, uses: ["browser:sso"]}
  - {step: /drafts, uses: ["browser:sso", "slack-draft:team"]}
YAML
cb plugin add "$SCRATCH/plugin-stages" >/dev/null
plan="$(cb day run --json)"
staged="$(python3 -c 'import json,sys; p=json.load(sys.stdin); print(" ".join(s["step"]+"@"+str(s["stage"]) for s in p["steps"]))' <<<"$plan")"
expect "$staged" "/careerbrew@1 /feed@4 /example-prep@2 /status-report@3 /review@2 /drafts@3" "each step's stage: after, uses, and long-running order the day"
expect "$plan" "shares browser:sso with /review (stage 2)" "why: a shared resource splits two steps"
expect "$plan" "after /example-prep (stage 2)" "why: after orders the stages"
expect "$plan" "finite resource work must finish" "why: a long-running step with unknown scope waits safely"
expect "$(cb day run)" "stages: 4" "the text plan counts the stages"
expect "$(cb day update /example-prep --status done --handled 'checkout:~/code/repo')" "/example-prep is done" "a step records the checkout it handled"
expect "$(cb day run --json)" '"handled": {"checkout:~/code/repo": "/example-prep"}' "later steps see who handled the checkout"
expect "$(cb day update /drafts --handled 'checkout:~/code/repo' 2>&1 || true)" "already handled today by /example-prep" "error case: a second owner of a checkout is refused"
expect "$(cb day update /drafts --handled 'branch:main' 2>&1 || true)" "breaks US-94" "error case: a bad handled item names the story"

# Readiness releases a successor while its unrelated stage peer still runs.
cb day update /careerbrew --status done >/dev/null
cb day update /review --status running >/dev/null
ready="$(cb day check --json)"
expect "$ready" '"ready": ["/status-report"]' "successor starts without waiting for the unrelated review"
# Quick owner-turn dispatchers are declared in the startup plan.
mkdir -p "$SCRATCH/plugin-operator"
printf 'name: operator-co\nversion: 1.0.0\nwork_steps: [{step: /dispatch, scope: sessions, operatorTurn: true}]\n' >"$SCRATCH/plugin-operator/careerbrew-plugin.yaml"
cb plugin add "$SCRATCH/plugin-operator" >/dev/null
expect "$(cb day run --json)" '"operator_turn": ["/dispatch"]' "initial-turn dispatcher is listed at startup"
# An owner can order a deployed external command without a plugin fallback.
mkdir -p "$CAREERBREW_CLIENT_HOME/.codex/prompts"
printf '%s\n' '---' 'description: Inspect synthetic sources.' '---' > "$CAREERBREW_CLIENT_HOME/.codex/prompts/example-check.md"
cb config set work_steps '[{"step":"/example-check","scope":["example-work"],"uses":["file:owner-result"],"after":["/example-prep"]}]' >/dev/null
owner_plan="$(cb day run --json)"
expect "$owner_plan" '"scope": ["example-work"]' "the owner mapping keeps its scope"
expect "$owner_plan" '"uses": ["file:owner-result"]' "the owner mapping keeps its resource"
expect "$owner_plan" '"after": ["/example-prep"]' "the owner mapping keeps its dependency"
mkdir -p "$SCRATCH/plugin-loop"
printf 'name: loop-co\nversion: 1.0.0\nwork_steps: [{step: /x, scope: a, after: /y}, {step: /y, scope: b, after: /x}]\n' >"$SCRATCH/plugin-loop/careerbrew-plugin.yaml"
cb plugin add "$SCRATCH/plugin-loop" >/dev/null
expect "$(cb day run 2>&1 || true)" "wait for each other" "error case: an after loop is refused"
mkdir -p "$SCRATCH/plugin-bad"
printf 'name: bad-co\nversion: 1.0.0\nwork_steps: [{step: /x, use: [browser:sso]}]\n' >"$SCRATCH/plugin-bad/careerbrew-plugin.yaml"
expect "$(cb plugin add "$SCRATCH/plugin-bad" 2>&1 || true)" "unknown field(s) use" "error case: a typo in a field is refused at install"
echo "ok US-96 work day stages"
