#!/usr/bin/env bash
# US-37: the web app's API is local-only: token required, foreign Host refused, job views blocked on corporate machines (and their sidebar group hidden), responses never cached or framed;
# `careerbrew open` stops a careerbrew server on its port that nothing tracks and opens a working link (issue #118).
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
out="$(cd "$ROOT" && .venv/bin/python -m pytest -q -p no:cacheprovider tests/unit/test_server.py 2>&1)" || { echo "$out"; fail "API tests"; }
expect "$out" " passed" "API guard tests"  # pytest exit code above already fails on any failure
# Real servers on a free port from the OS (never 8765), each stopped in teardown.
out="$(cd "$ROOT" && .venv/bin/python -m pytest -q -p no:cacheprovider -rs tests/unit/test_issue_118.py 2>&1)" || { echo "$out"; fail "untracked server on the port"; }
if command -v lsof >/dev/null 2>&1 && command -v ps >/dev/null 2>&1; then
  expect "$out" "5 passed" "open stops and replaces an untracked web app server, never another program, and two starts share one server"
else
  echo "skip: lsof or ps missing, so open cannot find the server on its port (scripts/doctor.sh says so)"
fi
cd "$ROOT/web"
out="$(npx playwright test --reporter=line -g "refuses requests without the token|corporate machine: the Job search group" 2>&1)" || { echo "$out"; fail "browser-side token and corporate sidebar checks"; }
expect "$out" "2 passed" "no token, no data; no blocked group on a corporate machine"
[ -s "$CAREERBREW_E2E_OUT/corporate-sidebar.png" ] || fail "screenshot of the corporate sidebar"
# careerbrew open: starts the app on its port and hands the signed-in link to the browser, never to stdout
cd "$ROOT"
cb init >/dev/null
trap 'web_stop; rm -rf "$SCRATCH"' EXIT
fake_browser
out="$(cb open --view week)"
expect "$out" "opened the web app in your browser" "open hands the link to the browser"
refute "$out" "token" "the token never prints"
expect "$(cat "$FAKE_BROWSER_LOG")" "#/week&token=" "the browser gets the view and the token"
web_stop
echo "ok US-37 web API"
