#!/usr/bin/env bash
# US-71: the web app stays signed in across restarts (issue #141): one token, encrypted at rest, reused until
# `careerbrew web rotate-token` or web_token_max_days; an HttpOnly, SameSite=Strict cookie for 127.0.0.1 signs in a
# reload, a new tab, or a bookmark; a cross-site page gets nothing; the token never reaches a log or an agent's stdout.
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
cd "$ROOT/web"
# Its own synthetic home and web app, started by `careerbrew open`: restart and reload, a bookmark, a cross-site page, rotate-token.
out="$(env -u CAREERBREW_HOME npx playwright test --reporter=line e2e/signin.spec.ts 2>&1)" || { echo "$out"; fail "browser sign-in across a restart"; }
expect "$out" "3 passed" "an old tab and a bookmark work after a restart; a cross-site page is refused; rotate-token signs tabs out and the sign-in page signs them in"
for shot in signin-page signin-bookmark; do [ -s "$CAREERBREW_E2E_OUT/$shot.png" ] || fail "screenshot $shot"; done
out="$(cd "$ROOT" && .venv/bin/python -m pytest -q -p no:cacheprovider -rs tests/unit/test_issue_141.py 2>&1)" || { echo "$out"; fail "web token tests"; }
expect "$out" " passed" "stable token, cookie attributes, cross-site refusal, rotate, max age, and no token in logs"

# The real CLI on the story's synthetic home: the confirm, the setting, encryption at rest, and content-free events.
cb init >/dev/null
out="$(cb web rotate-token 2>&1)" && fail "rotate-token without --yes must refuse"
expect "$out" "rerun with --yes" "rotate-token asks for --yes: it signs out every open tab"
out="$(cb web rotate-token --yes)"
expect "$out" "careerbrew open" "rotate-token names the command that signs you in again"
tok="$(data_cat day/web-token.json | "$ROOT/.venv/bin/python" -c 'import json, sys; print(json.load(sys.stdin)["token"])')"
[ -n "$tok" ] || fail "a token was made"
refute "$out" "$tok" "rotate-token never prints the token"
head -c 5 "$CAREERBREW_HOME/day/web-token.json" | grep -q "CBV1" || fail "the token file is encrypted at rest"
refute "$(cat "$CAREERBREW_HOME/day/web-token.json")" "$tok" "no plaintext token on disk"
out="$(cb config set web_token_max_days 0 2>&1)" && fail "web_token_max_days 0 must be refused"
expect "$out" "1 to 365" "the max age is validated"
cb config set web_token_max_days 14 >/dev/null
expect "$(cb config)" "web_token_max_days" "the max age is in the settings table"
event_seen web.token
refute "$(cat "$CAREERBREW_HOME/logs/events.jsonl")" "$tok" "the event log holds no token"
echo "ok US-71 web sign-in"
