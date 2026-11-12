#!/usr/bin/env bash
# US-76: every view is comfortable to read and tap on a phone and a desktop, in light and dark: no serious axe problem,
# no text under 14 px, no phone touch target under 44 px, no sideways scroll; the phone shell is one Menu button; the
# everyday jobs fit their tap budget; headings show friendly days.
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
cd "$ROOT/web"
# 1. Friendly days read an ISO day as that calendar day; anything else stays as it is (error case).
out="$(npx vitest run src/dates.test.ts 2>&1)" || { echo "$out"; fail "friendly day labels"; }
expect "$out" "2 passed" "friendly day labels"
# 2. The browser, on its own seeded data root: 18 pages x 4 modes meet the budget; Menu opens and closes; tap budget.
#    Three app.spec tests run first, as in the gate: they grade a card, set goals, and approve the promotion goal,
#    the crowded state where text spilled out of the Week columns and a number broke in Review before.
#    (Only those three: the whole app.spec beside the gate's other stories timed out a Teach me wait.)
out="$(env -u CAREERBREW_HOME -u CAREERBREW_PUBLIC_CARDS npx playwright test --reporter=line e2e/app.spec.ts e2e/comfort.spec.ts \
  -g "Practice: recall|Goals: only the owner|goal approval in the UI|comfort budget|Menu opens the views|tap budget" 2>&1)" || { echo "$out"; fail "comfort budget e2e"; }
expect "$out" "10 passed" "3 state builders, 4 modes within budget, the phone menu, tap budget, and context cleanup"
for m in phone-light phone-dark desktop-light desktop-dark; do [ -s "$CAREERBREW_E2E_OUT/comfort/$m/today.png" ] || fail "screenshot $m/today"; done
[ -s "$CAREERBREW_E2E_OUT/phone-menu.png" ] || fail "screenshot of the phone menu"
echo "ok US-76 comfort budget"
