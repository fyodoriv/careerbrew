#!/usr/bin/env bash
# US-99: promises the owner makes in sent messages become tracked projects, shown on Dashboard's Commitments
# block and in the handoff until done (issue #268).
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
cb init >/dev/null
export CAREERBREW_NOW="2026-11-02T09:00+00:00"
expect "$(cb work promise list)" "promises: none open" "no promise yet"
# 1. A sent reply "I'll look into the timeout" is tracked with its link and who it was made to; an ask is not.
printf '%s\n' \
  '{"id":"slack:D1:1","source":"slack","kind":"dm","at":"2026-11-02T08:00:00Z","title":"DM from Sam","actor":"other","to_me":true,"from":"Sam","thread":"slack:D1","text":"the nightly job times out again, I will retry it"}' \
  '{"id":"slack:D1:2","source":"slack","kind":"dm","at":"2026-11-02T08:05:00Z","title":"You wrote in a DM","actor":"me","thread":"slack:D1","url":"https://slack.example.com/archives/D1/p2","text":"Thanks! I'"'"'ll look into the timeout tomorrow."}' \
  | cb work feed add - >/dev/null
out="$(cb work promise list)"
expect "$out" "promises: 1 open" "only my own sent message holds a promise"
expect "$out" "due 2026-11-03: (text hidden) next: research https://slack.example.com/archives/D1/p2" "an agent sees the due day, the next step, and the link, never the text"
name="$(cb work promise list --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["promises"][0]["name"])')"
mkdir -p "$SCRATCH/p" && printf 'name: allow\nversion: 0.1.0\npolicy: {agent_may_read_business: true}\n' > "$SCRATCH/p/careerbrew-plugin.yaml"
cb plugin add "$SCRATCH/p" >/dev/null
expect "$(cb work promise list)" "$name due 2026-11-03: Look into the timeout tomorrow to Sam next: research" "with the owner's policy: the promised action and who it was made to"
expect "$(cb work project show "$name")" "$name" "a promise is a project in the one project store"
# 2. It shows in the handoff until done.
expect "$(cb day handoff show --today)" "[$name] Look into the timeout tomorrow (to Sam), due 2026-11-03" "the handoff lists the open promise"
# 3. The cycle records its steps; errors name the fix.
expect "$(cb work promise step "$name" research 'read the job log')" "next: draft" "a research step is recorded"
expect "$(cb work promise step "$name" send x 2>&1 || true)" "invalid choice" "error case: an unknown step"
expect "$(cb work promise done promise-00000000 2>&1 || true)" "no promise promise-00000000" "error case: an unknown promise names the list"
expect "$(cb work promise add 'Send the synthetic notes' --due 'next friday' 2>&1 || true)" "looks like 2026-11-03" "error case: a bad due day"
other="$(cb work promise add 'Send the synthetic notes' --to Kim --due 2026-10-30 | sed -n 's/^promise \(promise-[0-9a-f]*\) tracked.*/\1/p')"
expect "$(cb work promise list)" "$other due 2026-10-30 OVERDUE" "a promise added by hand, overdue first"
expect "$(cb work promise dropped "$other")" "dropped" "a wrong one is dropped"
cb work promise scan >/dev/null
# 4. Done closes it: it leaves the list and the handoff, and the next pull never makes it again.
expect "$(cb work promise done "$name")" "left Commitments and the handoff" "close a kept promise"
expect "$(cb work promise list)" "promises: none open" "the closed promise is gone"
expect "$(cb day handoff show --today)" "handoff: none" "and gone from the handoff"
expect "$(cb work promise scan)" "promises: 0 new" "a closed promise is never tracked again"
expect "$(cb work promise list --all)" "[done]" "--all keeps the closed ones"
event_seen work.promise
# 5. The web app: Commitments on Dashboard, with Done and Not a promise.
cd "$ROOT/web"
out="$(env -u CAREERBREW_HOME -u CAREERBREW_NOW npx playwright test --reporter=line e2e/promises.spec.ts 2>&1)" || { echo "$out"; fail "commitments e2e"; }
expect "$out" "2 passed" "Commitments lists each promise; Done and Not a promise close one"
[ -s "$CAREERBREW_E2E_OUT/commitments.png" ] || fail "screenshot of Commitments"
echo "ok US-99 promises"
