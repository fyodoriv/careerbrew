#!/usr/bin/env bash
# US-75: every story with web views has approved pictures of its key screens, at desktop and phone width, and the gate
# compares each screen with them on the synthetic seed with the clock pinned (web/playwright.visual.config.ts).
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
cd "$ROOT/web"
platform="$(node -p process.platform)"
if ! find visual/__screenshots__ -name "*-$platform.png" | grep -q .; then
  # Fonts render differently on each platform, so pictures are drawn per platform (docs/web.md, "Visual tests").
  echo "skip US-75: no approved pictures for $platform; draw and review them with: cd web && npm run visual:update"
  exit 0
fi

# The story checker: a story with web views has a visual test and each picture it names at both widths.
out="$("$ROOT/.venv/bin/python" "$ROOT/scripts/check-stories.py")" || { echo "$out"; fail "every story with web views has its pictures"; }
expect "$out" "with web views and pictures, 0 problem(s)" "check-stories counts the stories with pictures"

total="$(pw_total visual -c playwright.visual.config.ts)"
[ "${total:-0}" -ge 90 ] || fail "Playwright lists every visual test at both widths (got: ${total:-none})"
out="$(env -u CAREERBREW_HOME -u CAREERBREW_PUBLIC_CARDS npx playwright test -c playwright.visual.config.ts --reporter=line 2>&1)" || {
  echo "$out"
  fail "every screen matches its approved picture (a change on purpose: cd web && npm run visual:update, then open each picture)"
}
expect "$out" "$total passed" "every visual test ran and passed"
refute "$out" "skipped" "no visual test is skipped"
refute "$out" "flaky" "no visual test passed only on a retry"

# Error case: a picture that differs, and one that is missing, each fail the run, and the run writes no picture.
pictures="$SCRATCH/pictures/us-24-day-next.spec.ts"
mkdir -p "$pictures"
cp "visual/__screenshots__/us-24-day-next.spec.ts/next-step-phone-$platform.png" "$pictures/next-step-desktop-$platform.png"
out="$(env -u CAREERBREW_HOME -u CAREERBREW_PUBLIC_CARDS VISUAL_SNAPSHOTS="$SCRATCH/pictures" npx playwright test -c playwright.visual.config.ts us-24-day-next --reporter=line 2>&1)" && {
  echo "$out"
  fail "a screen that differs from its picture fails the run"
}
expect "$out" "2 failed" "the changed picture (desktop) and the missing one (phone) both fail"
expect "$out" "next-step-phone-$platform.png" "the missing picture is named"
cmp -s "$pictures/next-step-desktop-$platform.png" "visual/__screenshots__/us-24-day-next.spec.ts/next-step-phone-$platform.png" || fail "a failed run never rewrites a picture"
[ ! -e "$pictures/next-step-phone-$platform.png" ] || fail "a failed run never writes a missing picture"
echo "ok US-75 visual stories ($total visual tests on $platform)"
