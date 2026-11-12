#!/usr/bin/env bash
# US-90: a review ask names enough people for the PR's required approvals; a group handle never counts (issue #270).
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
export CAREERBREW_HTTP_FIXTURES="$FIX/http" CB_T="synthetic-not-a-token"
cb connect github --ref env:CB_T --user me >/dev/null
out="$(cb work review-ask https://github.com/example/shop/pull/7)"
expect "$out" "review ask: ready; needs 2 approvals (2 required by branch protection" "the branch's required approvals are read"
expect "$out" "names 2 person(s); 1 group handle(s) not counted" "two named people, the group handle not counted"
refute "$out" "ana-dev" "an agent sees counts, never names"
event_seen work.review_ask
printf 'Please review @ana-dev and @example/backend\n' >"$SCRATCH/draft.txt"
out="$(cb work review-ask https://github.com/example/shop/pull/7 --check "$SCRATCH/draft.txt" 2>&1 || true)"
expect "$out" "refused: the draft names 1 person(s) and 1 group handle(s); this PR needs 2 named people" "error case: one person plus a group handle is refused"
expect "$out" "breaks US-90" "the refusal names the story"
expect "$(printf '@ana-dev @ben-dev\n' | cb work review-ask https://github.com/example/shop/pull/7 --check -)" "ok: the draft names 2 person(s)" "two people pass the check"
out="$(cb work review-ask https://github.com/example/tools/pull/3 2>&1 || true)"
expect "$out" "refused: 1 more person(s) needed" "error case: too few people found, never a short draft passed as ready"
expect "$(cb work review-ask https://github.com/example/tools/pull/3 --reviewer eli-dev)" "review ask: ready" "a person the owner names fills the gap"
expect "$(cb work review-ask https://github.com/example/shop/issues/7 2>&1 || true)" "not a pull request link" "error case: not a PR link"
echo "ok US-90 review ask"
