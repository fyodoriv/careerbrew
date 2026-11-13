#!/usr/bin/env bash
# Move a careerbrew checkout from the old private history to the public history.
#
# careerbrew moved to a new history that starts at one commit (2026-11-12).
# A checkout made before that day cannot fast-forward: `git status` says
# "diverged" and the two histories share no commit. This script moves it over.
#
# It never deletes work:
#   - every local branch, tag and stash goes into one bundle file first;
#   - old branches, tags and stashes stay in the clone under refs/pre-public*/,
#     where no plain `git push` or `git push --tags` sends them;
#   - `private/` and every other ignored file stays where it is: the script
#     stops when a file on disk is in the way of a file the public tree tracks;
#   - it stops, and changes nothing, when a tracked file has uncommitted changes.
# One limit: a commit that only a branch's reflog still reaches (after a reset)
# is not in the bundle. Give it a branch first if you want to keep it.
#
# Run it from the checkout. It is safe to run again.
#   git fetch origin main && git show origin/main:scripts/migrate-history.sh > "${TMPDIR:-/tmp}/migrate-history.sh" && bash "${TMPDIR:-/tmp}/migrate-history.sh"
#
# Exit 0: the checkout is on the public history. Exit 1: stopped, nothing lost;
# the last line says what to do. Exit 2: this is not a careerbrew checkout.
set -Eeuo pipefail

# The first commit of the public history. A test may set another one.
PUBLIC_ROOT="${CAREERBREW_PUBLIC_ROOT:-22e3e591b70dd8a5f8a1111355201b85c8f68ed5}"
SAVE_DIR="${CAREERBREW_MIGRATE_SAVE_DIR:-$HOME/.local/state/careerbrew-cutover}"

say() { printf 'migrate-history: %s\n' "$*"; }
stop() { say "stopped: $*"; exit 1; }
trap 'say "stopped: a git command failed at line $LINENO. No work was deleted. Fix the error above, then run this again."; exit 1' ERR

# A commit is on the public history only when the public first commit is its one and only root.
is_public() { [ "$(git rev-list --max-parents=0 "$1" 2>/dev/null)" = "$PUBLIC_ROOT" ]; }
old_refs() { # old_refs <prefix>: print "sha ref" for each ref under the prefix that holds old history
  local sha ref
  while read -r sha ref; do
    is_public "$sha" || printf '%s %s\n' "$sha" "$ref"
  done < <(git for-each-ref --format='%(objectname) %(refname)' "$1")
}
drop_ref() { git update-ref --no-deref -d "$1"; }

top="$(git rev-parse --show-toplevel 2>/dev/null)" || { say "not a git checkout"; exit 2; }
cd "$top"
[ -f Agentfile.yaml ] && [ -d src/careerbrew ] || { say "not a careerbrew checkout: $top"; exit 2; }
git remote get-url origin >/dev/null 2>&1 || { say "the checkout has no remote named origin"; exit 2; }

git fetch --quiet --no-tags origin main || stop "cannot fetch origin main. Sign in to GitHub (gh auth login) and check the network, then run this again."
git rev-parse --verify --quiet origin/main >/dev/null || stop "origin has no main branch"
is_public origin/main || stop "origin/main does not start at the public first commit. Check that origin is the public careerbrew repository: $(git remote get-url origin)"

head_public=no
if is_public HEAD; then head_public=yes; fi
left="$( { old_refs refs/heads/; old_refs refs/tags/; } | wc -l | tr -d ' ')"
if [ "$head_public" = yes ] && [ "$left" = 0 ]; then
  say "this checkout is already on the public history. Nothing to do."
  exit 0
fi

others="$(git worktree list --porcelain | grep -c '^worktree ' || true)"
[ "$others" -le 1 ] || stop "$((others - 1)) linked worktree(s) exist. Finish or remove each one (git worktree list; git worktree remove <path>; git worktree prune for a folder that is gone), then run this again."

