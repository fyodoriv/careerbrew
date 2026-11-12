#!/usr/bin/env bash
# Unfinished work to continue before starting anything new: open PRs, unmerged branches,
# unpushed commits, and uncommitted changes. Read-only. Prints names and counts, never file contents.
set -uo pipefail
cd "$(dirname "$0")/.."

base="${BASE_BRANCH:-main}"
found=0
item() { echo "- $1"; found=1; }

if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
  item "uncommitted changes on $(git branch --show-current): $(git status --porcelain | wc -l | tr -d ' ') file(s)"
fi
git fetch -q origin 2>/dev/null || true
merged_into="origin/$base"
git rev-parse -q --verify "$merged_into" >/dev/null || merged_into="$base"
# A squash-merged PR's commits never reach the base branch, but its changes do (issue #174). Such a branch is
# not open work when its tip is a PR's head merged into the base (needs gh), its whole diff is one commit on the
# base (patch id) and no base commit reverts it, or merging it changes nothing. Up to 1000 base commits are read once.
base_pids="$(git log -p --no-merges --format='commit %H' -n 1000 "$merged_into" 2>/dev/null | git patch-id --stable | cut -d' ' -f1)"
merged_heads=""
if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  merged_heads="$(gh pr list --state merged --base "$base" --limit 500 --json headRefOid --jq '.[].headRefOid' 2>/dev/null)"
fi
in_base() {
  local tip fork pid back tree
  tip="$(git rev-parse "$1" 2>/dev/null)" || return 1
  [ -n "$merged_heads" ] && grep -qx "$tip" <<<"$merged_heads" && return 0
  fork="$(git merge-base "$merged_into" "$1" 2>/dev/null)" || return 1
  pid="$(git diff "$fork" "$1" | git patch-id --stable | cut -d' ' -f1)"
  back="$(git diff "$1" "$fork" | git patch-id --stable | cut -d' ' -f1)"  # a revert of it on the base: open again
  [ -n "$pid" ] && grep -qx "$pid" <<<"$base_pids" && ! grep -qx "$back" <<<"$base_pids" && return 0
  tree="$(git merge-tree --write-tree "$merged_into" "$1" 2>/dev/null)" || return 1
  [ "$tree" = "$(git rev-parse "$merged_into^{tree}" 2>/dev/null)" ]
}
# Work that landed as other PRs (split, combined, or reworded since) matches none of the rules above (issue #255).
# Such a branch is listed as likely landed when the base gained most of its added lines since the fork; it
# still shows, so the owner looks before deleting it. Prints the share in percent.
landed_pct() {
  local fork
  fork="$(git merge-base "$merged_into" "$1" 2>/dev/null)" || return 1
  git diff --no-color -U0 "$fork" "$1" 2>/dev/null | awk -v top="" -v fork="$fork" -v base="$merged_into" '
    # q: one shell word in single quotes, so a file name never runs as code.
    function q(s,   out, i, c) {
      out = "\047"
      for (i = 1; i <= length(s); i++) { c = substr(s, i, 1); out = out (c == "\047" ? "\047\\\047\047" : c) }
      return out "\047"
    }
    function trim(s) { gsub(/^[ \t]+|[ \t]+$/, "", s); return s }
    function count(rev, f, into,   cmd, l) {
      cmd = (top == "" ? "git" : "git -C " q(top)) " show " q(rev ":" f) " 2>/dev/null"
      while ((cmd | getline l) > 0) { l = trim(l); if (l != "") into[f, l]++ }
      close(cmd)
    }
    /^\+\+\+ / { file = ($0 == "+++ /dev/null") ? "" : substr($0, 7); next }
    /^\+/ && file != "" { line = trim(substr($0, 2)); if (line != "") { n++; want[file, line]++; files[file] = 1 } }
    END {
      if (n == 0) exit
      # Only lines the base gained since the fork count, each once: a move or a reindent gained nothing.
      for (f in files) { count(fork, f, before); count(base, f, after) }
      for (k in want) { g = after[k] - before[k]; if (g > 0) hit += (want[k] < g ? want[k] : g) }
      printf "%d\n", (100 * hit) / n
    }
  '
}
LANDED_PCT="${OPEN_WORK_LANDED_PCT:-90}"
# A branch the owner looked at and retired (issue #277): a tag archive/<name> on its tip hides it, with no branch
# delete. A new commit moves the tip off the tag, so the branch shows again. Deleting the tag undoes it.
retired() {
  local tag
  tag="$(git rev-parse -q --verify "refs/tags/archive/$2^{commit}" 2>/dev/null)" || return 1
  [ "$tag" = "$(git rev-parse "$1" 2>/dev/null)" ] || return 1
  echo "(retired: branch $2 matches tag archive/$2; a new commit lists it again)"
}
listed=" "  # local branches already listed: their own tip decides, not an older pushed one
while read -r b; do
  [ -z "$b" ] && continue
  if git rev-parse -q --verify "origin/$b" >/dev/null; then
    ahead="$(git rev-list --count "origin/$b..$b")"
    what="$ahead unpushed commit(s)"
  else
    ahead="$(git rev-list --count "$merged_into..$b" 2>/dev/null || echo 0)"
    what="$ahead commit(s) not in $base, never pushed"
  fi
  [ "$ahead" = "0" ] && continue
  retired "$b" "$b" && { listed="$listed$b "; continue; }
  in_base "$b" && continue
  pct=""
  # Commits not yet on the branch's own upstream are new work: never call them landed.
  case "$what" in *"unpushed"*) ;; *) pct="$(landed_pct "$b")" ;; esac
  if [ -n "$pct" ] && [ "$pct" -ge "$LANDED_PCT" ]; then
    item "branch $b: likely landed ($pct% of its added lines are on $base): look, then delete the branch"
  elif [ "$what" = "${what%never pushed}" ]; then
    item "branch $b: $what"
  else
    item "branch $b: $what (if its work landed elsewhere, retire it: git tag archive/$b $b)"
  fi
  listed="$listed$b "
done < <(git for-each-ref --format='%(refname:short)' refs/heads/)
while read -r b; do
  [ -z "$b" ] || [ "$b" = "origin/$base" ] || [ "$b" = "origin/HEAD" ] || [ "$b" = "origin" ] && continue
  n="$(git rev-list --count "origin/$base..$b" 2>/dev/null || echo 0)"
  [ "$n" = "0" ] && continue
  case "$listed" in *" ${b#origin/} "*) continue ;; esac
  retired "$b" "${b#origin/}" && continue
  in_base "$b" && continue
  pct="$(landed_pct "$b")"
  if [ -n "$pct" ] && [ "$pct" -ge "$LANDED_PCT" ]; then
    item "branch ${b#origin/}: likely landed ($pct% of its added lines are on $base): look, then delete the branch"
  else
    item "branch ${b#origin/}: $n commit(s) not in $base"
  fi
done < <(git for-each-ref --format='%(refname:short)' refs/remotes/origin/)
if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  while IFS=$'\t' read -r num title branch draft; do
    [ -z "$num" ] && continue
    item "PR #$num ($branch$([ "$draft" = true ] && echo ', draft')): $title"
  done < <(gh pr list --author @me --state open --json number,title,headRefName,isDraft --jq '.[] | [.number, .title, .headRefName, .isDraft] | @tsv' 2>/dev/null)
else
  echo "(gh not signed in: open PRs not listed; branches above still cover them)"
fi
[ "$found" = 0 ] && echo "none"
[ "$found" = 1 ] && echo "-> continue these before starting a new task (finish, or wrap up into a draft PR)"
exit 0
