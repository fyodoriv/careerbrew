#!/usr/bin/env bash
# US-42: connect a work tool by following only what careerbrew prints, with a who-am-I check.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_HTTP_FIXTURES="$FIX/http" CB_JIRA="synthetic-scoped-token"
cb init >/dev/null
# Error case first: a token that fails /myself is rejected and nothing is saved.
out="$(cb connect jira --ref env:CB_JIRA --base-url https://bad.atlassian.example.com --email me@example.com 2>&1 || true)"
expect "$out" "id.atlassian.com/manage-profile/security/api-tokens" "the token page is printed"
expect "$out" "FAIL  jira rejected the token" "error case: a bad token is rejected"
[ ! -e "$CAREERBREW_HOME/work/connectors.yaml" ] || fail "a rejected token must not be saved"
out="$(cb connect jira --ref env:CB_JIRA --base-url https://scoped.atlassian.example.com 2>&1 || true)"
expect "$out" "missing --email" "error case: the missing Jira email is named"
# A scoped token works through the gateway, which careerbrew finds and saves.
out="$(cb connect jira --ref env:CB_JIRA --base-url https://scoped.atlassian.example.com --email me@example.com)"
expect "$out" "through the API gateway" "scoped token moved to the gateway"
saved="$("$ROOT/.venv/bin/python" -c 'from careerbrew import paths, store; print(store.read_text(paths.data_root() / "work" / "connectors.yaml"))')"  # encrypted at rest
expect "$saved" "api.atlassian.com/ex/jira/synthetic-cloud-1" "gateway base_url saved"
echo "ok US-42 connect"
