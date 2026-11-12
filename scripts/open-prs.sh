#!/usr/bin/env bash
# List this repo's open pull requests with the next step to finish each.
# Exit 0: none open. Exit 1: some are open (finish them first). Exit 2: could not ask GitHub.
# Used by scripts/checkup.sh; works without the optional wrap-up skill.
set -uo pipefail
cd "$(dirname "$0")/.."
gh="${GH_BIN:-gh}"  # tests point GH_BIN at a fake
command -v "$gh" >/dev/null 2>&1 || { echo "FAIL  gh is missing, so open PRs were not checked -> fix: brew install gh && gh auth login"; exit 2; }
if ! out="$("$gh" pr list --state open --json number,title,isDraft,mergeable,headRefName,updatedAt \
  -q '.[] | "\(.number)\t\(.isDraft)\t\(.mergeable)\t\(.headRefName)\t\(.updatedAt[0:10])\t\(.title)"' 2>&1)"; then
  echo "FAIL  could not list open PRs: $(printf '%s' "$out" | head -1) -> fix: gh auth status (outside the agent sandbox)"
  exit 2
fi
[ -z "$out" ] && { echo "ok    no open pull requests"; exit 0; }
while IFS=$'\t' read -r num draft mergeable branch updated title; do
  next="review, then merge when the gate is green"
  [ "$draft" = true ] && next="finish the remaining work, mark it ready, then merge"
  [ "$mergeable" = CONFLICTING ] && next="merge main into $branch and resolve the conflicts, run scripts/check.sh, then ${next}"
  printf 'OPEN  #%s %s (branch %s, updated %s%s%s)\n      next: %s\n' "$num" "$title" "$branch" "$updated" \
    "$([ "$draft" = true ] && echo ', draft')" "$([ "$mergeable" = CONFLICTING ] && echo ', conflicts with main')" "$next"
done <<< "$out"
echo "-> finish open PRs before new work: gh pr checkout <number>"
exit 1
