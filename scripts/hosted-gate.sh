#!/usr/bin/env bash
# The hosted gate's result on main's current tip: the push runs of both merge
# workflows (.github/workflows/ci.yml and public-readiness.yml, docs/ci.md) for
# the commit origin's main points at now. The checkup reads this instead of
# running the full gate on this machine.
#   scripts/hosted-gate.sh   one line per fact; exit 0 only when both passed
# A run for an older commit never counts: the tip is green, red, or pending.
# HOSTED_GATE_BRANCH picks another branch (default main); HOSTED_GATE_TIP pins
# the commit instead of asking origin (tests).
set -uo pipefail
cd "$(dirname "$0")/.."
branch="${HOSTED_GATE_BRANCH:-main}"
local_fix="-> fix: CHECKUP_LOCAL_GATE=1 scripts/checkup.sh"

if ! command -v gh >/dev/null 2>&1; then
  echo "hosted-gate: unavailable: gh is not installed (brew install gh), so run the gate here $local_fix"
  exit 1
fi
tip="${HOSTED_GATE_TIP:-$(git ls-remote origin "refs/heads/$branch" 2>/dev/null | cut -f1)}"
if [ -z "$tip" ]; then
  echo "hosted-gate: unavailable: cannot read $branch's tip from origin (offline?), so run the gate here $local_fix"
  exit 1
fi
short="${tip:0:8}"

passed=() failed=() pending=()
for wf in ci.yml public-readiness.yml; do
  # Pushes only: a fork's pull request from its own main has head branch main too.
  if ! rows="$(gh run list --workflow "$wf" --branch "$branch" --event push --commit "$tip" --limit 5 \
    --json databaseId,status,conclusion \
    -q '.[] | [.databaseId, .status, (if (.conclusion // "") == "" then "-" else .conclusion end)] | @tsv' 2>/dev/null)"; then
    echo "hosted-gate: unavailable: gh could not read the runs (signed out or offline? gh auth status), so run the gate here $local_fix"
    exit 1
  fi
  run=""
  # Tabs are whitespace to read, so every field has a value ("-" for no conclusion yet).
  while IFS=$'\t' read -r id status conclusion; do
    [ -n "$id" ] || continue
    [ "$status" = completed ] && { [ "$conclusion" = cancelled ] || [ "$conclusion" = skipped ]; } && continue
    run="$id $status $conclusion"; break
  done <<< "$rows"
  if [ -z "$run" ]; then
    pending+=("$wf has no run for $short yet -> wait: gh run list --workflow $wf --branch $branch")
    continue
  fi
  read -r id status conclusion <<< "$run"
  if [ "$status" != completed ]; then
    pending+=("$wf run $id is $status -> wait: gh run watch $id")
  elif [ "$conclusion" = success ]; then
    passed+=("$wf run $id")
  else
    failed+=("$wf $conclusion (run $id) -> read it: gh run view $id --log-failed")
  fi
done

for f in ${failed[@]+"${failed[@]}"}; do echo "hosted-gate: $branch $short $f"; done
for p in ${pending[@]+"${pending[@]}"}; do echo "hosted-gate: $branch $short pending: $p"; done
[ ${#failed[@]} -eq 0 ] && [ ${#pending[@]} -eq 0 ] || exit 1
echo "hosted-gate: $branch $short passed (${passed[0]}, ${passed[1]})"
