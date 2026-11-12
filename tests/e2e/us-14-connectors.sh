#!/usr/bin/env bash
# US-14: connect Slack, Google Docs, Jira, GitHub, Zoom, and Outlook with tokens kept as secret references.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
export CAREERBREW_HTTP_FIXTURES="$FIX/http" CB_E2E_TOKEN="e2e-token-not-real" CAREERBREW_NOW="2026-10-16T16:00+00:00"  # the fixture review is on this day
cb connect github --ref env:CB_E2E_TOKEN --user me >/dev/null
cb connect jira --ref env:CB_E2E_TOKEN --base-url https://jira.example.com --email me@example.com >/dev/null
for k in slack gdocs zoom outlook; do cb connect "$k" --ref env:CB_E2E_TOKEN >/dev/null; done
expect "$(cb connectors list)" "zoom     secret=env allowed_by_policy=True" "connectors listed with secret kind"
if grep -rq "e2e-token-not-real" "$CAREERBREW_HOME" || data_has "e2e-token-not-real"; then fail "a token value was written to the data root"; fi
expect "$(cb work collect)" "evidence: 12 new, 12 total, 0 source error(s)" "all six connectors collect"
expect "$(CAREERBREW_NOW=2026-10-19T09:00+00:00 cb day plan)" "1 meeting(s)" "outlook connector feeds today's meetings with no calendars.yaml"
out="$(cb work promo --level senior)"
expect "$out" "overall coverage" "connector evidence feeds the promo case"
expect "$(cb connect slack --ref xoxp-literal 2>&1 || true)" "never a literal value" "error case: literal token refused"
out="$(cb connect slack < /dev/null 2>&1 || true)"
expect "$out" "refusing to read a token from a pipe" "error case: no token over a pipe"
out="$(cb secret set careerbrew.e2e < /dev/null 2>&1 || true)"
expect "$out" "refusing to read a secret from a pipe" "error case: secret set never reads a pipe"
echo "ok US-14 connectors"
