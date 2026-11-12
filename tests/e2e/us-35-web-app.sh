#!/usr/bin/env bash
# US-35: the web app: every view (Today, Week, Job hunt, Interviews, Work, Review, Setup) opens from the navigation with real data, with no sideways scrolling at 375, 768, or 1280 px; the sidebar has three groups (Plan, Job search, System).
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
cd "$ROOT/web"
# the public study deck stays on, so the fit check and the Library test open library pages with real code.
# This one seeded server also runs the view tests that have no browser run in their own story (one server
# start is ~11 s): Dashboard horizons (US-41), Library (US-44), the Notion jobs board (US-38), job suggestions
# (US-48), Actions (US-53), the daily log and a Done row (US-52), voice practice (US-57), the standup report (US-65), and Reports (US-67).
out="$(env -u CAREERBREW_PUBLIC_CARDS npx playwright test --reporter=line -g "every view opens|Today shows|company page|any company|fits its width|sidebar groups|Dashboard shows the best action|Library: pages are grouped|Notion jobs board|the owner chooses|every CLI action runs|keeps the daily log|click a Done row|practice by voice|a talking point first|Reports: the newest run page" 2>&1)" || { echo "$out"; fail "web views e2e"; }
expect "$out" "18 passed" "all view tests passed, the fit check at 3 widths included"
for v in today week job-hunt interviews work review setup; do [ -s "$CAREERBREW_E2E_OUT/view-$v.png" ] || fail "screenshot for $v"; done
[ -s "$CAREERBREW_E2E_OUT/sidebar-groups.png" ] || fail "screenshot of the sidebar groups"
for w in 375 768 1280; do [ -s "$CAREERBREW_E2E_OUT/fit-$w.png" ] || fail "screenshot at $w px"; done
# a fresh seeded data root on a Saturday: the sample standup repeats Monday to Friday, so Today must show no meeting and the tests must not expect one
out="$(env -u CAREERBREW_HOME -u CAREERBREW_PUBLIC_CARDS CAREERBREW_NOW=2026-12-05T09:00 npx playwright test --reporter=line -g "every view opens|Today shows" 2>&1)" || { echo "$out"; fail "web views on a weekend day"; }
expect "$out" "2 passed" "every view and Today pass on a weekend day"
echo "ok US-35 web app"
