#!/usr/bin/env bash
# US-64: a view opened again answers from the server's memory, a read never looks like new data,
# and a write by any process still shows within one poll of /api/changes. The server warms the work
# views at start and after new data, so their first open answers from memory too (issue #142).
source "$(dirname "$0")/lib.sh"
# 1. The rules, against a real server: hit on repeat, miss after a write, a setting, the clock, or a POST;
#    the cached-repeat time limit for /api/daylog; the content-free web.api event; an idle watcher cycle
#    moves no version, and a cycle that adds a result or a notification still does.
out="$(cd "$ROOT" && .venv/bin/python -m pytest -q -p no:cacheprovider tests/unit/test_api_cache.py tests/unit/test_watch_idle.py tests/unit/test_web_blocks.py 2>&1)" || { echo "$out"; fail "response cache tests"; }
expect "$out" " passed" "cache hits, invalidation, the cached-repeat limit, idle watcher cycles, block endpoints, the warmer, and the tab key"
# 2. The seeded web app: two requests per view. Every cached view answers the second from memory,
#    and no read moved the change version. Then an idle `day watch once` moves nothing, and every
#    cached view still answers from memory. Times are printed for the record, not asserted (machine load).
out="$(cd "$ROOT" && .venv/bin/python scripts/api-timing.py --check 2>&1)" || { echo "$out"; fail "cached views on the e2e seed"; }
echo "$out"
expect "$out" "/api/daylog" "the day log was timed"
expect "$out" "/api/changes version moved during the reads: no" "a read does not look like new data"
expect "$out" "/api/changes version moved by an idle watcher cycle: no" "an idle watcher cycle does not look like new data"
re='cached views answered from memory after the idle cycle: ([0-9]+)/([0-9]+)'
[[ "$out" =~ $re ]] && [ "${BASH_REMATCH[1]}" = "${BASH_REMATCH[2]}" ] && [ "${BASH_REMATCH[1]}" -gt 0 ] || fail "cached views stay hits across an idle cycle"
for when in "their first open" "the first open after new data"; do
  re="warm views answered $when from memory: ([0-9]+)/([0-9]+)"
  [[ "$out" =~ $re ]] && [ "${BASH_REMATCH[1]}" = "${BASH_REMATCH[2]}" ] && [ "${BASH_REMATCH[1]}" -ge 14 ] || fail "every warm view answers $when from memory"
done
# 3. Error case: the setting names its range.
expect "$(cb config set web_cache_seconds 99999 2>&1 || true)" "from 0 to 3600" "a bad web_cache_seconds names the range"
cb config set web_cache_seconds 0 >/dev/null
expect "$(cb config)" "web_cache_seconds" "the setting is listed with its source"
expect "$(cb config set web_tab_cache maybe 2>&1 || true)" "use true or false" "a bad web_tab_cache names the values"
cb config set web_tab_cache false >/dev/null
expect "$(cb config)" "web_tab_cache" "the page cache setting is listed with its source"
echo "ok US-64 fast views"