branch="$(git symbolic-ref --quiet --short HEAD || true)"
if [ "$head_public" = no ]; then
  # Nothing below may lose work. Tracked changes must be committed first.
  [ -n "$branch" ] || stop "HEAD is detached. Check out a branch, then run this again."
  if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
    stop "tracked files have uncommitted changes. Commit them on a branch (they go into the bundle), then run this again."
  fi
  hidden="$(git ls-files -v | grep -c '^[a-zS]' || true)"
  [ "$hidden" = 0 ] || stop "$hidden file(s) are marked assume-unchanged or skip-worktree, so git cannot see edits in them. Clear the marks (git update-index --no-assume-unchanged --no-skip-worktree <path>), then run this again."
  # A file on disk that git does not track here, at a path the new tree needs, would be
  # overwritten or deleted. Check every path of the tree that step 3 checks out, and each
  # parent folder of it. Names are read NUL-separated, so no name is quoted.
  target=origin/main
  if git show-ref --verify --quiet refs/heads/main && is_public refs/heads/main; then target=refs/heads/main; fi
  blocked=0
  first=""
  block() { blocked=$((blocked + 1)); [ -n "$first" ] || first="$1"; }
  tracked() { git ls-files --error-unmatch -- "$1" >/dev/null 2>&1; }
  while IFS= read -r -d '' path; do
    dir="$path"
    while [ "$dir" != "${dir%/*}" ]; do
      dir="${dir%/*}"
      # A parent that is a file or a link, and not a tracked one, would be replaced by a folder.
      if [ -L "$dir" ] || { [ -e "$dir" ] && [ ! -d "$dir" ]; }; then
        tracked "$dir" || block "$dir"
        break
      fi
    done
    if [ -L "$path" ] || [ -f "$path" ]; then
      tracked "$path" || block "$path"
    elif [ -d "$path" ]; then
      # A folder where the new tree has a file: every file in it must be tracked and clean.
      [ -z "$(git status --porcelain --ignored --untracked-files=all -- "$path" | head -1)" ] || block "$path"
    elif [ -e "$path" ]; then
      block "$path"
    fi
  done < <(git ls-tree -r -z --name-only "$target")
  [ "$blocked" = 0 ] || stop "$blocked untracked or ignored file(s) are in the way of files the public history tracks (first: $first). Move each one out of the checkout, then run this again."
fi

# 1. Save every ref outside the checkout. Each stash gets its own ref first:
#    a bundle holds only the newest stash otherwise.
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
n=0
while read -r sha; do
  # A rerun finds the copy an earlier run made and does not add another.
  [ -n "$(git for-each-ref --points-at "$sha" refs/pre-public-stash/)" ] || git update-ref "refs/pre-public-stash/$stamp-$n" "$sha"
  n=$((n + 1))
done < <(git reflog show --format=%H refs/stash 2>/dev/null || true)
mkdir -p "$SAVE_DIR" 2>/dev/null || stop "could not make the folder for the bundle: $SAVE_DIR"
bundle="$SAVE_DIR/pre-public-$stamp.bundle"
git bundle create "$bundle" --all >/dev/null 2>&1 || stop "could not write the bundle to $SAVE_DIR"
git bundle verify "$bundle" >/dev/null 2>&1 || stop "the bundle did not verify: $bundle"
say "saved every branch, tag and stash ($n) to $bundle"

# 2. Copy each old branch and tag aside. Remove it from refs/heads/ and refs/tags/,
#    so no plain push can send old history. The current branch goes last, in step 4.
moved=0
while read -r sha ref; do
  name="${ref#refs/heads/}"
  # A name can clash with a ref an earlier run parked (foo, then foo/bar): add the time stamp then.
  git update-ref "refs/pre-public/$name" "$sha" 2>/dev/null || git update-ref "refs/pre-public/$name-$stamp" "$sha"
  [ "$name" = "$branch" ] || drop_ref "$ref"
  moved=$((moved + 1))
done < <(old_refs refs/heads/)
tags=0
while read -r sha ref; do
  git update-ref "refs/pre-public-tags/${ref#refs/tags/}" "$sha"
  drop_ref "$ref"
  tags=$((tags + 1))
done < <(old_refs refs/tags/)

# 3. Put the working tree on the public main. Ignored files stay.
if [ "$head_public" = no ]; then
  if git show-ref --verify --quiet refs/heads/main && is_public refs/heads/main; then
    # A local main that is already public may hold unpushed commits: keep it as it is.
    git checkout --quiet --no-overwrite-ignore main || true
  else
    git checkout --quiet --no-overwrite-ignore -B main origin/main || true
  fi
  # A hook can make checkout fail after it moved HEAD. Judge by where HEAD is.
  if [ "$(git symbolic-ref --quiet --short HEAD || true)" != main ] || ! is_public HEAD; then
    stop "git could not check out the public main. Read the git error above, fix it, then run this again. Your old branch is at refs/pre-public/$branch and in the bundle."
  fi
  git branch --quiet --set-upstream-to=origin/main main >/dev/null 2>&1 || true
fi

# 4. Remove the old current branch now that HEAD has left it.
now="$(git symbolic-ref --quiet --short HEAD || true)"
while read -r sha ref; do
  [ "${ref#refs/heads/}" != "$now" ] || continue
  drop_ref "$ref"
done < <(old_refs refs/heads/)
git remote prune origin >/dev/null 2>&1 || true

say "moved $moved old branch(es) to refs/pre-public/ and $tags old tag(s) to refs/pre-public-tags/. Never push them: they hold the private history."
say "the checkout is on the public history at $(git rev-parse --short HEAD)."
say "next: scripts/setup.sh, then scripts/doctor.sh"
