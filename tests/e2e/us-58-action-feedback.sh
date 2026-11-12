#!/usr/bin/env bash
# US-58: every web action shows a loading state while it runs and an error with the fix when it fails;
# every block of every view says what it loads, fails on its own with Retry, and keeps a gap from the next card;
# on Dashboard and Today one block waits and fails alone while the others show their data.
# blocks.spec.ts also checks US-64's page cache: a reload shows each view's last data before any read answers,
# and after /api/changes moved, a view switch says new data is coming.
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
cd "$ROOT/web"
out="$(env -u CAREERBREW_HOME -u CAREERBREW_PUBLIC_CARDS npx playwright test --reporter=line e2e/feedback.spec.ts e2e/blocks.spec.ts 2>&1)" || { echo "$out"; fail "web action feedback and block states e2e"; }
expect "$out" "33 passed" "action states in all 13 views and a stopped server (14), block states in all 16 views (16), one block alone on Dashboard and Today (2), the stale note (1)"
echo "ok US-58 action feedback"
