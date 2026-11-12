#!/usr/bin/env bash
# US-03: careerbrew proposes a Want score; I rate jobs; it learns my weights.
source "$(dirname "$0")/lib.sh"
seed_jobs
cb jobs scan >/dev/null
out="$(cb jobs list --top 10)"
expect "$out" "[ashby:Fabrikam:f-1]" "list shows posting ids with scores"
expect "$out" "rate with: careerbrew jobs rate" "list tells me how to rate"
for r in "gh:Northwind:101 5" "ashby:Fabrikam:f-1 5" "lever:Contoso:c-1 3" "gh:Northwind:102 2" "ashby:Fabrikam:f-2 1" "lever:Contoso:c-2 2"; do
  # shellcheck disable=SC2086
  cb jobs rate $r >/dev/null
done
out="$(cb jobs learn)"
expect "$out" '"status": "' "learn reports kept or reverted"
refute "$out" "need-more-ratings" "six ratings are enough"
data_cat metrics.jsonl | grep -q want_agreement || fail "agreement metric recorded"
out="$(cb jobs rate no-such-id 3 || true)"
expect "$out" "unknown posting id" "error case: unknown id"
echo "ok US-03 want + rate + learn"
