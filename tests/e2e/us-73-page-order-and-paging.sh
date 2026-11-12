#!/usr/bin/env bash
# US-73: every web view puts the block the owner acts on first at the top, in the order docs/web.md lists; a list of
# live data longer than its page shows one page, "Showing 1–10 of N", and Previous and Next, with the page and the
# filter in the URL hash (bare tokens only), so a reload keeps the place; folded blocks show their count.
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
cd "$ROOT/web"
# 1. docs/web.md and VIEW_BLOCKS list the same blocks in the same order and page sizes; the pager's page math, hash
#    (error case: free text and junk keys are dropped), filter, focus page, and fold hold.
out="$(npx vitest run src/order.test.ts src/pager.test.tsx src/router.test.ts 2>&1)" || { echo "$out"; fail "order and pager unit tests"; }
expect "$out" "3 passed" "the order table, the pager, and the hash state"
# 2. The browser, on its own seeded data root: each of the 16 views starts with its first block and keeps the order;
#    a long list pages and keeps its page and filter across a reload; a page past the end shows the last page and
#    typed text never lands in the URL (error case); Closed and Housekeeping fold with their counts; Week keeps its days;
#    per view with paged lists (7), every list longer than its page shows one page, "Showing", and Next.
out="$(env -u CAREERBREW_HOME -u CAREERBREW_PUBLIC_CARDS npx playwright test --reporter=line e2e/order-paging.spec.ts 2>&1)" || { echo "$out"; fail "order and paging e2e"; }
expect "$out" "27 passed" "16 views in order, paging across a reload, the error case, folds with counts, Week's days, 7 views' long lists"
for s in order-today order-dashboard paging; do [ -s "$CAREERBREW_E2E_OUT/$s.png" ] || fail "screenshot $s"; done
echo "ok US-73 page order and paging"
