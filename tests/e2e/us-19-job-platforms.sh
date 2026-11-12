#!/usr/bin/env bash
# US-19: find remote and nearby software jobs across the best open job platforms.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
export CAREERBREW_HTTP_FIXTURES="$FIX/http" CB_ADZ="e2e-not-a-key"
expect "$(cb jobs preset)" "Remote preset written" "preset"
# The preset picks no country: Adzuna is skipped with the fix until the user sets one.
out="$(cb jobs scan)"
expect "$out" "skipped adzuna: adzuna source needs \`country\`" "error case: an Adzuna source with no country is skipped with the reason"
expect "$out" "careerbrew data edit jobs/sources.yaml" "the skip line names the fix"
data_cat jobs/sources.yaml | sed -e 's/keychain:careerbrew.adzuna.id/env:CB_ADZ/' -e 's/keychain:careerbrew.adzuna.key/env:CB_ADZ/' | awk '{print} /kind: adzuna/{print "  country: gb"}' | data_write jobs/sources.yaml
expect "$(data_cat jobs/sources.yaml)" "country: gb" "the Adzuna source has a country"
out="$(cb jobs scan)"
expect "$out" "region=2" "remote roles limited to regions not allowed are filtered"
out="$(cb jobs list --top 20)"
for c in AcmeCo NorthCo ZzPlatformCo LakeCo PineCo CedarCo; do expect "$out" "$c" "platform posting from $c"; done
refute "$out" "UsOnlyCo" "US-only role hidden"
expect "$out" "jobs via" "sources are credited"
unset CB_ADZ
out="$(cb jobs scan --no-cache)"
expect "$out" "skipped adzuna" "missing optional credential is explained"
expect "$out" "5 source(s); 1 skipped" "available source count excludes missing credentials"
expect "$out" "scanned 7 posting(s)" "free source postings are saved"
out="$(cb jobs list --top 20)"
expect "$out" "AcmeCo" "free source remains available"
refute "$out" "CedarCo" "old Adzuna posting is not reported as freshly fetched"
expect "$(cb jobs preset 2>&1 || true)" "--force" "error case: preset never overwrites silently"
echo "ok US-19 job platforms"
