#!/usr/bin/env bash
# US-72: every control in the web app works. The gate runs the whole Playwright suite here, on one seeded server
# with its own CAREERBREW_E2E_PORT: no -g filter, so a browser test runs the moment it exists. The suite includes the
# control inventory (web/e2e/controls.spec.ts): every input, select, textarea, checkbox, button, and form in every
# view, on a personal and a corporate machine, needs an entry in web/e2e/controls.json naming the test that exercises it.
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
cd "$ROOT/web"
total="$(unset CAREERBREW_HOME CAREERBREW_PUBLIC_CARDS; pw_total e2e)"
[ "${total:-0}" -gt 60 ] || fail "Playwright lists every test (got: ${total:-none})"
env -u CAREERBREW_HOME -u CAREERBREW_PUBLIC_CARDS PLAYWRIGHT_JSON_OUTPUT_FILE="$SCRATCH/discovery.json" npx playwright test --list --reporter=json >/dev/null || fail "machine-readable test discovery"
# the public study deck stays on, as in US-35: Practice, Library, and the card steps read real explanations.
out="$(env -u CAREERBREW_HOME -u CAREERBREW_PUBLIC_CARDS PLAYWRIGHT_JSON_OUTPUT_FILE="$SCRATCH/execution.json" npx playwright test --reporter=line,json 2>&1)" || { echo "$out"; fail "the whole browser suite"; }
expect "$out" "$total passed" "every browser test ran and passed"
refute "$out" "skipped" "no browser test is skipped"
refute "$out" "flaky" "no browser test passed only on a retry"
for m in personal corporate; do
  [ -s "$CAREERBREW_E2E_OUT/controls-$m.json" ] || fail "the control inventory on a $m machine"
done
grep -q '"Week: date Due date"' "$CAREERBREW_E2E_OUT/controls-personal.json" || fail "the inventory lists controls by accessible name"
# A separate expected-failure probe proves Playwright still reports route-handler errors.
npx playwright test --config route-owner-probe/playwright.config.ts > "$SCRATCH/route-owner-report.json" || fail "route-owner probe runner"
node route-owner-probe/assert-report.cjs "$SCRATCH/route-owner-report.json" || fail "route-owner error propagation"
"$ROOT/.venv/bin/python" "$ROOT/scripts/browser-test-receipts.py" "$SCRATCH/discovery.json" "$SCRATCH/execution.json" || fail "each discovered browser test passed once"
echo "ok US-72 every control works ($total browser tests)"
