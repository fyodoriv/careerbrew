#!/usr/bin/env bash
# The hosted gate's result on main: the newest finished run of the CI workflow
# (.github/workflows/ci.yml, docs/ci.md). The checkup reads this instead of
# running the full gate on this machine.
#   scripts/hosted-gate.sh   one line per fact; exit 0 green, 1 red or unknown
# HOSTED_GATE_BRANCH picks another branch (default main).
set -uo pipefail
cd "$(dirname "$0")/.."
branch="${HOSTED_GATE_BRANCH:-main}"

if ! command -v gh >/dev/null 2>&1; then
  echo "hosted-gate: unavailable: gh is not installed (brew install gh), so run the gate here -> fix: CHECKUP_LOCAL_GATE=1 scripts/checkup.sh"
  exit 1
fi
if ! runs="$(gh run list --workflow ci.yml --branch "$branch" --limit 10 \
  --json databaseId,status,conclusion,headSha,event \
  -q '.[] | [.databaseId, .status, (if (.conclusion // "") == "" then "-" else .conclusion end), .headSha[0:8], .event] | @tsv' 2>/dev/null)"; then
  echo "hosted-gate: unavailable: gh could not read the runs (signed out or offline? gh auth status), so run the gate here -> fix: CHECKUP_LOCAL_GATE=1 scripts/checkup.sh"
  exit 1
fi

running="" done=""
while IFS=$'\t' read -r id status conclusion sha event; do
  [ -n "$id" ] || continue
  if [ "$status" != completed ]; then
    [ -n "$running" ] || running="$id $sha"
  elif [ "$conclusion" != cancelled ] && [ "$conclusion" != skipped ]; then
    done="$id $conclusion $sha $event"; break
  fi
done <<< "$runs"

[ -z "$running" ] || echo "hosted-gate: run ${running% *} on $branch ${running#* } is still running"
if [ -z "$done" ]; then
  echo "hosted-gate: unavailable: no finished CI run on $branch -> fix: gh workflow run ci.yml --ref $branch"
  exit 1
fi
read -r id conclusion sha event <<< "$done"
if [ "$conclusion" = success ]; then
  echo "hosted-gate: $branch $sha passed (run $id, $event)"
  exit 0
fi
echo "hosted-gate: $branch $sha $conclusion (run $id, $event) -> read it: gh run view $id --log-failed"
exit 1
