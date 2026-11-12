#!/usr/bin/env bash
# US-91: a background agent reports each step with `work agent event` while it works, and Work's Agent activity
# shows it within seconds, grouped by project with its last event and link (issue #272).
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
cb init >/dev/null
# 1. The CLI: one call per step; an agent sees counts and names, never titles or links.
expect "$(cb work agent show)" "no agent activity yet" "no agent has reported"
expect "$(cb work agent event review-bot "CANARYAGENT Reviewing PR 7" --project billing --url https://github.example.com/org/shop/pull/7)" "event stored: review-bot running" "one call reports a step"
cb work agent event fix-bot "Fixing the flaky test" --project ledger >/dev/null
cb work agent event review-bot "Left 3 comments" --status done --project billing >/dev/null
out="$(cb work agent show)"
expect "$out" "2 run(s): 1 running, 0 quiet, 1 done, 0 failed" "each run's state from its last event"
refute "$out" "CANARYAGENT" "an agent never sees a title"
expect "$(cb work agent show --json)" '"running": 1' "the runs as JSON"
out="$(cb work agent event bot "x" --url javascript:alert 2>&1 || true)"
expect "$out" "full http(s) URL" "error case: a link that is not http(s) is refused with the fix"
expect "$out" "breaks US-91" "the refusal names the story"
expect "$(cb work agent event "a b" "x" 2>&1 || true)" "bad agent name" "error case: a bad agent name is refused"
event_seen work.agent.event
# 2. The web app: an event posted from outside the page shows within seconds, with no reload.
cd "$ROOT/web"
out="$(env -u CAREERBREW_HOME -u CAREERBREW_NOW npx playwright test --reporter=line e2e/agents.spec.ts 2>&1)" || { echo "$out"; fail "agent activity e2e"; }
expect "$out" "1 passed" "Agent activity shows an agent's event within seconds and done ends the run"
[ -s "$CAREERBREW_E2E_OUT/work-agent-activity.png" ] || fail "screenshot of Agent activity"
echo "ok US-91 agent activity"
